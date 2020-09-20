// Copyright 2020 WHTCORPS INC Project Authors. Licensed under Apache-2.0.

#![feature(test)]
#![feature(box_patterns)]
#![feature(custom_test_frameworks)]
#![test_runner(test_util::run_tests)]

extern crate test;

extern crate encryption;
#[macro_use]
extern crate einsteindb_util;
extern crate fidel_client;

mod config;
mod interlock;
mod import;
mod fidel;
mod violetabftstore;
mod server;
mod server_encryption;
mod persistence;
