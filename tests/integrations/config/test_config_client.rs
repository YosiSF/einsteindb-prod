// Copyright 2019 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

use configuration::{ConfigChange, Configuration};
use violetabftstore::store::Config as VioletaBftstoreConfig;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use einsteindb::config::*;

fn change(name: &str, value: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(name.to_owned(), value.to_owned());
    m
}

#[test]
fn test_ufidelate_config() {
    let (causetg, _dir) = EINSTEINDBConfig::with_tmp().unwrap();
    let causetg_controller = ConfigController::new(causetg);
    let mut causetg = causetg_controller.get_current().clone();

    // normal ufidelate
    causetg_controller
        .ufidelate(change("violetabftstore.violetabft-log-gc-memory_barrier", "2000"))
        .unwrap();
    causetg.raft_store.raft_log_gc_memory_barrier = 2000;
    assert_eq!(causetg_controller.get_current(), causetg);

    // ufidelate not support config
    let res = causetg_controller.ufidelate(change("server.addr", "localhost:3000"));
    assert!(res.is_err());
    assert_eq!(causetg_controller.get_current(), causetg);

    // ufidelate to invalid config
    let res = causetg_controller.ufidelate(change("violetabftstore.violetabft-log-gc-memory_barrier", "0"));
    assert!(res.is_err());
    assert_eq!(causetg_controller.get_current(), causetg);

    // bad ufidelate request
    let res = causetg_controller.ufidelate(change("xxx.yyy", "0"));
    assert!(res.is_err());
    let res = causetg_controller.ufidelate(change("violetabftstore.xxx", "0"));
    assert!(res.is_err());
    let res = causetg_controller.ufidelate(change("violetabftstore.violetabft-log-gc-memory_barrier", "10MB"));
    assert!(res.is_err());
    let res = causetg_controller.ufidelate(change("violetabft-log-gc-memory_barrier", "10MB"));
    assert!(res.is_err());
    assert_eq!(causetg_controller.get_current(), causetg);
}

#[test]
fn test_dispatch_change() {
    use configuration::ConfigManager;
    use std::error::Error;
    use std::result::Result;

    #[derive(Clone)]
    struct CfgManager(Arc<Mutex<VioletaBftstoreConfig>>);

    impl ConfigManager for CfgManager {
        fn dispatch(&mut self, c: ConfigChange) -> Result<(), Box<dyn Error>> {
            self.0.lock().unwrap().ufidelate(c);
            Ok(())
        }
    }

    let (causetg, _dir) = EINSTEINDBConfig::with_tmp().unwrap();
    let causetg_controller = ConfigController::new(causetg);
    let mut causetg = causetg_controller.get_current().clone();
    let mgr = CfgManager(Arc::new(Mutex::new(causetg.raft_store.clone())));
    causetg_controller.register(Module::VioletaBftstore, Box::new(mgr.clone()));

    causetg_controller
        .ufidelate(change("violetabftstore.violetabft-log-gc-memory_barrier", "2000"))
        .unwrap();

    // config ufidelate
    causetg.raft_store.raft_log_gc_memory_barrier = 2000;
    assert_eq!(causetg_controller.get_current(), causetg);

    // config change should also dispatch to violetabftstore config manager
    assert_eq!(mgr.0.lock().unwrap().raft_log_gc_memory_barrier, 2000);
}

#[test]
fn test_write_ufidelate_to_file() {
    let (mut causetg, tmp_dir) = EINSTEINDBConfig::with_tmp().unwrap();
    causetg.causetg_path = tmp_dir.path().join("causetg_file").to_str().unwrap().to_owned();
    {
        let c = r#"
## comment should be reserve
[violetabftstore]

# config that comment out by one `#` should be ufidelate in place
## fidel-heartbeat-tick-interval = "30s"
# fidel-heartbeat-tick-interval = "30s"

[rocksdb.defaultcauset]
## config should be ufidelate in place
block-cache-size = "10GB"

[rocksdb.lockcauset]
## this config will not ufidelate even it has the same last 
## name as `rocksdb.defaultcauset.block-cache-size`
block-cache-size = "512MB"

[interlock]
## the ufidelate to `interlock.brane-split-tuplespaceInstanton`, which do not show up
## as key-value pair after [interlock], will be written at the lightlike of [interlock]

[gc]
## config should be ufidelate in place
max-write-bytes-per-sec = "1KB"

[rocksdb.defaultcauset.titan]
blob-run-mode = "normal"
"#;
        let mut f = File::create(&causetg.causetg_path).unwrap();
        f.write_all(c.as_bytes()).unwrap();
        f.sync_all().unwrap();
    }
    let causetg_controller = ConfigController::new(causetg);
    let change = {
        let mut change = HashMap::new();
        change.insert(
            "violetabftstore.fidel-heartbeat-tick-interval".to_owned(),
            "1h".to_owned(),
        );
        change.insert(
            "interlock.brane-split-tuplespaceInstanton".to_owned(),
            "10000".to_owned(),
        );
        change.insert("gc.max-write-bytes-per-sec".to_owned(), "100MB".to_owned());
        change.insert(
            "rocksdb.defaultcauset.block-cache-size".to_owned(),
            "1GB".to_owned(),
        );
        change.insert(
            "rocksdb.defaultcauset.titan.blob-run-mode".to_owned(),
            "read-only".to_owned(),
        );
        change
    };
    causetg_controller.ufidelate(change).unwrap();
    let res = {
        let mut buf = Vec::new();
        let mut f = File::open(&causetg_controller.get_current().causetg_path).unwrap();
        f.read_to_lightlike(&mut buf).unwrap();
        buf
    };

    let expect = r#"
## comment should be reserve
[violetabftstore]

# config that comment out by one `#` should be ufidelate in place
## fidel-heartbeat-tick-interval = "30s"
fidel-heartbeat-tick-interval = "1h"

[rocksdb.defaultcauset]
## config should be ufidelate in place
block-cache-size = "1GB"

[rocksdb.lockcauset]
## this config will not ufidelate even it has the same last 
## name as `rocksdb.defaultcauset.block-cache-size`
block-cache-size = "512MB"

[interlock]
## the ufidelate to `interlock.brane-split-tuplespaceInstanton`, which do not show up
## as key-value pair after [interlock], will be written at the lightlike of [interlock]

brane-split-tuplespaceInstanton = 10000
[gc]
## config should be ufidelate in place
max-write-bytes-per-sec = "100MB"

[rocksdb.defaultcauset.titan]
blob-run-mode = "read-only"
"#;
    assert_eq!(expect.as_bytes(), res.as_slice());
}
