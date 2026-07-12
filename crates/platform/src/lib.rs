//! Haiker shared infrastructure.
//!
//! Provides configuration loading, telemetry setup, database connection
//! management, object storage, job queue, and transactional outbox shared
//! by the API server and worker processes.

pub mod clock;
pub mod config;
pub mod database;
pub mod job_queue;
// pub mod object_storage;  // Requires Rust 1.94+ for AWS SDK
pub mod outbox;
pub mod telemetry;
