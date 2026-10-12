//! SyncFleet — a secure, local-first, peer-to-peer edge database
//! synchronization engine.
//!
//! This crate is split into a library (this file) and a thin binary
//! (`src/main.rs`). The library exposes the building blocks — configuration,
//! node identity, and the SQLite-backed storage layer — so they can be tested
//! and reused independently of the CLI.
//!
//! Several error variants and storage helpers model the full replication
//! contract and are exercised incrementally as phases land (see docs/ROADMAP.md),
//! so dead-code detection is relaxed at the crate root for now.
#![allow(dead_code)]

pub mod cli;
pub mod config;
pub mod error;
pub mod identity;
pub mod net;
pub mod storage;
pub mod tls;
