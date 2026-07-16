//! Haiker shared infrastructure.
//!
//! Provides configuration loading, telemetry setup, database connection
//! management, object storage, job queue, and transactional outbox shared
//! by the API server and worker processes.

pub mod activity_persistence;
pub mod audit;
pub mod auth_middleware;
pub mod clock;
pub mod config;
pub mod database;
pub mod error;
pub mod export_worker;
pub mod import_cleanup;
pub mod import_commit;
pub mod import_persistence;
pub mod import_worker;
pub mod job_queue;
pub mod metrics;
pub mod object_storage;
pub mod outbox;
pub mod ownership;
pub mod publication_commit;
pub mod rate_limit;
pub mod recorded_route_persistence;
pub mod request_id;
pub mod route_editing_gateways;
pub mod route_editing_persistence;
pub mod session;
pub mod telemetry;
pub mod worker_runtime;
