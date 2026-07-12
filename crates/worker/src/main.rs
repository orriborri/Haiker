use haiker_platform::telemetry::{self, TelemetryConfig};

#[tokio::main]
async fn main() {
    let config = TelemetryConfig::from_env();
    telemetry::init_telemetry(&config);

    tracing::info!("Starting Haiker background worker");

    // TODO: Initialize job queue and start processing
    tracing::info!("Worker shutting down");
}
