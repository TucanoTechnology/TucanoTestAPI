//! Tucano Test API.
//!
//! The crate is layered: [`api`] speaks HTTP, [`domain`] holds the rules,
//! [`storage`] owns the bytes on disk, and [`models`] describes the documents.
//! Each layer only depends on the one beneath it. [`auth`] sits beside them:
//! it is the vocabulary authentication and project authorisation are written
//! in, and every other layer may call into it, but it depends on none of them.

pub mod api;
pub mod auth;
pub mod domain;
pub mod models;
pub mod repository;
pub mod storage;
