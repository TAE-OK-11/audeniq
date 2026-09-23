pub mod api;
pub mod auth;
pub mod catalog;
pub mod config;
pub mod contracts;
pub mod database;
pub mod domain;
pub mod error;
pub mod operations;
pub mod packages;
pub mod storage;
pub mod uploads;
pub mod states {
    include!(concat!(env!("OUT_DIR"), "/states.rs"));
}
