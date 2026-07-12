# Architecture

## Architecture Style: Modular Monolith

Haiker is a server-authoritative modular monolith. All domain logic lives in a single deployable unit organized by bounded context. Separate worker processes handle background jobs but share the same codebase.

```
React + TypeScript + Vite (SPA)
        |
Generated REST client (OpenAPI)
        |
REST over HTTPS (OpenAPI 3.1.1)
        |
Rust + Axum modular monolith
        |
Vertical slices inside DDD bounded contexts
        |
PostgreSQL + PostGIS
S3-compatible object storage
PostgreSQL-backed job queue
Transactional outbox
        |
Separate Rust worker processes
```

Do NOT introduce microservices, message brokers, or separate deployable services unless there is a demonstrated need backed by an ADR.

## Vertical Slice Architecture

Every feature is delivered as a complete vertical slice that includes:

- User interaction (frontend)
- HTTP contract (OpenAPI operation)
- Authentication and authorization
- Application command or query
- Domain behavior (aggregate, invariants)
- Persistence (migration, repository)
- Events and audit behavior
- Error handling
- Tests (domain, integration, contract)
- Telemetry
- Documentation

An endpoint alone is NOT a completed slice. A database migration alone is NOT a completed slice. Every slice must deliver a meaningful user or operational outcome from UI to persistence.

## Domain-Driven Design

### Bounded Contexts

Each bounded context owns its schema, migrations, domain model, and application services. Contexts communicate through application interfaces (synchronous) or domain/integration events (asynchronous).

| Context | Owns |
|---------|------|
| Identity & Access | Authentication, sessions, ownership, authorization |
| Activity Catalog | Activity identity, title, type, timestamps, current route version, summary stats, lifecycle |
| Activity Importing | Upload, import state machine, validation, parsing orchestration, duplicate detection |
| Recorded Activity | Source artifacts, source revisions, recorded tracks, segments, point streams, sensor samples |
| Route Editing | Drafts, operations, undo/redo, concurrency, draft persistence, validation |
| Route Versioning | Immutable published routes, version numbers, corrected statistics, publication |
| Route Exporting | Export requests, jobs, generated artifacts, download authorization, expiration |
| Polar Integration | OAuth, encrypted tokens, sync state, provider identifiers |

### Cross-Context Rules

- A context may NOT write to another context's tables directly.
- Cross-context reads use application interfaces or dedicated read models.
- Foreign keys across contexts are allowed selectively but must not undermine module boundaries.
- Use a transactional outbox for reliable async event delivery between contexts.

### Database Schema Ownership

One PostgreSQL database with context-owned schemas:

```
identity.*
activity_catalog.*
imports.*
recorded_activity.*
route_editing.*
route_versioning.*
exports.*
polar_integration.*
platform.*
```

Each context owns its own migrations.

## Lightweight CQRS

- Commands change state (and return minimal confirmation).
- Queries return client-oriented read models.
- Command and query models may differ.
- Do NOT implement full event sourcing.

## Internal Communication

Inside the monolith:

- **Synchronous**: Direct application interface calls between modules.
- **Asynchronous**: Domain or integration events via transactional outbox.
- **Outbox**: Events written in the same transaction as state changes, dispatched by a background process.

## Concurrency Model

- Route draft mutations use **optimistic concurrency** with `expectedRevision`.
- Mismatched revision returns `409 Conflict`.
- No automatic merging of overlapping geometry edits in MVP.

## Idempotency

All retryable commands require an `Idempotency-Key` header:

- Starting imports
- Applying route operations
- Publishing a draft
- Requesting exports
- Manual synchronization

Duplicate keys with identical payloads replay the original response. Duplicate keys with different payloads return an error.

## Key Architecture Rules

1. The server database is the single source of truth.
2. Recorded tracks are immutable after import.
3. Published route versions are immutable.
4. Clients are non-authoritative.
5. Every feature is a complete vertical slice.
6. Domain code must be independent of framework types (Axum, SQLx, S3 SDK).
7. Prefer explicit code over framework magic.
8. Avoid generic repositories and base services.
9. Keep the shared kernel small.
10. Extract services only after demonstrated need.
