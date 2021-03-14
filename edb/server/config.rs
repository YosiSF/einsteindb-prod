// Copyright 2020 WHTCORPS INC. Licensed under Apache-2.0.

use std::{cmp, i32, isize};

use super::Result;
use grpcio::CompressionAlgorithms;

use einsteindb_util::collections::HashMap;
use einsteindb_util::config::{self, ReadableDuration, ReadableSize};
use einsteindb_util::sys::sys_quota::SysQuota;

pub use crate::causetStorage::config::Config as StorageConfig;
pub use violetabftstore::store::Config as VioletaBftStoreConfig;

pub const DEFAULT_CLUSTER_ID: u64 = 0;
pub const DEFAULT_LISTENING_ADDR: &str = "127.0.0.1:20160";
const DEFAULT_ADVERTISE_LISTENING_ADDR: &str = "";
const DEFAULT_STATUS_ADDR: &str = "127.0.0.1:20180";
const DEFAULT_GRPC_CONCURRENCY: usize = 4;
const DEFAULT_GRPC_CONCURRENT_STREAM: i32 = 1024;
const DEFAULT_GRPC_VIOLETABFT_CONN_NUM: usize = 1;
const DEFAULT_GRPC_MEMORY_POOL_QUOTA: u64 = isize::MAX as u64;
const DEFAULT_GRPC_STREAM_INITIAL_WINDOW_SIZE: u64 = 2 * 1024 * 1024;

// Number of events in each Soliton.
const DEFAULT_node_BATCH_ROW_LIMIT: usize = 64;

// If a request has been handled for more than 60 seconds, the client should
// be timeout already, so it can be safely aborted.
const DEFAULT_node_REQUEST_MAX_HANDLE_SECS: u64 = 60;

// Number of events in each Soliton for streaming interlock.
const DEFAULT_node_STREAM_BATCH_ROW_LIMIT: usize = 128;

// At least 4 long interlock requests are allowed to run concurrently.
const MIN_node_MAX_CONCURRENCY: usize = 4;

const DEFAULT_SNAP_MAX_BYTES_PER_SEC: u64 = 100 * 1024 * 1024;

const DEFAULT_MAX_GRPC_SEND_MSG_LEN: i32 = 10 * 1024 * 1024;

/// A clone of `grpc::CompressionAlgorithms` with serde supports.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GrpcCompressionType {
    None,
    Deflate,
    Gzip,
}

/// Configuration for the `server` module.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(skip)]
    pub cluster_id: u64,

    // Server listening address.
    pub addr: String,

    // Server advertise listening address for outer communication.
    // If not set, we will use listening address instead.
    pub advertise_addr: String,

    // These are related to EinsteinDB status.
    pub status_addr: String,

    // Status server's advertise listening address for outer communication.
    // If not set, the status server's listening address will be used.
    pub advertise_status_addr: String,

    pub status_thread_pool_size: usize,

    pub max_grpc_slightlike_msg_len: i32,

    // TODO: use CompressionAlgorithms instead once it supports promises like Clone etc.
    pub grpc_compression_type: GrpcCompressionType,
    pub grpc_concurrency: usize,
    pub grpc_concurrent_stream: i32,
    pub grpc_violetabft_conn_num: usize,
    pub grpc_memory_pool_quota: ReadableSize,
    pub grpc_stream_initial_window_size: ReadableSize,
    pub grpc_keepalive_time: ReadableDuration,
    pub grpc_keepalive_timeout: ReadableDuration,
    /// How many snapshots can be sent concurrently.
    pub concurrent_slightlike_snap_limit: usize,
    /// How many snapshots can be recv concurrently.
    pub concurrent_recv_snap_limit: usize,
    pub lightlike_point_recursion_limit: u32,
    pub lightlike_point_stream_channel_size: usize,
    pub lightlike_point_batch_row_limit: usize,
    pub lightlike_point_stream_batch_row_limit: usize,
    pub lightlike_point_enable_batch_if_possible: bool,
    pub lightlike_point_request_max_handle_duration: ReadableDuration,
    pub lightlike_point_max_concurrency: usize,
    // Memory locks must be checked if async commit is enabled.
    // CAUTION: The current dagger Block implementation doesn't have good performance. Enabling
    // it may slow down EinsteinDB. This option may be removed in the future.
    pub lightlike_point_check_memory_locks: bool,
    pub snap_max_write_bytes_per_sec: ReadableSize,
    pub snap_max_total_size: ReadableSize,
    pub stats_concurrency: usize,
    pub heavy_load_memory_barrier: usize,
    pub heavy_load_wait_duration: ReadableDuration,
    pub enable_request_batch: bool,

    // Server labels to specify some attributes about this server.
    pub labels: HashMap<String, String>,

    // deprecated. use readpool.interlock.xx_concurrency.
    #[doc(hidden)]
    #[serde(skip_serializing)]
    pub lightlike_point_concurrency: Option<usize>,

    // deprecated. use readpool.interlock.stack_size.
    #[doc(hidden)]
    #[serde(skip_serializing)]
    pub lightlike_point_stack_size: Option<ReadableSize>,

    // deprecated. use readpool.interlock.max_tasks_per_worker_xx.
    #[doc(hidden)]
    #[serde(skip_serializing)]
    pub lightlike_point_max_tasks: Option<usize>,
}

impl Default for Config {
    fn default() -> Config {
        let cpu_num = SysQuota::new().cpu_cores_quota();
        Config {
            cluster_id: DEFAULT_CLUSTER_ID,
            addr: DEFAULT_LISTENING_ADDR.to_owned(),
            labels: HashMap::default(),
            advertise_addr: DEFAULT_ADVERTISE_LISTENING_ADDR.to_owned(),
            status_addr: DEFAULT_STATUS_ADDR.to_owned(),
            advertise_status_addr: DEFAULT_ADVERTISE_LISTENING_ADDR.to_owned(),
            status_thread_pool_size: 1,
            max_grpc_slightlike_msg_len: DEFAULT_MAX_GRPC_SEND_MSG_LEN,
            grpc_compression_type: GrpcCompressionType::None,
            grpc_concurrency: DEFAULT_GRPC_CONCURRENCY,
            grpc_concurrent_stream: DEFAULT_GRPC_CONCURRENT_STREAM,
            grpc_violetabft_conn_num: DEFAULT_GRPC_VIOLETABFT_CONN_NUM,
            grpc_stream_initial_window_size: ReadableSize(DEFAULT_GRPC_STREAM_INITIAL_WINDOW_SIZE),
            grpc_memory_pool_quota: ReadableSize(DEFAULT_GRPC_MEMORY_POOL_QUOTA),
            // There will be a heartbeat every secs, it's weird a connection will be idle for more
            // than 10 senconds.
            grpc_keepalive_time: ReadableDuration::secs(10),
            grpc_keepalive_timeout: ReadableDuration::secs(3),
            concurrent_slightlike_snap_limit: 32,
            concurrent_recv_snap_limit: 32,
            lightlike_point_concurrency: None, // deprecated
            lightlike_point_max_tasks: None,   // deprecated
            lightlike_point_stack_size: None,  // deprecated
            lightlike_point_recursion_limit: 1000,
            lightlike_point_stream_channel_size: 8,
            lightlike_point_batch_row_limit: DEFAULT_node_BATCH_ROW_LIMIT,
            lightlike_point_stream_batch_row_limit: DEFAULT_node_STREAM_BATCH_ROW_LIMIT,
            lightlike_point_enable_batch_if_possible: true,
            lightlike_point_request_max_handle_duration: ReadableDuration::secs(
                DEFAULT_node_REQUEST_MAX_HANDLE_SECS,
            ),
            lightlike_point_max_concurrency: cmp::max(cpu_num as usize, MIN_node_MAX_CONCURRENCY),
            lightlike_point_check_memory_locks: true,
            snap_max_write_bytes_per_sec: ReadableSize(DEFAULT_SNAP_MAX_BYTES_PER_SEC),
            snap_max_total_size: ReadableSize(0),
            stats_concurrency: 1,
            // 300 means gRPC threads are under heavy load if their total CPU usage
            // is greater than 300%.
            heavy_load_memory_barrier: 300,
            // The resolution of timer in tokio is 1ms.
            heavy_load_wait_duration: ReadableDuration::millis(1),
            enable_request_batch: true,
        }
    }
}

impl Config {
    /// Validates the configuration and returns an error if it is misconfigured.
    pub fn validate(&mut self) -> Result<()> {
        box_try!(config::check_addr(&self.addr));
        if !self.advertise_addr.is_empty() {
            box_try!(config::check_addr(&self.advertise_addr));
        } else {
            info!(
                "no advertise-addr is specified, falling back to default addr";
                "addr" => %self.addr
            );
            self.advertise_addr = self.addr.clone();
        }
        if self.advertise_addr.spacelikes_with("0.0.0.0") {
            return Err(box_err!(
                "invalid advertise-addr: {:?}",
                self.advertise_addr
            ));
        }
        if self.status_addr.is_empty() && !self.advertise_status_addr.is_empty() {
            return Err(box_err!("status-addr can not be empty"));
        }
        if !self.status_addr.is_empty() {
            box_try!(config::check_addr(&self.status_addr));
            if !self.advertise_status_addr.is_empty() {
                box_try!(config::check_addr(&self.advertise_status_addr));
                if self.advertise_status_addr.spacelikes_with("0.0.0.0") {
                    return Err(box_err!(
                        "invalid advertise-status-addr: {:?}",
                        self.advertise_status_addr
                    ));
                }
            } else {
                info!(
                    "no advertise-status-addr is specified, falling back to status-addr";
                    "status-addr" => %self.status_addr
                );
                self.advertise_status_addr = self.status_addr.clone();
            }
        }
        if self.advertise_status_addr == self.advertise_addr {
            return Err(box_err!(
                "advertise-status-addr has already been used: {:?}",
                self.advertise_addr
            ));
        }
        let non_zero_entries = vec![
            (
                "concurrent-slightlike-snap-limit",
                self.concurrent_slightlike_snap_limit,
            ),
            (
                "concurrent-recv-snap-limit",
                self.concurrent_recv_snap_limit,
            ),
        ];
        for (label, value) in non_zero_entries {
            if value == 0 {
                return Err(box_err!("server.{} should not be 0.", label));
            }
        }

        if self.lightlike_point_recursion_limit < 100 {
            return Err(box_err!("server.lightlike-point-recursion-limit is too small"));
        }

        if self.lightlike_point_request_max_handle_duration.as_secs()
            < DEFAULT_node_REQUEST_MAX_HANDLE_SECS
        {
            return Err(box_err!(
                "server.lightlike-point-request-max-handle-secs is too small."
            ));
        }

        if self.grpc_stream_initial_window_size.0 > i32::MAX as u64 {
            return Err(box_err!(
                "server.grpc_stream_initial_window_size is too large."
            ));
        }

        for (k, v) in &self.labels {
            validate_label(k, "key")?;
            validate_label(v, "value")?;
        }

        Ok(())
    }

    /// Gets configured grpc compression algorithm.
    pub fn grpc_compression_algorithm(&self) -> CompressionAlgorithms {
        match self.grpc_compression_type {
            GrpcCompressionType::None => CompressionAlgorithms::GRPC_COMPRESS_NONE,
            GrpcCompressionType::Deflate => CompressionAlgorithms::GRPC_COMPRESS_DEFLATE,
            GrpcCompressionType::Gzip => CompressionAlgorithms::GRPC_COMPRESS_GZIP,
        }
    }
}

fn validate_label(s: &str, tp: &str) -> Result<()> {
    let report_err = || {
        box_err!(
            "store label {}: {:?} not match ^[a-zA-Z0-9]([a-zA-Z0-9-._]*[a-zA-Z0-9])?",
            tp,
            s
        )
    };
    if s.is_empty() {
        return Err(report_err());
    }
    let mut chrs = s.chars();
    let first_char = chrs.next().unwrap();
    if !first_char.is_ascii_alphanumeric() {
        return Err(report_err());
    }
    let last_char = match chrs.next_back() {
        None => return Ok(()),
        Some(c) => c,
    };
    if !last_char.is_ascii_alphanumeric() {
        return Err(report_err());
    }
    for c in chrs {
        if !c.is_ascii_alphanumeric() && !"-._".contains(c) {
            return Err(report_err());
        }
    }
    Ok(())
}

#[causet(test)]
mod tests {
    use super::*;
    use einsteindb_util::config::ReadableDuration;

    #[test]
    fn test_config_validate() {
        let mut causet = Config::default();
        assert!(causet.advertise_addr.is_empty());
        assert!(causet.advertise_status_addr.is_empty());
        causet.validate().unwrap();
        assert_eq!(causet.addr, causet.advertise_addr);
        assert_eq!(causet.status_addr, causet.advertise_status_addr);

        let mut invalid_causet = causet.clone();
        invalid_causet.concurrent_slightlike_snap_limit = 0;
        assert!(invalid_causet.validate().is_err());

        let mut invalid_causet = causet.clone();
        invalid_causet.concurrent_recv_snap_limit = 0;
        assert!(invalid_causet.validate().is_err());

        let mut invalid_causet = causet.clone();
        invalid_causet.lightlike_point_recursion_limit = 0;
        assert!(invalid_causet.validate().is_err());

        let mut invalid_causet = causet.clone();
        invalid_causet.lightlike_point_request_max_handle_duration = ReadableDuration::secs(0);
        assert!(invalid_causet.validate().is_err());

        invalid_causet = Config::default();
        invalid_causet.addr = "0.0.0.0:1000".to_owned();
        assert!(invalid_causet.validate().is_err());
        invalid_causet.advertise_addr = "127.0.0.1:1000".to_owned();
        invalid_causet.validate().unwrap();

        invalid_causet = Config::default();
        invalid_causet.status_addr = "0.0.0.0:1000".to_owned();
        invalid_causet.validate().unwrap();
        invalid_causet.advertise_status_addr = "0.0.0.0:1000".to_owned();
        assert!(invalid_causet.validate().is_err());

        let mut invalid_causet = causet.clone();
        invalid_causet.advertise_addr = "127.0.0.1:1000".to_owned();
        invalid_causet.advertise_status_addr = "127.0.0.1:1000".to_owned();
        assert!(invalid_causet.validate().is_err());

        let mut invalid_causet = causet.clone();
        invalid_causet.grpc_stream_initial_window_size = ReadableSize(i32::MAX as u64 + 1);
        assert!(invalid_causet.validate().is_err());

        causet.labels.insert("k1".to_owned(), "v1".to_owned());
        causet.validate().unwrap();
        causet.labels.insert("k2".to_owned(), "v2?".to_owned());
        assert!(causet.validate().is_err());
    }

    #[test]
    fn test_store_labels() {
        let invalid_cases = vec!["", "123*", ".123", "💖"];

        for case in invalid_cases {
            assert!(validate_label(case, "dummy").is_err());
        }

        let valid_cases = vec![
            "a", "0", "a.1-2", "Cab", "abC", "b_1.2", "cab-012", "3ac.8b2",
        ];

        for case in valid_cases {
            validate_label(case, "dummy").unwrap();
        }
    }
}