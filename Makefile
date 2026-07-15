.PHONY: build check test fmt lint run-api run-worker backend frontend infra infra-down clean

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

# Run the API server and worker together
backend:
	@trap 'kill 0' EXIT; \
	cargo run --bin haiker-api & \
	cargo run --bin haiker-worker & \
	wait

# Install frontend deps and run the dev server
frontend:
	cd frontend && pnpm install && pnpm dev

# Start local infrastructure (PostgreSQL + MinIO)
infra:
	docker compose up -d

# Stop local infrastructure
infra-down:
	docker compose down

# Clean build artifacts
clean:
	cargo clean
