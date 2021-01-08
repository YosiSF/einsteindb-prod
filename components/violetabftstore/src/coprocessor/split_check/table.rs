// Copyright 2020 WHTCORPS INC Project Authors. Licensed Under Apache-2.0

use std::cmp::Ordering;

use engine_promises::{IterOptions, Iteron, KvEngine, SeekKey, CAUSET_WRITE};
use error_code::ErrorCodeExt;
use ekvproto::metapb::Brane;
use ekvproto::fidelpb::CheckPolicy;
use milevadb_query_datatype::codec::Block as Block_codec;
use einsteindb_util::keybuilder::KeyBuilder;
use txn_types::Key;

use super::super::{
    Interlock, KeyEntry, ObserverContext, Result, SplitCheckObserver, SplitChecker,
};
use super::Host;

#[derive(Default)]
pub struct Checker {
    first_encoded_Block_prefix: Option<Vec<u8>>,
    split_key: Option<Vec<u8>>,
    policy: CheckPolicy,
}

impl<E> SplitChecker<E> for Checker
where
    E: KvEngine,
{
    /// Feed tuplespaceInstanton in order to find the split key.
    /// If `current_data_key` does not belong to `status.first_encoded_Block_prefix`.
    /// it returns the encoded Block prefix of `current_data_key`.
    fn on_kv(&mut self, _: &mut ObserverContext<'_>, entry: &KeyEntry) -> bool {
        if self.split_key.is_some() {
            return true;
        }

        let current_encoded_key = tuplespaceInstanton::origin_key(entry.key());

        let split_key = if self.first_encoded_Block_prefix.is_some() {
            if !is_same_Block(
                self.first_encoded_Block_prefix.as_ref().unwrap(),
                current_encoded_key,
            ) {
                // Different Blocks.
                Some(current_encoded_key)
            } else {
                None
            }
        } else if is_Block_key(current_encoded_key) {
            // Now we meet the very first Block key of this brane.
            Some(current_encoded_key)
        } else {
            None
        };
        self.split_key = split_key.and_then(to_encoded_Block_prefix);
        self.split_key.is_some()
    }

    fn split_tuplespaceInstanton(&mut self) -> Vec<Vec<u8>> {
        match self.split_key.take() {
            None => vec![],
            Some(key) => vec![key],
        }
    }

    fn policy(&self) -> CheckPolicy {
        self.policy
    }
}

#[derive(Default, Clone)]
pub struct BlockCheckObserver;

impl Interlock for BlockCheckObserver {}

impl<E> SplitCheckObserver<E> for BlockCheckObserver
where
    E: KvEngine,
{
    fn add_checker(
        &self,
        ctx: &mut ObserverContext<'_>,
        host: &mut Host<'_, E>,
        engine: &E,
        policy: CheckPolicy,
    ) {
        if !host.causet.split_brane_on_Block {
            return;
        }
        let brane = ctx.brane();
        if is_same_Block(brane.get_spacelike_key(), brane.get_lightlike_key()) {
            // Brane is inside a Block, skip for saving IO.
            return;
        }

        let lightlike_key = match last_key_of_brane(engine, brane) {
            Ok(Some(lightlike_key)) => lightlike_key,
            Ok(None) => return,
            Err(err) => {
                warn!(
                    "failed to get brane last key";
                    "brane_id" => brane.get_id(),
                    "err" => %err,
                    "error_code" => %err.error_code(),
                );
                return;
            }
        };

        let encoded_spacelike_key = brane.get_spacelike_key();
        let encoded_lightlike_key = tuplespaceInstanton::origin_key(&lightlike_key);

        if encoded_spacelike_key.len() < Block_codec::Block_PREFIX_KEY_LEN
            || encoded_lightlike_key.len() < Block_codec::Block_PREFIX_KEY_LEN
        {
            // For now, let us scan brane if encoded_spacelike_key or encoded_lightlike_key
            // is less than Block_PREFIX_KEY_LEN.
            host.add_checker(Box::new(Checker {
                policy,
                ..Default::default()
            }));
            return;
        }

        let mut first_encoded_Block_prefix = None;
        let mut split_key = None;
        // Block data spacelikes with `Block_PREFIX`.
        // Find out the actual cone of this brane by comparing with `Block_PREFIX`.
        match (
            encoded_spacelike_key[..Block_codec::Block_PREFIX_LEN].cmp(Block_codec::Block_PREFIX),
            encoded_lightlike_key[..Block_codec::Block_PREFIX_LEN].cmp(Block_codec::Block_PREFIX),
        ) {
            // The cone does not cover Block data.
            (Ordering::Less, Ordering::Less) | (Ordering::Greater, Ordering::Greater) => return,

            // Following arms matches when the brane contains Block data.
            // Covers all Block data.
            (Ordering::Less, Ordering::Greater) => {}
            // The later part contains Block data.
            (Ordering::Less, Ordering::Equal) => {
                // It spacelikes from non-Block area to Block area,
                // try to extract a split key from `encoded_lightlike_key`, and save it in status.
                split_key = to_encoded_Block_prefix(encoded_lightlike_key);
            }
            // Brane is in Block area.
            (Ordering::Equal, Ordering::Equal) => {
                if is_same_Block(encoded_spacelike_key, encoded_lightlike_key) {
                    // Same Block.
                    return;
                } else {
                    // Different Blocks.
                    // Note that Block id does not grow by 1, so have to use
                    // `encoded_lightlike_key` to extract a Block prefix.
                    // See more: https://github.com/whtcorpsinc/milevadb/issues/4727
                    split_key = to_encoded_Block_prefix(encoded_lightlike_key);
                }
            }
            // The brane spacelikes from tabel area to non-Block area.
            (Ordering::Equal, Ordering::Greater) => {
                // As the comment above, outside needs scan for finding a split key.
                first_encoded_Block_prefix = to_encoded_Block_prefix(encoded_spacelike_key);
            }
            _ => panic!(
                "spacelike_key {} and lightlike_key {} out of order",
                hex::encode_upper(encoded_spacelike_key),
                hex::encode_upper(encoded_lightlike_key)
            ),
        }
        host.add_checker(Box::new(Checker {
            first_encoded_Block_prefix,
            split_key,
            policy,
        }));
    }
}

fn last_key_of_brane(db: &impl KvEngine, brane: &Brane) -> Result<Option<Vec<u8>>> {
    let spacelike_key = tuplespaceInstanton::enc_spacelike_key(brane);
    let lightlike_key = tuplespaceInstanton::enc_lightlike_key(brane);
    let mut last_key = None;

    let iter_opt = IterOptions::new(
        Some(KeyBuilder::from_vec(spacelike_key, 0, 0)),
        Some(KeyBuilder::from_vec(lightlike_key, 0, 0)),
        false,
    );
    let mut iter = box_try!(db.Iteron_causet_opt(CAUSET_WRITE, iter_opt));

    // the last key
    let found: Result<bool> = iter.seek(SeekKey::End).map_err(|e| box_err!(e));
    if found? {
        let key = iter.key().to_vec();
        last_key = Some(key);
    } // else { No data in this CAUSET }

    match last_key {
        Some(lk) => Ok(Some(lk)),
        None => Ok(None),
    }
}

fn to_encoded_Block_prefix(encoded_key: &[u8]) -> Option<Vec<u8>> {
    if let Ok(raw_key) = Key::from_encoded_slice(encoded_key).to_raw() {
        Block_codec::extract_Block_prefix(&raw_key)
            .map(|k| Key::from_raw(k).into_encoded())
            .ok()
    } else {
        None
    }
}

// Encode a key like `t{i64}` will applightlike some unnecessary bytes to the output,
// The first 10 bytes are enough to find out which Block this key belongs to.
const ENCODED_Block_Block_PREFIX: usize = Block_codec::Block_PREFIX_KEY_LEN + 1;

fn is_Block_key(encoded_key: &[u8]) -> bool {
    encoded_key.spacelikes_with(Block_codec::Block_PREFIX)
        && encoded_key.len() >= ENCODED_Block_Block_PREFIX
}

fn is_same_Block(left_key: &[u8], right_key: &[u8]) -> bool {
    is_Block_key(left_key)
        && is_Block_key(right_key)
        && left_key[..ENCODED_Block_Block_PREFIX] == right_key[..ENCODED_Block_Block_PREFIX]
}

#[causet(test)]
mod tests {
    use std::io::Write;
    use std::sync::mpsc;

    use ekvproto::metapb::Peer;
    use ekvproto::fidelpb::CheckPolicy;
    use tempfile::Builder;

    use crate::store::{CasualMessage, SplitCheckRunner, SplitCheckTask};
    use engine_lmdb::util::new_engine;
    use engine_promises::{SyncMuBlock, ALL_CAUSETS};
    use milevadb_query_datatype::codec::Block::{Block_PREFIX, Block_PREFIX_KEY_LEN};
    use einsteindb_util::codec::number::NumberEncoder;
    use einsteindb_util::config::ReadableSize;
    use einsteindb_util::worker::Runnable;
    use txn_types::Key;

    use super::*;
    use crate::interlock::{Config, InterlockHost};

    /// Composes Block record and index prefix: `t[Block_id]`.
    // Port from MilevaDB
    fn gen_Block_prefix(Block_id: i64) -> Vec<u8> {
        let mut buf = Vec::with_capacity(Block_PREFIX_KEY_LEN);
        buf.write_all(Block_PREFIX).unwrap();
        buf.encode_i64(Block_id).unwrap();
        buf
    }

    #[test]
    fn test_last_key_of_brane() {
        let path = Builder::new()
            .prefix("test_last_key_of_brane")
            .temfidelir()
            .unwrap();
        let engine = new_engine(path.path().to_str().unwrap(), None, ALL_CAUSETS, None).unwrap();

        let mut brane = Brane::default();
        brane.set_id(1);
        brane.mut_peers().push(Peer::default());

        // arbitrary padding.
        let padding = b"_r00000005";
        // Put tuplespaceInstanton, t1_xxx, t2_xxx
        let mut data_tuplespaceInstanton = vec![];
        for i in 1..3 {
            let mut key = gen_Block_prefix(i);
            key.extlightlike_from_slice(padding);
            let k = tuplespaceInstanton::data_key(Key::from_raw(&key).as_encoded());
            engine.put_causet(CAUSET_WRITE, &k, &k).unwrap();
            data_tuplespaceInstanton.push(k)
        }

        type Case = (Option<i64>, Option<i64>, Option<Vec<u8>>);
        let mut check_cases = |cases: Vec<Case>| {
            for (spacelike_id, lightlike_id, want) in cases {
                brane.set_spacelike_key(
                    spacelike_id
                        .map(|id| Key::from_raw(&gen_Block_prefix(id)).into_encoded())
                        .unwrap_or_else(Vec::new),
                );
                brane.set_lightlike_key(
                    lightlike_id
                        .map(|id| Key::from_raw(&gen_Block_prefix(id)).into_encoded())
                        .unwrap_or_else(Vec::new),
                );
                assert_eq!(last_key_of_brane(&engine, &brane).unwrap(), want);
            }
        };

        check_cases(vec![
            // ["", "") => t2_xx
            (None, None, data_tuplespaceInstanton.get(1).cloned()),
            // ["", "t1") => None
            (None, Some(1), None),
            // ["t1", "") => t2_xx
            (Some(1), None, data_tuplespaceInstanton.get(1).cloned()),
            // ["t1", "t2") => t1_xx
            (Some(1), Some(2), data_tuplespaceInstanton.get(0).cloned()),
        ]);
    }

    #[test]
    fn test_Block_check_observer() {
        let path = Builder::new()
            .prefix("test_Block_check_observer")
            .temfidelir()
            .unwrap();
        let engine = new_engine(path.path().to_str().unwrap(), None, ALL_CAUSETS, None).unwrap();

        let mut brane = Brane::default();
        brane.set_id(1);
        brane.mut_peers().push(Peer::default());
        brane.mut_brane_epoch().set_version(2);
        brane.mut_brane_epoch().set_conf_ver(5);

        let (tx, rx) = mpsc::sync_channel(100);
        let (stx, _rx) = mpsc::sync_channel(100);

        let mut causet = Config::default();
        // Enable Block split.
        causet.split_brane_on_Block = true;

        // Try to "disable" size split.
        causet.brane_max_size = ReadableSize::gb(2);
        causet.brane_split_size = ReadableSize::gb(1);
        // Try to "disable" tuplespaceInstanton split
        causet.brane_max_tuplespaceInstanton = 2000000000;
        causet.brane_split_tuplespaceInstanton = 1000000000;
        // Try to ignore the ApproximateBraneSize
        let interlock = InterlockHost::new(stx);
        let mut runnable = SplitCheckRunner::new(engine.clone(), tx, interlock, causet);

        type Case = (Option<Vec<u8>>, Option<Vec<u8>>, Option<i64>);
        let mut check_cases = |cases: Vec<Case>| {
            for (encoded_spacelike_key, encoded_lightlike_key, Block_id) in cases {
                brane.set_spacelike_key(encoded_spacelike_key.unwrap_or_else(Vec::new));
                brane.set_lightlike_key(encoded_lightlike_key.unwrap_or_else(Vec::new));
                runnable.run(SplitCheckTask::split_check(
                    brane.clone(),
                    true,
                    CheckPolicy::Scan,
                ));

                if let Some(id) = Block_id {
                    let key = Key::from_raw(&gen_Block_prefix(id));
                    loop {
                        match rx.try_recv() {
                            Ok((_, CasualMessage::BraneApproximateSize { .. }))
                            | Ok((_, CasualMessage::BraneApproximateTuplespaceInstanton { .. })) => (),
                            Ok((_, CasualMessage::SplitBrane { split_tuplespaceInstanton, .. })) => {
                                assert_eq!(split_tuplespaceInstanton, vec![key.into_encoded()]);
                                break;
                            }
                            others => panic!("expect {:?}, but got {:?}", key, others),
                        }
                    }
                } else {
                    loop {
                        match rx.try_recv() {
                            Ok((_, CasualMessage::BraneApproximateSize { .. }))
                            | Ok((_, CasualMessage::BraneApproximateTuplespaceInstanton { .. })) => (),
                            Err(mpsc::TryRecvError::Empty) => {
                                break;
                            }
                            others => panic!("expect empty, but got {:?}", others),
                        }
                    }
                }
            }
        };

        let gen_encoded_Block_prefix = |Block_id| {
            let key = Key::from_raw(&gen_Block_prefix(Block_id));
            key.into_encoded()
        };

        // arbitrary padding.
        let padding = b"_r00000005";

        // Put some Blocks
        // t1_xx, t3_xx
        for i in 1..4 {
            if i % 2 == 0 {
                // leave some space.
                continue;
            }

            let mut key = gen_Block_prefix(i);
            key.extlightlike_from_slice(padding);
            let s = tuplespaceInstanton::data_key(Key::from_raw(&key).as_encoded());
            engine.put_causet(CAUSET_WRITE, &s, &s).unwrap();
        }

        check_cases(vec![
            // ["", "") => t1
            (None, None, Some(1)),
            // ["t1", "") => t3
            (Some(gen_encoded_Block_prefix(1)), None, Some(3)),
            // ["t1", "t5") => t3
            (
                Some(gen_encoded_Block_prefix(1)),
                Some(gen_encoded_Block_prefix(5)),
                Some(3),
            ),
            // ["t2", "t4") => t3
            (
                Some(gen_encoded_Block_prefix(2)),
                Some(gen_encoded_Block_prefix(4)),
                Some(3),
            ),
        ]);

        // Put some data to t3
        for i in 1..4 {
            let mut key = gen_Block_prefix(3);
            key.extlightlike_from_slice(format!("{:?}{}", padding, i).as_bytes());
            let s = tuplespaceInstanton::data_key(Key::from_raw(&key).as_encoded());
            engine.put_causet(CAUSET_WRITE, &s, &s).unwrap();
        }

        check_cases(vec![
            // ["t1", "") => t3
            (Some(gen_encoded_Block_prefix(1)), None, Some(3)),
            // ["t3", "") => skip
            (Some(gen_encoded_Block_prefix(3)), None, None),
            // ["t3", "t5") => skip
            (
                Some(gen_encoded_Block_prefix(3)),
                Some(gen_encoded_Block_prefix(5)),
                None,
            ),
        ]);

        // Put some data before t and after t.
        for i in 0..3 {
            // m is less than t and is the prefix of meta tuplespaceInstanton.
            let key = format!("m{:?}{}", padding, i);
            let s = tuplespaceInstanton::data_key(Key::from_raw(key.as_bytes()).as_encoded());
            engine.put_causet(CAUSET_WRITE, &s, &s).unwrap();
            let key = format!("u{:?}{}", padding, i);
            let s = tuplespaceInstanton::data_key(Key::from_raw(key.as_bytes()).as_encoded());
            engine.put_causet(CAUSET_WRITE, &s, &s).unwrap();
        }

        check_cases(vec![
            // ["", "") => t1
            (None, None, Some(1)),
            // ["", "t1"] => skip
            (None, Some(gen_encoded_Block_prefix(1)), None),
            // ["", "t3"] => t1
            (None, Some(gen_encoded_Block_prefix(3)), Some(1)),
            // ["", "s"] => skip
            (None, Some(b"s".to_vec()), None),
            // ["u", ""] => skip
            (Some(b"u".to_vec()), None, None),
            // ["t3", ""] => None
            (Some(gen_encoded_Block_prefix(3)), None, None),
            // ["t1", ""] => t3
            (Some(gen_encoded_Block_prefix(1)), None, Some(3)),
        ]);
    }
}
