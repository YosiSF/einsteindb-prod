// Copyright 2020 EinsteinDB Project Authors. Licensed under Apache-2.0.

use std::path::Path;
pub trait CompactionJobInfo {
    type TablePropertiesCollectionView;
    type CompactionReason;
    fn status(&self) -> Result<(), String>;
    fn causet_name(&self) -> &str;
    fn input_file_count(&self) -> usize;
    fn input_file_at(&self, pos: usize) -> &Path;
    fn output_file_count(&self) -> usize;
    fn output_file_at(&self, pos: usize) -> &Path;
    fn table_properties(&self) -> &Self::TablePropertiesCollectionView;
    fn elapsed_micros(&self) -> u64;
    fn num_corrupt_tuplespaceInstanton(&self) -> u64;
    fn output_level(&self) -> i32;
    fn input_records(&self) -> u64;
    fn output_records(&self) -> u64;
    fn total_input_bytes(&self) -> u64;
    fn total_output_bytes(&self) -> u64;
    fn compaction_reason(&self) -> Self::CompactionReason;
}
