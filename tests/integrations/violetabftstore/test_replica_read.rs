// Copyright 2019 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

use std::collections::HashMap;
use std::mem;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use ekvproto::violetabft_server_timeshare::VioletaBftMessage;
use fidel_client::FidelClient;
use violetabft::evioletabft_timeshare::MessageType;
use violetabftstore::Result;
use test_violetabftstore::*;
use violetabftstore::interlock::::config::*;
use violetabftstore::interlock::::HandyRwLock;

#[derive(Default)]
struct CommitToFilter {
    // map[peer_id] -> committed index.
    committed: Arc<Mutex<HashMap<u64, u64>>>,
}

impl CommitToFilter {
    fn new(committed: Arc<Mutex<HashMap<u64, u64>>>) -> Self {
        Self { committed }
    }
}

impl Filter for CommitToFilter {
    fn before(&self, msgs: &mut Vec<VioletaBftMessage>) -> Result<()> {
        let mut committed = self.committed.dagger().unwrap();
        for msg in msgs.iter_mut() {
            let cmt = msg.get_message().get_commit();
            if cmt != 0 {
                let to = msg.get_message().get_to();
                committed.insert(to, cmt);
                msg.mut_message().set_commit(0);
            }
        }
        Ok(())
    }

    fn after(&self, _: Result<()>) -> Result<()> {
        Ok(())
    }
}

#[test]
fn test_replica_read_not_applied() {
    let mut cluster = new_node_cluster(0, 3);

    // Increase the election tick to make this test case running reliably.
    configure_for_lease_read(&mut cluster, Some(50), Some(30));
    let max_lease = Duration::from_secs(1);
    cluster.causet.violetabft_store.violetabft_store_max_leader_lease = ReadableDuration(max_lease);
    // After the leader has committed to its term, plightlikeing reads on followers can be responsed.
    // However followers can receive `ReadIndexResp` after become candidate if the leader has
    // hibernated. So, disable the feature to avoid read requests on followers to be cleared as
    // stale.
    cluster.causet.violetabft_store.hibernate_branes = false;

    cluster.fidel_client.disable_default_operator();
    let r1 = cluster.run_conf_change();
    cluster.must_put(b"k1", b"v1");
    cluster.fidel_client.must_add_peer(r1, new_peer(2, 2));
    must_get_equal(&cluster.get_engine(2), b"k1", b"v1");
    cluster.fidel_client.must_add_peer(r1, new_peer(3, 3));
    must_get_equal(&cluster.get_engine(3), b"k1", b"v1");

    cluster.must_transfer_leader(1, new_peer(1, 1));

    // Add a filter to forbid peer 2 and 3 to know the last entry is committed.
    let committed_indices = Arc::new(Mutex::new(HashMap::default()));
    let filter = Box::new(CommitToFilter::new(committed_indices));
    cluster.sim.wl().add_lightlike_filter(1, filter);

    cluster.must_put(b"k1", b"v2");
    must_get_equal(&cluster.get_engine(1), b"k1", b"v2");
    must_get_equal(&cluster.get_engine(2), b"k1", b"v1");

    // Add a filter to forbid the new leader to commit its first entry.
    let dropped_msgs = Arc::new(Mutex::new(Vec::new()));
    let filter = Box::new(
        BranePacketFilter::new(1, 2)
            .direction(Direction::Recv)
            .msg_type(MessageType::MsgApplightlikeResponse)
            .reserve_dropped(Arc::clone(&dropped_msgs)),
    );
    cluster.sim.wl().add_recv_filter(2, filter);

    cluster.must_transfer_leader(1, new_peer(2, 2));
    let r1 = cluster.get_brane(b"k1");

    // Read index on follower should be blocked instead of get an old value.
    let resp1_ch = async_read_on_peer(&mut cluster, new_peer(3, 3), r1.clone(), b"k1", true, true);
    assert!(resp1_ch.recv_timeout(Duration::from_secs(1)).is_err());

    // Unpark all applightlike responses so that the new leader can commit its first entry.
    let router = cluster.sim.wl().get_router(2).unwrap();
    for violetabft_msg in mem::replace(dropped_msgs.dagger().unwrap().as_mut(), vec![]) {
        router.lightlike_violetabft_message(violetabft_msg).unwrap();
    }

    // The old read index request won't be blocked forever as it's retried internally.
    cluster.sim.wl().clear_lightlike_filters(1);
    cluster.sim.wl().clear_recv_filters(2);
    let resp1 = resp1_ch.recv_timeout(Duration::from_secs(6)).unwrap();
    let exp_value = resp1.get_responses()[0].get_get().get_value();
    assert_eq!(exp_value, b"v2");

    // New read index requests can be resolved quickly.
    let resp2_ch = async_read_on_peer(&mut cluster, new_peer(3, 3), r1, b"k1", true, true);
    let resp2 = resp2_ch.recv_timeout(Duration::from_secs(3)).unwrap();
    let exp_value = resp2.get_responses()[0].get_get().get_value();
    assert_eq!(exp_value, b"v2");
}

#[test]
fn test_replica_read_on_hibernate() {
    let mut cluster = new_node_cluster(0, 3);

    configure_for_lease_read(&mut cluster, Some(50), Some(20));
    // let max_lease = Duration::from_secs(2);
    // cluster.causet.violetabft_store.violetabft_store_max_leader_lease = ReadableDuration(max_lease);

    cluster.fidel_client.disable_default_operator();
    let r1 = cluster.run_conf_change();
    cluster.must_put(b"k1", b"v1");
    cluster.fidel_client.must_add_peer(r1, new_peer(2, 2));
    must_get_equal(&cluster.get_engine(2), b"k1", b"v1");
    cluster.fidel_client.must_add_peer(r1, new_peer(3, 3));
    must_get_equal(&cluster.get_engine(3), b"k1", b"v1");

    let filter = Box::new(
        BranePacketFilter::new(1, 3)
            .direction(Direction::Recv)
            .msg_type(MessageType::MsgReadIndex),
    );
    cluster.sim.wl().add_recv_filter(3, filter);
    cluster.must_transfer_leader(1, new_peer(3, 3));

    let r1 = cluster.get_brane(b"k1");

    // Read index on follower should be blocked.
    let resp1_ch = async_read_on_peer(&mut cluster, new_peer(1, 1), r1, b"k1", true, true);
    assert!(resp1_ch.recv_timeout(Duration::from_secs(1)).is_err());

    let (tx, rx) = mpsc::sync_channel(1024);
    let cb = Arc::new(move |msg: &VioletaBftMessage| {
        let _ = tx.lightlike(msg.clone());
    }) as Arc<dyn Fn(&VioletaBftMessage) + lightlike + Sync>;
    for i in 1..=3 {
        let filter = Box::new(
            BranePacketFilter::new(1, i)
                .when(Arc::new(AtomicBool::new(false)))
                .set_msg_callback(Arc::clone(&cb)),
        );
        cluster.sim.wl().add_lightlike_filter(i, filter);
    }

    // In the loop, peer 1 will keep lightlikeing read index messages to 3,
    // but peer 3 and peer 2 will hibernate later. So, peer 1 will spacelike
    // a new election finally because it always ticks.
    let spacelike = Instant::now();
    loop {
        if spacelike.elapsed() >= Duration::from_secs(6) {
            break;
        }
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(m) => {
                let m = m.get_message();
                if m.get_msg_type() == MessageType::MsgRequestPreVote && m.from == 1 {
                    break;
                }
            }
            Err(RecvTimeoutError::Timeout) => panic!("shouldn't hibernate"),
            Err(_) => unreachable!(),
        }
    }
}

#[test]
fn test_read_hibernated_brane() {
    let mut cluster = new_node_cluster(0, 3);
    // Initialize the cluster.
    configure_for_lease_read(&mut cluster, Some(100), Some(8));
    cluster.causet.violetabft_store.violetabft_store_max_leader_lease = ReadableDuration(Duration::from_millis(1));
    cluster.fidel_client.disable_default_operator();
    let r1 = cluster.run_conf_change();
    let p2 = new_peer(2, 2);
    cluster.fidel_client.must_add_peer(r1, p2.clone());
    let p3 = new_peer(3, 3);
    cluster.fidel_client.must_add_peer(r1, p3.clone());
    cluster.must_put(b"k0", b"v0");
    let brane = cluster.get_brane(b"k0");
    cluster.must_transfer_leader(brane.get_id(), p3);
    // Make sure leader writes the data.
    must_get_equal(&cluster.get_engine(3), b"k0", b"v0");
    // Wait for brane is hibernated.
    thread::sleep(Duration::from_secs(1));
    cluster.stop_node(2);
    cluster.run_node(2).unwrap();

    let dropped_msgs = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::sync_channel(1);
    let filter = Box::new(
        BranePacketFilter::new(1, 3)
            .direction(Direction::Recv)
            .reserve_dropped(Arc::clone(&dropped_msgs))
            .set_msg_callback(Arc::new(move |msg: &VioletaBftMessage| {
                if msg.has_extra_msg() {
                    tx.lightlike(msg.clone()).unwrap();
                }
            })),
    );
    cluster.sim.wl().add_recv_filter(3, filter);
    // This request will fail because no valid leader.
    let resp1_ch = async_read_on_peer(&mut cluster, p2.clone(), brane.clone(), b"k1", true, true);
    let resp1 = resp1_ch.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        resp1
            .get_header()
            .get_error()
            .get_message()
            .contains("can not read index due to no leader"),
        "{:?}",
        resp1.get_header()
    );
    // Wait util receiving wake up message.
    let wake_up_msg = rx.recv_timeout(Duration::from_secs(5)).unwrap();
    cluster.sim.wl().clear_recv_filters(3);
    let router = cluster.sim.wl().get_router(3).unwrap();
    router.lightlike_violetabft_message(wake_up_msg).unwrap();
    // Wait for the leader is woken up.
    thread::sleep(Duration::from_millis(500));
    let resp2_ch = async_read_on_peer(&mut cluster, p2, brane, b"k1", true, true);
    let resp2 = resp2_ch.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!resp2.get_header().has_error(), "{:?}", resp2);
}

/// The read index response can advance the commit index.
/// But in previous implemtation, we forget to set term in read index response
/// which causes panic in violetabft-rs. This test is to reproduce the case.
#[test]
fn test_replica_read_on_stale_peer() {
    let mut cluster = new_node_cluster(0, 3);

    configure_for_lease_read(&mut cluster, Some(50), Some(30));
    let fidel_client = Arc::clone(&cluster.fidel_client);
    fidel_client.disable_default_operator();

    cluster.run();

    let brane = fidel_client.get_brane(b"k1").unwrap();

    let peer_on_store1 = find_peer(&brane, 1).unwrap().to_owned();
    cluster.must_transfer_leader(brane.get_id(), peer_on_store1);
    let peer_on_store3 = find_peer(&brane, 3).unwrap().to_owned();

    cluster.must_put(b"k1", b"v1");
    must_get_equal(&cluster.get_engine(3), b"k1", b"v1");

    let filter = Box::new(
        BranePacketFilter::new(brane.get_id(), 3)
            .direction(Direction::Recv)
            .msg_type(MessageType::MsgApplightlike),
    );
    cluster.sim.wl().add_recv_filter(3, filter);
    cluster.must_put(b"k2", b"v2");
    let resp1_ch = async_read_on_peer(
        &mut cluster,
        peer_on_store3.clone(),
        brane.clone(),
        b"k2",
        true,
        true,
    );
    // must be timeout
    assert!(resp1_ch.recv_timeout(Duration::from_micros(100)).is_err());
}

#[test]
fn test_read_index_out_of_order() {
    let mut cluster = new_node_cluster(0, 2);

    // Use long election timeout and short lease.
    configure_for_lease_read(&mut cluster, Some(1000), Some(10));
    cluster.causet.violetabft_store.violetabft_store_max_leader_lease =
        ReadableDuration(Duration::from_millis(100));

    let fidel_client = Arc::clone(&cluster.fidel_client);
    fidel_client.disable_default_operator();

    let rid = cluster.run_conf_change();
    fidel_client.must_add_peer(rid, new_peer(2, 2));

    cluster.must_put(b"k1", b"v1");
    must_get_equal(&cluster.get_engine(2), b"k1", b"v1");

    cluster.must_transfer_leader(1, new_peer(1, 1));

    let filter = Box::new(
        BranePacketFilter::new(1, 1)
            .direction(Direction::Recv)
            .msg_type(MessageType::MsgHeartbeatResponse),
    );
    cluster.sim.wl().add_recv_filter(1, filter);

    // Can't get read resonse because heartbeat responses are blocked.
    let r1 = cluster.get_brane(b"k1");
    let resp1 = async_read_on_peer(&mut cluster, new_peer(1, 1), r1.clone(), b"k1", true, true);
    assert!(resp1.recv_timeout(Duration::from_secs(2)).is_err());

    fidel_client.must_remove_peer(rid, new_peer(2, 2));

    // After peer 2 is removed, we can get 2 read responses.
    let resp2 = async_read_on_peer(&mut cluster, new_peer(1, 1), r1.clone(), b"k1", true, true);
    assert!(resp2.recv_timeout(Duration::from_secs(1)).is_ok());
    assert!(resp1.recv_timeout(Duration::from_secs(1)).is_ok());
}
