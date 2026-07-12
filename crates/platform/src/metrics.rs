//! Metrics instrumentation using tracing structured events.
//!
//! These helper functions emit tracing events with well-known field names
//! that a metrics subscriber layer (e.g., tracing-opentelemetry or a custom
//! metrics aggregator) can consume to produce counters and histograms.
//!
//! Metric events are emitted at INFO level in the `metrics` target so they
//! can be filtered independently from application logs.

/// Record an HTTP request metric event.
///
/// Fields: method, path, status, duration_ms.
///
/// Callers should invoke this from their HTTP request middleware layer (e.g.,
/// the request-ID middleware in `request_id.rs` or a dedicated metrics
/// middleware). It is not called automatically by the platform crate itself.
pub fn record_http_request(method: &str, path: &str, status: u16, duration_ms: u64) {
    tracing::info!(
        target: "metrics",
        metric = "http_requests",
        method = %method,
        path = %path,
        status = status,
        duration_ms = duration_ms,
        "http request completed"
    );
}

/// Record a database query metric event.
///
/// Fields: query_name, duration_ms, success.
pub fn record_db_query(query_name: &str, duration_ms: u64, success: bool) {
    tracing::info!(
        target: "metrics",
        metric = "db_query",
        query_name = %query_name,
        duration_ms = duration_ms,
        success = success,
        "database query executed"
    );
}

/// Record a storage operation metric event.
///
/// Fields: operation, duration_ms, success.
pub fn record_storage_op(operation: &str, duration_ms: u64, success: bool) {
    tracing::info!(
        target: "metrics",
        metric = "storage_operation",
        operation = %operation,
        duration_ms = duration_ms,
        success = success,
        "storage operation completed"
    );
}

/// Record a job processing metric event.
///
/// Fields: job_type, duration_ms, success.
pub fn record_job_processed(job_type: &str, duration_ms: u64, success: bool) {
    tracing::info!(
        target: "metrics",
        metric = "job_processed",
        job_type = %job_type,
        duration_ms = duration_ms,
        success = success,
        "job processed"
    );
}

/// Record queue depth metric event.
///
/// Fields: depth.
pub fn record_queue_depth(depth: u64) {
    tracing::info!(
        target: "metrics",
        metric = "queue_depth",
        depth = depth,
        "queue depth measured"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_http_request_does_not_panic() {
        record_http_request("GET", "/health", 200, 5);
    }

    #[test]
    fn record_db_query_does_not_panic() {
        record_db_query("find_user_by_id", 12, true);
    }

    #[test]
    fn record_storage_op_does_not_panic() {
        record_storage_op("put_object", 200, true);
    }

    #[test]
    fn record_job_processed_does_not_panic() {
        record_job_processed("send_email", 50, true);
    }

    #[test]
    fn record_queue_depth_does_not_panic() {
        record_queue_depth(42);
    }
}
