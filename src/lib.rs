//! Tucano Test API.
//!
//! The crate is layered: [`api`] speaks HTTP, [`domain`] holds the rules,
//! [`storage`] owns the bytes on disk, and [`models`] describes the documents.
//! Each layer only depends on the one beneath it.

pub mod api;
pub mod domain;
pub mod models;
pub mod repository;
pub mod storage;
