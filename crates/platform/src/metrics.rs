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

/// Record an import state transition with the time spent in the previous state.
///
/// Fields: import_status (the state being exited), duration_in_state_ms.
/// No private labels (no user_id, owner_id, filename, or file path).
pub fn record_import_state_transition(import_status: &str, duration_in_state_ms: u64) {
    tracing::info!(
        target: "metrics",
        metric = "import_state_transition",
        import_status = %import_status,
        duration_in_state_ms = duration_in_state_ms,
        "import state transition"
    );
}

/// Record an import processing attempt.
///
/// Fields: job_type, attempt_number, success.
/// No private labels (no user_id, owner_id, filename, or file path).
pub fn record_import_attempt(job_type: &str, attempt_number: i32, success: bool) {
    tracing::info!(
        target: "metrics",
        metric = "import_attempt",
        job_type = %job_type,
        attempt_number = attempt_number,
        success = success,
        "import attempt"
    );
}

/// Record an import failure with its failure code.
///
/// Fields: failure_code.
/// No private labels (no user_id, owner_id, filename, or file path).
pub fn record_import_failure(failure_code: &str) {
    tracing::info!(
        target: "metrics",
        metric = "import_failure",
        failure_code = %failure_code,
        "import failure"
    );
}

/// Record file-level metrics for a completed import.
///
/// Fields: file_size_bytes, point_count.
/// No private labels (no user_id, owner_id, filename, or file path).
pub fn record_import_file_metrics(file_size_bytes: u64, point_count: u64) {
    tracing::info!(
        target: "metrics",
        metric = "import_file_metrics",
        file_size_bytes = file_size_bytes,
        point_count = point_count,
        "import file metrics"
    );
}

/// Record a rate limit decision metric event.
///
/// Fields: route_category, decision.
/// Privacy-safe: uses only low-cardinality labels (route_category, decision).
/// No user_id, IP address, or other PII.
pub fn record_rate_limit_decision(route_category: &str, decision: &str) {
    tracing::info!(
        target: "metrics",
        metric = "rate_limit_decision",
        route_category = %route_category,
        decision = %decision,
        "rate limit decision"
    );
}

/// Record worker backpressure metric event.
///
/// Emitted when all worker job slots are occupied and a new poll cycle
/// cannot acquire a permit.
///
/// Fields: active_jobs, max_concurrent.
/// Privacy-safe: uses only low-cardinality numeric labels.
pub fn record_worker_backpressure(active_jobs: u64, max_concurrent: u64) {
    tracing::info!(
        target: "metrics",
        metric = "worker_backpressure",
        active_jobs = active_jobs,
        max_concurrent = max_concurrent,
        "worker at capacity, all job slots occupied"
    );
}

/// Record when a worker job execution is terminated due to timeout.
///
/// Fields: job_type.
/// Privacy-safe: uses only the low-cardinality job_type label.
pub fn record_worker_job_timeout(job_type: &str) {
    tracing::info!(
        target: "metrics",
        metric = "worker_job_timeout",
        job_type = %job_type,
        "job execution timed out"
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

    #[test]
    fn record_import_state_transition_does_not_panic() {
        record_import_state_transition("queued", 1500);
        record_import_state_transition("parsing", 3200);
        record_import_state_transition("committing", 450);
    }

    #[test]
    fn record_import_attempt_does_not_panic() {
        record_import_attempt("parse_gpx", 1, true);
        record_import_attempt("parse_gpx", 3, false);
    }

    #[test]
    fn record_import_failure_does_not_panic() {
        record_import_failure("PARSE_ERROR");
        record_import_failure("CHECKSUM_MISMATCH");
        record_import_failure("INTERNAL_ERROR");
    }

    #[test]
    fn record_import_file_metrics_does_not_panic() {
        record_import_file_metrics(1_048_576, 2500);
        record_import_file_metrics(0, 0);
    }

    #[test]
    fn record_rate_limit_decision_does_not_panic() {
        record_rate_limit_decision("auth", "allowed");
        record_rate_limit_decision("read", "rejected");
        record_rate_limit_decision("mutation", "allowed");
    }

    #[test]
    fn record_worker_backpressure_does_not_panic() {
        record_worker_backpressure(5, 5);
        record_worker_backpressure(0, 10);
    }

    #[test]
    fn record_worker_job_timeout_does_not_panic() {
        record_worker_job_timeout("parse_gpx");
        record_worker_job_timeout("send_email");
    }
}
