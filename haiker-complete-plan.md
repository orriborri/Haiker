# Haiker Complete Product and Engineering Plan

**Status:** Implementation-ready baseline  
**Architecture:** Server-authoritative modular monolith  
**Delivery model:** Vertical slices with domain-driven design  
**Backend:** Rust  
**External API:** REST over HTTPS with OpenAPI 3.1.1  
**Primary data store:** PostgreSQL with PostGIS  
**Source of truth:** Haiker server-side application database

---

## 1. Executive summary

Haiker is a hiking-route management application that lets users import historical hiking activities, view them on a map, correct inaccurate GPS tracks, preserve the original recording, publish corrected route versions, and export corrected routes.

The first product goal is a complete end-to-end journey:

```text
Upload GPX
-> Import activity
-> View recorded route
-> Create editing draft
-> Correct route
-> Publish corrected version
-> Export corrected GPX
```

The system will be built as a Rust modular monolith organized around domain capabilities. Work will be delivered as vertical slices that include frontend behavior, REST contract, domain rules, persistence, authorization, tests, telemetry, and operational handling.

The server-side application database is the authoritative source of truth. Polar Flow and uploaded GPX, FIT, and TCX files are ingestion sources. The browser cache is never authoritative.

---

## 2. Product vision

### 2.1 Problem

Hiking activities recorded by GPS devices can contain:

- GPS jumps.
- Missing route sections.
- Incorrect trail placement.
- Pauses or segment gaps.
- Inaccurate distance caused by noisy coordinates.

Users need a reliable way to correct the route while preserving what the device originally recorded.

### 2.2 Primary user outcome

A user can import a recorded hike, inspect the original route, correct mistakes, publish a corrected version, and export the corrected route without destroying the original data.

### 2.3 Product principles

1. Preserve the original recording.
2. Make all corrections reversible and transparent.
3. Distinguish recorded truth from corrected route geometry.
4. Keep routes private by default.
5. Make the server authoritative.
6. Prefer explicit domain actions over generic CRUD.
7. Deliver complete user outcomes through vertical slices.
8. Avoid premature microservices.
9. Treat uploaded files as hostile input.
10. Keep the public API stable and contract-driven.

---

## 3. Scope

### 3.1 MVP scope

The MVP must support:

- User authentication.
- GPX 1.0 and GPX 1.1 upload.
- Secure asynchronous GPX import.
- Activity library.
- Activity detail page.
- Recorded route display.
- Original-versus-edited route comparison.
- Route draft creation.
- Move point.
- Add point.
- Delete point.
- Delete route section.
- Draw replacement section.
- Split segment.
- Join compatible segment ends.
- Undo and redo.
- Reset draft to its base version.
- Publish immutable corrected route version.
- Display recorded and corrected distance separately.
- Export corrected GPX.
- Exact duplicate-file detection.
- Server-side audit events.
- Backup and restore procedures.

### 3.2 Post-MVP scope

Later phases may add:

- FIT import.
- TCX import.
- ZIP batch import.
- Polar OAuth connection.
- Synchronization of new Polar activities.
- Source revision reconciliation.
- Route smoothing.
- Route simplification controls.
- Trail snapping.
- Map matching.
- Elevation enrichment.
- Public or private route sharing.
- Cross-device offline editing.
- Native mobile clients.

### 3.3 Explicitly excluded from MVP

- Live hike recording.
- Social network features.
- Public route discovery.
- Editing heart-rate or sensor values.
- Fabricating timestamps for added points.
- Rewriting recorded duration.
- Automatic trail snapping.
- Full event sourcing.
- Microservice decomposition.
- Browser-facing gRPC.
- GraphQL.
- tRPC as the system API.

---

## 4. Architecture decisions

### 4.1 System of record

The Haiker server-side database is the authoritative system of record for:

- Activities.
- Imported source artifacts.
- Source revisions.
- Recorded tracks.
- Sensor samples.
- Route drafts.
- Published route versions.
- Current accepted route selection.
- Derived statistics.
- Import and export status.
- Audit history.

External providers and imported files may create new source revisions, but they do not automatically overwrite user corrections.

### 4.2 Architecture style

Use a modular monolith with separate worker processes.

```text
Web client
    |
REST + OpenAPI
    |
Rust Axum API
    |
Rust modular monolith
    |
PostgreSQL/PostGIS + object storage + job queue
    |
Rust workers
```

### 4.3 Delivery style

Use vertical slices. Every slice must deliver a meaningful user or operational outcome from UI to persistence.

A slice includes:

- User interaction.
- HTTP contract.
- Authentication and authorization.
- Application command or query.
- Domain behavior.
- Persistence.
- Events and audit behavior.
- Error handling.
- Tests.
- Telemetry.
- Documentation.

### 4.4 Domain-driven design

Use bounded contexts, explicit aggregates, value objects, invariants, and a small shared kernel.

### 4.5 CQRS

Use lightweight CQRS:

- Commands change state.
- Queries return client-oriented read models.
- Command and query models may differ.
- Do not implement full event sourcing.

### 4.6 Internal communication

Inside the modular monolith:

- Use direct application interfaces for synchronous collaboration.
- Use domain or integration events for asynchronous reactions.
- Use a transactional outbox for reliable event delivery.
- Do not let modules write directly to each other's tables.

---

## 5. Technology stack

### 5.1 Frontend

- React.
- TypeScript.
- Vite.
- TanStack Query.
- MapLibre GL JS.
- Zod for client-side validation.
- IndexedDB for cache and temporary unsaved drafts.
- Service worker for application-shell caching.
- OpenAPI-generated TypeScript client.

### 5.2 Backend

- Rust.
- Axum.
- Tokio.
- Serde.
- SQLx.
- utoipa or equivalent for OpenAPI integration.
- tower middleware.
- tracing.
- OpenTelemetry.
- garde or validator for request validation.

### 5.3 Data and infrastructure

- PostgreSQL.
- PostGIS.
- S3-compatible object storage.
- PostgreSQL-backed job queue initially, unless benchmarks show Redis is required.
- Transactional outbox in PostgreSQL.
- Managed OpenID Connect provider.

### 5.4 Geometry and parsing

Evaluate and benchmark:

- `geo`.
- `geo-types`.
- `geozero`.
- `rstar` where spatial indexing is needed.
- `proj` only for required coordinate transformations.
- A maintained GPX parser or a deliberately limited in-house parser with hardened XML handling.

### 5.5 Protocol decision

Use REST externally.

Do not use GraphQL, tRPC, or browser-facing gRPC initially.

Use gRPC only if a future independently deployed internal service has a demonstrated need for generated binary contracts or streaming.

---

## 6. Bounded contexts

### 6.1 Identity and Access

Owns:

- Authenticated identity.
- Sessions.
- Ownership.
- Authorization policies.
- Future sharing permissions.

Initial slices:

- Create session.
- End session.
- Get current user.
- Authorize activity access.

### 6.2 Activity Catalog

Owns the user-facing hike:

- Activity identity.
- Title.
- Type.
- Start and end times.
- Current route version.
- Summary statistics.
- Lifecycle state.

Initial slices:

- List activities.
- Get activity.
- Rename activity.
- Delete activity.
- Restore activity.
- Select current route version.

### 6.3 Activity Importing

Owns:

- Upload registration.
- Import state machine.
- Checksum calculation.
- Validation.
- Parsing orchestration.
- Duplicate detection.
- Transactional import completion.
- Retry and failure handling.

Initial slices:

- Start import.
- Complete upload.
- Queue import.
- Validate import.
- Parse GPX.
- Review probable duplicate.
- Commit imported activity.
- Get import status.
- Cancel import.
- Retry failed import.

### 6.4 Recorded Activity

Owns immutable source-derived data:

- Source artifacts.
- Source revisions.
- Recorded tracks.
- Track segments.
- Point streams.
- Elevation.
- Sensor samples.
- Recorded statistics.

Initial slices:

- Create recorded activity.
- Get recorded route.
- Get recorded summary.
- Add source revision.
- Compare source revisions.
- Get source history.

### 6.5 Route Editing

Owns mutable editing sessions:

- Draft creation.
- Operations.
- Undo and redo.
- Optimistic concurrency.
- Draft persistence.
- Draft validation.
- Draft expiration.

Initial slices:

- Create route draft.
- Get route draft.
- Apply route operation.
- Undo.
- Redo.
- Reset draft.
- Validate draft.
- Discard draft.

### 6.6 Route Versioning

Owns:

- Immutable published routes.
- Parent-child relationships.
- Version numbers.
- Corrected statistics.
- Publication.
- Current-version selection.

Initial slices:

- Publish route draft.
- List route versions.
- Get route version.
- Compare route versions.
- Select current route version.

### 6.7 Route Exporting

Owns:

- Export requests.
- Export jobs.
- Generated artifacts.
- Download authorization.
- Export expiration.

Initial slices:

- Request GPX export.
- Generate GPX export.
- Get export status.
- Download export.
- Expire export.

### 6.8 Polar Integration

Owns:

- Polar OAuth.
- Encrypted tokens.
- Synchronization state.
- Provider identifiers.
- Retry and disconnect behavior.

Initial later-phase slices:

- Start Polar authorization.
- Complete Polar authorization.
- Get connection status.
- Synchronize activities.
- Refresh token.
- Disconnect Polar.

Polar synchronization creates imports through the Activity Importing context. It does not write activities directly.

---

## 7. Domain model

### 7.1 Activity aggregate

```text
Activity
- ActivityId
- OwnerId
- ActivityTitle
- ActivityType
- StartedAt
- EndedAt
- CurrentRouteVersionId
- RecordedSummary
- CorrectedSummary
- LifecycleState
- CreatedAt
- UpdatedAt
```

Invariants:

- The current route version must belong to the activity.
- A deleted activity cannot accept new edits.
- Recorded and corrected summaries remain separate.

### 7.2 Import aggregate

```text
Import
- ImportId
- OwnerId
- SourceArtifactId
- ImportFormat
- ImportStatus
- Checksum
- ValidationResult
- DuplicateResult
- FailureReason
- CreatedAt
- UpdatedAt
```

State machine:

```text
Requested
-> Uploading
-> Uploaded
-> Validating
-> Queued
-> Parsing
-> DuplicateReview
-> Committing
-> Completed
```

Terminal states:

- Failed.
- Cancelled.

Invalid transitions must be rejected by the domain.

### 7.3 Source artifact

```text
SourceArtifact
- SourceArtifactId
- OwnerId
- Provider
- ProviderActivityId
- ImportMethod
- FileFormat
- ObjectStorageKey
- FileChecksum
- FileSize
- UploadedAt
```

The object storage key is internal and never exposed directly to normal clients.

### 7.4 Source revision aggregate

```text
SourceRevision
- SourceRevisionId
- ActivityId
- SourceArtifactId
- RevisionNumber
- Provider
- ProviderActivityId
- ParserVersion
- RecordedTrack
- RecordedStatistics
- ImportedAt
```

Invariants:

- Source revisions are immutable after successful import.
- New external data creates a new revision.
- New revisions do not overwrite corrected routes.

### 7.5 Recorded track

```text
RecordedTrack
- RecordedTrackId
- SourceRevisionId
- Geometry
- BoundingBox
- StartedAt
- EndedAt
- RecordedDistance
- PointCount
- SegmentCount
- CalculationVersion
```

Recorded tracks are immutable.

### 7.6 Route draft aggregate

```text
RouteDraft
- RouteDraftId
- ActivityId
- OwnerId
- BaseRouteVersionId
- Revision
- Geometry
- AppliedOperations
- UndoneOperations
- DraftState
- CreatedAt
- UpdatedAt
- ExpiresAt
```

Invariants:

- A draft belongs to one activity.
- A draft has one base version.
- Every accepted operation increments the revision.
- Reused operation IDs are idempotent.
- Stale expected revisions fail.
- Invalid geometry cannot be persisted as valid.
- Published and discarded drafts cannot be edited.

### 7.7 Route version aggregate

```text
RouteVersion
- RouteVersionId
- ActivityId
- ParentVersionId
- VersionNumber
- Geometry
- BoundingBox
- CorrectedStatistics
- CalculationVersion
- EditSummary
- CreatedBy
- CreatedAt
```

Invariants:

- Published versions are immutable.
- Parent and child belong to the same activity.
- Version number is unique within an activity.
- Statistics are derived from the version geometry.

### 7.8 Export job aggregate

```text
ExportJob
- ExportJobId
- ActivityId
- RouteVersionId
- RequestedBy
- ExportFormat
- ExportStatus
- ObjectStorageKey
- Checksum
- ExpiresAt
```

Exports always reference a specific immutable route version.

---

## 8. Route-editing semantics

### 8.1 Recorded truth and corrected truth

The application distinguishes:

- Recorded route: what the device reported.
- Corrected route: geometry accepted by the user.

The corrected route does not claim that the device recorded the corrected coordinates.

### 8.2 Supported MVP operations

- Move point.
- Add point.
- Delete point.
- Delete section.
- Replace section.
- Split segment.
- Join segments.
- Reset to base.
- Undo.
- Redo.

### 8.3 New-point semantics

A newly added point may contain:

- Latitude.
- Longitude.
- Optional elevation.

It must not automatically receive fabricated:

- Timestamp.
- Heart rate.
- Speed.
- Temperature.
- Cadence.

### 8.4 Publishing workflow

```text
Load draft
-> authorize actor
-> verify expected revision
-> validate geometry
-> calculate corrected statistics
-> create immutable route version
-> update activity current route pointer
-> mark draft published
-> append audit event and outbox messages
-> commit one transaction
```

---

## 9. Data architecture

### 9.1 Database ownership

Use one PostgreSQL database with context-owned schemas:

```text
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

Rules:

- Each context owns its migrations.
- A context may not write another context's tables directly.
- Cross-context reads use application interfaces or read models.
- Foreign keys may be used selectively, but must not undermine module boundaries.

### 9.2 Geometry storage

Store at least:

1. Full-resolution geometry for editing and export.
2. Preview geometry for list and map loading.
3. Bounding boxes and spatial indexes for geographic queries.

The physical full-point representation must be selected through benchmarks. Candidate approaches:

- One row per point.
- Chunked compressed binary in PostgreSQL.
- Compact binary object in object storage with metadata in PostgreSQL.

Do not finalize this choice without representative Polar files and load tests.

### 9.3 Object storage

Store:

- Original files.
- Generated exports.
- Potential large point-stream blobs.
- Backup artifacts where appropriate.

Requirements:

- Private buckets.
- Encryption at rest.
- Versioning for critical source artifacts.
- Short-lived signed URLs.
- Checksum verification.
- Lifecycle policies.

### 9.4 Initial database constraints

At minimum:

- Unique file checksum per owner and import scope where exact duplicate behavior requires it.
- Unique provider plus provider activity ID where available.
- Unique route version number per activity.
- Current route version must belong to the activity.
- Published route versions cannot be updated.
- Source revisions cannot be updated after finalization.
- Operation ID unique per route draft.
- Positive point counts and nonnegative revisions.
- Latitude and longitude validation during import and domain construction.

---

## 10. API strategy

### 10.1 Standard

The API must conform to OpenAPI Specification 3.1.1.

OpenAPI is the authoritative external HTTP contract. Domain rules remain authoritative in the domain model, and persisted state remains authoritative in the server database.

The OpenAPI specification describes a language-agnostic HTTP API contract, supports referenced multi-document descriptions, and recommends an entry document named `openapi.yaml` or `openapi.json`.

### 10.2 Entry document

```text
openapi/openapi.yaml
```

Root metadata:

```yaml
openapi: 3.1.1
info:
  title: Haiker API
  summary: Import, correct, version, and export hiking activities.
  version: 1.0.0
```

### 10.3 API base path

```text
/v1
```

The following are distinct:

- OpenAPI version: `3.1.1`.
- API description version: `info.version`.
- Public compatibility generation: `/v1`.

### 10.4 Contract ownership

Each vertical slice owns:

- Path.
- Method.
- Stable operation ID.
- Tags.
- Parameters.
- Request schema.
- Response schemas.
- Security requirements.
- Examples.
- Error mapping.

### 10.5 Resource paths

Initial endpoints:

```http
GET    /v1/activities
GET    /v1/activities/{activityId}
PATCH  /v1/activities/{activityId}/title
DELETE /v1/activities/{activityId}

POST   /v1/imports
GET    /v1/imports/{importId}
POST   /v1/imports/{importId}/completion
POST   /v1/imports/{importId}/cancellation

GET    /v1/activities/{activityId}/recorded-route
GET    /v1/activities/{activityId}/route-versions
GET    /v1/route-versions/{routeVersionId}
GET    /v1/route-versions/{routeVersionId}/geometry

POST   /v1/activities/{activityId}/route-drafts
GET    /v1/route-drafts/{draftId}
POST   /v1/route-drafts/{draftId}/operations
POST   /v1/route-drafts/{draftId}/undo
POST   /v1/route-drafts/{draftId}/redo
POST   /v1/route-drafts/{draftId}/reset
POST   /v1/route-drafts/{draftId}/publication
DELETE /v1/route-drafts/{draftId}

POST   /v1/activities/{activityId}/exports
GET    /v1/exports/{exportId}
GET    /v1/exports/{exportId}/download
```

### 10.6 Operation IDs

Examples:

- `listActivities`.
- `getActivity`.
- `startImport`.
- `getImportStatus`.
- `createRouteDraft`.
- `applyRouteOperation`.
- `publishRouteDraft`.
- `requestGpxExport`.

Operation IDs are stable client-generation contracts.

### 10.7 Error contract

Use one common error envelope based on HTTP Problem Details semantics:

```json
{
  "type": "/problems/stale-route-draft",
  "title": "Route draft revision is stale",
  "status": 409,
  "code": "ROUTE_DRAFT_REVISION_CONFLICT",
  "detail": "Expected revision 12, but the current revision is 13.",
  "requestId": "req_01J..."
}
```

### 10.8 Status code policy

- `200 OK`: successful read or command with body.
- `201 Created`: resource created synchronously.
- `202 Accepted`: asynchronous import or export accepted.
- `204 No Content`: successful deletion without body.
- `400 Bad Request`: malformed request.
- `401 Unauthorized`: missing or invalid authentication.
- `403 Forbidden`: authenticated but not authorized.
- `404 Not Found`: resource unavailable to the caller.
- `409 Conflict`: stale revision or idempotency conflict.
- `415 Unsupported Media Type`: invalid upload media type.
- `422 Unprocessable Content`: domain validation failure.
- `429 Too Many Requests`: rate limit.
- `500 Internal Server Error`: unexpected failure.

### 10.9 Idempotency

Require `Idempotency-Key` for:

- Starting imports.
- Applying route operations.
- Publishing a draft.
- Requesting exports.
- Manual synchronization.

Document:

- Scope.
- Retention period.
- Replay response.
- Behavior when the same key is reused with a different payload.

### 10.10 Concurrency

Use explicit `expectedRevision` for route draft mutations.

A mismatch returns `409 Conflict`.

The first version will not automatically merge overlapping geometry edits.

### 10.11 Pagination

Use cursor pagination consistently for collections.

### 10.12 Geometry media type

Use `application/geo+json` for route geometry where practical.

GeoJSON coordinate arrays must use:

```text
longitude, latitude
```

Domain objects that use named latitude and longitude fields must not be confused with GeoJSON ordering.

### 10.13 Request validation

Request schemas should generally use:

```yaml
type: object
additionalProperties: false
```

Rust request DTOs should use `#[serde(deny_unknown_fields)]` where this matches the OpenAPI schema.

### 10.14 OpenAPI repository structure

```text
openapi/
├── openapi.yaml
├── paths/
│   ├── activities.yaml
│   ├── imports.yaml
│   ├── route-editing.yaml
│   ├── route-versions.yaml
│   ├── exports.yaml
│   └── polar-integration.yaml
└── components/
    ├── common.yaml
    ├── activities.yaml
    ├── imports.yaml
    ├── route-editing.yaml
    ├── errors.yaml
    └── security.yaml
```

CI produces:

```text
dist/openapi.yaml
```

---

## 11. Import pipeline

### 11.1 Workflow

```text
1. Client creates an import.
2. Server issues an upload target.
3. Client uploads the file.
4. Server calculates SHA-256.
5. Exact duplicate check runs.
6. Original artifact is stored.
7. Import job is queued.
8. Worker validates and parses GPX.
9. Coordinates and metadata are normalized.
10. Probable duplicate detection runs.
11. Records are written transactionally.
12. Original route version is created.
13. Activity appears in the library.
14. Import status becomes completed.
```

### 11.2 Import states

- Requested.
- Uploading.
- Uploaded.
- Validating.
- Queued.
- Parsing.
- Duplicate review.
- Committing.
- Completed.
- Failed.
- Cancelled.

### 11.3 Security controls

- File-size limit.
- Point-count limit.
- Segment-count limit.
- XML depth limit.
- Parsing timeout.
- Worker memory limits.
- External entities disabled.
- No external URL retrieval from file metadata.
- ZIP expansion limits when ZIP is added.
- Transactional cleanup on failure.
- Sanitized filenames and metadata.

### 11.4 Initial limits

Provisional targets:

- GPX file size: 50 MB.
- Maximum route points: 500,000.
- Maximum segments: 10,000.
- Worker execution timeout: benchmark-derived, initially 5 minutes.

These limits must be validated through realistic fixtures and load tests.

---

## 12. Duplicate strategy

### 12.1 Exact duplicate

Match SHA-256 of original file bytes.

Result:

- Do not create a second activity.
- Return the existing import or activity reference where authorized.

### 12.2 Provider duplicate

Match:

```text
provider + provider_activity_id
```

Result:

- Add or compare a source revision.
- Do not overwrite corrected route versions.

### 12.3 Probable duplicate

Compare:

- Start time.
- Duration.
- Distance.
- Bounding box.
- Route similarity.
- Device metadata.

Result:

- Ask the user to review.
- Never silently merge an uncertain match.

---

## 13. Background jobs

Use workers for:

- Parsing.
- Duplicate analysis.
- Geometry simplification.
- Export generation.
- Polar synchronization.
- Source reprocessing.
- Draft expiration.
- Export expiration.
- Outbox dispatch.

Every job must have:

- Stable job ID.
- Idempotency.
- Retry policy.
- Maximum attempts.
- Dead-letter or failed state.
- Correlation ID.
- Structured status.
- Bounded execution time.
- Metrics.
- Cancellation where practical.

Existing activity reads must remain available when workers are unavailable.

---

## 14. Frontend architecture

### 14.1 Feature organization

Organize by user capability:

```text
src/
├── features/
│   ├── activity-library/
│   ├── activity-detail/
│   ├── import-activity/
│   ├── route-editor/
│   ├── route-history/
│   └── export-route/
├── api/
├── auth/
├── map/
└── platform/
```

### 14.2 State strategy

- Server state: TanStack Query.
- Editor interaction state: local feature store or reducer.
- Unsaved local recovery: IndexedDB.
- Do not duplicate authoritative server state in a global client store.

### 14.3 Route editor requirements

- Original route and corrected route use distinct line styles, not color alone.
- Large touch targets.
- Keyboard-accessible editing actions where feasible.
- Visible focus states.
- Undo immediately available.
- Explicit selection state.
- Clear stale-revision conflict flow.
- Map tile failure does not hide route geometry.

### 14.4 Offline behavior

MVP:

- Cache application shell.
- Cache recent activity summaries and previews.
- Allow read-only cached access when offline.
- Preserve unsaved local draft operations for recovery.
- Prevent unsafe server writes while disconnected.

Full offline editing and synchronization is later scope.

---

## 15. Security and privacy

### 15.1 Authentication

Use a managed OIDC provider.

First-party web client:

- Secure HTTP-only session cookie.
- SameSite protection.
- CSRF defense where applicable.
- Short-lived server-side session.
- Explicit logout.

### 15.2 Authorization

Every resource request must be scoped to the authenticated actor.

Never load a resource only by client-supplied ID without ownership or permission checks.

### 15.3 Privacy defaults

- Routes private by default.
- No public links in MVP.
- No implicit sharing.
- No route coordinates in normal logs.
- No Polar access tokens in logs.
- No uploaded file contents in error reports.

### 15.4 Secrets

- Store secrets in a managed secret store.
- Never store Polar client secrets in clients.
- Encrypt provider tokens at rest.
- Rotate secrets through documented procedures.

### 15.5 Threat model topics

Before production launch, review:

- Broken object-level authorization.
- XML attacks.
- ZIP bombs.
- Metadata XSS.
- Signed URL leakage.
- Session fixation.
- CSRF.
- Brute-force or abusive upload traffic.
- Resource exhaustion.
- Duplicate replay attacks.
- Stale-write conflicts.
- Accidental route exposure through analytics or logs.

---

## 16. Observability

Use structured tracing across API, workers, database, storage, and queue.

Capture:

- Request count and latency.
- Error rate by operation ID.
- Database latency.
- Import duration.
- Export duration.
- Queue depth.
- Worker failure rate.
- File sizes.
- Point counts.
- Geometry-processing time.
- Duplicate-detection outcomes.
- Revision conflict rate.
- OpenAPI contract-test failures.

Use correlation IDs across:

```text
HTTP request
-> upload
-> job
-> worker
-> database transaction
-> audit event
```

Do not use user IDs, activity IDs, or coordinates as high-cardinality metric labels.

---

## 17. Reliability, backup, and recovery

### 17.1 Required capabilities

- Automated PostgreSQL backups.
- Point-in-time recovery.
- Versioned object storage.
- Independent backup copies.
- Restore drills.
- Migration rollback strategy.
- Database integrity checks.
- Documented incident response.

### 17.2 Initial recovery objectives

Provisional launch targets:

- RPO: 15 minutes.
- RTO: 4 hours.

These must be reviewed against hosting cost and user expectations before production.

### 17.3 Failure modes

- API unavailable: cached client reads may continue.
- Workers unavailable: imports and exports remain queued.
- Object storage unavailable: metadata remains readable; uploads and downloads fail safely.
- Map provider unavailable: route geometry and metadata still load.
- Polar unavailable: stored activities remain unaffected.
- Database unavailable: service enters read-failure mode and must not accept writes.

---

## 18. Testing strategy

### 18.1 Domain tests

Test aggregates and value objects without HTTP or a database.

Examples:

- Invalid import transition fails.
- Stale route draft revision fails.
- Duplicate operation ID is idempotent.
- Published route version cannot change.
- Undo restores exact previous geometry.

### 18.2 Property-based tests

Use `proptest` for:

- Geometry operations.
- Split and join.
- Undo and redo sequences.
- Export and re-import.
- Invalid coordinate ranges.
- Repeated idempotent commands.

### 18.3 Integration tests

Use a real PostgreSQL/PostGIS test environment for:

- Transactions.
- Migrations.
- Spatial queries.
- Outbox behavior.
- Optimistic concurrency.
- Idempotency.
- Import completion.
- Publication workflow.

### 18.4 Contract tests

Test:

- OpenAPI validation.
- Request validation.
- Status codes.
- Error schemas.
- Security requirements.
- TypeScript client generation.
- Rust implementation compatibility.

### 18.5 End-to-end tests

Critical journey:

```text
Sign in
-> upload GPX
-> wait for import
-> open activity
-> create draft
-> edit route
-> publish
-> export GPX
-> download
-> re-import export
```

Additional E2E scenarios:

- Exact duplicate upload.
- Concurrent edit conflict.
- Import failure.
- Worker outage.
- Expired export.
- Unauthorized access.

### 18.6 Fixture library

Maintain immutable fixtures for:

- GPX 1.0.
- GPX 1.1.
- Multiple tracks.
- Multiple segments.
- Missing timestamps.
- Missing elevation.
- Duplicate timestamps.
- Invalid coordinates.
- Large tracks.
- GPS spikes.
- Corrupt XML.
- Non-ASCII names.
- Midnight and timezone boundaries.
- Vendor extensions.
- Polar-exported files.

---

## 19. OpenAPI governance and CI

Every pull request must:

1. Bundle the multi-file OpenAPI description.
2. Validate against OpenAPI 3.1.1.
3. Resolve every `$ref`.
4. Enforce unique operation IDs.
5. Run style linting.
6. Detect breaking changes against the latest released contract.
7. Generate the TypeScript client.
8. Fail if generated artifacts are stale.
9. Run Rust contract tests.
10. Compile the frontend against the generated client.
11. Publish preview API documentation.

A change cannot merge when:

- The OpenAPI document is invalid.
- A breaking change is unapproved.
- A response or error is undocumented.
- Rust behavior differs from the contract.
- Generated clients are stale.

---

## 20. Repository structure

```text
haiker/
├── Cargo.toml
├── crates/
│   ├── app/
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
│   ├── api/
│   ├── worker/
│   ├── platform/
│   └── test_support/
├── frontend/
├── migrations/
├── openapi/
├── docs/
│   ├── adr/
│   ├── runbooks/
│   └── threat-model/
└── tests/
```

Within a domain module:

```text
route_editing/
├── domain/
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

---

## 21. Delivery roadmap

### Phase 0: Foundation

Deliverables:

- Rust workspace.
- Axum API shell.
- PostgreSQL/PostGIS.
- SQLx migrations.
- OIDC integration.
- Object storage integration.
- PostgreSQL job queue.
- Transactional outbox.
- OpenAPI 3.1.1 baseline.
- CI pipeline.
- Structured tracing.
- Staging deployment.

Exit criteria:

- Authenticated health and current-user flows work.
- Database migrations run automatically in staging.
- OpenAPI checks run in CI.
- A worker can claim and complete a test job.

### Phase 1: Upload GPX

User outcome:

> A user can upload a GPX file and see that Haiker received it.

Includes:

- Create import.
- Upload target.
- File-size validation.
- Checksum calculation.
- Object storage.
- Import status endpoint.
- UI upload flow.

Acceptance criteria:

- Authenticated user can upload valid GPX.
- Unauthorized user cannot upload.
- Oversized file is rejected.
- Duplicate idempotency key does not create two imports.
- Import status is queryable.

### Phase 2: Import activity

User outcome:

> A user can upload GPX and see a hike in the activity library.

Includes:

- GPX parsing.
- Validation.
- Recorded track creation.
- Activity creation.
- Original route version.
- Exact duplicate detection.
- Failure reporting.

Acceptance criteria:

- Valid GPX creates exactly one activity.
- Failed import leaves no partial activity.
- Original file remains immutable.
- Exact duplicate does not create another activity.

### Phase 3: View route

User outcome:

> A user can open an imported hike and see its recorded route.

Includes:

- Activity list.
- Activity detail.
- Preview geometry.
- Full geometry endpoint.
- Map rendering.
- Recorded metadata.

Acceptance criteria:

- Only the owner can access the activity.
- Route loads without exposing object storage keys.
- Map failure does not hide route geometry data.
- Large previews load within performance target.

### Phase 4: Correct one point

User outcome:

> A user can move one inaccurate point and undo the change.

Includes:

- Route draft creation.
- Move-point operation.
- Revision checks.
- Draft persistence.
- Undo.
- UI edit interaction.

Acceptance criteria:

- Moving a point increments revision.
- Replaying operation ID is idempotent.
- Stale revision returns conflict.
- Undo restores exact geometry.

### Phase 5: Replace route section

User outcome:

> A user can delete an incorrect section and draw a replacement.

Includes:

- Section selection.
- Delete section.
- Replace section.
- Add and delete point.
- Split and join segment.
- Redo.
- Preview distance calculation.

Acceptance criteria:

- Invalid geometry is rejected.
- Undo and redo remain deterministic.
- Added points do not receive fabricated telemetry.

### Phase 6: Publish corrected route

User outcome:

> A user can save a corrected route without changing the recorded route.

Includes:

- Draft validation.
- Immutable version creation.
- Corrected statistics.
- Current-route selection.
- Audit event.
- Outbox event.

Acceptance criteria:

- Publication is transactional.
- Recorded route remains unchanged.
- Published version cannot be modified.
- Activity displays recorded and corrected distances separately.

### Phase 7: Export corrected GPX

User outcome:

> A user can download the corrected route as GPX.

Includes:

- Export request.
- Worker generation.
- Object storage.
- Status endpoint.
- Authorized download.
- Expiration.

Acceptance criteria:

- Export references a fixed route version.
- Re-imported export preserves corrected geometry within tolerance.
- Unauthorized download fails.

### Phase 8: Production hardening

Includes:

- Rate limits.
- Backup and restore drill.
- Threat-model review.
- Load testing.
- Worker resource limits.
- Accessibility audit.
- Browser compatibility testing.
- Operational runbooks.

### Phase 9: FIT and TCX

Includes:

- FIT parser.
- TCX parser.
- Sensor ingestion.
- Multisport and multi-segment handling.
- ZIP batch import.

### Phase 10: Polar integration

Includes:

- OAuth.
- Token encryption.
- Connection status.
- New-activity synchronization.
- Provider duplicate matching.
- Source revisions.
- Disconnect behavior.

---

## 22. Vertical-slice definition of done

A vertical slice is complete only when it has:

- A clear user or operational outcome.
- Agreed domain terminology.
- Domain invariants implemented.
- Frontend behavior.
- OpenAPI 3.1.1 operation.
- Stable operation ID.
- Authentication and authorization.
- Persistence and migration.
- Idempotency where needed.
- Optimistic concurrency where needed.
- Structured errors.
- Telemetry.
- Domain tests.
- PostgreSQL integration tests.
- API contract tests.
- End-to-end coverage for the main path.
- Failure-state UX.
- Documentation and runbook updates.

An endpoint alone is not a completed slice.

---

## 23. Non-functional targets

Provisional targets:

### API

- Metadata request p95: under 300 ms.
- Preview geometry p95: under 500 ms.
- Normal mutation acknowledgement p95: under 500 ms, excluding asynchronous work.

### Editor

- Pointer feedback target: 16 ms where practical.
- Autosave acknowledgement: under 1 second under normal conditions.

### Imports

- 50 MB GPX accepted.
- 500,000 points accepted subject to benchmark validation.
- API process never parses large files synchronously.

### Availability

- Existing activities remain readable when workers are unavailable.
- Map-provider failure does not block route access.
- Polar failure does not affect stored data.

### Recovery

- Initial RPO: 15 minutes.
- Initial RTO: 4 hours.

---

## 24. Architecture decision records

Create ADRs for at least:

1. Server-side database as source of truth.
2. Rust backend.
3. REST plus OpenAPI 3.1.1.
4. Modular monolith.
5. Vertical slices and DDD.
6. Lightweight CQRS without full event sourcing.
7. PostgreSQL/PostGIS.
8. S3-compatible object storage.
9. PostgreSQL-backed job queue initially.
10. Transactional outbox.
11. Immutable recorded tracks and published route versions.
12. Explicit expected revisions for editing concurrency.
13. GPX-first import strategy.
14. Managed OIDC provider.
15. GeoJSON for geometry exchange.

Each ADR must include:

- Context.
- Decision.
- Alternatives considered.
- Consequences.
- Status.
- Date.
- Owners.

---

## 25. Open decisions

The following require early spikes or explicit selection:

1. Managed OIDC provider.
2. Hosting platform.
3. S3-compatible storage provider.
4. PostgreSQL hosting provider.
5. PostgreSQL-backed queue library.
6. GPX parser library versus hardened custom parser.
7. Full-resolution point storage representation.
8. Map tile provider and licensing.
9. Error-reporting provider and redaction controls.
10. Exact retention periods for source files, exports, logs, and audit events.
11. Production RPO and RTO approval.
12. Draft expiration duration.
13. Idempotency-key retention duration.
14. API rate limits.

Open decisions must be resolved through ADRs or time-boxed technical spikes before they block a vertical slice.

---

## 26. Risk register

### Risk: route editor complexity

Mitigation:

- Deliver one operation at a time.
- Build deterministic domain tests.
- Use realistic route fixtures.
- Avoid automatic trail snapping in MVP.

### Risk: large route performance

Mitigation:

- Worker-based parsing.
- Preview geometries.
- Benchmarked point storage.
- WebGL map rendering.
- Explicit performance budgets.

### Risk: data loss

Mitigation:

- Server authority.
- Immutable originals.
- Versioned routes.
- PITR.
- Object versioning.
- Restore drills.

### Risk: privacy exposure

Mitigation:

- Private-by-default routes.
- Strict ownership checks.
- Log redaction.
- Short-lived signed URLs.
- Threat modeling.

### Risk: API drift

Mitigation:

- OpenAPI-first review.
- Contract tests.
- Breaking-change detection.
- Generated TypeScript client.

### Risk: DDD overengineering

Mitigation:

- Keep bounded contexts logical, not separately deployed.
- Keep shared kernel small.
- Avoid generic abstractions.
- Extract services only after demonstrated need.

### Risk: Rust delivery speed

Mitigation:

- Use well-supported libraries.
- Keep architecture simple.
- Establish coding conventions.
- Pair reviews on async, ownership, and SQLx patterns.
- Avoid unnecessary custom frameworks.

---

## 27. Team practices

- Use ubiquitous language in code, backlog, API, and UX.
- Review each slice with product, design, backend, frontend, QA, and security perspectives.
- Keep pull requests aligned to one vertical slice or a clearly isolated enabling change.
- Require ADRs for significant architecture changes.
- Keep domain code independent of Axum, SQLx, Redis, and S3 SDKs.
- Prefer explicit code over framework magic.
- Avoid generic repositories and base services.
- Use feature-level ownership and code review.

---

## 28. Complete first release backlog

### Epic A: Platform foundation

Stories:

- Initialize Rust workspace.
- Establish module boundaries.
- Configure PostgreSQL/PostGIS.
- Configure migrations.
- Add OIDC authentication.
- Add session middleware.
- Add object storage.
- Add job queue.
- Add transactional outbox.
- Add tracing and metrics.
- Add OpenAPI 3.1.1 pipeline.
- Add staging deployment.

### Epic B: Import GPX

Stories:

- Start import.
- Upload file.
- Complete upload.
- Validate file.
- Calculate checksum.
- Detect exact duplicate.
- Queue parse job.
- Parse GPX.
- Normalize segments and points.
- Commit recorded activity.
- Expose import status.
- Display import progress and errors.

### Epic C: Activity library

Stories:

- List activities.
- Display activity summary.
- View activity detail.
- Load route preview.
- Load full route.
- Rename activity.
- Delete activity.

### Epic D: Route editor

Stories:

- Create draft.
- Load draft.
- Move point.
- Add point.
- Delete point.
- Select section.
- Delete section.
- Replace section.
- Split segment.
- Join segment.
- Undo.
- Redo.
- Reset draft.
- Handle revision conflict.
- Recover unsaved local work.

### Epic E: Route versions

Stories:

- Validate draft.
- Publish version.
- Calculate corrected distance.
- List route versions.
- Compare versions.
- Select current route.
- Display recorded versus corrected statistics.

### Epic F: Export

Stories:

- Request GPX export.
- Generate GPX.
- Store export.
- Display export status.
- Download export.
- Expire export.
- Verify round trip.

### Epic G: Production readiness

Stories:

- Rate limiting.
- Security review.
- Accessibility review.
- Browser matrix validation.
- Load tests.
- Backup automation.
- Restore drill.
- Runbooks.
- Alerting.
- Release checklist.

---

## 29. Release criteria

The first production release may launch when:

- The complete GPX-to-export journey works.
- Recorded data remains immutable.
- Published routes are immutable.
- Authorization tests cover every protected operation.
- OpenAPI contract is valid and published.
- No unapproved breaking API changes exist.
- Backups and one full restore have succeeded.
- Load tests meet agreed targets.
- Threat-model findings rated critical or high are resolved.
- Accessibility review has no blocking issues.
- Audit and telemetry are operational.
- Error handling is user-visible and actionable.
- Operational runbooks are approved.

---

## 30. Final architecture summary

```text
React + TypeScript + Vite
        |
Generated REST client
        |
REST over HTTPS
OpenAPI 3.1.1
        |
Rust + Axum modular monolith
        |
Vertical slices inside DDD bounded contexts
        |
PostgreSQL + PostGIS
S3-compatible object storage
PostgreSQL-backed jobs
Transactional outbox
        |
Separate Rust workers
```

Key rules:

1. The server database is authoritative.
2. Uploaded files and Polar are ingestion sources.
3. Recorded tracks are immutable.
4. Corrected routes are versioned.
5. Published versions are immutable.
6. Clients are non-authoritative.
7. Every route edit uses optimistic concurrency.
8. Every retryable command is idempotent.
9. Every external API operation is documented in OpenAPI 3.1.1.
10. Every feature is delivered as a complete vertical slice.
11. The application starts as a modular monolith.
12. Microservices are introduced only after demonstrated need.

---

## 31. Immediate next actions

1. Approve this plan as the implementation baseline.
2. Create the initial ADR set.
3. Select hosting, OIDC, PostgreSQL, and object-storage providers.
4. Create the Rust workspace and module skeleton.
5. Write the first OpenAPI contract for authentication, imports, and activity reads.
6. Implement Phase 0 foundation.
7. Deliver the first three vertical slices: upload GPX, import GPX, and view route.
8. Benchmark large-route parsing and geometry storage before implementing the full editor.

---

## 32. Reference

OpenAPI requirements in this plan are based on the OpenAPI Specification 3.1.1 published by Swagger and the OpenAPI Initiative. The specification defines a language-agnostic interface for HTTP APIs, permits multi-document descriptions, and recommends `openapi.yaml` or `openapi.json` as the entry-document name.
