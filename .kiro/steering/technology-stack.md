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

## Crate Responsibilities & Frontend Parallel

Both frontend and backend follow the same **domain/common** organizational principle:

| Role | Backend (Rust) | Frontend (TypeScript) |
|------|---------------|----------------------|
| Domain logic by bounded context | `crates/app/src/{context}/` | `src/domain/{context}/` |
| Shared infrastructure | `crates/infrastructure/src/` | `src/common/` |
| External interface (HTTP/UI) | `crates/api/src/` | `src/domain/{context}/*.tsx` |
| Background processing | `crates/worker/src/` | — |
| API contract | `openapi/` | `src/api/` |

Each bounded context uses the same ubiquitous language across frontend and backend:

| Domain | Backend module | Frontend folder |
|--------|---------------|-----------------|
| Activity Catalog | `activity_catalog/` | `activity/` |
| Activity Importing | `imports/` | `importing/` |
| Recorded Activity | `recorded_activity/` | (consumed via `activity/detail/`) |
| Route Editing | `route_editing/` | `route-editing/` |
| Route Versioning | `route_versioning/` | (consumed via API) |
| Route Exporting | `exports/` | `exporting/` |
| Identity & Auth | `identity.rs` | `auth/` |

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
│   ├── infrastructure/          # Shared infrastructure (db, storage, telemetry)
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

## Frontend Organization

The frontend follows a **domain/common** pattern inspired by domain-driven design. Code is organized by bounded context under `domain/`, with shared generic infrastructure in `common/`.

```
src/
├── domain/                     # Business domains (bounded contexts)
│   ├── app/                    # App shell, router, layout
│   ├── activity/               # Activity catalog domain
│   │   ├── library/            # Activity list UI
│   │   └── detail/             # Activity detail UI
│   ├── importing/              # Activity importing domain
│   ├── route-editing/          # Route editing domain
│   ├── exporting/              # Route exporting domain
│   └── auth/                   # Authentication domain
├── common/                     # Shared, generic infrastructure
│   ├── components/             # Generic UI (EmptyState, LoadingSpinner, ErrorBoundary)
│   ├── hooks/                  # Generic hooks (useDocumentTitle)
│   └── utils/                  # Shared utilities (formatDate, formatDistance)
├── api/                        # API client (generated or hand-written)
├── test-utils/                 # Test helpers
└── main.tsx
```

### Domain folder rules

- Each domain folder owns its full stack: components, hooks, queries, mutations, types, constants, translations.
- Domain folders mirror backend bounded contexts and use the same ubiquitous language.
- A domain folder must NOT import from another domain folder directly. Cross-domain communication goes through `api/` or shared types.

### Common folder rules

- `common/` contains only generic, domain-agnostic code.
- Nothing in `common/` should reference a business concept (activity, route, import, etc.).
- Extract to `common/` only after the rule of three (proven duplication across domains).

## Frontend State Strategy

- **Server state**: TanStack Query (never duplicate in global store)
- **Editor interaction state**: Local feature store or reducer
- **Unsaved local recovery**: IndexedDB
- **Authoritative state**: Always server-side
