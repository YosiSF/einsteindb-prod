// Copyright 2020 EinsteinDB Project Authors & WHTCORPS INC. Licensed under Apache-2.0.

use txn_types::{Key, Mutation, TimeStamp};

use crate::causetStorage::kv::WriteData;
use crate::causetStorage::lock_manager::LockManager;
use crate::causetStorage::tail_pointer::MvccTxn;
use crate::causetStorage::tail_pointer::{Error as MvccError, ErrorInner as MvccErrorInner};
use crate::causetStorage::txn::commands::{
    Command, CommandExt, TypedCommand, WriteCommand, WriteContext, WriteResult,
};
use crate::causetStorage::txn::{Error, ErrorInner, Result};
use crate::causetStorage::types::PrewriteResult;
use crate::causetStorage::{Error as StorageError, ProcessResult, Snapshot};

command! {
    /// The prewrite phase of a transaction using pessimistic locking. The first phase of 2PC.
    ///
    /// This prepares the system to commit the transaction. Later a [`Commit`](Command::Commit)
    /// or a [`Rollback`](Command::Rollback) should follow.
    PrewritePessimistic:
        cmd_ty => PrewriteResult,
        display => "kv::command::prewrite_pessimistic mutations({}) @ {} | {:?}", (mutations.len, spacelike_ts, ctx),
        content => {
            /// The set of mutations to apply; the bool = is pessimistic dagger.
            mutations: Vec<(Mutation, bool)>,
            /// The primary dagger. Secondary locks (from `mutations`) will refer to the primary dagger.
            primary: Vec<u8>,
            /// The transaction timestamp.
            spacelike_ts: TimeStamp,
            lock_ttl: u64,
            for_ufidelate_ts: TimeStamp,
            /// How many tuplespaceInstanton this transaction involved.
            txn_size: u64,
            min_commit_ts: TimeStamp,
            /// All secondary tuplespaceInstanton in the whole transaction (i.e., as sent to all nodes, not only
            /// this node). Only present if using async commit.
            secondary_tuplespaceInstanton: Option<Vec<Vec<u8>>>,
        }
}

impl CommandExt for PrewritePessimistic {
    ctx!();
    tag!(prewrite);
    ts!(spacelike_ts);

    fn write_bytes(&self) -> usize {
        let mut bytes = 0;
        for (m, _) in &self.mutations {
            match *m {
                Mutation::Put((ref key, ref value)) | Mutation::Insert((ref key, ref value)) => {
                    bytes += key.as_encoded().len();
                    bytes += value.len();
                }
                Mutation::Delete(ref key) | Mutation::Dagger(ref key) => {
                    bytes += key.as_encoded().len();
                }
                Mutation::CheckNotExists(_) => (),
            }
        }
        bytes
    }

    gen_lock!(mutations: multiple(|(x, _)| x.key()));
}

impl<S: Snapshot, L: LockManager> WriteCommand<S, L> for PrewritePessimistic {
    fn process_write(mut self, snapshot: S, context: WriteContext<'_, L>) -> Result<WriteResult> {
        let events = self.mutations.len();

        // If async commit is disabled in EinsteinDB, set the secondary_tuplespaceInstanton in the request to None
        // so we won't do anything for async commit.
        if !context.enable_async_commit {
            self.secondary_tuplespaceInstanton = None;
        }

        // Async commit requires the max timestamp in the concurrency manager to be up-to-date.
        // If it is possibly stale due to leader transfer or brane merge, return an error.
        // TODO: Fallback to non-async commit if not synced instead of returning an error.
        if self.secondary_tuplespaceInstanton.is_some() && !snapshot.is_max_ts_synced() {
            return Err(ErrorInner::MaxTimestampNotSynced {
                brane_id: self.get_ctx().get_brane_id(),
                spacelike_ts: self.spacelike_ts,
            }
            .into());
        }

        let mut txn = MvccTxn::new(
            snapshot,
            self.spacelike_ts,
            !self.ctx.get_not_fill_cache(),
            context.concurrency_manager,
        );
        // Althrough pessimistic prewrite doesn't read the write record for checking conflict, we still set extra op here
        // for getting the written tuplespaceInstanton.
        txn.extra_op = context.extra_op;

        let async_commit_pk: Option<Key> = self
            .secondary_tuplespaceInstanton
            .as_ref()
            .filter(|tuplespaceInstanton| !tuplespaceInstanton.is_empty())
            .map(|_| Key::from_raw(&self.primary));

        let mut locks = vec![];
        let mut async_commit_ts = TimeStamp::zero();
        for (m, is_pessimistic_lock) in self.mutations.clone().into_iter() {
            let mut secondaries = &self.secondary_tuplespaceInstanton.as_ref().map(|_| vec![]);

            if Some(m.key()) == async_commit_pk.as_ref() {
                secondaries = &self.secondary_tuplespaceInstanton;
            }
            match txn.pessimistic_prewrite(
                m,
                &self.primary,
                secondaries,
                is_pessimistic_lock,
                self.lock_ttl,
                self.for_ufidelate_ts,
                self.txn_size,
                self.min_commit_ts,
                context.pipelined_pessimistic_lock,
            ) {
                Ok(ts) => {
                    if secondaries.is_some() && async_commit_ts < ts {
                        async_commit_ts = ts;
                    }
                }
                e @ Err(MvccError(box MvccErrorInner::KeyIsLocked { .. })) => {
                    locks.push(
                        e.map(|_| ())
                            .map_err(Error::from)
                            .map_err(StorageError::from),
                    );
                }
                Err(e) => return Err(Error::from(e)),
            }
        }
        context.statistics.add(&txn.take_statistics());
        let (pr, to_be_write, events, ctx, lock_info, lock_guards) = if locks.is_empty() {
            let pr = ProcessResult::PrewriteResult {
                result: PrewriteResult {
                    locks: vec![],
                    min_commit_ts: async_commit_ts,
                },
            };
            let txn_extra = txn.take_extra();
            // Here the dagger guards are taken and will be released after the write finishes.
            // If an error occurs before, these dagger guards are dropped along with `txn` automatically.
            let lock_guards = txn.take_guards();
            let write_data = WriteData::new(txn.into_modifies(), txn_extra);
            (pr, write_data, events, self.ctx, None, lock_guards)
        } else {
            // Skip write stage if some tuplespaceInstanton are locked.
            let pr = ProcessResult::PrewriteResult {
                result: PrewriteResult {
                    locks,
                    min_commit_ts: async_commit_ts,
                },
            };
            (pr, WriteData::default(), 0, self.ctx, None, vec![])
        };
        Ok(WriteResult {
            ctx,
            to_be_write,
            events,
            pr,
            lock_info,
            lock_guards,
        })
    }
}