// Copyright 2020 WHTCORPS INC. Licensed under Apache-2.0.

use std::sync::atomic::*;
use std::sync::*;
use std::thread;
use std::time::Duration;

use rand;
use rand::Rng;

use ekvproto::violetabft_cmdpb::VioletaBftCmdResponse;
use violetabft::evioletabftpb::MessageType;

use engine_lmdb::Compat;
use engine_promises::Peekable;
use violetabftstore::router::VioletaBftStoreRouter;
use violetabftstore::store::*;
use violetabftstore::Result;
use rand::RngCore;
use test_violetabftstore::*;
use einsteindb_util::config::*;
use einsteindb_util::HandyRwLock;

fn test_multi_base<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.run();

    test_multi_base_after_bootstrap(cluster);
}

fn test_multi_base_after_bootstrap<T: Simulator>(cluster: &mut Cluster<T>) {
    let (key, value) = (b"k1", b"v1");

    cluster.must_put(key, value);
    assert_eq!(cluster.must_get(key), Some(value.to_vec()));

    let brane_id = cluster.get_brane_id(b"");
    let prev_last_index = cluster.violetabft_local_state(brane_id, 1).get_last_index();

    // sleep 200ms in case the commit packet is dropped by simulated transport.
    thread::sleep(Duration::from_millis(200));

    cluster.assert_quorum(
        |engine| match engine.c().get_value(&tuplespaceInstanton::data_key(key)).unwrap() {
            None => false,
            Some(v) => &*v == value,
        },
    );

    cluster.must_delete(key);
    assert_eq!(cluster.must_get(key), None);

    // sleep 200ms in case the commit packet is dropped by simulated transport.
    thread::sleep(Duration::from_millis(200));

    cluster.assert_quorum(|engine| {
        engine
            .c()
            .get_value(&tuplespaceInstanton::data_key(key))
            .unwrap()
            .is_none()
    });

    let last_index = cluster.violetabft_local_state(brane_id, 1).get_last_index();
    let apply_state = cluster.apply_state(brane_id, 1);
    assert!(apply_state.get_last_commit_index() < last_index);
    assert!(apply_state.get_last_commit_index() >= prev_last_index);
    // TODO add epoch not match test cases.
}

fn test_multi_leader_crash<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.run();

    let (key1, value1) = (b"k1", b"v1");

    cluster.must_put(key1, value1);

    let last_leader = cluster.leader_of_brane(1).unwrap();
    cluster.stop_node(last_leader.get_store_id());

    sleep_ms(800);
    cluster.reset_leader_of_brane(1);
    let new_leader = cluster
        .leader_of_brane(1)
        .expect("leader should be elected.");
    assert_ne!(new_leader, last_leader);

    assert_eq!(cluster.get(key1), Some(value1.to_vec()));

    let (key2, value2) = (b"k2", b"v2");

    cluster.must_put(key2, value2);
    cluster.must_delete(key1);
    must_get_none(
        &cluster.engines[&last_leader.get_store_id()].kv.as_inner(),
        key2,
    );
    must_get_equal(
        &cluster.engines[&last_leader.get_store_id()].kv.as_inner(),
        key1,
        value1,
    );

    // week up
    cluster.run_node(last_leader.get_store_id()).unwrap();

    must_get_equal(
        &cluster.engines[&last_leader.get_store_id()].kv.as_inner(),
        key2,
        value2,
    );
    must_get_none(
        &cluster.engines[&last_leader.get_store_id()].kv.as_inner(),
        key1,
    );
}

fn test_multi_cluster_respacelike<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.run();

    let (key, value) = (b"k1", b"v1");

    assert_eq!(cluster.get(key), None);
    cluster.must_put(key, value);

    assert_eq!(cluster.get(key), Some(value.to_vec()));

    cluster.shutdown();

    // avoid TIMEWAIT
    sleep_ms(500);

    cluster.spacelike().unwrap();

    assert_eq!(cluster.get(key), Some(value.to_vec()));
}

fn test_multi_lost_majority<T: Simulator>(cluster: &mut Cluster<T>, count: usize) {
    cluster.run();

    let half = (count as u64 + 1) / 2;
    for i in 1..=half {
        cluster.stop_node(i);
    }
    if let Some(leader) = cluster.leader_of_brane(1) {
        if leader.get_store_id() > half {
            cluster.stop_node(leader.get_store_id());
        }
    }
    cluster.reset_leader_of_brane(1);
    sleep_ms(600);

    assert!(cluster.leader_of_brane(1).is_none());
}

fn test_multi_random_respacelike<T: Simulator>(
    cluster: &mut Cluster<T>,
    node_count: usize,
    respacelike_count: u32,
) {
    cluster.run();

    let mut rng = rand::thread_rng();
    let mut value = [0u8; 5];

    for i in 1..respacelike_count {
        let id = 1 + rng.gen_cone(0, node_count as u64);
        cluster.stop_node(id);

        let key = i.to_string().into_bytes();

        rng.fill_bytes(&mut value);
        cluster.must_put(&key, &value);
        assert_eq!(cluster.get(&key), Some(value.to_vec()));

        cluster.run_node(id).unwrap();

        // verify whether data is actually being replicated and waiting for node online.
        must_get_equal(&cluster.get_engine(id), &key, &value);

        cluster.must_delete(&key);
        assert_eq!(cluster.get(&key), None);
    }
}

#[test]
fn test_multi_node_base() {
    let count = 5;
    let mut cluster = new_node_cluster(0, count);
    test_multi_base(&mut cluster)
}

fn test_multi_drop_packet<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.run();
    cluster.add_slightlike_filter(CloneFilterFactory(DropPacketFilter::new(30)));
    test_multi_base_after_bootstrap(cluster);
}

#[test]
fn test_multi_node_latency() {
    let count = 5;
    let mut cluster = new_node_cluster(0, count);
    test_multi_latency(&mut cluster);
}

#[test]
fn test_multi_node_drop_packet() {
    let count = 5;
    let mut cluster = new_node_cluster(0, count);
    test_multi_drop_packet(&mut cluster);
}

#[test]
fn test_multi_server_base() {
    let count = 5;
    let mut cluster = new_server_cluster(0, count);
    test_multi_base(&mut cluster)
}

fn test_multi_latency<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.run();
    cluster.add_slightlike_filter(CloneFilterFactory(DelayFilter::new(Duration::from_millis(
        30,
    ))));
    test_multi_base_after_bootstrap(cluster);
}

#[test]
fn test_multi_server_latency() {
    let count = 5;
    let mut cluster = new_server_cluster(0, count);
    test_multi_latency(&mut cluster);
}

fn test_multi_random_latency<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.run();
    cluster.add_slightlike_filter(CloneFilterFactory(RandomLatencyFilter::new(50)));
    test_multi_base_after_bootstrap(cluster);
}

#[test]
fn test_multi_node_random_latency() {
    let count = 5;
    let mut cluster = new_node_cluster(0, count);
    test_multi_random_latency(&mut cluster);
}

#[test]
fn test_multi_server_random_latency() {
    let count = 5;
    let mut cluster = new_server_cluster(0, count);
    test_multi_random_latency(&mut cluster);
}

#[test]
fn test_multi_server_drop_packet() {
    let count = 5;
    let mut cluster = new_server_cluster(0, count);
    test_multi_drop_packet(&mut cluster);
}

#[test]
fn test_multi_node_leader_crash() {
    let count = 5;
    let mut cluster = new_node_cluster(0, count);
    test_multi_leader_crash(&mut cluster)
}

#[test]
fn test_multi_server_leader_crash() {
    let count = 5;
    let mut cluster = new_server_cluster(0, count);
    test_multi_leader_crash(&mut cluster)
}

#[test]
fn test_multi_node_cluster_respacelike() {
    let count = 5;
    let mut cluster = new_node_cluster(0, count);
    test_multi_cluster_respacelike(&mut cluster)
}

#[test]
fn test_multi_server_cluster_respacelike() {
    let count = 5;
    let mut cluster = new_server_cluster(0, count);
    test_multi_cluster_respacelike(&mut cluster)
}

#[test]
fn test_multi_node_lost_majority() {
    let mut tests = vec![4, 5];
    for count in tests.drain(..) {
        let mut cluster = new_node_cluster(0, count);
        test_multi_lost_majority(&mut cluster, count)
    }
}

#[test]
fn test_multi_server_lost_majority() {
    let mut tests = vec![4, 5];
    for count in tests.drain(..) {
        let mut cluster = new_server_cluster(0, count);
        test_multi_lost_majority(&mut cluster, count)
    }
}

#[test]
fn test_multi_node_random_respacelike() {
    let count = 5;
    let mut cluster = new_node_cluster(0, count);
    test_multi_random_respacelike(&mut cluster, count, 10);
}

#[test]
fn test_multi_server_random_respacelike() {
    let count = 5;
    let mut cluster = new_server_cluster(0, count);
    test_multi_random_respacelike(&mut cluster, count, 10);
}

fn test_leader_change_with_uncommitted_log<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.causetg.violetabft_store.violetabft_election_timeout_ticks = 50;
    // disable compact log to make test more sBlock.
    cluster.causetg.violetabft_store.violetabft_log_gc_memory_barrier = 1000;
    // We use three peers([1, 2, 3]) for this test.
    cluster.run();

    sleep_ms(500);

    // guarantee peer 1 is leader
    cluster.must_transfer_leader(1, new_peer(1, 1));

    // So peer 3 won't replicate any message of the brane but still can vote.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 3).msg_type(MessageType::MsgApplightlike),
    ));
    cluster.must_put(b"k1", b"v1");

    // peer 1 and peer 2 must have k2, but peer 3 must not.
    for i in 1..3 {
        let engine = cluster.get_engine(i);
        must_get_equal(&engine, b"k1", b"v1");
    }

    let engine3 = cluster.get_engine(3);
    must_get_none(&engine3, b"k1");

    // now only peer 1 and peer 2 can step to leader.

    // hack: first MsgApplightlike will applightlike log, second MsgApplightlike will set commit index,
    // So only allowing first MsgApplightlike to make peer 2 have uncommitted entries.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 2)
            .msg_type(MessageType::MsgApplightlike)
            .direction(Direction::Recv)
            .allow(1),
    ));
    // Make peer 2 have no way to know the uncommitted entries can be applied
    // when it becomes leader.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 1)
            .msg_type(MessageType::MsgHeartbeatResponse)
            .direction(Direction::Slightlike),
    ));
    // Make peer 2's msg won't be replicated when it becomes leader,
    // so the uncommitted entries won't be applied immediately.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 1)
            .msg_type(MessageType::MsgApplightlike)
            .direction(Direction::Recv),
    ));
    // Make peer 2 have no way to know the uncommitted entries can be applied
    // when it's still follower.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 2)
            .msg_type(MessageType::MsgHeartbeat)
            .direction(Direction::Recv),
    ));
    debug!("putting k2");
    cluster.must_put(b"k2", b"v2");

    // peer 1 must have committed, but peer 2 has not.
    must_get_equal(&cluster.get_engine(1), b"k2", b"v2");

    cluster.must_transfer_leader(1, util::new_peer(2, 2));

    must_get_none(&cluster.get_engine(2), b"k2");

    let brane = cluster.get_brane(b"");
    let reqs = vec![new_put_cmd(b"k3", b"v3")];
    let mut put = new_request(
        brane.get_id(),
        brane.get_brane_epoch().clone(),
        reqs,
        false,
    );
    debug!("requesting: {:?}", put);
    put.mut_header().set_peer(new_peer(2, 2));
    cluster.clear_slightlike_filters();
    let resp = cluster.call_command(put, Duration::from_secs(5)).unwrap();
    assert!(!resp.get_header().has_error(), "{:?}", resp);

    for i in 1..4 {
        must_get_equal(&cluster.get_engine(i), b"k2", b"v2");
        must_get_equal(&cluster.get_engine(i), b"k3", b"v3");
    }
}

#[test]
fn test_node_leader_change_with_uncommitted_log() {
    let mut cluster = new_node_cluster(0, 3);
    test_leader_change_with_uncommitted_log(&mut cluster);
}

#[test]
fn test_server_leader_change_with_uncommitted_log() {
    let mut cluster = new_server_cluster(0, 3);
    test_leader_change_with_uncommitted_log(&mut cluster);
}

#[test]
fn test_node_leader_change_with_log_overlap() {
    let mut cluster = new_node_cluster(0, 3);
    cluster.causetg.violetabft_store.violetabft_election_timeout_ticks = 50;
    // disable compact log to make test more sBlock.
    cluster.causetg.violetabft_store.violetabft_log_gc_memory_barrier = 1000;
    // We use three peers([1, 2, 3]) for this test.
    cluster.run();

    sleep_ms(500);

    // guarantee peer 1 is leader
    cluster.must_transfer_leader(1, new_peer(1, 1));

    // So peer 3 won't replicate any message of the brane but still can vote.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 3).msg_type(MessageType::MsgApplightlike),
    ));
    cluster.must_put(b"k1", b"v1");

    // peer 1 and peer 2 must have k1, but peer 3 must not.
    for i in 1..3 {
        let engine = cluster.get_engine(i);
        must_get_equal(&engine, b"k1", b"v1");
    }

    let engine3 = cluster.get_engine(3);
    must_get_none(&engine3, b"k1");

    // now only peer 1 and peer 2 can step to leader.
    // Make peer 1's msg won't be replicated,
    // so the proposed entries won't be committed.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 1)
            .msg_type(MessageType::MsgApplightlike)
            .direction(Direction::Slightlike),
    ));
    let put_msg = vec![new_put_cmd(b"k2", b"v2")];
    let brane = cluster.get_brane(b"");
    let mut put_req = new_request(
        brane.get_id(),
        brane.get_brane_epoch().clone(),
        put_msg,
        false,
    );
    put_req.mut_header().set_peer(new_peer(1, 1));
    let called = Arc::new(AtomicBool::new(false));
    let called_ = Arc::clone(&called);
    cluster
        .sim
        .rl()
        .get_node_router(1)
        .slightlike_command(
            put_req,
            Callback::Write(Box::new(move |resp: WriteResponse| {
                called_.store(true, Ordering::SeqCst);
                assert!(resp.response.get_header().has_error());
                assert!(resp.response.get_header().get_error().has_stale_command());
            })),
        )
        .unwrap();

    // Now let peer(1, 1) steps down. Can't use transfer leader here, because
    // it still has plightlikeing proposed entries.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 1)
            .msg_type(MessageType::MsgHeartbeat)
            .direction(Direction::Slightlike),
    ));
    // make sure k2 has not been committed.
    must_get_none(&cluster.get_engine(1), b"k2");

    // Here just use `must_transfer_leader` to wait for peer (2, 2) becomes leader.
    cluster.must_transfer_leader(1, new_peer(2, 2));

    must_get_none(&cluster.get_engine(2), b"k2");

    cluster.clear_slightlike_filters();

    for _ in 0..50 {
        sleep_ms(100);
        if called.load(Ordering::SeqCst) {
            return;
        }
    }
    panic!("callback has not been called after 5s.");
}

fn test_read_leader_with_unapplied_log<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.causetg.violetabft_store.violetabft_election_timeout_ticks = 50;
    // disable compact log to make test more sBlock.
    cluster.causetg.violetabft_store.violetabft_log_gc_memory_barrier = 1000;
    // We use three peers([1, 2, 3]) for this test.
    cluster.run();

    sleep_ms(500);

    // guarantee peer 1 is leader
    cluster.must_transfer_leader(1, new_peer(1, 1));

    // if peer 2 is unreachable, leader will not slightlike MsgApplightlike to peer 2, and the leader will
    // slightlike MsgApplightlike with committed information to peer 2 after network recovered, and peer 2
    // will apply the entry regardless of we add an filter, so we put k0/v0 to make sure the
    // network is reachable.
    let (k0, v0) = (b"k0", b"v0");
    cluster.must_put(k0, v0);

    for i in 1..4 {
        must_get_equal(&cluster.get_engine(i), k0, v0);
    }

    // hack: first MsgApplightlike will applightlike log, second MsgApplightlike will set commit index,
    // So only allowing first MsgApplightlike to make peer 2 have uncommitted entries.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 2)
            .msg_type(MessageType::MsgApplightlike)
            .direction(Direction::Recv)
            .allow(1),
    ));

    // Make peer 2's msg won't be replicated when it becomes leader,
    // so the uncommitted entries won't be applied immediately.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 2)
            .msg_type(MessageType::MsgApplightlike)
            .direction(Direction::Slightlike),
    ));

    // Make peer 2 have no way to know the uncommitted entries can be applied
    // when it's still follower.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 2)
            .msg_type(MessageType::MsgHeartbeat)
            .direction(Direction::Recv),
    ));

    let (k, v) = (b"k", b"v");
    cluster.must_put(k, v);

    // peer 1 must have committed, but peer 2 has not.
    must_get_equal(&cluster.get_engine(1), k, v);

    cluster.must_transfer_leader(1, util::new_peer(2, 2));

    // leader's term not equal applied index's term, if we read local, we may get old value
    // in this situation we need use violetabft read
    must_get_none(&cluster.get_engine(2), k);

    // internal read will use violetabft read no matter read_quorum is false or true, cause applied
    // index's term not equal leader's term, and will failed with timeout
    let req = get_with_timeout(cluster, k, false, Duration::from_secs(10)).unwrap();
    assert!(
        req.get_header().get_error().has_stale_command(),
        "read should be dropped immediately, but got {:?}",
        req
    );

    // recover network
    cluster.clear_slightlike_filters();

    assert_eq!(cluster.get(k).unwrap(), v);
}

#[test]
fn test_node_read_leader_with_unapplied_log() {
    let mut cluster = new_node_cluster(0, 3);
    test_read_leader_with_unapplied_log(&mut cluster);
}

#[test]
fn test_server_read_leader_with_unapplied_log() {
    let mut cluster = new_server_cluster(0, 3);
    test_read_leader_with_unapplied_log(&mut cluster);
}

fn get_with_timeout<T: Simulator>(
    cluster: &mut Cluster<T>,
    key: &[u8],
    read_quorum: bool,
    timeout: Duration,
) -> Result<VioletaBftCmdResponse> {
    let mut brane = cluster.get_brane(key);
    let brane_id = brane.get_id();
    let req = new_request(
        brane_id,
        brane.take_brane_epoch(),
        vec![new_get_cmd(key)],
        read_quorum,
    );
    cluster.call_command_on_leader(req, timeout)
}

fn test_remove_leader_with_uncommitted_log<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.causetg.violetabft_store.violetabft_election_timeout_ticks = 50;
    // disable compact log to make test more sBlock.
    cluster.causetg.violetabft_store.violetabft_log_gc_memory_barrier = 1000;
    // We use three peers([1, 2, 3]) for this test.
    cluster.run();

    cluster.must_put(b"k1", b"v1");

    // guarantee peer 1 is leader
    cluster.must_transfer_leader(1, new_peer(1, 1));

    // stop peer 2 replicate messages.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 2)
            .msg_type(MessageType::MsgApplightlike)
            .direction(Direction::Recv),
    ));
    // peer 2 can't step to leader.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 2)
            .msg_type(MessageType::MsgRequestVote)
            .direction(Direction::Slightlike),
    ));

    let fidel_client = Arc::clone(&cluster.fidel_client);
    fidel_client.remove_peer(1, new_peer(1, 1));

    // wait for the leader receive the remove order.
    sleep_ms(1000);

    let brane = cluster.get_brane(b"");
    let reqs = vec![new_put_cmd(b"k3", b"v3")];
    let mut put = new_request(
        brane.get_id(),
        brane.get_brane_epoch().clone(),
        reqs,
        false,
    );
    debug!("requesting: {:?}", put);
    put.mut_header().set_peer(new_peer(1, 1));
    cluster.clear_slightlike_filters();
    let resp = cluster.call_command(put, Duration::from_secs(5)).unwrap();
    assert!(resp.get_header().has_error());
    assert!(
        resp.get_header().get_error().has_brane_not_found(),
        "{:?} should have brane not found",
        resp
    );
}

#[test]
fn test_node_remove_leader_with_uncommitted_log() {
    let mut cluster = new_node_cluster(0, 2);
    test_remove_leader_with_uncommitted_log(&mut cluster);
}

#[test]
fn test_server_remove_leader_with_uncommitted_log() {
    let mut cluster = new_server_cluster(0, 2);
    test_remove_leader_with_uncommitted_log(&mut cluster);
}

// In some rare cases, a proposal may be dropped inside violetabft silently.
// We need to make sure the callback of dropped proposal should be cleaned
// up eventually.
#[test]
fn test_node_dropped_proposal() {
    let mut cluster = new_node_cluster(0, 3);
    cluster.causetg.violetabft_store.violetabft_election_timeout_ticks = 50;
    // disable compact log to make test more sBlock.
    cluster.causetg.violetabft_store.violetabft_log_gc_memory_barrier = 1000;
    // We use three peers([1, 2, 3]) for this test.
    cluster.run();

    // make sure peer 1 is leader
    cluster.must_transfer_leader(1, new_peer(1, 1));

    // so peer 3 won't have latest messages, it can't become leader.
    cluster.add_slightlike_filter(CloneFilterFactory(
        BranePacketFilter::new(1, 3).msg_type(MessageType::MsgApplightlike),
    ));
    cluster.must_put(b"k1", b"v1");

    // peer 1 and peer 2 must have k1, but peer 3 must not.
    for i in 1..3 {
        let engine = cluster.get_engine(i);
        must_get_equal(&engine, b"k1", b"v1");
    }

    let engine3 = cluster.get_engine(3);
    must_get_none(&engine3, b"k1");

    let put_msg = vec![new_put_cmd(b"k2", b"v2")];
    let brane = cluster.get_brane(b"");
    let mut put_req = new_request(
        brane.get_id(),
        brane.get_brane_epoch().clone(),
        put_msg,
        false,
    );
    put_req.mut_header().set_peer(new_peer(1, 1));
    // peer (3, 3) won't become leader and transfer leader request will be canceled
    // after about an election timeout. Before it's canceled, all proposal will be dropped
    // silently.
    cluster.transfer_leader(1, new_peer(3, 3));

    let (tx, rx) = mpsc::channel();
    cluster
        .sim
        .rl()
        .get_node_router(1)
        .slightlike_command(
            put_req,
            Callback::Write(Box::new(move |resp: WriteResponse| {
                let _ = tx.slightlike(resp.response);
            })),
        )
        .unwrap();

    // Although proposal is dropped, callback should be cleaned up in time.
    rx.recv_timeout(Duration::from_secs(5))
        .expect("callback should have been called with in 5s.");
}

fn test_consistency_check<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.causetg.violetabft_store.violetabft_election_timeout_ticks = 50;
    // disable compact log to make test more sBlock.
    cluster.causetg.violetabft_store.violetabft_log_gc_memory_barrier = 1000;
    cluster.causetg.violetabft_store.consistency_check_interval = ReadableDuration::secs(1);
    // We use three peers([1, 2, 3]) for this test.
    cluster.run();

    for i in 0..300 {
        cluster.must_put(
            format!("k{:06}", i).as_bytes(),
            format!("k{:06}", i).as_bytes(),
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn test_node_consistency_check() {
    let mut cluster = new_node_cluster(0, 2);
    test_consistency_check(&mut cluster);
}

fn test_batch_write<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.run();
    let r = cluster.get_brane(b"");
    cluster.must_split(&r, b"k3");
    // ufidelate epoch.
    let r = cluster.get_brane(b"");

    let req = new_request(
        r.get_id(),
        r.get_brane_epoch().clone(),
        vec![new_put_cmd(b"k1", b"v1"), new_put_cmd(b"k2", b"v2")],
        false,
    );
    let resp = cluster
        .call_command_on_leader(req, Duration::from_secs(3))
        .unwrap();
    assert!(!resp.get_header().has_error());
    assert_eq!(cluster.must_get(b"k1"), Some(b"v1".to_vec()));
    assert_eq!(cluster.must_get(b"k2"), Some(b"v2".to_vec()));

    let req = new_request(
        r.get_id(),
        r.get_brane_epoch().clone(),
        vec![new_put_cmd(b"k1", b"v3"), new_put_cmd(b"k3", b"v3")],
        false,
    );
    let resp = cluster
        .call_command_on_leader(req, Duration::from_secs(3))
        .unwrap();
    assert!(resp.get_header().has_error());
    assert_eq!(cluster.must_get(b"k1"), Some(b"v1".to_vec()));
    assert_eq!(cluster.must_get(b"k3"), None);
}

#[test]
fn test_server_batch_write() {
    let mut cluster = new_server_cluster(0, 3);
    test_batch_write(&mut cluster);
}

// Tests whether logs are catch up quickly.
#[test]
fn test_node_catch_up_logs() {
    let mut cluster = new_node_cluster(0, 3);
    cluster.causetg.violetabft_store.violetabft_base_tick_interval = ReadableDuration::millis(500);
    cluster.causetg.violetabft_store.violetabft_max_size_per_msg = ReadableSize(5);
    cluster.causetg.violetabft_store.violetabft_election_timeout_ticks = 50;
    cluster.causetg.violetabft_store.max_leader_missing_duration = ReadableDuration::hours(1);
    cluster.causetg.violetabft_store.peer_stale_state_check_interval = ReadableDuration::minutes(30);
    cluster.causetg.violetabft_store.abnormal_leader_missing_duration = ReadableDuration::hours(1);
    // disable compact log to make test more sBlock.
    cluster.causetg.violetabft_store.violetabft_log_gc_memory_barrier = 3000;
    cluster.fidel_client.disable_default_operator();
    // We use three peers([1, 2, 3]) for this test.
    let r1 = cluster.run_conf_change();
    cluster.fidel_client.must_add_peer(r1, new_peer(2, 2));
    cluster.fidel_client.must_add_peer(r1, new_peer(3, 3));

    cluster.must_put(b"k1", b"v1");
    must_get_equal(&cluster.get_engine(3), b"k1", b"v1");
    cluster.stop_node(3);
    for i in 0..10 {
        let v = format!("{:04}", i);
        cluster.async_put(v.as_bytes(), v.as_bytes()).unwrap();
    }
    must_get_equal(&cluster.get_engine(1), b"0009", b"0009");
    cluster.run_node(3).unwrap();
    must_get_equal(&cluster.get_engine(3), b"0009", b"0009");
}
