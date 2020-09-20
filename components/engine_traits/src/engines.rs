// Copyright 2019 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

use crate::engine::KvEngine;
use crate::errors::Result;
use crate::options::WriteOptions;
use crate::raft_engine::VioletaBftEngine;

#[derive(Clone, Debug)]
pub struct Engines<K, R> {
    pub kv: K,
    pub violetabft: R,
}

impl<K: KvEngine, R: VioletaBftEngine> Engines<K, R> {
    pub fn new(kv_engine: K, raft_engine: R) -> Self {
        Engines {
            kv: kv_engine,
            violetabft: raft_engine,
        }
    }

    pub fn write_kv(&self, wb: &K::WriteBatch) -> Result<()> {
        self.kv.write(wb)
    }

    pub fn write_kv_opt(&self, wb: &K::WriteBatch, opts: &WriteOptions) -> Result<()> {
        self.kv.write_opt(wb, opts)
    }

    pub fn sync_kv(&self) -> Result<()> {
        self.kv.sync()
    }
}
