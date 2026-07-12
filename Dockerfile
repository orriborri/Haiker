# Stage 1: Builder
FROM rust:1.92-bookworm AS builder
WORKDIR /app
# Copy manifests first for caching
COPY Cargo.toml Cargo.lock ./
COPY crates/app/Cargo.toml crates/app/
COPY crates/api/Cargo.toml crates/api/
COPY crates/worker/Cargo.toml crates/worker/
COPY crates/platform/Cargo.toml crates/platform/
COPY crates/test_support/Cargo.toml crates/test_support/
# Create dummy source files for dependency caching
RUN mkdir -p crates/app/src crates/api/src crates/worker/src crates/platform/src crates/test_support/src && \
    echo "pub fn main() {}" > crates/api/src/main.rs && \
    echo "pub fn main() {}" > crates/worker/src/main.rs && \
    echo "" > crates/app/src/lib.rs && \
    echo "" > crates/platform/src/lib.rs && \
    echo "" > crates/test_support/src/lib.rs
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin haiker-api --bin haiker-worker 2>/dev/null || true
# Now copy real source
COPY crates/ crates/
COPY migrations/ migrations/
RUN cargo build --release --bin haiker-api --bin haiker-worker

# Stage 2: Runtime
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/* && \
    useradd --system --create-home haiker
COPY --from=builder /app/target/release/haiker-api /usr/local/bin/
COPY --from=builder /app/target/release/haiker-worker /usr/local/bin/
COPY migrations/ /app/migrations/
USER haiker
EXPOSE 3000
ENV RUST_LOG=info
ENV LOG_FORMAT=json
CMD ["haiker-api"]
