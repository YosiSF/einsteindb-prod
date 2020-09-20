// Copyright 2020 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

use std::sync::Arc;
use std::{thread, time};

use engine_promises::MiscExt;
use fidel_client::FidelClient;
use test_violetabftstore::*;
use einsteindb_util::config::*;

fn flush<T: Simulator>(cluster: &mut Cluster<T>) {
    for engines in cluster.engines.values() {
        engines.kv.flush(true).unwrap();
    }
}

fn test_ufidelate_brane_size<T: Simulator>(cluster: &mut Cluster<T>) {
    cluster.causetg.raft_store.fidel_heartbeat_tick_interval = ReadableDuration::millis(50);
    cluster.causetg.raft_store.split_brane_check_tick_interval = ReadableDuration::millis(50);
    cluster.causetg.raft_store.brane_split_check_diff = ReadableSize::kb(1);
    cluster
        .causetg
        .rocksdb
        .defaultcauset
        .level0_file_num_compaction_trigger = 10;
    cluster.spacelike().unwrap();

    for _ in 0..2 {
        for i in 0..1000 {
            let (k, v) = (format!("k{}", i), format!("value{}", i));
            cluster.must_put(k.as_bytes(), v.as_bytes());
        }
        flush(cluster);
        for i in 1000..2000 {
            let (k, v) = (format!("k{}", i), format!("value{}", i));
            cluster.must_put(k.as_bytes(), v.as_bytes());
        }
        flush(cluster);
        for i in 2000..3000 {
            let (k, v) = (format!("k{}", i), format!("value{}", i));
            cluster.must_put(k.as_bytes(), v.as_bytes());
        }
        flush(cluster);
    }

    // Make sure there are multiple branes, so it will cover all cases of
    // function `violetabftstore.on_compaction_finished`.
    let fidel_client = Arc::clone(&cluster.fidel_client);
    let brane = fidel_client.get_brane(b"").unwrap();
    cluster.must_split(&brane, b"k2000");

    thread::sleep(time::Duration::from_millis(300));
    let brane_id = cluster.get_brane_id(b"");
    let old_brane_size = cluster
        .fidel_client
        .get_brane_approximate_size(brane_id)
        .unwrap();

    cluster.compact_data();

    thread::sleep(time::Duration::from_millis(300));
    let new_brane_size = cluster
        .fidel_client
        .get_brane_approximate_size(brane_id)
        .unwrap();

    assert_ne!(old_brane_size, new_brane_size);
}

#[test]
fn test_server_ufidelate_brane_size() {
    let count = 1;
    let mut cluster = new_server_cluster(0, count);
    test_ufidelate_brane_size(&mut cluster);
}
