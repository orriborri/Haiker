# Haiker

Haiker is a hiking route management platform. The backend is a Rust workspace
(Axum HTTP API + background worker) backed by PostgreSQL/PostGIS and
S3-compatible object storage (MinIO for local dev). The frontend is a
React + Vite single-page app.

## Tech Stack

- **Backend:** Rust, [Axum](https://github.com/tokio-rs/axum), [SQLx](https://github.com/launchbadge/sqlx), Tokio
- **Database:** PostgreSQL with PostGIS (`postgis/postgis:16-3.4`)
- **Object storage:** MinIO (S3-compatible)
- **Frontend:** React 19, Vite, TanStack Query/Router, Tailwind CSS
- **API docs:** OpenAPI (served via Swagger UI at `/docs`)

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (edition 2021, toolchain `1.86`+; the Dockerfile builds with `1.94`)
- [Docker](https://docs.docker.com/get-docker/) + Docker Compose (for PostgreSQL and MinIO)
- [Node.js](https://nodejs.org/) 22+ and [pnpm](https://pnpm.io/installation) (for the frontend)
- [`sqlx-cli`](https://github.com/launchbadge/sqlx/tree/main/sqlx-cli) — install with:
  ```bash
  cargo install sqlx-cli --no-default-features --features rustls,postgres
  ```

> A `shell.nix` is also provided if you use Nix: `nix-shell` gives you `rustc`, `cargo`, `rustfmt`, `clippy`, and `sqlx-cli`.

## 1. Clone the repository

```bash
git clone https://github.com/orriborri/Haiker.git
cd Haiker
```

## 2. Configure environment variables

Copy the example env file and adjust if needed (the defaults match the local Docker Compose services):

```bash
cp .env.example .env
```

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | `postgres://haiker:haiker_dev_password@localhost:5432/haiker_dev` | PostgreSQL connection string |
| `S3_ENDPOINT` | `http://localhost:9000` | MinIO endpoint |
| `S3_ACCESS_KEY` | `minioadmin` | MinIO access key |
| `S3_SECRET_KEY` | `minioadmin` | MinIO secret key |
| `S3_BUCKET` | `haiker-artifacts` | MinIO bucket for uploaded artifacts |
| `RUST_LOG` | `info,haiker_api=debug,haiker_worker=debug` | Log verbosity |
| `API_PORT` | `3000` | Port the API listens on |

## 3. Start local infrastructure (PostgreSQL + MinIO)

Using `just`:

```bash
just dev-up
```

Or with `make`:

```bash
make infra
```

Or directly:

```bash
docker compose up -d
```

This starts:
- PostgreSQL (PostGIS) on `localhost:5432`
- MinIO on `localhost:9000` (console on `localhost:9001`, login `minioadmin` / `minioadmin`)
- A one-off `minio-init` job that creates the `haiker-artifacts` bucket

To stop services: `just dev-down` / `make infra-down` / `docker compose down`.
To wipe volumes and restart fresh: `just dev-reset`.

## 4. Run database migrations

```bash
DATABASE_URL=postgres://haiker:haiker_dev_password@localhost:5432/haiker_dev \
  cargo sqlx migrate run --source migrations/
```

Or use the helper script (reads `DATABASE_URL` from your environment/`.env`):

```bash
./scripts/run-migrations.sh
```

## 5. Run the backend

Build the whole workspace:

```bash
make build
# or
cargo build
```

Run the API server:

```bash
make run-api
# or
cargo run --bin haiker-api
```

Run the background worker (in a separate terminal):

```bash
make run-worker
# or
cargo run --bin haiker-worker
```

Or run both together in one terminal:

```bash
make backend
```

Once running, the API is available at `http://localhost:3000`:
- Health check: `GET http://localhost:3000/health`
- Readiness check: `GET http://localhost:3000/ready`
- Interactive API docs (Swagger UI): `http://localhost:3000/docs`
- Raw OpenAPI spec: `http://localhost:3000/api-docs/openapi.json`

## 6. Run the frontend

```bash
cd frontend
pnpm install
pnpm dev
```

Or use the Makefile shortcut from the repo root:

```bash
make frontend
```

The Vite dev server proxies requests under `/v1` to `http://localhost:3000` (see `frontend/vite.config.ts`), so make sure the API server is running first.

## Running with Docker

A multi-stage `Dockerfile` builds both `haiker-api` and `haiker-worker` binaries into a slim runtime image:

```bash
docker build -t haiker:local .
docker run --rm -p 3000:3000 \
  -e DATABASE_URL=postgres://haiker:haiker_dev_password@<db-host>:5432/haiker_dev \
  -e S3_ENDPOINT=http://<minio-host>:9000 \
  haiker:local
```

By default the container runs `haiker-api`; override the command (e.g. `haiker-worker`) to run the worker instead.

## Common development commands

| Command | Description |
|---|---|
| `make build` | Build the entire Rust workspace |
| `make check` | Run formatting check, clippy, and tests |
| `make test` | Run all Rust tests (`cargo test --workspace`) |
| `make fmt` | Format Rust code |
| `make lint` | Run clippy with warnings as errors |
| `make run-api` | Run the API server |
| `make run-worker` | Run the background worker |
| `make backend` | Run API + worker together |
| `make frontend` | Install deps and run the frontend dev server |
| `make infra` / `make infra-down` | Start/stop PostgreSQL + MinIO via Docker Compose |
| `make clean` | Remove build artifacts (`cargo clean`) |

Frontend-specific commands (run from `frontend/`):

| Command | Description |
|---|---|
| `pnpm dev` | Start the Vite dev server |
| `pnpm build` | Type-check and build for production |
| `pnpm preview` | Preview the production build locally |
| `pnpm typecheck` | Run TypeScript type checking only |
| `pnpm test` | Run frontend tests with Vitest |

## Testing

Run the full backend test suite (requires a running PostgreSQL instance matching `DATABASE_URL`, or the offline SQLx cache in `.sqlx/`):

```bash
make test
# or
cargo test --workspace
```

Run frontend tests:

```bash
cd frontend && pnpm test
```

## OpenAPI spec validation

The OpenAPI spec lives in `openapi/` (entry point `openapi/openapi.yaml`). To bundle and validate it locally (requires [Redocly CLI](https://redocly.com/docs/cli/)):

```bash
npm install -g @redocly/cli
bash scripts/bundle-openapi.sh
```

## Project structure

```
Haiker/
├── crates/
│   ├── api/         # Axum HTTP API server (binary: haiker-api)
│   ├── app/          # Application/domain logic (use cases, business rules)
│   ├── platform/     # Infrastructure adapters (Postgres, S3/MinIO, telemetry)
│   ├── worker/        # Background job processor (binary: haiker-worker)
│   └── test_support/  # Shared test utilities
├── frontend/          # React + Vite single-page app
├── migrations/        # SQLx SQL migrations
├── openapi/           # OpenAPI specification (paths, components)
├── scripts/           # Operational scripts (migrations, backups, restore drills)
├── docs/              # ADRs, runbooks, templates
├── docker-compose.yml # Local PostgreSQL + MinIO
├── Dockerfile         # Multi-stage build for API + worker
├── Makefile           # Common dev commands
└── Justfile           # Docker Compose service shortcuts
```

## CI

GitHub Actions (`.github/workflows/ci.yml`) runs on every push/PR to `main`:
formatting and clippy checks, the test suite (against a PostGIS service
container), a release build, OpenAPI spec validation, and a Docker image build.
