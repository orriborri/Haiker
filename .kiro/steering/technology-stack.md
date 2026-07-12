# Technology Stack

## Backend

| Concern | Technology |
|---------|-----------|
| Language | Rust (latest stable) |
| HTTP framework | Axum |
| Async runtime | Tokio |
| Serialization | Serde |
| Database driver | SQLx (compile-time checked queries) |
| OpenAPI generation | utoipa or equivalent |
| HTTP middleware | tower |
| Structured logging | tracing + tracing-subscriber |
| Telemetry | OpenTelemetry |
| Request validation | garde or validator |

## Frontend

| Concern | Technology |
|---------|-----------|
| UI framework | React |
| Language | TypeScript (strict mode) |
| Build tool | Vite |
| Server state | TanStack Query |
| Map rendering | MapLibre GL JS |
| Client validation | Zod |
| Local cache | IndexedDB |
| App shell caching | Service Worker |
| API client | OpenAPI-generated TypeScript client |

## Data & Infrastructure

| Concern | Technology |
|---------|-----------|
| Primary database | PostgreSQL + PostGIS |
| Object storage | S3-compatible (MinIO for local dev) |
| Job queue | PostgreSQL-backed (initially) |
| Event delivery | Transactional outbox in PostgreSQL |
| Authentication | Managed OpenID Connect provider |

## Geometry & Parsing Libraries

Evaluate and benchmark before committing:

- `geo` / `geo-types` for geometry operations
- `geozero` for format conversions
- `rstar` for spatial indexing (where needed)
- `proj` only for required coordinate transformations
- GPX parser: maintained crate or deliberately limited in-house parser with hardened XML handling

## Protocol Decisions

- **External API**: REST over HTTPS with OpenAPI 3.1.1
- **No** GraphQL, tRPC, or browser-facing gRPC
- **Internal gRPC**: Only if a future independently deployed service has a demonstrated need

## Repository Structure

```
haiker/
├── Cargo.toml                  # Workspace root
├── crates/
│   ├── app/                    # Domain modules (library crate)
│   │   └── src/
│   │       ├── identity/
│   │       ├── activity_catalog/
│   │       ├── imports/
│   │       ├── recorded_activity/
│   │       ├── route_editing/
│   │       ├── route_versioning/
│   │       ├── exports/
│   │       ├── polar_integration/
│   │       └── lib.rs
│   ├── api/                    # Axum HTTP server (binary)
│   ├── worker/                 # Background job processor (binary)
│   ├── platform/               # Shared infrastructure (db, storage, telemetry)
│   └── test_support/           # Test utilities and fixtures
├── frontend/                   # React SPA
├── migrations/                 # Database migrations (per context)
├── openapi/                    # OpenAPI spec (multi-file)
│   ├── openapi.yaml
│   ├── paths/
│   └── components/
├── docs/
│   ├── adr/                    # Architecture Decision Records
│   ├── runbooks/
│   └── threat-model/
└── tests/                      # Integration and E2E tests
```

## Domain Module Structure

Within each bounded context module:

```
route_editing/
├── features/
│   ├── create_draft/
│   ├── get_draft/
│   ├── apply_operation/
│   ├── undo_operation/
│   ├── redo_operation/
│   └── discard_draft/
├── persistence/
└── mod.rs
```

## Frontend Feature Organization

```
src/
├── features/
│   ├── activity-library/
│   ├── activity-detail/
│   ├── import-activity/
│   ├── route-editor/
│   ├── route-history/
│   └── export-route/
├── api/                        # Generated client
├── auth/
├── map/
└── platform/
```

## Frontend State Strategy

- **Server state**: TanStack Query (never duplicate in global store)
- **Editor interaction state**: Local feature store or reducer
- **Unsaved local recovery**: IndexedDB
- **Authoritative state**: Always server-side
