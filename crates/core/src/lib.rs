pub mod api;
pub mod artifacts;
pub mod auth;
pub mod catalog;
pub mod config;
pub mod contracts;
pub mod database;
pub mod domain;
pub mod drafts;
pub mod error;
pub mod operations;
pub mod packages;
pub mod qc;
pub mod storage;
pub mod submission;
pub mod uploads;
pub mod states {
    include!(concat!(env!("OUT_DIR"), "/states.rs"));
}
