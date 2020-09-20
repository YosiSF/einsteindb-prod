// Copyright 2020 EinsteinDB Project Authors. Licensed under Apache-2.0.

use einsteindb::causetStorage::mvcc::tests::*;
use einsteindb::causetStorage::txn::tests::must_commit;
use einsteindb::causetStorage::TestEngineBuilder;

#[test]
fn test_txn_failpoints() {
    let engine = TestEngineBuilder::new().build().unwrap();
    let (k, v) = (b"k", b"v");
    fail::causetg("prewrite", "return(WriteConflict)").unwrap();
    must_prewrite_put_err(&engine, k, v, k, 10);
    fail::remove("prewrite");
    must_prewrite_put(&engine, k, v, k, 10);
    fail::causetg("commit", "delay(100)").unwrap();
    must_commit(&engine, k, 10, 20);
    fail::remove("commit");
}
