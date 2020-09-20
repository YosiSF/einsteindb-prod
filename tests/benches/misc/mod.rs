// Copyright 2016 EinsteinDB Project Authors. Licensed under Apache-2.0.

#![feature(test)]

extern crate test;

mod interlock;
mod keybuilder;
mod raftkv;
mod serialization;
mod causetStorage;
mod util;
mod writebatch;

#[bench]
fn _bench_check_requirement(_: &mut test::Bencher) {
    einsteindb_util::config::check_max_open_fds(4096).unwrap();
}
