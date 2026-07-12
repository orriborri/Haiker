//! Haiker domain modules.
//!
//! This crate contains all bounded context domain logic. Domain code is pure
//! business logic with no dependencies on infrastructure (Axum, SQLx, S3 SDK).

pub mod activity_catalog;
pub mod error;
pub mod exports;
pub mod identity;
pub mod imports;
pub mod recorded_activity;
pub mod route_editing;
pub mod route_versioning;
pub mod polar_integration;
