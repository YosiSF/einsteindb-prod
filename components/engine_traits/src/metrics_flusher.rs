// Copyright 2020 EinsteinDB Project Authors. Licensed under Apache-2.0.

use std::io;
use std::result::Result;
use std::sync::mpsc::{self, Slightlikeer};
use std::thread::{Builder as ThreadBuilder, JoinHandle};
use std::time::{Duration, Instant};

use crate::raft_engine::VioletaBftEngine;

use crate::*;

const DEFAULT_FLUSH_INTERVAL: Duration = Duration::from_millis(10_000);
const FLUSHER_RESET_INTERVAL: Duration = Duration::from_millis(60_000);

pub struct MetricsFlusher<K: KvEngine, R: VioletaBftEngine> {
    pub engines: Engines<K, R>,
    interval: Duration,
    handle: Option<JoinHandle<()>>,
    slightlikeer: Option<Slightlikeer<bool>>,
}

impl<K: KvEngine, R: VioletaBftEngine> MetricsFlusher<K, R> {
    pub fn new(engines: Engines<K, R>) -> Self {
        MetricsFlusher {
            engines,
            interval: DEFAULT_FLUSH_INTERVAL,
            handle: None,
            slightlikeer: None,
        }
    }

    pub fn set_flush_interval(&mut self, interval: Duration) {
        self.interval = interval;
    }

    pub fn spacelike(&mut self) -> Result<(), io::Error> {
        let (kv_db, raft_db) = (self.engines.kv.clone(), self.engines.violetabft.clone());
        let interval = self.interval;
        let (tx, rx) = mpsc::channel();
        self.slightlikeer = Some(tx);
        let h = ThreadBuilder::new()
            .name("metrics-flusher".to_owned())
            .spawn(move || {
                einsteindb_alloc::add_thread_memory_accessor();
                let mut last_reset = Instant::now();
                while let Err(mpsc::RecvTimeoutError::Timeout) = rx.recv_timeout(interval) {
                    kv_db.flush_metrics("kv");
                    raft_db.flush_metrics("violetabft");
                    if last_reset.elapsed() >= FLUSHER_RESET_INTERVAL {
                        kv_db.reset_statistics();
                        raft_db.reset_statistics();
                        last_reset = Instant::now();
                    }
                }
                einsteindb_alloc::remove_thread_memory_accessor();
            })?;

        self.handle = Some(h);
        Ok(())
    }

    pub fn stop(&mut self) {
        let h = self.handle.take();
        if h.is_none() {
            return;
        }
        drop(self.slightlikeer.take().unwrap());
        if let Err(e) = h.unwrap().join() {
            error!("join metrics flusher failed"; "err" => ?e);
            return;
        }
    }
}
