//Copyright 2020 EinsteinDB Project Authors & WHTCORPS Inc. Licensed under Apache-2.0.

mod applied_lock_collector;
mod compaction_filter;
mod config;
mod gc_manager;
mod gc_worker;

// TODO: Use separated error type for GCWorker instead.
pub use crate::causetStorage::{Callback, Error, ErrorInner, Result};
pub use compaction_filter::WriteCompactionFilterFactory;
use compaction_filter::{is_compaction_filter_allowd, CompactionFilterInitializer};
pub use config::{GcConfig, GcWorkerConfigManager, DEFAULT_GC_BATCH_KEYS};
pub use gc_manager::AutoGcConfig;
pub use gc_worker::{sync_gc, GcSafePointProvider, GcTask, GcWorker, GC_MAX_EXECUTING_TASKS};

#[causet(test)]
pub use compaction_filter::tests::gc_by_compact;