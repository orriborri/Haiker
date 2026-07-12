.PHONY: build check test fmt lint run-api run-worker infra infra-down clean

# Build the entire workspace
build:
	cargo build

# Run all checks (format, lint, test)
check: fmt-check lint test

# Run tests
test:
	cargo test --workspace

# Check formatting
fmt-check:
	cargo fmt --all -- --check

# Format code
fmt:
	cargo fmt --all

# Run clippy lints
lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Run the API server
run-api:
	cargo run --bin haiker-api

# Run the background worker
run-worker:
	cargo run --bin haiker-worker

# Start local infrastructure (PostgreSQL + MinIO)
infra:
	docker compose up -d

# Stop local infrastructure
infra-down:
	docker compose down

# Clean build artifacts
clean:
	cargo clean
