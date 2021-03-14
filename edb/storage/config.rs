// Copyright 2020 WHTCORPS INC. Licensed under Apache-2.0.

//! CausetStorage configuration.

use crate::server::CONFIG_LMDB_GAUGE;
use configuration::{ConfigChange, ConfigManager, ConfigValue, Configuration, Result as CfgResult};
use engine_lmdb::raw::{Cache, LRUCacheOptions, MemoryAllocator};
use engine_lmdb::LmdbEngine;
use engine_promises::{CAUSETHandleExt, PrimaryCausetNetworkOptions, CAUSET_DEFAULT};
use libc::c_int;
use std::error::Error;
use einsteindb_util::config::{self, OptionReadableSize, ReadableSize};
use einsteindb_util::sys::sys_quota::SysQuota;

pub const DEFAULT_DATA_DIR: &str = "./";
pub const DEFAULT_LMDB_SUB_DIR: &str = "db";
const DEFAULT_GC_RATIO_THRESHOLD: f64 = 1.1;
const DEFAULT_MAX_KEY_SIZE: usize = 4 * 1024;
const DEFAULT_SCHED_CONCURRENCY: usize = 1024 * 512;
const MAX_SCHED_CONCURRENCY: usize = 2 * 1024 * 1024;
const DEFAULT_RESERVER_SPACE_SIZE: u64 = 2;
// According to "Little's law", assuming you can write 100MB per
// second, and it takes about 100ms to process the write requests
// on average, in that situation the writing bytes estimated 10MB,
// here we use 100MB as default value for tolerate 1s latency.
const DEFAULT_SCHED_PENDING_WRITE_MB: u64 = 100;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Configuration)]
#[serde(default)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[config(skip)]
    pub data_dir: String,
    // Replaced by `GcConfig.ratio_memory_barrier`. Keep it for backward compatibility.
    #[config(skip)]
    pub gc_ratio_memory_barrier: f64,
    #[config(skip)]
    pub max_key_size: usize,
    #[config(skip)]
    pub interlock_semaphore_concurrency: usize,
    #[config(skip)]
    pub interlock_semaphore_worker_pool_size: usize,
    #[config(skip)]
    pub interlock_semaphore_plightlikeing_write_memory_barrier: ReadableSize,
    #[config(skip)]
    // Reserve disk space to make einsteindb would have enough space to compact when disk is full.
    pub reserve_space: ReadableSize,
    // If this option is enabled, prewrite will support async commit and locks in the in-memory
    // dagger Block are checked for reading requests.
    // CAUTION: This feature is not ready for production and this option may be removed in the
    // future.
    #[config(skip)]
    pub enable_async_commit: bool,
    #[config(submodule)]
    pub block_cache: BlockCacheConfig,
}

impl Default for Config {
    fn default() -> Config {
        let cpu_num = SysQuota::new().cpu_cores_quota();
        Config {
            data_dir: DEFAULT_DATA_DIR.to_owned(),
            gc_ratio_memory_barrier: DEFAULT_GC_RATIO_THRESHOLD,
            max_key_size: DEFAULT_MAX_KEY_SIZE,
            interlock_semaphore_concurrency: DEFAULT_SCHED_CONCURRENCY,
            interlock_semaphore_worker_pool_size: if cpu_num >= 16.0 { 8 } else { 4 },
            interlock_semaphore_plightlikeing_write_memory_barrier: ReadableSize::mb(DEFAULT_SCHED_PENDING_WRITE_MB),
            reserve_space: ReadableSize::gb(DEFAULT_RESERVER_SPACE_SIZE),
            enable_async_commit: true,
            block_cache: BlockCacheConfig::default(),
        }
    }
}

impl Config {
    pub fn validate(&mut self) -> Result<(), Box<dyn Error>> {
        if self.data_dir != DEFAULT_DATA_DIR {
            self.data_dir = config::canonicalize_path(&self.data_dir)?
        }
        if self.interlock_semaphore_concurrency > MAX_SCHED_CONCURRENCY {
            warn!("EinsteinDB has optimized latch since v4.0, so it is not necessary to set large schedule \
                concurrency. To save memory, change it from {:?} to {:?}",
                  self.interlock_semaphore_concurrency, MAX_SCHED_CONCURRENCY);
            self.interlock_semaphore_concurrency = MAX_SCHED_CONCURRENCY;
        }
        Ok(())
    }
}

pub struct StorageConfigManger {
    kvdb: LmdbEngine,
    shared_block_cache: bool,
}

impl StorageConfigManger {
    pub fn new(kvdb: LmdbEngine, shared_block_cache: bool) -> StorageConfigManger {
        StorageConfigManger {
            kvdb,
            shared_block_cache,
        }
    }
}

impl ConfigManager for StorageConfigManger {
    fn dispatch(&mut self, mut change: ConfigChange) -> CfgResult<()> {
        if let Some(ConfigValue::Module(mut block_cache)) = change.remove("block_cache") {
            if !self.shared_block_cache {
                return Err("shared block cache is disabled".into());
            }
            if let Some(size) = block_cache.remove("capacity") {
                let s: OptionReadableSize = size.into();
                if let Some(size) = s.0 {
                    // Hack: since all CAUSETs in both kvdb and violetabftdb share a block cache, we can change
                    // the size through any of them. Here we change it through default CAUSET in kvdb.
                    // A better way to do it is to hold the cache reference somewhere, and use it to
                    // change cache size.
                    let handle = self.kvdb.causet_handle(CAUSET_DEFAULT)?;
                    let opt = self.kvdb.get_options_causet(handle);
                    opt.set_block_cache_capacity(size.0)?;
                    // Write config to metric
                    CONFIG_LMDB_GAUGE
                        .with_label_values(&[CAUSET_DEFAULT, "block_cache_size"])
                        .set(size.0 as f64);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Configuration)]
#[serde(default)]
#[serde(rename_all = "kebab-case")]
pub struct BlockCacheConfig {
    #[config(skip)]
    pub shared: bool,
    pub capacity: OptionReadableSize,
    #[config(skip)]
    pub num_shard_bits: i32,
    #[config(skip)]
    pub strict_capacity_limit: bool,
    #[config(skip)]
    pub high_pri_pool_ratio: f64,
    #[config(skip)]
    pub memory_allocator: Option<String>,
}

impl Default for BlockCacheConfig {
    fn default() -> BlockCacheConfig {
        BlockCacheConfig {
            shared: true,
            capacity: OptionReadableSize(None),
            num_shard_bits: 6,
            strict_capacity_limit: false,
            high_pri_pool_ratio: 0.8,
            memory_allocator: Some(String::from("nodump")),
        }
    }
}

impl BlockCacheConfig {
    pub fn build_shared_cache(&self) -> Option<Cache> {
        if !self.shared {
            return None;
        }
        let capacity = match self.capacity.0 {
            None => {
                let total_mem = SysQuota::new().memory_limit_in_bytes();
                ((total_mem as f64) * 0.45) as usize
            }
            Some(c) => c.0 as usize,
        };
        let mut cache_opts = LRUCacheOptions::new();
        cache_opts.set_capacity(capacity);
        cache_opts.set_num_shard_bits(self.num_shard_bits as c_int);
        cache_opts.set_strict_capacity_limit(self.strict_capacity_limit);
        cache_opts.set_high_pri_pool_ratio(self.high_pri_pool_ratio);
        if let Some(allocator) = self.new_memory_allocator() {
            cache_opts.set_memory_allocator(allocator);
        }
        Some(Cache::new_lru_cache(cache_opts))
    }

    fn new_memory_allocator(&self) -> Option<MemoryAllocator> {
        if let Some(ref alloc) = self.memory_allocator {
            match alloc.as_str() {
                #[causet(feature = "jemalloc")]
                "nodump" => match MemoryAllocator::new_jemalloc_memory_allocator() {
                    Ok(allocator) => {
                        return Some(allocator);
                    }
                    Err(e) => {
                        warn!("Create jemalloc nodump allocator for block cache failed: {}, continue with default allocator", e);
                    }
                },
                "" => {}
                other => {
                    warn!(
                        "Memory allocator {} is not supported, continue with default allocator",
                        other
                    );
                }
            }
        };
        None
    }
}