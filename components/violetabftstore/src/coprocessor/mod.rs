// Copyright 2020 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

use std::vec::IntoIter;

use engine_promises::CfName;
use ekvproto::metapb::Brane;
use ekvproto::fidelpb::CheckPolicy;
use ekvproto::raft_cmdpb::{AdminRequest, AdminResponse, VioletaBftCmdRequest, VioletaBftCmdResponse, Request};
use violetabft::StateRole;

pub mod config;
mod consistency_check;
pub mod dispatcher;
mod error;
mod metrics;
pub mod brane_info_accessor;
mod split_check;
pub mod split_observer;

pub use self::config::{Config, ConsistencyCheckMethod};
pub use self::consistency_check::{ConsistencyCheckObserver, Raw as RawConsistencyCheckObserver};
pub use self::dispatcher::{
    BoxAdminObserver, BoxApplySnapshotObserver, BoxCmdObserver, BoxConsistencyCheckObserver,
    BoxQueryObserver, BoxBraneChangeObserver, BoxRoleObserver, BoxSplitCheckObserver,
    InterlockHost, Registry,
};
pub use self::error::{Error, Result};
pub use self::brane_info_accessor::{
    Callback as BraneInfoCallback, BraneCollector, BraneInfo, BraneInfoAccessor,
    BraneInfoProvider, SeekBraneCallback,
};
pub use self::split_check::{
    get_brane_approximate_tuplespaceInstanton, get_brane_approximate_tuplespaceInstanton_causet, get_brane_approximate_middle,
    get_brane_approximate_size, get_brane_approximate_size_causet, HalfCheckObserver,
    Host as SplitCheckerHost, TuplespaceInstantonCheckObserver, SizeCheckObserver, TableCheckObserver,
};

use crate::store::fsm::ObserveID;
pub use crate::store::KeyEntry;

/// Interlock is used to provide a convenient way to inject code to
/// KV processing.
pub trait Interlock: Slightlike {
    fn spacelike(&self) {}
    fn stop(&self) {}
}

/// Context of observer.
pub struct ObserverContext<'a> {
    brane: &'a Brane,
    /// Whether to bypass following observer hook.
    pub bypass: bool,
}

impl<'a> ObserverContext<'a> {
    pub fn new(brane: &Brane) -> ObserverContext<'_> {
        ObserverContext {
            brane,
            bypass: false,
        }
    }

    pub fn brane(&self) -> &Brane {
        self.brane
    }
}

pub trait AdminObserver: Interlock {
    /// Hook to call before proposing admin request.
    fn pre_propose_admin(&self, _: &mut ObserverContext<'_>, _: &mut AdminRequest) -> Result<()> {
        Ok(())
    }

    /// Hook to call before applying admin request.
    fn pre_apply_admin(&self, _: &mut ObserverContext<'_>, _: &AdminRequest) {}

    /// Hook to call after applying admin request.
    fn post_apply_admin(&self, _: &mut ObserverContext<'_>, _: &mut AdminResponse) {}
}

pub trait QueryObserver: Interlock {
    /// Hook to call before proposing write request.
    ///
    /// We don't propose read request, hence there is no hook for it yet.
    fn pre_propose_query(&self, _: &mut ObserverContext<'_>, _: &mut Vec<Request>) -> Result<()> {
        Ok(())
    }

    /// Hook to call before applying write request.
    fn pre_apply_query(&self, _: &mut ObserverContext<'_>, _: &[Request]) {}

    /// Hook to call after applying write request.
    fn post_apply_query(&self, _: &mut ObserverContext<'_>, _: &mut Cmd) {}
}

pub trait ApplySnapshotObserver: Interlock {
    /// Hook to call after applying key from plain file.
    /// This may be invoked multiple times for each plain file, and each time a batch of key-value
    /// pairs will be passed to the function.
    fn apply_plain_kvs(&self, _: &mut ObserverContext<'_>, _: CfName, _: &[(Vec<u8>, Vec<u8>)]) {}

    /// Hook to call after applying sst file. Currently the content of the snapshot can't be
    /// passed to the observer.
    fn apply_sst(&self, _: &mut ObserverContext<'_>, _: CfName, _path: &str) {}
}

/// SplitChecker is invoked during a split check scan, and decides to use
/// which tuplespaceInstanton to split a brane.
pub trait SplitChecker<E> {
    /// Hook to call for every kv scanned during split.
    ///
    /// Return true to abort scan early.
    fn on_kv(&mut self, _: &mut ObserverContext<'_>, _: &KeyEntry) -> bool {
        false
    }

    /// Get the desired split tuplespaceInstanton.
    fn split_tuplespaceInstanton(&mut self) -> Vec<Vec<u8>>;

    /// Get approximate split tuplespaceInstanton without scan.
    fn approximate_split_tuplespaceInstanton(&mut self, _: &Brane, _: &E) -> Result<Vec<Vec<u8>>> {
        Ok(vec![])
    }

    /// Get split policy.
    fn policy(&self) -> CheckPolicy;
}

pub trait SplitCheckObserver<E>: Interlock {
    /// Add a checker for a split scan.
    fn add_checker(
        &self,
        _: &mut ObserverContext<'_>,
        _: &mut SplitCheckerHost<'_, E>,
        _: &E,
        policy: CheckPolicy,
    );
}

pub trait RoleObserver: Interlock {
    /// Hook to call when role of a peer changes.
    ///
    /// Please note that, this hook is not called at realtime. There maybe a
    /// situation that the hook is not called yet, however the role of some peers
    /// have changed.
    fn on_role_change(&self, _: &mut ObserverContext<'_>, _: StateRole) {}
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BraneChangeEvent {
    Create,
    Ufidelate,
    Destroy,
}

pub trait BraneChangeObserver: Interlock {
    /// Hook to call when a brane changed on this EinsteinDB
    fn on_brane_changed(&self, _: &mut ObserverContext<'_>, _: BraneChangeEvent, _: StateRole) {}
}

#[derive(Clone, Debug)]
pub struct Cmd {
    pub index: u64,
    pub request: VioletaBftCmdRequest,
    pub response: VioletaBftCmdResponse,
}

impl Cmd {
    pub fn new(index: u64, request: VioletaBftCmdRequest, response: VioletaBftCmdResponse) -> Cmd {
        Cmd {
            index,
            request,
            response,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CmdBatch {
    pub observe_id: ObserveID,
    pub brane_id: u64,
    pub cmds: Vec<Cmd>,
}

impl CmdBatch {
    pub fn new(observe_id: ObserveID, brane_id: u64) -> CmdBatch {
        CmdBatch {
            observe_id,
            brane_id,
            cmds: Vec::new(),
        }
    }

    pub fn push(&mut self, observe_id: ObserveID, brane_id: u64, cmd: Cmd) {
        assert_eq!(brane_id, self.brane_id);
        assert_eq!(observe_id, self.observe_id);
        self.cmds.push(cmd)
    }

    pub fn into_iter(self, brane_id: u64) -> IntoIter<Cmd> {
        assert_eq!(self.brane_id, brane_id);
        self.cmds.into_iter()
    }

    pub fn len(&self) -> usize {
        self.cmds.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cmds.is_empty()
    }

    pub fn size(&self) -> usize {
        let mut cmd_bytes = 0;
        for cmd in self.cmds.iter() {
            let Cmd {
                ref request,
                ref response,
                ..
            } = cmd;
            if !response.get_header().has_error() {
                if !request.has_admin_request() {
                    for req in request.requests.iter() {
                        let put = req.get_put();
                        cmd_bytes += put.get_key().len();
                        cmd_bytes += put.get_value().len();
                    }
                }
            }
        }
        cmd_bytes
    }
}

pub trait CmdObserver<E>: Interlock {
    /// Hook to call after preparing for applying write requests.
    fn on_prepare_for_apply(&self, observe_id: ObserveID, brane_id: u64);
    /// Hook to call after applying a write request.
    fn on_apply_cmd(&self, observe_id: ObserveID, brane_id: u64, cmd: Cmd);
    /// Hook to call after flushing writes to db.
    fn on_flush_apply(&self, engine: E);
}
