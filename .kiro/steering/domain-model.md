# Domain Model

## Core Aggregates

### Activity (Activity Catalog)

```
Activity
├── ActivityId
├── OwnerId
├── ActivityTitle
├── ActivityType
├── StartedAt
├── EndedAt
├── CurrentRouteVersionId
├── RecordedSummary
├── CorrectedSummary
├── LifecycleState
├── CreatedAt
└── UpdatedAt
```

Invariants:
- The current route version must belong to the activity.
- A deleted activity cannot accept new edits.
- Recorded and corrected summaries remain separate — never merged into one value.

### Import (Activity Importing)

```
Import
├── ImportId
├── OwnerId
├── SourceArtifactId
├── ImportFormat
├── ImportStatus (state machine)
├── Checksum
├── ValidationResult
├── DuplicateResult
├── FailureReason
├── CreatedAt
└── UpdatedAt
```

State machine:
```
Requested → Uploading → Uploaded → Validating → Queued → Parsing
    → DuplicateReview → Committing → Completed
```

Terminal states: `Failed`, `Cancelled`.

Invariant: Invalid state transitions MUST be rejected by the domain. The state machine is enforced in domain code, not only in the database.

### Source Revision (Recorded Activity)

```
SourceRevision
├── SourceRevisionId
├── ActivityId
├── SourceArtifactId
├── RevisionNumber
├── Provider
├── ProviderActivityId
├── ParserVersion
├── RecordedTrack
├── RecordedStatistics
└── ImportedAt
```

Invariants:
- Source revisions are immutable after successful import.
- New external data creates a new revision — never overwrites existing.
- New revisions do NOT overwrite corrected routes.

### Recorded Track (Recorded Activity)

```
RecordedTrack
├── RecordedTrackId
├── SourceRevisionId
├── Geometry
├── BoundingBox
├── StartedAt
├── EndedAt
├── RecordedDistance
├── PointCount
├── SegmentCount
└── CalculationVersion
```

Invariant: Recorded tracks are **immutable** — they represent what the device reported.

### Route Draft (Route Editing)

```
RouteDraft
├── RouteDraftId
├── ActivityId
├── OwnerId
├── BaseRouteVersionId
├── Revision
├── Geometry
├── AppliedOperations
├── UndoneOperations
├── DraftState
├── CreatedAt
├── UpdatedAt
└── ExpiresAt
```

Invariants:
- A draft belongs to one activity.
- A draft has one base version.
- Every accepted operation increments the revision.
- Reused operation IDs are idempotent (same result, no error).
- Stale `expectedRevision` values MUST fail with conflict.
- Invalid geometry cannot be persisted as valid.
- Published and discarded drafts cannot be edited.

### Route Version (Route Versioning)

```
RouteVersion
├── RouteVersionId
├── ActivityId
├── ParentVersionId
├── VersionNumber
├── Geometry
├── BoundingBox
├── CorrectedStatistics
├── CalculationVersion
├── EditSummary
├── CreatedBy
└── CreatedAt
```

Invariants:
- Published versions are **immutable** — they can never be modified.
- Parent and child belong to the same activity.
- Version number is unique within an activity.
- Statistics are derived from the version geometry at publication time.

### Export Job (Route Exporting)

```
ExportJob
├── ExportJobId
├── ActivityId
├── RouteVersionId
├── RequestedBy
├── ExportFormat
├── ExportStatus
├── ObjectStorageKey
├── Checksum
└── ExpiresAt
```

Invariant: Exports always reference a specific immutable route version — never "current display state."

## Route-Editing Operations (MVP)

| Operation | Description |
|-----------|-------------|
| MovePoint | Relocate an existing point to new coordinates |
| AddPoint | Insert a new point between two existing points |
| DeletePoint | Remove a single point |
| DeleteSection | Remove points between two indices |
| ReplaceSection | Remove section and insert replacement geometry |
| SplitSegment | Break one segment into two at a given point |
| JoinSegments | Merge two compatible segment ends |
| Reset | Revert draft to its base version |
| Undo | Reverse the last applied operation |
| Redo | Re-apply the last undone operation |

### New-Point Semantics

A newly added point may contain:
- Latitude
- Longitude
- Optional elevation

It must NOT automatically receive fabricated:
- Timestamp
- Heart rate
- Speed
- Temperature
- Cadence

### Publication Workflow

```
Load draft → Authorize actor → Verify expectedRevision → Validate geometry
→ Calculate corrected statistics → Create immutable route version
→ Update activity current route pointer → Mark draft published
→ Append audit event and outbox messages → Commit (single transaction)
```

Publication is all-or-nothing. Partial publication must never occur.

## Duplicate Detection Strategy

### Exact Duplicate

Match: SHA-256 of original file bytes.

Result: Do not create a second activity. Return the existing import/activity reference.

### Provider Duplicate

Match: `provider + provider_activity_id`.

Result: Add or compare a source revision. Do not overwrite corrected route versions.

### Probable Duplicate

Compare: start time, duration, distance, bounding box, route similarity, device metadata.

Result: Ask the user to review. Never silently merge an uncertain match.

## Domain Events

Key domain events (written to transactional outbox):

| Event | Context | Trigger |
|-------|---------|---------|
| ImportRequested | Importing | Import created |
| ArtifactUploaded | Importing | Upload confirmed |
| ImportValidated | Importing | Validation passed |
| ImportParsingStarted | Importing | Worker begins parsing |
| ImportedActivityCommitted | Importing | Transactional import complete |
| ImportFailed | Importing | Unrecoverable failure |
| RouteVersionPublished | Versioning | Draft published as immutable version |

## Value Object Guidelines

- Use strong types for all IDs (e.g., `ActivityId`, `ImportId`, `RouteDraftId`).
- Use value objects for domain concepts (e.g., `Checksum`, `BoundingBox`, `Geometry`).
- Value objects enforce their own invariants at construction.
- Prefer newtypes over primitive types for domain identifiers.
- Coordinates must validate latitude (-90..90) and longitude (-180..180) at construction.

## Aggregate Design Rules

- Aggregates are consistency boundaries — all invariants within an aggregate are enforced in a single transaction.
- Cross-aggregate consistency is eventual (via events/outbox).
- Keep aggregates small. Do not put the entire activity graph in one aggregate.
- Domain code must be framework-independent (no Axum, SQLx, or S3 types).
- Test aggregates and value objects without HTTP or database.
