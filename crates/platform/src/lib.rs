//! Haiker shared infrastructure.
//!
//! Provides configuration loading, telemetry setup, database connection
//! management, object storage, job queue, and transactional outbox shared
//! by the API server and worker processes.

pub mod clock;
pub mod config;
pub mod database;
pub mod job_queue;
pub mod object_storage;
pub mod outbox;
pub mod telemetry;
