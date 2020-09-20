// Copyright 2020 EinsteinDB Project Authors. Licensed under Apache-2.0.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::*;
use std::time::Duration;

use concurrency_manager::ConcurrencyManager;
use engine_lmdb::LmdbEngine;
use futures::executor::block_on;
use futures::StreamExt;
use grpcio::{ChannelBuilder, Environment};
use grpcio::{ClientDuplexReceiver, ClientDuplexSlightlikeer, ClientUnaryReceiver};
use ekvproto::cdcpb::{create_change_data, ChangeDataClient, ChangeDataEvent, ChangeDataRequest};
use ekvproto::kvrpcpb::*;
use ekvproto::einsteindbpb::EINSTEINDBClient;
use violetabftstore::interlock::InterlockHost;
use security::*;
use test_violetabftstore::*;
use einsteindb::config::CdcConfig;
use einsteindb_util::collections::HashMap;
use einsteindb_util::worker::Worker;
use einsteindb_util::HandyRwLock;
use txn_types::TimeStamp;

use cdc::{CdcObserver, Task};
static INIT: Once = Once::new();

pub fn init() {
    INIT.call_once(test_util::setup_for_ci);
}

#[allow(clippy::type_complexity)]
pub fn new_event_feed(
    client: &ChangeDataClient,
) -> (
    ClientDuplexSlightlikeer<ChangeDataRequest>,
    Rc<Cell<Option<ClientDuplexReceiver<ChangeDataEvent>>>>,
    impl Fn(bool) -> ChangeDataEvent,
) {
    let (req_tx, resp_rx) = client.event_feed().unwrap();
    let event_feed_wrap = Rc::new(Cell::new(Some(resp_rx)));
    let event_feed_wrap_clone = event_feed_wrap.clone();

    let receive_event = move |keep_resolved_ts: bool| loop {
        let event_feed = event_feed_wrap_clone.as_ref();
        let mut events = event_feed.replace(None).unwrap();
        let change_data = block_on(events.next());
        event_feed.set(Some(events));
        let change_data_event = change_data.unwrap().unwrap();
        if !keep_resolved_ts && change_data_event.has_resolved_ts() {
            continue;
        }
        einsteindb_util::info!("receive event {:?}", change_data_event);
        break change_data_event;
    };
    (req_tx, event_feed_wrap, receive_event)
}

pub struct TestSuite {
    pub cluster: Cluster<ServerCluster>,
    pub lightlikepoints: HashMap<u64, Worker<Task>>,
    pub obs: HashMap<u64, CdcObserver>,
    einsteindb_cli: HashMap<u64, EINSTEINDBClient>,
    cdc_cli: HashMap<u64, ChangeDataClient>,
    concurrency_managers: HashMap<u64, ConcurrencyManager>,

    env: Arc<Environment>,
}

impl TestSuite {
    pub fn new(count: usize) -> TestSuite {
        let mut cluster = new_server_cluster(1, count);
        // Increase the VioletaBft tick interval to make this test case running reliably.
        configure_for_lease_read(&mut cluster, Some(100), None);
        Self::with_cluster(count, cluster)
    }

    pub fn with_cluster(count: usize, mut cluster: Cluster<ServerCluster>) -> TestSuite {
        init();
        let fidel_cli = cluster.fidel_client.clone();
        let mut lightlikepoints = HashMap::default();
        let mut obs = HashMap::default();
        let mut concurrency_managers = HashMap::default();
        // Hack! node id are generated from 1..count+1.
        for id in 1..=count as u64 {
            // Create and run cdc lightlikepoints.
            let worker = Worker::new(format!("cdc-{}", id));
            let mut sim = cluster.sim.wl();

            // Register cdc service to gRPC server.
            let security_mgr = Arc::new(SecurityManager::new(&SecurityConfig::default()).unwrap());
            let scheduler = worker.scheduler();
            sim.plightlikeing_services
                .entry(id)
                .or_default()
                .push(Box::new(move || {
                    create_change_data(cdc::Service::new(scheduler.clone(), security_mgr.clone()))
                }));
            let scheduler = worker.scheduler();
            let cdc_ob = cdc::CdcObserver::new(scheduler.clone());
            obs.insert(id, cdc_ob.clone());
            sim.interlock_hooks.entry(id).or_default().push(Box::new(
                move |host: &mut InterlockHost<LmdbEngine>| {
                    cdc_ob.register_to(host);
                },
            ));
            lightlikepoints.insert(id, worker);
        }

        cluster.run();
        for (id, worker) in &mut lightlikepoints {
            let sim = cluster.sim.wl();
            let raft_router = sim.get_server_router(*id);
            let cdc_ob = obs.get(&id).unwrap().clone();
            let cm = ConcurrencyManager::new(1.into());
            let mut cdc_lightlikepoint = cdc::Endpoint::new(
                &CdcConfig::default(),
                fidel_cli.clone(),
                worker.scheduler(),
                raft_router,
                cdc_ob,
                cluster.store_metas[id].clone(),
                cm.clone(),
            );
            cdc_lightlikepoint.set_min_ts_interval(Duration::from_millis(100));
            cdc_lightlikepoint.set_scan_batch_size(2);
            concurrency_managers.insert(*id, cm);
            worker.spacelike(cdc_lightlikepoint).unwrap();
        }

        TestSuite {
            cluster,
            lightlikepoints,
            obs,
            concurrency_managers,
            env: Arc::new(Environment::new(1)),
            einsteindb_cli: HashMap::default(),
            cdc_cli: HashMap::default(),
        }
    }

    pub fn stop(mut self) {
        for (_, mut worker) in self.lightlikepoints {
            worker.stop().unwrap().join().unwrap();
        }
        self.cluster.shutdown();
    }

    pub fn new_changedata_request(&mut self, brane_id: u64) -> ChangeDataRequest {
        let mut req = ChangeDataRequest::default();
        req.brane_id = brane_id;
        req.set_brane_epoch(self.get_context(brane_id).take_brane_epoch());
        // Assume batch resolved ts will be release in v4.0.7
        // For easy of testing (nightly CI), we lower the gate to v4.0.6
        // TODO bump the version when cherry pick to release branch.
        req.mut_header().set_ticdc_version("4.0.6".into());
        req
    }

    pub fn must_kv_prewrite(
        &mut self,
        brane_id: u64,
        muts: Vec<Mutation>,
        pk: Vec<u8>,
        ts: TimeStamp,
    ) {
        let mut prewrite_req = PrewriteRequest::default();
        prewrite_req.set_context(self.get_context(brane_id));
        prewrite_req.set_mutations(muts.into_iter().collect());
        prewrite_req.primary_lock = pk;
        prewrite_req.spacelike_version = ts.into_inner();
        prewrite_req.lock_ttl = prewrite_req.spacelike_version + 1;
        let prewrite_resp = self
            .get_einsteindb_client(brane_id)
            .kv_prewrite(&prewrite_req)
            .unwrap();
        assert!(
            !prewrite_resp.has_brane_error(),
            "{:?}",
            prewrite_resp.get_brane_error()
        );
        assert!(
            prewrite_resp.errors.is_empty(),
            "{:?}",
            prewrite_resp.get_errors()
        );
    }

    pub fn must_kv_commit(
        &mut self,
        brane_id: u64,
        tuplespaceInstanton: Vec<Vec<u8>>,
        spacelike_ts: TimeStamp,
        commit_ts: TimeStamp,
    ) {
        let mut commit_req = CommitRequest::default();
        commit_req.set_context(self.get_context(brane_id));
        commit_req.spacelike_version = spacelike_ts.into_inner();
        commit_req.set_tuplespaceInstanton(tuplespaceInstanton.into_iter().collect());
        commit_req.commit_version = commit_ts.into_inner();
        let commit_resp = self
            .get_einsteindb_client(brane_id)
            .kv_commit(&commit_req)
            .unwrap();
        assert!(
            !commit_resp.has_brane_error(),
            "{:?}",
            commit_resp.get_brane_error()
        );
        assert!(!commit_resp.has_error(), "{:?}", commit_resp.get_error());
    }

    pub fn must_kv_rollback(&mut self, brane_id: u64, tuplespaceInstanton: Vec<Vec<u8>>, spacelike_ts: TimeStamp) {
        let mut rollback_req = BatchRollbackRequest::default();
        rollback_req.set_context(self.get_context(brane_id));
        rollback_req.spacelike_version = spacelike_ts.into_inner();
        rollback_req.set_tuplespaceInstanton(tuplespaceInstanton.into_iter().collect());
        let rollback_resp = self
            .get_einsteindb_client(brane_id)
            .kv_batch_rollback(&rollback_req)
            .unwrap();
        assert!(
            !rollback_resp.has_brane_error(),
            "{:?}",
            rollback_resp.get_brane_error()
        );
        assert!(
            !rollback_resp.has_error(),
            "{:?}",
            rollback_resp.get_error()
        );
    }

    pub fn async_kv_commit(
        &mut self,
        brane_id: u64,
        tuplespaceInstanton: Vec<Vec<u8>>,
        spacelike_ts: TimeStamp,
        commit_ts: TimeStamp,
    ) -> ClientUnaryReceiver<CommitResponse> {
        let mut commit_req = CommitRequest::default();
        commit_req.set_context(self.get_context(brane_id));
        commit_req.spacelike_version = spacelike_ts.into_inner();
        commit_req.set_tuplespaceInstanton(tuplespaceInstanton.into_iter().collect());
        commit_req.commit_version = commit_ts.into_inner();
        self.get_einsteindb_client(brane_id)
            .kv_commit_async(&commit_req)
            .unwrap()
    }

    pub fn get_context(&mut self, brane_id: u64) -> Context {
        let epoch = self.cluster.get_brane_epoch(brane_id);
        let leader = self.cluster.leader_of_brane(brane_id).unwrap();
        let mut context = Context::default();
        context.set_brane_id(brane_id);
        context.set_peer(leader);
        context.set_brane_epoch(epoch);
        context
    }

    pub fn get_einsteindb_client(&mut self, brane_id: u64) -> &EINSTEINDBClient {
        let leader = self.cluster.leader_of_brane(brane_id).unwrap();
        let store_id = leader.get_store_id();
        let addr = self.cluster.sim.rl().get_addr(store_id).to_owned();
        let env = self.env.clone();
        self.einsteindb_cli
            .entry(leader.get_store_id())
            .or_insert_with(|| {
                let channel = ChannelBuilder::new(env).connect(&addr);
                EINSTEINDBClient::new(channel)
            })
    }

    pub fn get_brane_cdc_client(&mut self, brane_id: u64) -> &ChangeDataClient {
        let leader = self.cluster.leader_of_brane(brane_id).unwrap();
        let store_id = leader.get_store_id();
        let addr = self.cluster.sim.rl().get_addr(store_id).to_owned();
        let env = self.env.clone();
        self.cdc_cli.entry(store_id).or_insert_with(|| {
            let channel = ChannelBuilder::new(env)
                .max_receive_message_len(std::i32::MAX)
                .connect(&addr);
            ChangeDataClient::new(channel)
        })
    }

    pub fn get_store_cdc_client(&mut self, store_id: u64) -> &ChangeDataClient {
        let addr = self.cluster.sim.rl().get_addr(store_id).to_owned();
        let env = self.env.clone();
        self.cdc_cli.entry(store_id).or_insert_with(|| {
            let channel = ChannelBuilder::new(env).connect(&addr);
            ChangeDataClient::new(channel)
        })
    }

    pub fn get_txn_concurrency_manager(&self, store_id: u64) -> Option<ConcurrencyManager> {
        self.concurrency_managers.get(&store_id).cloned()
    }

    pub fn set_tso(&self, ts: impl Into<TimeStamp>) {
        self.cluster.fidel_client.set_tso(ts.into());
    }
}
