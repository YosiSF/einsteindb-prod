// Copyright 2019 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

mod point_getter;
mod reader;
mod scanner;

pub use self::point_getter::{PointGetter, PointGetterBuilder};
pub use self::reader::{check_need_gc, MvccReader, TxnCommitRecord};
pub use self::scanner::test_util;
pub use self::scanner::{
    has_data_in_cone, seek_for_valid_write, DeltaScanner, EntryScanner, Scanner, ScannerBuilder,
};

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum NewerTsCheckState {
    Unknown,
    Met,
    NotMetYet,
}
