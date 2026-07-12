# Development Workflow

## Local Development Setup

### Prerequisites

- Rust (latest stable via rustup)
- Docker & Docker Compose
- Node.js (LTS) + pnpm
- sqlx-cli (`cargo install sqlx-cli`)

### Start Local Services

```bash
docker compose up -d
```

This starts:
- PostgreSQL + PostGIS (port 5432)
- MinIO S3-compatible storage (port 9000, console 9001)

### Run Database Migrations

```bash
cargo sqlx migrate run
```

### Environment Configuration

Copy `.env.example` to `.env` for local development. Key variables:

```
DATABASE_URL=postgres://haiker:haiker@localhost:5432/haiker
S3_ENDPOINT=http://localhost:9000
S3_ACCESS_KEY=minioadmin
S3_SECRET_KEY=minioadmin
S3_BUCKET=haiker-dev
```

## Build Commands

```bash
# Build all crates
cargo build

# Build in release mode
cargo build --release

# Check without building (faster feedback)
cargo check
```

## Test Commands

```bash
# Run all tests
cargo test

# Run domain tests only (no database required)
cargo test -p app

# Run integration tests (requires running PostgreSQL)
cargo test -p haiker-tests

# Run a specific test
cargo test test_name

# Run with output visible
cargo test -- --nocapture
```

## Lint & Format

```bash
# Run clippy lints
cargo clippy -- -D warnings

# Check formatting
cargo fmt -- --check

# Apply formatting
cargo fmt
```

## Frontend Commands

```bash
# Install dependencies
cd frontend && pnpm install

# Development server
pnpm dev

# Type check
pnpm typecheck

# Lint
pnpm lint

# Build for production
pnpm build

# Regenerate API client from OpenAPI spec
pnpm generate-client
```

## Running the Application

```bash
# Start API server
cargo run -p api

# Start background worker
cargo run -p worker
```

## CI Pipeline

The GitHub Actions CI pipeline runs on every push to main and on PRs:

1. `cargo check` — fast compilation check
2. `cargo clippy -- -D warnings` — lint with warnings as errors
3. `cargo fmt -- --check` — formatting check
4. `cargo test` — all tests (with PostgreSQL service container)
5. OpenAPI validation and bundling
6. TypeScript client generation freshness check
7. Frontend type check and build

A PR cannot merge if any CI step fails.

## Git Workflow

- Work on feature branches named after the slice or task.
- Keep PRs aligned to one vertical slice or one clearly isolated enabling change.
- Commit messages: imperative mood, concise subject line (e.g., "Add import state machine domain model").
- Squash merge to main for clean history.
- Never push directly to main.

## OpenAPI Workflow

When modifying the API:

1. Edit the relevant file in `openapi/paths/` or `openapi/components/`.
2. Bundle: validates and produces `dist/openapi.yaml`.
3. Regenerate TypeScript client.
4. Update Rust handler to match contract.
5. Ensure contract tests pass.

## Database Migration Workflow

When adding a new migration:

```bash
# Create a new migration
cargo sqlx migrate add -r {context}_{description}

# Run migrations
cargo sqlx migrate run

# Revert last migration
cargo sqlx migrate revert
```

Rules:
- Each bounded context owns its migrations (prefixed by context name).
- Migrations must be reversible in development.
- Never modify a migration that has been applied to a shared environment.
- Test migrations in CI with a fresh database.

## Background Jobs

Jobs are enqueued from the API process and executed by the worker process. For local development, both run as separate processes:

```bash
# Terminal 1: API
cargo run -p api

# Terminal 2: Worker
cargo run -p worker
```

## Useful Aliases

Add to your shell profile for convenience:

```bash
alias hb="cargo build"
alias ht="cargo test"
alias hc="cargo clippy -- -D warnings"
alias hf="cargo fmt"
alias hr="cargo run -p api"
alias hw="cargo run -p worker"
```
