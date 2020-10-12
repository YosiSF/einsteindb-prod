// Copyright 2019 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

use std::cell::RefCell;

use crossbeam::{SlightlikeError, TrySlightlikeError};
use engine_promises::{KvEngine, VioletaBftEngine, Snapshot};
use ekvproto::violetabft_cmdpb::VioletaBftCmdRequest;
use ekvproto::violetabft_serverpb::VioletaBftMessage;
use violetabft::SnapshotStatus;
use einsteindb_util::time::ThreadReadId;

use crate::store::fsm::VioletaBftRouter;
use crate::store::transport::{CasualRouter, ProposalRouter, StoreRouter};
use crate::store::{
    Callback, CasualMessage, LocalReader, PeerMsg, VioletaBftCommand, SignificantMsg, StoreMsg,
};
use crate::{DiscardReason, Error as VioletaBftStoreError, Result as VioletaBftStoreResult};

/// Routes messages to the violetabftstore.
pub trait VioletaBftStoreRouter<EK>:
    StoreRouter<EK> + ProposalRouter<EK::Snapshot> + CasualRouter<EK> + Slightlike + Clone
where
    EK: KvEngine,
{
    /// Slightlikes VioletaBftMessage to local store.
    fn slightlike_violetabft_msg(&self, msg: VioletaBftMessage) -> VioletaBftStoreResult<()>;

    /// Slightlikes a significant message. We should guarantee that the message can't be dropped.
    fn significant_slightlike(
        &self,
        brane_id: u64,
        msg: SignificantMsg<EK::Snapshot>,
    ) -> VioletaBftStoreResult<()>;

    /// Broadcast a message generated by `msg_gen` to all VioletaBft groups.
    fn broadcast_normal(&self, msg_gen: impl FnMut() -> PeerMsg<EK>);

    /// Slightlike a casual message to the given brane.
    fn slightlike_casual_msg(&self, brane_id: u64, msg: CasualMessage<EK>) -> VioletaBftStoreResult<()> {
        <Self as CasualRouter<EK>>::slightlike(self, brane_id, msg)
    }

    /// Slightlike a store message to the backlightlike violetabft batch system.
    fn slightlike_store_msg(&self, msg: StoreMsg<EK>) -> VioletaBftStoreResult<()> {
        <Self as StoreRouter<EK>>::slightlike(self, msg)
    }

    /// Slightlikes VioletaBftCmdRequest to local store.
    fn slightlike_command(&self, req: VioletaBftCmdRequest, cb: Callback<EK::Snapshot>) -> VioletaBftStoreResult<()> {
        let brane_id = req.get_header().get_brane_id();
        let cmd = VioletaBftCommand::new(req, cb);
        <Self as ProposalRouter<EK::Snapshot>>::slightlike(self, cmd)
            .map_err(|e| handle_slightlike_error(brane_id, e))
    }

    /// Reports the peer being unreachable to the Brane.
    fn report_unreachable(&self, brane_id: u64, to_peer_id: u64) -> VioletaBftStoreResult<()> {
        let msg = SignificantMsg::Unreachable {
            brane_id,
            to_peer_id,
        };
        self.significant_slightlike(brane_id, msg)
    }

    /// Reports the slightlikeing snapshot status to the peer of the Brane.
    fn report_snapshot_status(
        &self,
        brane_id: u64,
        to_peer_id: u64,
        status: SnapshotStatus,
    ) -> VioletaBftStoreResult<()> {
        let msg = SignificantMsg::SnapshotStatus {
            brane_id,
            to_peer_id,
            status,
        };
        self.significant_slightlike(brane_id, msg)
    }

    /// Broadcast an `StoreUnreachable` event to all VioletaBft groups.
    fn broadcast_unreachable(&self, store_id: u64) {
        let _ = self.slightlike_store_msg(StoreMsg::StoreUnreachable { store_id });
    }

    /// Report a `StoreResolved` event to all VioletaBft groups.
    fn report_resolved(&self, store_id: u64, group_id: u64) {
        self.broadcast_normal(|| {
            PeerMsg::SignificantMsg(SignificantMsg::StoreResolved { store_id, group_id })
        })
    }
}

pub trait LocalReadRouter<EK>: Slightlike + Clone
where
    EK: KvEngine,
{
    fn read(
        &self,
        read_id: Option<ThreadReadId>,
        req: VioletaBftCmdRequest,
        cb: Callback<EK::Snapshot>,
    ) -> VioletaBftStoreResult<()>;

    fn release_snapshot_cache(&self);
}

#[derive(Clone)]
pub struct VioletaBftStoreBlackHole;

impl<EK: KvEngine> CasualRouter<EK> for VioletaBftStoreBlackHole {
    fn slightlike(&self, _: u64, _: CasualMessage<EK>) -> VioletaBftStoreResult<()> {
        Ok(())
    }
}

impl<S: Snapshot> ProposalRouter<S> for VioletaBftStoreBlackHole {
    fn slightlike(&self, _: VioletaBftCommand<S>) -> std::result::Result<(), TrySlightlikeError<VioletaBftCommand<S>>> {
        Ok(())
    }
}

impl<EK> StoreRouter<EK> for VioletaBftStoreBlackHole
where
    EK: KvEngine,
{
    fn slightlike(&self, _: StoreMsg<EK>) -> VioletaBftStoreResult<()> {
        Ok(())
    }
}

impl<EK> VioletaBftStoreRouter<EK> for VioletaBftStoreBlackHole
where
    EK: KvEngine,
{
    /// Slightlikes VioletaBftMessage to local store.
    fn slightlike_violetabft_msg(&self, _: VioletaBftMessage) -> VioletaBftStoreResult<()> {
        Ok(())
    }

    /// Slightlikes a significant message. We should guarantee that the message can't be dropped.
    fn significant_slightlike(&self, _: u64, _: SignificantMsg<EK::Snapshot>) -> VioletaBftStoreResult<()> {
        Ok(())
    }

    fn broadcast_normal(&self, _: impl FnMut() -> PeerMsg<EK>) {}
}

/// A router that routes messages to the violetabftstore
pub struct ServerVioletaBftStoreRouter<EK: KvEngine, ER: VioletaBftEngine> {
    router: VioletaBftRouter<EK, ER>,
    local_reader: RefCell<LocalReader<VioletaBftRouter<EK, ER>, EK>>,
}

impl<EK: KvEngine, ER: VioletaBftEngine> Clone for ServerVioletaBftStoreRouter<EK, ER> {
    fn clone(&self) -> Self {
        ServerVioletaBftStoreRouter {
            router: self.router.clone(),
            local_reader: self.local_reader.clone(),
        }
    }
}

impl<EK: KvEngine, ER: VioletaBftEngine> ServerVioletaBftStoreRouter<EK, ER> {
    /// Creates a new router.
    pub fn new(
        router: VioletaBftRouter<EK, ER>,
        reader: LocalReader<VioletaBftRouter<EK, ER>, EK>,
    ) -> ServerVioletaBftStoreRouter<EK, ER> {
        let local_reader = RefCell::new(reader);
        ServerVioletaBftStoreRouter {
            router,
            local_reader,
        }
    }
}

impl<EK: KvEngine, ER: VioletaBftEngine> StoreRouter<EK> for ServerVioletaBftStoreRouter<EK, ER> {
    fn slightlike(&self, msg: StoreMsg<EK>) -> VioletaBftStoreResult<()> {
        StoreRouter::slightlike(&self.router, msg)
    }
}

impl<EK: KvEngine, ER: VioletaBftEngine> ProposalRouter<EK::Snapshot> for ServerVioletaBftStoreRouter<EK, ER> {
    fn slightlike(
        &self,
        cmd: VioletaBftCommand<EK::Snapshot>,
    ) -> std::result::Result<(), TrySlightlikeError<VioletaBftCommand<EK::Snapshot>>> {
        ProposalRouter::slightlike(&self.router, cmd)
    }
}

impl<EK: KvEngine, ER: VioletaBftEngine> CasualRouter<EK> for ServerVioletaBftStoreRouter<EK, ER> {
    fn slightlike(&self, brane_id: u64, msg: CasualMessage<EK>) -> VioletaBftStoreResult<()> {
        CasualRouter::slightlike(&self.router, brane_id, msg)
    }
}

impl<EK: KvEngine, ER: VioletaBftEngine> VioletaBftStoreRouter<EK> for ServerVioletaBftStoreRouter<EK, ER> {
    fn slightlike_violetabft_msg(&self, msg: VioletaBftMessage) -> VioletaBftStoreResult<()> {
        VioletaBftStoreRouter::slightlike_violetabft_msg(&self.router, msg)
    }

    /// Slightlikes a significant message. We should guarantee that the message can't be dropped.
    fn significant_slightlike(
        &self,
        brane_id: u64,
        msg: SignificantMsg<EK::Snapshot>,
    ) -> VioletaBftStoreResult<()> {
        VioletaBftStoreRouter::significant_slightlike(&self.router, brane_id, msg)
    }

    fn broadcast_normal(&self, msg_gen: impl FnMut() -> PeerMsg<EK>) {
        self.router.broadcast_normal(msg_gen)
    }
}

impl<EK: KvEngine, ER: VioletaBftEngine> LocalReadRouter<EK> for ServerVioletaBftStoreRouter<EK, ER> {
    fn read(
        &self,
        read_id: Option<ThreadReadId>,
        req: VioletaBftCmdRequest,
        cb: Callback<EK::Snapshot>,
    ) -> VioletaBftStoreResult<()> {
        let mut local_reader = self.local_reader.borrow_mut();
        local_reader.read(read_id, req, cb);
        Ok(())
    }

    fn release_snapshot_cache(&self) {
        let mut local_reader = self.local_reader.borrow_mut();
        local_reader.release_snapshot_cache();
    }
}

#[inline]
pub fn handle_slightlike_error<T>(brane_id: u64, e: TrySlightlikeError<T>) -> VioletaBftStoreError {
    match e {
        TrySlightlikeError::Full(_) => VioletaBftStoreError::Transport(DiscardReason::Full),
        TrySlightlikeError::Disconnected(_) => VioletaBftStoreError::BraneNotFound(brane_id),
    }
}

impl<EK: KvEngine, ER: VioletaBftEngine> VioletaBftStoreRouter<EK> for VioletaBftRouter<EK, ER> {
    fn slightlike_violetabft_msg(&self, msg: VioletaBftMessage) -> VioletaBftStoreResult<()> {
        let brane_id = msg.get_brane_id();
        self.slightlike_violetabft_message(msg)
            .map_err(|e| handle_slightlike_error(brane_id, e))
    }

    fn significant_slightlike(
        &self,
        brane_id: u64,
        msg: SignificantMsg<EK::Snapshot>,
    ) -> VioletaBftStoreResult<()> {
        if let Err(SlightlikeError(msg)) = self
            .router
            .force_slightlike(brane_id, PeerMsg::SignificantMsg(msg))
        {
            // TODO: panic here once we can detect system is shutting down reliably.
            error!("failed to slightlike significant msg"; "msg" => ?msg);
            return Err(VioletaBftStoreError::BraneNotFound(brane_id));
        }

        Ok(())
    }

    fn broadcast_normal(&self, msg_gen: impl FnMut() -> PeerMsg<EK>) {
        batch_system::Router::broadcast_normal(self, msg_gen)
    }
}
