# Coding Conventions

## Rust Conventions

### Naming

- Crates: `snake_case` (e.g., `test_support`)
- Modules: `snake_case` (e.g., `route_editing`)
- Types: `PascalCase` (e.g., `RouteDraft`, `ActivityId`)
- Functions/methods: `snake_case` (e.g., `apply_operation`)
- Constants: `SCREAMING_SNAKE_CASE`
- Domain IDs: newtype wrappers (e.g., `struct ActivityId(Uuid)`)

### Error Handling

- Use `thiserror` for domain error types.
- Each bounded context defines its own error enum.
- Use `anyhow` only in binary crates (api, worker) for top-level error propagation, never in library code.
- Map domain errors to HTTP status codes at the API boundary — never inside domain code.
- Never use `.unwrap()` or `.expect()` in production code paths. Use them only in tests or where a panic is genuinely impossible and documented with a comment.

### Domain Code Rules

- Domain types live in `crates/app/src/{context}/`.
- Domain code must NOT depend on Axum, SQLx, S3 SDK, or any infrastructure type.
- Domain aggregates enforce their own invariants — the database is a secondary defense.
- Use `impl` blocks on aggregates for behavior. Avoid anemic data structs with external logic.
- State machines are encoded in Rust's type system or as explicit enum variants with transition methods.
- Prefer exhaustive `match` over `if let` for state transitions.

### Serde & Serialization

- Use `#[serde(deny_unknown_fields)]` on request DTOs.
- Use `#[serde(rename_all = "camelCase")]` for JSON API types.
- Domain types should NOT derive Serialize/Deserialize directly. Use separate DTO types at boundaries.
- Keep domain types clean of framework annotations.

### Persistence

- Use SQLx compile-time checked queries.
- Repository traits are defined in the domain module. Implementations live in `persistence/` submodule.
- Never expose database row types outside the persistence layer.
- Map database rows to domain types explicitly — no magic ORM.
- Use transactions for multi-step operations that must be atomic.

### Async

- All I/O-bound operations are async.
- Domain logic (pure computation) is synchronous.
- Use `tokio::spawn` for CPU-heavy work that would block the runtime.
- Never hold a mutex guard across an `.await` point.

### Visibility

- Default to private (`pub(crate)` or private).
- Use `pub` only for types that must cross crate boundaries.
- Use `pub(super)` for intra-module sharing.
- Each module's `mod.rs` explicitly re-exports its public API.

## TypeScript/React Conventions

### Naming

- Components: `PascalCase` (e.g., `ActivityDetail`)
- Hooks: `camelCase` with `use` prefix (e.g., `useRouteDraft`)
- Files: `kebab-case` for non-component files, `PascalCase` for component files
- Types/Interfaces: `PascalCase`
- Constants: `UPPER_SNAKE_CASE` (e.g., `MAX_REPLACEMENT_POINTS`)

### Frontend Architecture (domain/common pattern)

- Code is organized under `src/domain/` by bounded context, with `src/common/` for generic shared code.
- Each domain folder owns its components, hooks, queries, mutations, types, constants, and tests.
- Domain folders mirror backend bounded contexts (e.g., `route-editing/`, `importing/`, `exporting/`).
- `common/` contains only domain-agnostic utilities, generic UI components, and shared hooks.
- Use absolute imports via `@/` alias (e.g., `@/domain/activity/library/ActivityLibrary`).
- A domain folder must NOT import directly from another domain folder. Cross-domain data flows through `@/api/`.
- Extract to `common/` only after the rule of three (proven duplication across domains).

### Branded Types (TypeScript Domain Types)

Use branded types (zero runtime cost) to prevent mixing semantically different values that share the same underlying primitive. Branded types live in `src/types/branded.ts`.

```typescript
// Brand utility — creates a nominal type from a structural one
type Brand<T, B extends string> = T & { readonly __brand: B };
```

**Entity IDs — never pass one entity's ID where another is expected:**

```typescript
type ActivityId = Brand<string, "ActivityId">;
type DraftId = Brand<string, "DraftId">;
type ImportId = Brand<string, "ImportId">;
type ExportId = Brand<string, "ExportId">;
type LegId = Brand<string, "LegId">;
type UserId = Brand<string, "UserId">;
type RouteVersionId = Brand<string, "RouteVersionId">;
```

**Measurements — never mix units or confuse a distance with a count:**

```typescript
type Meters = Brand<number, "Meters">;
type Kilometers = Brand<number, "Kilometers">;
type Latitude = Brand<number, "Latitude">;
type Longitude = Brand<number, "Longitude">;
```

**Indices — never swap a segment index for a point index:**

```typescript
type SegmentIndex = Brand<number, "SegmentIndex">;
type PointIndex = Brand<number, "PointIndex">;
```

**Temporal — distinguish date strings from arbitrary strings:**

```typescript
type IsoDateTime = Brand<string, "IsoDateTime">;
type IsoDate = Brand<string, "IsoDate">;
```

**Rules:**

- All API response schemas must use branded types for IDs, coordinates, and measurements.
- Function parameters that accept an entity ID must use the specific branded type, not `string`.
- Use constructor functions to create branded values from raw primitives:
  ```typescript
  function activityId(raw: string): ActivityId { return raw as ActivityId; }
  function meters(raw: number): Meters { return raw as Meters; }
  ```
- Zod schemas use `.transform()` to produce branded types from parsed values.
- Components receive branded types in props — never accept raw `string` for an entity ID.

### Patterns

- Use functional components with hooks exclusively.
- Co-locate component, hook, and test in the same domain directory.
- Server state through TanStack Query — never duplicate in Redux/Zustand.
- Use Zod for runtime validation of external data.
- Generated API client types are the source of truth for request/response shapes.
- Favor composing views from small, focused components over reusing large multi-purpose components.
- Boolean props should start with `is`, `has`, or `can` (e.g., `isLoading`, `hasAccess`).
- Callback props use `onX` format (`onClick`, `onClose`, `onSubmit`).
- Prefer passing entity objects as a single prop over spreading fields into separate props.

## Testing Strategy

### Domain Tests (Unit)

- Test aggregates and value objects WITHOUT HTTP or database.
- Test every state machine transition (valid and invalid).
- Test invariant enforcement (e.g., stale revision, invalid geometry).
- Test idempotency (duplicate operation IDs).
- Test undo/redo determinism.

### Property-Based Tests

Use `proptest` for:
- Geometry operations
- Split and join
- Undo/redo sequences
- Export and re-import round-trips
- Invalid coordinate ranges
- Repeated idempotent commands

### Integration Tests

Use a real PostgreSQL/PostGIS test database for:
- Transactions and isolation
- Migrations
- Spatial queries
- Outbox behavior
- Optimistic concurrency
- Full import-to-publish workflows

### Contract Tests

- OpenAPI validation
- Request/response schema conformance
- Status code correctness
- Security requirement enforcement
- TypeScript client generation compatibility

### Test File Location

- Domain unit tests: inline `#[cfg(test)] mod tests` in the same file.
- Integration tests: `tests/` directory at workspace root.
- Fixtures: `crates/test_support/fixtures/`.

## Code Organization Within a Feature

Each feature (vertical slice) follows this structure:

```
{feature_name}/
├── mod.rs              # Public API (command/query entry point)
├── command.rs          # Command type and handler
├── handler.rs          # HTTP handler (in api crate)
├── repository.rs       # Repository trait
├── persistence.rs      # SQLx implementation
└── tests.rs            # Feature-specific tests
```

## General Rules

- Prefer explicit over clever.
- No generic base repositories or base services.
- No inheritance hierarchies — use composition and traits.
- Extract shared abstractions only after the rule of three (proven duplication).
- Keep functions short and focused — one level of abstraction per function.
- Document public APIs with `///` doc comments. Internal code uses `//` comments only when non-obvious.
- All public types have doc comments explaining their purpose and invariants.
- Use `clippy` with default lints. Do not suppress warnings without justification.
- Format with `rustfmt` — no manual formatting debates.
