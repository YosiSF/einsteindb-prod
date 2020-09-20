// Copyright 2020 EinsteinDB Project Authors. Licensed under Apache-2.0.

use grpcio::{ChannelBuilder, Environment};
use ekvproto::kvrpcpb::*;
use ekvproto::einsteindbpb::EINSTEINDBClient;
use std::sync::Arc;
use test_violetabftstore::new_server_cluster;
use einsteindb_util::collections::HashSet;
use einsteindb_util::HandyRwLock;

#[test]
fn test_check_cn_success() {
    let mut cluster = new_server_cluster(0, 1);
    let mut allowed_cn = HashSet::default();
    allowed_cn.insert("einsteindb-server".to_owned());
    cluster.causetg.security = test_util::new_security_causetg(Some(allowed_cn));
    cluster.run();

    let leader = cluster.get_brane(b"").get_peers()[0].clone();
    let addr = cluster.sim.rl().get_addr(leader.get_store_id()).to_owned();

    let env = Arc::new(Environment::new(1));
    let cred = test_util::new_channel_cred();
    let channel = ChannelBuilder::new(env).secure_connect(&addr, cred);

    let client = EINSTEINDBClient::new(channel);
    let status = client.kv_get(&GetRequest::default());
    assert!(status.is_ok());
}

#[test]
fn test_check_cn_fail() {
    let mut cluster = new_server_cluster(0, 1);
    let mut allowed_cn = HashSet::default();
    allowed_cn.insert("invaild-server".to_owned());
    cluster.causetg.security = test_util::new_security_causetg(Some(allowed_cn));
    cluster.run();

    let leader = cluster.get_brane(b"").get_peers()[0].clone();
    let addr = cluster.sim.rl().get_addr(leader.get_store_id()).to_owned();

    let env = Arc::new(Environment::new(1));
    let cred = test_util::new_channel_cred();
    let channel = ChannelBuilder::new(env).secure_connect(&addr, cred);

    let client = EINSTEINDBClient::new(channel);
    let status = client.kv_get(&GetRequest::default());
    assert!(status.is_err());
}
