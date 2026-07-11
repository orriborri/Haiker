#!/usr/bin/env bash
set -euo pipefail

# Haiker - GitHub Issues & Epics Setup Script
# Run this with: ./scripts/create-github-issues.sh
# Requires: gh CLI authenticated (gh auth login)

REPO="orriborri/Haiker"

echo "=== Creating Labels ==="

gh label create "epic" --repo "$REPO" --color "3E4B9E" --description "Epic tracking issue" --force
gh label create "slice" --repo "$REPO" --color "0E8A16" --description "Vertical slice" --force
gh label create "infrastructure" --repo "$REPO" --color "D93F0B" --description "Infrastructure and platform setup" --force
gh label create "identity" --repo "$REPO" --color "C2E0C6" --description "Identity & Access context" --force
gh label create "activity-catalog" --repo "$REPO" --color "BFD4F2" --description "Activity Catalog context" --force
gh label create "importing" --repo "$REPO" --color "FEF2C0" --description "Activity Importing context" --force
gh label create "recorded-activity" --repo "$REPO" --color "D4C5F9" --description "Recorded Activity context" --force
gh label create "route-editing" --repo "$REPO" --color "F9D0C4" --description "Route Editing context" --force
gh label create "route-versioning" --repo "$REPO" --color "C5DEF5" --description "Route Versioning context" --force
gh label create "exporting" --repo "$REPO" --color "BFDADC" --description "Route Exporting context" --force
gh label create "polar-integration" --repo "$REPO" --color "E99695" --description "Polar Integration context" --force
gh label create "domain" --repo "$REPO" --color "5319E7" --description "Domain model and business logic" --force
gh label create "api" --repo "$REPO" --color "1D76DB" --description "API endpoint and contract" --force
gh label create "persistence" --repo "$REPO" --color "006B75" --description "Database and storage" --force
gh label create "worker" --repo "$REPO" --color "FBCA04" --description "Background worker process" --force

echo ""
echo "=== Creating Epics ==="


# Epic 0: Project Infrastructure & Foundation
EPIC0=$(gh issue create --repo "$REPO" --title "Epic: Project Infrastructure & Foundation" \
  --label "epic,infrastructure" \
  --body "## Project Infrastructure & Foundation

Set up the foundational Rust workspace, tooling, CI/CD, and platform infrastructure required before feature slices can be built.

### Scope
- Rust workspace with Cargo workspaces (app, api, worker, platform, test_support crates)
- Axum HTTP server skeleton
- PostgreSQL + PostGIS database setup
- S3-compatible object storage integration
- Database migration tooling (sqlx)
- OpenAPI documentation setup
- CI pipeline (build, test, lint, format)
- Docker Compose for local development
- Telemetry and structured logging
- Configuration management
- Error handling conventions

### Acceptance Criteria
- [ ] Cargo workspace compiles and all crates build
- [ ] API server starts and responds to health check
- [ ] Database migrations run successfully
- [ ] Object storage client connects
- [ ] CI pipeline passes on push
- [ ] Local development environment starts with one command
" | tail -1)
echo "Created Epic 0: Infrastructure - $EPIC0"


# Epic 1: Upload a GPX file
EPIC1=$(gh issue create --repo "$REPO" --title "Epic: Slice 1 - Upload a GPX file" \
  --label "epic,slice,importing" \
  --body "## Slice 1: Upload a GPX file

### User Outcome
> A user can upload a GPX file and see that it was received.

### Scope
- Authenticated upload request
- Object storage location
- File-size validation
- Checksum calculation
- Import record creation
- Upload-completed state transition
- Import-status endpoint
- OpenAPI definitions
- Integration tests

### Bounded Context
Activity Importing

### Import State Machine (relevant states)
\`\`\`
Requested → Uploading → Uploaded
\`\`\`

### Domain Events
- ImportRequested
- ArtifactUploaded

### Acceptance Criteria
- [ ] POST endpoint accepts GPX file upload with authentication
- [ ] File size is validated (reject oversized files)
- [ ] File is stored in S3-compatible object storage
- [ ] SHA-256 checksum is calculated and stored
- [ ] Import record is created in database
- [ ] Import status transitions: Requested → Uploading → Uploaded
- [ ] GET endpoint returns current import status
- [ ] Invalid state transitions are rejected
- [ ] OpenAPI spec documents both endpoints
- [ ] Integration tests cover happy path and error cases
" | tail -1)
echo "Created Epic 1: Upload GPX - $EPIC1"


# Epic 2: Import a GPX activity
EPIC2=$(gh issue create --repo "$REPO" --title "Epic: Slice 2 - Import a GPX activity" \
  --label "epic,slice,importing,recorded-activity,activity-catalog" \
  --body "## Slice 2: Import a GPX activity

### User Outcome
> A user can upload a GPX file and see a hike in the activity library.

### Scope
- Background GPX parsing worker
- GPX validation (schema, required elements)
- Normalized recorded route extraction
- Activity creation in catalog
- Original route version creation
- Source revision creation
- Transactional import completion
- Failure reporting
- Import state machine progression

### Bounded Contexts
- Activity Importing (orchestration)
- Recorded Activity (source revision, recorded track)
- Activity Catalog (activity creation)

### Import State Machine (relevant states)
\`\`\`
Uploaded → Validating → Queued → Parsing → Committing → Completed
                                                      → Failed
\`\`\`

### Domain Events
- ImportValidated
- ImportParsingStarted
- ImportedActivityCommitted
- ImportFailed

### Acceptance Criteria
- [ ] Worker picks up queued imports
- [ ] GPX file is validated (well-formed XML, required elements)
- [ ] Track points are extracted and normalized
- [ ] Recorded Activity with source revision is created
- [ ] Activity appears in Activity Catalog
- [ ] Initial route version is created from recorded track
- [ ] Import status reaches Completed on success
- [ ] Import status reaches Failed with reason on error
- [ ] All writes occur in single transaction
- [ ] Integration tests cover valid GPX, invalid GPX, and worker failure
" | tail -1)
echo "Created Epic 2: Import GPX - $EPIC2"


# Epic 3: View an imported route
EPIC3=$(gh issue create --repo "$REPO" --title "Epic: Slice 3 - View an imported route" \
  --label "epic,slice,activity-catalog,recorded-activity" \
  --body "## Slice 3: View an imported route

### User Outcome
> A user can open an imported hike and see its route.

### Scope
- Activity detail query endpoint
- Preview route geometry representation
- Recorded metadata display
- Authorization (owner-only access)
- List activities endpoint
- Performance instrumentation

### Bounded Contexts
- Activity Catalog (activity detail, list)
- Recorded Activity (route geometry)
- Identity & Access (authorization)

### Acceptance Criteria
- [ ] GET /activities returns paginated list for authenticated user
- [ ] GET /activities/:id returns full activity detail
- [ ] Response includes route geometry (GeoJSON or coordinate array)
- [ ] Response includes recorded metadata (distance, duration, elevation)
- [ ] Only activity owner can access their activities
- [ ] 404 returned for non-existent or unauthorized activities
- [ ] Response times are instrumented with telemetry
- [ ] OpenAPI spec documents list and detail endpoints
- [ ] Integration tests cover authorization and data retrieval
" | tail -1)
echo "Created Epic 3: View route - $EPIC3"


# Epic 4: Correct one route point
EPIC4=$(gh issue create --repo "$REPO" --title "Epic: Slice 4 - Correct one route point" \
  --label "epic,slice,route-editing" \
  --body "## Slice 4: Correct one route point

### User Outcome
> A user can move an inaccurate GPS point and undo the change.

### Scope
- Route draft creation from current route version
- MovePoint operation
- Draft revision tracking with optimistic concurrency
- Draft persistence
- Editor response model (geometry + revision)
- Undo for MovePoint operation
- Draft autosave

### Bounded Context
Route Editing

### Core Invariants
- A route draft belongs to one activity
- A route draft has one base route version
- Every accepted operation increments the draft revision
- Duplicate operation IDs are idempotent
- A stale expected revision is rejected (optimistic concurrency)
- Undo must produce deterministic geometry

### Acceptance Criteria
- [ ] POST /activities/:id/drafts creates a new route draft from current version
- [ ] POST /drafts/:id/operations applies MovePoint operation
- [ ] Response includes updated geometry and new revision number
- [ ] Stale revision in request is rejected with 409 Conflict
- [ ] Duplicate operation ID is handled idempotently
- [ ] POST /drafts/:id/undo reverses the last operation
- [ ] Undo restores exact previous geometry
- [ ] Draft state persists across requests
- [ ] Only draft owner can edit
- [ ] Domain tests verify concurrency and idempotency rules
- [ ] Integration tests cover create → edit → undo flow
" | tail -1)
echo "Created Epic 4: Correct point - $EPIC4"


# Epic 5: Replace an incorrect route section
EPIC5=$(gh issue create --repo "$REPO" --title "Epic: Slice 5 - Replace an incorrect route section" \
  --label "epic,slice,route-editing" \
  --body "## Slice 5: Replace an incorrect route section

### User Outcome
> A user can remove an incorrect GPS section and draw the correct path.

### Scope
- DeleteSection operation
- ReplaceSection operation
- Section selection model
- Replacement geometry validation
- Distance recalculation for preview
- Optimistic concurrency
- Undo and redo for section operations

### Bounded Context
Route Editing

### Operations
- DeleteSection: removes points between two indices
- ReplaceSection: removes section and inserts replacement geometry

### Acceptance Criteria
- [ ] DeleteSection removes specified point range
- [ ] ReplaceSection removes range and inserts new geometry
- [ ] Replacement geometry is validated (no self-intersections, reasonable bounds)
- [ ] Distance is recalculated in response for preview
- [ ] Undo restores deleted/replaced section exactly
- [ ] Redo re-applies the operation
- [ ] Optimistic concurrency still enforced
- [ ] Multiple operations can be chained (move + replace + move)
- [ ] Integration tests cover section operations with undo/redo
" | tail -1)
echo "Created Epic 5: Replace section - $EPIC5"


# Epic 6: Publish a corrected route
EPIC6=$(gh issue create --repo "$REPO" --title "Epic: Slice 6 - Publish a corrected route" \
  --label "epic,slice,route-versioning,route-editing,activity-catalog" \
  --body "## Slice 6: Publish a corrected route

### User Outcome
> A user can save a corrected route without changing the recorded route.

### Scope
- Draft geometry validation before publish
- Immutable route version creation
- Corrected distance/elevation calculation
- Activity current route version update
- Draft marked as published
- Audit event
- Transactional outbox for async reactions
- Parent-child version relationship

### Bounded Contexts
- Route Editing (draft validation, mark published)
- Route Versioning (create immutable version)
- Activity Catalog (update current route pointer)

### Publication Workflow
\`\`\`
Load draft → Check ownership & revision → Validate geometry
→ Calculate corrected stats → Create immutable version
→ Update activity pointer → Mark draft published
→ Write audit event → Commit transaction
\`\`\`

### Core Invariants
- Published route versions are immutable
- Version numbers are unique within an activity
- Publication is all-or-nothing (single transaction)
- Recorded route is never modified

### Domain Events
- RouteVersionPublished

### Acceptance Criteria
- [ ] POST /drafts/:id/publish creates immutable route version
- [ ] Corrected distance and elevation are calculated from geometry
- [ ] Activity's current route version is updated
- [ ] Draft is marked as published (cannot be edited further)
- [ ] Version has correct parent reference
- [ ] Route version list shows all published versions
- [ ] Recorded activity/route remains unchanged
- [ ] Invalid geometry prevents publication
- [ ] Entire operation is atomic (single DB transaction)
- [ ] RouteVersionPublished event written to outbox
- [ ] Integration tests cover publish flow and immutability
" | tail -1)
echo "Created Epic 6: Publish route - $EPIC6"


# Epic 7: Export the corrected route
EPIC7=$(gh issue create --repo "$REPO" --title "Epic: Slice 7 - Export the corrected route" \
  --label "epic,slice,exporting" \
  --body "## Slice 7: Export the corrected route

### User Outcome
> A user can download the corrected route as GPX.

### Scope
- Export request endpoint
- Background GPX generation worker
- GPX file creation from route version geometry
- Object storage for generated artifact
- Export status endpoint
- Authorized download endpoint
- Export expiration
- Round-trip test (import → edit → publish → export → validate GPX)

### Bounded Context
Route Exporting

### Export State Machine
\`\`\`
Requested → Generating → Available → Expired
                       → Failed
\`\`\`

### Core Rule
Export reads a specific immutable route version (not \"current display state\")

### Acceptance Criteria
- [ ] POST /activities/:id/exports requests export of specific route version
- [ ] Worker generates valid GPX from route version geometry
- [ ] Generated GPX is stored in object storage
- [ ] GET /exports/:id returns export status
- [ ] GET /exports/:id/download returns GPX file (when available)
- [ ] Only export owner can download
- [ ] Exports expire after configured duration
- [ ] Round-trip test: imported GPX → edit → publish → export → valid GPX
- [ ] Failed generation is reported with reason
- [ ] OpenAPI spec documents export endpoints
" | tail -1)
echo "Created Epic 7: Export route - $EPIC7"


# Epic 8: Detect an exact duplicate
EPIC8=$(gh issue create --repo "$REPO" --title "Epic: Slice 8 - Detect an exact duplicate" \
  --label "epic,slice,importing" \
  --body "## Slice 8: Detect an exact duplicate

### User Outcome
> Uploading the same file twice does not create duplicate hikes.

### Scope
- SHA-256 checksum matching on upload
- Idempotent import behavior
- Reference to existing activity when duplicate detected
- User-facing duplicate response
- Probable duplicate detection (future: similar timestamps/distance)

### Bounded Context
Activity Importing

### Detection Levels
1. **Exact duplicate**: Same file checksum → reject with reference to existing
2. **Probable duplicate** (future): Similar time/location → DuplicateReview state

### Acceptance Criteria
- [ ] Upload with matching checksum returns existing activity reference
- [ ] No new import record created for exact duplicate
- [ ] Response clearly indicates duplicate with link to existing activity
- [ ] Different files with same content are detected (checksum-based)
- [ ] Files with different content proceed normally
- [ ] Checksum comparison is efficient (indexed lookup)
- [ ] Integration tests cover duplicate detection scenarios
" | tail -1)
echo "Created Epic 8: Duplicate detection - $EPIC8"


# Epic 9: Connect Polar
EPIC9=$(gh issue create --repo "$REPO" --title "Epic: Slice 9 - Connect Polar" \
  --label "epic,slice,polar-integration" \
  --body "## Slice 9: Connect Polar

### User Outcome
> A user can securely connect and disconnect their Polar account.

### Scope
- OAuth 2.0 authorization start
- OAuth callback handling
- Encrypted token storage
- Connection status endpoint
- Token refresh mechanism
- Disconnect/revoke flow
- Error handling for OAuth failures

### Bounded Context
Polar Integration

### OAuth Flow
\`\`\`
User clicks Connect → Redirect to Polar OAuth
→ User authorizes → Callback with code
→ Exchange code for tokens → Store encrypted
→ Connection active
\`\`\`

### PolarConnection Aggregate States
\`\`\`
Disconnected → Authorizing → Connected → Disconnected
                           → Failed
\`\`\`

### Acceptance Criteria
- [ ] GET /polar/authorize redirects to Polar OAuth with correct params
- [ ] GET /polar/callback exchanges code for access/refresh tokens
- [ ] Tokens are encrypted at rest
- [ ] GET /polar/status returns connection state
- [ ] POST /polar/disconnect revokes tokens and clears connection
- [ ] Token refresh happens automatically when expired
- [ ] OAuth errors are handled gracefully with user-facing messages
- [ ] CSRF protection on OAuth flow (state parameter)
- [ ] Integration tests cover connect, status, and disconnect
" | tail -1)
echo "Created Epic 9: Connect Polar - $EPIC9"


# Epic 10: Import new Polar activities
EPIC10=$(gh issue create --repo "$REPO" --title "Epic: Slice 10 - Import new Polar activities" \
  --label "epic,slice,polar-integration,importing" \
  --body "## Slice 10: Import new Polar activities

### User Outcome
> Newly synchronized Polar hikes appear without overwriting corrections.

### Scope
- Synchronization job (background worker)
- Polar API exercise listing
- Provider activity identity mapping
- Standard import pipeline reuse
- Duplicate reconciliation (provider ID matching)
- Source revision creation for updated data
- Synchronization cursor tracking

### Bounded Contexts
- Polar Integration (sync job, Polar API, cursor)
- Activity Importing (standard pipeline)
- Recorded Activity (new source revisions)

### Integration Rule
Polar context does NOT directly mutate Activity or Route Version records.
It creates import requests through the Importing context.

\`\`\`
Polar sync → Discover exercise → Create provider import → Standard import pipeline
\`\`\`

### Key Invariant
New Polar data creates a new source revision — it never overwrites user corrections.

### Acceptance Criteria
- [ ] Sync job fetches new exercises from Polar API
- [ ] New exercises create imports through standard pipeline
- [ ] Already-imported exercises (by provider ID) are skipped
- [ ] Updated exercises create new source revision (not overwrite)
- [ ] User's route corrections remain untouched
- [ ] Sync cursor advances only after successful processing
- [ ] Failed sync does not lose unprocessed exercises
- [ ] Token refresh is handled during sync
- [ ] Integration tests cover new activity, duplicate, and update scenarios
" | tail -1)
echo "Created Epic 10: Polar import - $EPIC10"

echo ""
echo "=== Creating Implementation Issues ==="
echo ""


echo "--- Infrastructure Issues ---"

gh issue create --repo "$REPO" --title "Set up Rust Cargo workspace structure" \
  --label "infrastructure" \
  --body "## Task
Create the Cargo workspace with the following crates:
- \`crates/app\` — Domain modules (library crate)
- \`crates/api\` — Axum HTTP server (binary crate)
- \`crates/worker\` — Background job processor (binary crate)
- \`crates/platform\` — Shared infrastructure (database, storage, telemetry)
- \`crates/test_support\` — Test utilities and fixtures

## Acceptance Criteria
- [ ] \`cargo build\` compiles all crates
- [ ] \`cargo test\` runs (even if no tests yet)
- [ ] Workspace dependencies are defined in root Cargo.toml
- [ ] Each crate has appropriate dependencies declared

## Reference
Architecture doc section 15: Rust workspace structure"

gh issue create --repo "$REPO" --title "Set up Axum HTTP server with health check" \
  --label "infrastructure,api" \
  --body "## Task
Configure Axum server in \`crates/api\` with:
- Router setup
- Health check endpoint (GET /health)
- Graceful shutdown
- Configuration loading (environment variables)
- Port binding

## Acceptance Criteria
- [ ] Server starts and listens on configured port
- [ ] GET /health returns 200 OK
- [ ] Server shuts down gracefully on SIGTERM
- [ ] Port is configurable via environment variable"

gh issue create --repo "$REPO" --title "Set up PostgreSQL with migrations (sqlx)" \
  --label "infrastructure,persistence" \
  --body "## Task
Configure PostgreSQL database connectivity and migration tooling:
- sqlx setup with compile-time checked queries
- Migration directory structure per bounded context
- Initial migration: create database schemas (identity, activity_catalog, imports, recorded_activity, route_editing, route_versioning, exports, polar_integration, platform)
- Connection pooling
- PostGIS extension enablement

## Acceptance Criteria
- [ ] Database connection pool is established on startup
- [ ] Migrations run automatically or via CLI
- [ ] All context schemas are created
- [ ] PostGIS extension is enabled
- [ ] Connection string is configurable"


gh issue create --repo "$REPO" --title "Set up S3-compatible object storage client" \
  --label "infrastructure" \
  --body "## Task
Configure S3-compatible object storage client in \`crates/platform\`:
- Client initialization with configurable endpoint
- Upload/download/delete operations
- Presigned URL generation
- Local development support (MinIO)

## Acceptance Criteria
- [ ] Object storage client connects to configured endpoint
- [ ] Upload, download, and delete operations work
- [ ] Presigned URLs can be generated
- [ ] Works with MinIO for local development"

gh issue create --repo "$REPO" --title "Set up Docker Compose for local development" \
  --label "infrastructure" \
  --body "## Task
Create Docker Compose configuration for local development:
- PostgreSQL + PostGIS
- MinIO (S3-compatible storage)
- Optional: pgAdmin for database inspection
- Volume mounts for data persistence
- Environment variable configuration

## Acceptance Criteria
- [ ] \`docker compose up\` starts all services
- [ ] PostgreSQL is accessible on configured port
- [ ] MinIO is accessible with configured credentials
- [ ] Data persists across restarts
- [ ] README documents local setup steps"

gh issue create --repo "$REPO" --title "Set up CI pipeline (GitHub Actions)" \
  --label "infrastructure" \
  --body "## Task
Create GitHub Actions CI pipeline:
- Build all crates
- Run all tests
- Run clippy lints
- Check formatting (rustfmt)
- Cache dependencies
- PostgreSQL service container for integration tests

## Acceptance Criteria
- [ ] CI runs on push to main and on PRs
- [ ] Build, test, lint, format checks all pass
- [ ] Dependencies are cached between runs
- [ ] Integration tests have database access
- [ ] Pipeline completes in reasonable time"


gh issue create --repo "$REPO" --title "Set up structured logging and telemetry" \
  --label "infrastructure" \
  --body "## Task
Configure structured logging and observability in \`crates/platform\`:
- tracing + tracing-subscriber for structured logs
- Request ID propagation
- Request/response logging middleware
- Span creation for domain operations
- JSON log output for production

## Acceptance Criteria
- [ ] All HTTP requests are logged with method, path, status, duration
- [ ] Request IDs are generated and propagated
- [ ] Domain operations create named spans
- [ ] Log level is configurable
- [ ] JSON output mode for production deployment"

gh issue create --repo "$REPO" --title "Set up error handling conventions" \
  --label "infrastructure,api" \
  --body "## Task
Establish consistent error handling across the application:
- Domain error types per context
- API error response format (structured JSON)
- Error code enumeration
- Mapping from domain errors to HTTP status codes
- Error logging with context

## Error Response Format
\`\`\`json
{
  \"error\": {
    \"code\": \"STALE_REVISION\",
    \"message\": \"The draft has been modified since your last read\",
    \"details\": { \"expected\": 3, \"actual\": 5 }
  }
}
\`\`\`

## Acceptance Criteria
- [ ] Consistent error response schema
- [ ] Domain errors map cleanly to HTTP codes
- [ ] Errors are logged with request context
- [ ] Internal errors don't leak implementation details
- [ ] Error types are defined per bounded context"

gh issue create --repo "$REPO" --title "Set up OpenAPI documentation" \
  --label "infrastructure,api" \
  --body "## Task
Configure OpenAPI documentation generation:
- utoipa or similar for Rust OpenAPI generation
- Swagger UI endpoint for development
- OpenAPI spec export (JSON/YAML)
- Schema definitions for shared types

## Acceptance Criteria
- [ ] OpenAPI spec is generated from code annotations
- [ ] Swagger UI available at /docs in development
- [ ] Spec can be exported as JSON file
- [ ] Common types (pagination, errors) are documented"


gh issue create --repo "$REPO" --title "Set up authentication middleware" \
  --label "infrastructure,identity,api" \
  --body "## Task
Implement authentication middleware for the API:
- Session-based authentication
- Extract authenticated user from request
- Actor extractor for handlers
- Unauthenticated request rejection

## Acceptance Criteria
- [ ] Middleware extracts session from request
- [ ] Actor type available in route handlers
- [ ] Unauthenticated requests receive 401
- [ ] Session validation is efficient (no extra DB call per request if possible)"

gh issue create --repo "$REPO" --title "Set up background job queue infrastructure" \
  --label "infrastructure,worker" \
  --body "## Task
Create job queue infrastructure for background processing:
- Job queue abstraction in \`crates/platform\`
- PostgreSQL-based job queue (or compatible)
- Job scheduling and polling
- Retry logic with backoff
- Dead letter handling
- Worker process in \`crates/worker\`

## Acceptance Criteria
- [ ] Jobs can be enqueued from API process
- [ ] Worker picks up and processes jobs
- [ ] Failed jobs are retried with exponential backoff
- [ ] Dead jobs are moved to dead letter queue
- [ ] Job status is queryable
- [ ] Multiple job types supported"

gh issue create --repo "$REPO" --title "Set up transactional outbox" \
  --label "infrastructure,persistence" \
  --body "## Task
Implement transactional outbox pattern:
- Outbox table in platform schema
- Domain events written in same transaction as state change
- Background dispatcher reads and publishes outbox entries
- Idempotent event handlers
- At-least-once delivery guarantee

## Acceptance Criteria
- [ ] Events are written to outbox in same transaction
- [ ] Dispatcher polls and processes outbox entries
- [ ] Processed entries are marked as dispatched
- [ ] Handlers receive event and can process idempotently
- [ ] No events are lost if application crashes after commit"

echo ""
echo "--- Slice 1 Issues: Upload GPX ---"


gh issue create --repo "$REPO" --title "Slice 1: Import domain model and state machine" \
  --label "slice,importing,domain" \
  --body "## Task
Implement the Import aggregate domain model:
- ImportId, OwnerId, SourceArtifactId value objects
- ImportFormat enum
- ImportStatus state machine (Requested → Uploading → Uploaded)
- Checksum value object
- Invalid state transition rejection
- Domain events: ImportRequested, ArtifactUploaded

## Acceptance Criteria
- [ ] Import aggregate enforces valid state transitions
- [ ] Invalid transitions return domain error
- [ ] Domain events are produced on state changes
- [ ] Unit tests verify all valid and invalid transitions
- [ ] Value objects enforce their invariants"

gh issue create --repo "$REPO" --title "Slice 1: StartImport feature (request upload)" \
  --label "slice,importing,api" \
  --body "## Task
Implement the StartImport vertical slice:
- POST /imports endpoint
- Request validation (file name, content type, size limit)
- Create Import record in Requested state
- Generate S3 presigned upload URL
- Return import ID and upload URL

## Request
\`\`\`json
{
  \"filename\": \"morning_hike.gpx\",
  \"content_type\": \"application/gpx+xml\",
  \"file_size_bytes\": 245000
}
\`\`\`

## Response
\`\`\`json
{
  \"import_id\": \"uuid\",
  \"upload_url\": \"https://...\",
  \"status\": \"requested\"
}
\`\`\`

## Acceptance Criteria
- [ ] Authenticated POST creates import record
- [ ] File size over limit is rejected (400)
- [ ] Presigned upload URL is generated
- [ ] Import status is Requested
- [ ] OpenAPI spec documents endpoint"

gh issue create --repo "$REPO" --title "Slice 1: CompleteUpload feature (confirm upload)" \
  --label "slice,importing,api" \
  --body "## Task
Implement the CompleteUpload vertical slice:
- POST /imports/:id/upload-complete endpoint
- Verify file exists in object storage
- Calculate SHA-256 checksum
- Transition import to Uploaded state
- Emit ArtifactUploaded event

## Acceptance Criteria
- [ ] Endpoint transitions import from Uploading to Uploaded
- [ ] File existence in S3 is verified
- [ ] SHA-256 checksum is calculated and stored
- [ ] Invalid state transition returns 409
- [ ] Only import owner can complete upload
- [ ] Integration test covers full upload flow"


gh issue create --repo "$REPO" --title "Slice 1: GetImportStatus feature" \
  --label "slice,importing,api" \
  --body "## Task
Implement the GetImportStatus query:
- GET /imports/:id endpoint
- Return current import state and metadata
- Authorization (owner only)

## Response
\`\`\`json
{
  \"import_id\": \"uuid\",
  \"status\": \"uploaded\",
  \"filename\": \"morning_hike.gpx\",
  \"file_size_bytes\": 245000,
  \"checksum\": \"sha256:...\",
  \"created_at\": \"2025-01-15T10:00:00Z\",
  \"updated_at\": \"2025-01-15T10:00:05Z\"
}
\`\`\`

## Acceptance Criteria
- [ ] Returns current import status
- [ ] Only owner can query their imports
- [ ] 404 for non-existent or unauthorized imports
- [ ] OpenAPI spec documents response schema"

gh issue create --repo "$REPO" --title "Slice 1: Import persistence layer" \
  --label "slice,importing,persistence" \
  --body "## Task
Implement persistence for the Import aggregate:
- imports.imports table migration
- imports.import_events table (outbox)
- SqlxImportStore implementation
- CRUD operations for Import aggregate

## Schema
\`\`\`sql
CREATE TABLE imports.imports (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    file_size_bytes BIGINT NOT NULL,
    status TEXT NOT NULL,
    checksum TEXT,
    object_storage_key TEXT,
    failure_reason TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
\`\`\`

## Acceptance Criteria
- [ ] Migration creates imports schema and table
- [ ] Import can be inserted, loaded, and updated
- [ ] Status transitions are persisted correctly
- [ ] Checksum is stored after upload completion
- [ ] Integration tests verify persistence"

echo ""
echo "--- Slice 2 Issues: Import GPX Activity ---"


gh issue create --repo "$REPO" --title "Slice 2: GPX parser implementation" \
  --label "slice,importing,worker" \
  --body "## Task
Implement GPX file parsing:
- GPX XML validation (well-formed, schema compliant)
- Track extraction (trk/trkseg/trkpt)
- Coordinate extraction (lat, lon, ele, time)
- Metadata extraction (name, time, description)
- Segment boundary detection
- Error reporting for malformed files

## Acceptance Criteria
- [ ] Valid GPX 1.1 files are parsed correctly
- [ ] Track points with lat/lon/ele/time are extracted
- [ ] Multiple segments are preserved
- [ ] Activity metadata (name, time) is extracted
- [ ] Malformed XML returns clear error
- [ ] Missing required elements return clear error
- [ ] Unit tests with sample GPX files"

gh issue create --repo "$REPO" --title "Slice 2: Import processing worker job" \
  --label "slice,importing,worker" \
  --body "## Task
Implement background import processing:
- Worker picks up Uploaded imports from job queue
- Validates GPX file
- Parses track data
- Transitions through: Validating → Queued → Parsing → Committing → Completed
- Handles failures at each stage

## Acceptance Criteria
- [ ] Worker processes queued import jobs
- [ ] GPX validation runs before parsing
- [ ] Invalid files transition to Failed with reason
- [ ] Valid files are fully parsed
- [ ] State transitions are persisted at each step
- [ ] Worker is idempotent (safe to retry)
- [ ] Integration test with real GPX file"

gh issue create --repo "$REPO" --title "Slice 2: Create Recorded Activity from parsed GPX" \
  --label "slice,recorded-activity,domain" \
  --body "## Task
Implement Recorded Activity creation from parsed import:
- SourceRevision aggregate creation
- RecordedTrack with segments and coordinates
- RecordedStatistics calculation (distance, duration, elevation gain)
- Provider and parser version tracking
- Immutability enforcement after acceptance

## Acceptance Criteria
- [ ] Source revision is created with parsed track data
- [ ] Recorded statistics are calculated correctly
- [ ] Distance calculation uses haversine/vincenty
- [ ] Elevation gain uses appropriate smoothing
- [ ] Source revision is immutable after creation
- [ ] Domain tests verify statistics calculations"


gh issue create --repo "$REPO" --title "Slice 2: Create Activity in catalog from import" \
  --label "slice,activity-catalog,domain" \
  --body "## Task
Implement Activity creation in the catalog when an import completes:
- Activity aggregate creation
- Title from GPX metadata (or filename fallback)
- Activity type (hike)
- Start/end times from track data
- Initial route version reference
- Integration handler for ImportedActivityCommitted event

## Acceptance Criteria
- [ ] Activity is created with correct metadata
- [ ] Title comes from GPX name element (fallback to filename)
- [ ] Start/end times come from first/last track point
- [ ] Current route version points to initial version
- [ ] Activity appears in owner's activity list
- [ ] Domain tests verify creation logic"

gh issue create --repo "$REPO" --title "Slice 2: Transactional import completion (CommitImportedActivity)" \
  --label "slice,importing,persistence" \
  --body "## Task
Implement atomic import completion:
- Single transaction creates: Activity + Source Revision + Route Version + Import completion
- Import transitions to Completed state
- ImportedActivityCommitted event emitted
- Rollback on any failure

## Transaction Contents
1. Create source revision with recorded track
2. Create initial route version from recorded geometry
3. Create activity in catalog with references
4. Mark import as Completed
5. Write ImportedActivityCommitted to outbox

## Acceptance Criteria
- [ ] All entities created in single transaction
- [ ] Partial failure rolls back everything
- [ ] Import status is Completed after success
- [ ] ImportedActivityCommitted event in outbox
- [ ] Integration test verifies atomicity"

echo ""
echo "--- Slice 3 Issues: View Imported Route ---"


gh issue create --repo "$REPO" --title "Slice 3: ListActivities query" \
  --label "slice,activity-catalog,api" \
  --body "## Task
Implement activity listing for authenticated user:
- GET /activities endpoint
- Paginated response
- Filter by lifecycle state (active, archived, deleted)
- Sort by start time (descending)
- Summary data per activity (title, type, date, distance)

## Response
\`\`\`json
{
  \"activities\": [...],
  \"pagination\": { \"page\": 1, \"per_page\": 20, \"total\": 42 }
}
\`\`\`

## Acceptance Criteria
- [ ] Returns only activities owned by authenticated user
- [ ] Pagination works correctly
- [ ] Activities sorted by start time descending
- [ ] Summary includes title, type, date, distance
- [ ] Empty list returns empty array (not error)
- [ ] OpenAPI spec documents endpoint"

gh issue create --repo "$REPO" --title "Slice 3: GetActivity detail query" \
  --label "slice,activity-catalog,api" \
  --body "## Task
Implement activity detail endpoint:
- GET /activities/:id endpoint
- Full activity metadata
- Current route version geometry (for map rendering)
- Recorded summary statistics
- Corrected summary statistics (if different)
- Authorization check

## Response includes
- Activity metadata (title, type, dates, lifecycle)
- Route geometry (GeoJSON LineString or coordinate array)
- Recorded stats (distance, duration, elevation, from device)
- Corrected stats (if route has been edited)
- Route version info (version number, published date)

## Acceptance Criteria
- [ ] Returns full activity detail for owner
- [ ] Route geometry included for map rendering
- [ ] Both recorded and corrected stats shown
- [ ] 403/404 for non-owner access
- [ ] Response is optimized (no unnecessary data)
- [ ] OpenAPI spec documents response schema"

gh issue create --repo "$REPO" --title "Slice 3: Activity and route persistence queries" \
  --label "slice,activity-catalog,persistence" \
  --body "## Task
Implement read-optimized queries for activity display:
- Activity list query with pagination
- Activity detail query with joins
- Route geometry retrieval
- Consider read model if query becomes complex

## Acceptance Criteria
- [ ] List query is efficient with pagination
- [ ] Detail query retrieves all needed data
- [ ] Route geometry loaded efficiently (consider size)
- [ ] Queries use appropriate indexes
- [ ] Integration tests verify data retrieval"

echo ""
echo "--- Slice 4 Issues: Correct One Route Point ---"


gh issue create --repo "$REPO" --title "Slice 4: RouteDraft domain model" \
  --label "slice,route-editing,domain" \
  --body "## Task
Implement the RouteDraft aggregate:
- RouteDraftId, Revision value objects
- DraftState enum (Active, Published, Discarded)
- Geometry representation (ordered coordinate list)
- Operation application with revision increment
- Optimistic concurrency (expected revision check)
- Operation ID idempotency
- Undo stack

## Core Invariants
- Every accepted operation increments revision
- Duplicate operation IDs are idempotent
- Stale expected revision is rejected
- Published/discarded drafts cannot be edited

## Acceptance Criteria
- [ ] Draft tracks revision correctly
- [ ] Stale revision check rejects concurrent edits
- [ ] Duplicate operations are idempotent
- [ ] Published draft rejects new operations
- [ ] Undo stack tracks applied operations
- [ ] Unit tests verify all invariants"

gh issue create --repo "$REPO" --title "Slice 4: MovePoint operation" \
  --label "slice,route-editing,domain" \
  --body "## Task
Implement the MovePoint route editing operation:
- Identify point by index in geometry
- Move to new coordinates (lat, lon, optional ele)
- Validate target coordinates (bounds check)
- Record operation for undo
- Return updated geometry

## Acceptance Criteria
- [ ] Point at specified index is moved to new position
- [ ] Invalid index returns error
- [ ] Coordinates outside valid bounds are rejected
- [ ] Operation is recorded in undo stack
- [ ] Undo restores original point position exactly
- [ ] Unit tests cover edge cases (first point, last point, out of bounds)"

gh issue create --repo "$REPO" --title "Slice 4: CreateRouteDraft feature" \
  --label "slice,route-editing,api" \
  --body "## Task
Implement route draft creation:
- POST /activities/:id/drafts endpoint
- Load current route version as base
- Create draft with base geometry
- Return draft ID, geometry, and initial revision (0)

## Acceptance Criteria
- [ ] Draft is created from activity's current route version
- [ ] Base version ID is recorded
- [ ] Initial revision is 0
- [ ] Only activity owner can create draft
- [ ] Only one active draft per activity (or configurable)
- [ ] Response includes geometry for editor initialization"


gh issue create --repo "$REPO" --title "Slice 4: ApplyRouteOperation feature" \
  --label "slice,route-editing,api" \
  --body "## Task
Implement operation application endpoint:
- POST /drafts/:id/operations endpoint
- Validate expected revision (optimistic concurrency)
- Apply operation to geometry
- Persist updated draft
- Return new geometry and revision

## Request
\`\`\`json
{
  \"operation_id\": \"uuid\",
  \"expected_revision\": 0,
  \"operation\": {
    \"type\": \"move_point\",
    \"point_index\": 42,
    \"new_position\": { \"lat\": 60.1234, \"lon\": 24.5678 }
  }
}
\`\`\`

## Acceptance Criteria
- [ ] Operation applied when revision matches
- [ ] 409 Conflict when revision is stale
- [ ] Operation ID deduplication (idempotent)
- [ ] New revision returned in response
- [ ] Updated geometry returned for editor refresh
- [ ] Only draft owner can apply operations"

gh issue create --repo "$REPO" --title "Slice 4: UndoRouteOperation feature" \
  --label "slice,route-editing,api" \
  --body "## Task
Implement undo for route operations:
- POST /drafts/:id/undo endpoint
- Pop last operation from applied stack
- Reverse geometry change
- Push to redo stack
- Increment revision

## Acceptance Criteria
- [ ] Undo reverses the last applied operation
- [ ] Geometry matches state before the operation
- [ ] Operation moves to redo stack
- [ ] Revision increments (undo is also a change)
- [ ] Undo on empty stack returns 422
- [ ] Multiple undos work correctly (stack behavior)"

gh issue create --repo "$REPO" --title "Slice 4: Route editing persistence" \
  --label "slice,route-editing,persistence" \
  --body "## Task
Implement persistence for route drafts:
- route_editing.route_drafts table
- route_editing.draft_operations table (for undo/redo)
- Store current geometry snapshot
- Store operation history

## Schema
\`\`\`sql
CREATE TABLE route_editing.route_drafts (
    id UUID PRIMARY KEY,
    activity_id UUID NOT NULL,
    owner_id UUID NOT NULL,
    base_version_id UUID NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0,
    geometry JSONB NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
\`\`\`

## Acceptance Criteria
- [ ] Draft persists across requests
- [ ] Geometry is stored and loaded correctly
- [ ] Operation history supports undo/redo
- [ ] Revision is consistent after reload"

echo ""
echo "--- Slice 5 Issues: Replace Route Section ---"


gh issue create --repo "$REPO" --title "Slice 5: DeleteSection operation" \
  --label "slice,route-editing,domain" \
  --body "## Task
Implement DeleteSection route editing operation:
- Define section by start and end point indices
- Remove all points in range (inclusive)
- Validate section bounds
- Record deleted points for undo
- Handle edge cases (empty result, single point)

## Acceptance Criteria
- [ ] Points between start and end indices are removed
- [ ] Invalid indices return error
- [ ] Deleting all points is prevented
- [ ] Deleted points stored for undo restoration
- [ ] Undo restores exact deleted points in correct position
- [ ] Unit tests cover boundary cases"

gh issue create --repo "$REPO" --title "Slice 5: ReplaceSection operation" \
  --label "slice,route-editing,domain" \
  --body "## Task
Implement ReplaceSection route editing operation:
- Define section to replace (start/end indices)
- Provide replacement geometry (array of coordinates)
- Validate replacement geometry
- Remove old section and insert new geometry
- Record both old and new for undo

## Validation Rules
- Replacement must have at least 2 points
- Coordinates must be valid (lat -90..90, lon -180..180)
- No self-intersections (basic check)

## Acceptance Criteria
- [ ] Section is replaced with new geometry
- [ ] Replacement geometry is validated
- [ ] Invalid geometry is rejected with clear error
- [ ] Undo restores original section exactly
- [ ] Redo re-applies replacement
- [ ] Distance recalculated in response preview"

gh issue create --repo "$REPO" --title "Slice 5: Redo operation support" \
  --label "slice,route-editing,domain" \
  --body "## Task
Implement redo functionality:
- POST /drafts/:id/redo endpoint
- Pop from redo stack
- Re-apply operation to geometry
- Push to applied stack
- Clear redo stack when new operation applied

## Acceptance Criteria
- [ ] Redo re-applies last undone operation
- [ ] Geometry matches state after original application
- [ ] Redo on empty stack returns 422
- [ ] New operation clears redo stack
- [ ] Multiple redo operations work (stack behavior)
- [ ] Undo → Redo produces identical geometry"

echo ""
echo "--- Slice 6 Issues: Publish Corrected Route ---"


gh issue create --repo "$REPO" --title "Slice 6: RouteVersion domain model" \
  --label "slice,route-versioning,domain" \
  --body "## Task
Implement the RouteVersion aggregate:
- RouteVersionId value object
- Immutable geometry storage
- Parent version reference
- Version number (unique per activity)
- CorrectedStatistics (distance, elevation, duration)
- CalculationVersion for reproducibility
- EditSummary, CreatedBy, CreatedAt

## Core Invariants
- Published versions are immutable (no setters)
- Parent belongs to same activity
- Version numbers unique within activity

## Acceptance Criteria
- [ ] RouteVersion cannot be modified after creation
- [ ] Version number uniqueness enforced
- [ ] Parent-child relationship tracked
- [ ] Statistics calculated from geometry
- [ ] Domain tests verify immutability"

gh issue create --repo "$REPO" --title "Slice 6: PublishRouteDraft feature" \
  --label "slice,route-versioning,route-editing,api" \
  --body "## Task
Implement the PublishRouteDraft vertical slice:
- POST /drafts/:id/publish endpoint
- Validate draft geometry
- Calculate corrected statistics
- Create immutable route version
- Update activity current route pointer
- Mark draft as published
- Write RouteVersionPublished to outbox
- All in single transaction

## Acceptance Criteria
- [ ] Valid draft produces immutable route version
- [ ] Invalid geometry rejects publication (422)
- [ ] Statistics calculated from published geometry
- [ ] Activity points to new current version
- [ ] Draft cannot be edited after publication
- [ ] RouteVersionPublished event in outbox
- [ ] Single transaction (atomic)
- [ ] Integration test covers full publish flow"

gh issue create --repo "$REPO" --title "Slice 6: Route version history (ListRouteVersions)" \
  --label "slice,route-versioning,api" \
  --body "## Task
Implement route version listing:
- GET /activities/:id/versions endpoint
- List all published versions for an activity
- Include version number, date, parent, summary stats
- Mark current version

## Acceptance Criteria
- [ ] Returns all versions ordered by version number
- [ ] Current version is indicated
- [ ] Each version shows stats and creation date
- [ ] Only activity owner can view versions
- [ ] OpenAPI spec documents endpoint"

gh issue create --repo "$REPO" --title "Slice 6: Route versioning persistence" \
  --label "slice,route-versioning,persistence" \
  --body "## Task
Implement persistence for route versions:
- route_versioning.route_versions table
- Geometry storage (PostGIS or JSONB)
- Statistics storage
- Version number generation

## Schema
\`\`\`sql
CREATE TABLE route_versioning.route_versions (
    id UUID PRIMARY KEY,
    activity_id UUID NOT NULL,
    parent_version_id UUID,
    version_number INTEGER NOT NULL,
    geometry JSONB NOT NULL,
    distance_meters DOUBLE PRECISION,
    elevation_gain_meters DOUBLE PRECISION,
    calculation_version TEXT NOT NULL,
    edit_summary TEXT,
    created_by UUID NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    UNIQUE (activity_id, version_number)
);
\`\`\`

## Acceptance Criteria
- [ ] Route versions persist correctly
- [ ] Geometry stored and retrieved without loss
- [ ] Version numbers are unique per activity
- [ ] Parent references are valid"

echo ""
echo "--- Slice 7 Issues: Export Corrected Route ---"


gh issue create --repo "$REPO" --title "Slice 7: ExportJob domain model" \
  --label "slice,exporting,domain" \
  --body "## Task
Implement the ExportJob aggregate:
- ExportJobId value object
- ExportFormat enum (GPX initially)
- ExportStatus state machine (Requested → Generating → Available → Expired / Failed)
- ObjectStorageKey, Checksum, Expiration
- State transition enforcement

## Acceptance Criteria
- [ ] State machine enforces valid transitions
- [ ] Invalid transitions are rejected
- [ ] Expiration is set on creation
- [ ] Domain tests verify state machine"

gh issue create --repo "$REPO" --title "Slice 7: RequestGpxExport feature" \
  --label "slice,exporting,api" \
  --body "## Task
Implement export request endpoint:
- POST /activities/:id/exports endpoint
- Specify route version to export
- Create ExportJob in Requested state
- Queue generation job

## Request
\`\`\`json
{
  \"route_version_id\": \"uuid\",
  \"format\": \"gpx\"
}
\`\`\`

## Acceptance Criteria
- [ ] Export job created for specified version
- [ ] Job queued for background processing
- [ ] Only activity owner can request export
- [ ] Returns export job ID and status
- [ ] OpenAPI spec documents endpoint"

gh issue create --repo "$REPO" --title "Slice 7: GPX generation worker" \
  --label "slice,exporting,worker" \
  --body "## Task
Implement background GPX file generation:
- Worker picks up export jobs
- Loads immutable route version geometry
- Generates valid GPX 1.1 XML
- Stores generated file in object storage
- Transitions export to Available
- Calculates file checksum

## GPX Output Requirements
- Valid GPX 1.1 schema
- Track with segments and points
- Coordinates with lat/lon/ele where available
- Metadata (name, time)

## Acceptance Criteria
- [ ] Worker generates valid GPX from route version
- [ ] GPX validates against GPX 1.1 schema
- [ ] File stored in object storage
- [ ] Export status transitions to Available
- [ ] Failed generation transitions to Failed with reason
- [ ] Round-trip test: parse generated GPX back"


gh issue create --repo "$REPO" --title "Slice 7: DownloadExport feature" \
  --label "slice,exporting,api" \
  --body "## Task
Implement export download:
- GET /exports/:id/download endpoint
- Verify export is in Available state
- Verify not expired
- Generate presigned download URL or stream file
- Authorization check

## Acceptance Criteria
- [ ] Download works when export is Available
- [ ] Expired exports return 410 Gone
- [ ] Pending exports return 202 (not ready)
- [ ] Only export owner can download
- [ ] File is served with correct content type
- [ ] Integration test downloads and validates GPX"

echo ""
echo "--- Slice 8 Issues: Detect Exact Duplicate ---"

gh issue create --repo "$REPO" --title "Slice 8: Checksum-based duplicate detection" \
  --label "slice,importing,domain" \
  --body "## Task
Implement exact duplicate detection in import flow:
- On upload completion, check if checksum already exists
- If match found, return reference to existing activity
- No new import created for exact duplicates
- Efficient indexed lookup on checksum column

## Acceptance Criteria
- [ ] Duplicate checksum detected before import processing
- [ ] Response includes reference to existing activity
- [ ] No duplicate activity created
- [ ] Detection is efficient (indexed query)
- [ ] Different files proceed normally
- [ ] Unit and integration tests cover duplicate scenarios"

gh issue create --repo "$REPO" --title "Slice 8: Duplicate response and user feedback" \
  --label "slice,importing,api" \
  --body "## Task
Implement user-facing duplicate detection response:
- API returns clear duplicate indication
- Include link/reference to existing activity
- Appropriate HTTP status (409 Conflict or 200 with duplicate flag)

## Response
\`\`\`json
{
  \"duplicate\": true,
  \"existing_activity_id\": \"uuid\",
  \"message\": \"This file has already been imported\"
}
\`\`\`

## Acceptance Criteria
- [ ] Clear indication that file is a duplicate
- [ ] Existing activity ID provided
- [ ] User can navigate to existing activity
- [ ] OpenAPI documents duplicate response"

echo ""
echo "--- Slice 9 Issues: Connect Polar ---"


gh issue create --repo "$REPO" --title "Slice 9: PolarConnection domain model" \
  --label "slice,polar-integration,domain" \
  --body "## Task
Implement the PolarConnection aggregate:
- ConnectionStatus states (Disconnected, Authorizing, Connected, Failed)
- Encrypted token storage model
- Token expiration tracking
- Synchronization cursor
- State transition rules

## Acceptance Criteria
- [ ] State machine enforces valid transitions
- [ ] Tokens modeled as encrypted values
- [ ] Expiration tracked for refresh decisions
- [ ] Domain tests verify state transitions"

gh issue create --repo "$REPO" --title "Slice 9: StartPolarAuthorization feature" \
  --label "slice,polar-integration,api" \
  --body "## Task
Implement OAuth authorization start:
- GET /polar/authorize endpoint
- Generate state parameter (CSRF protection)
- Store state in session/database
- Redirect to Polar OAuth authorization URL
- Include required scopes

## Acceptance Criteria
- [ ] Redirect to Polar with correct client_id, redirect_uri, scope
- [ ] State parameter generated and stored
- [ ] Only authenticated users can start authorization
- [ ] Existing connection prompts re-authorization or error"

gh issue create --repo "$REPO" --title "Slice 9: CompletePolarAuthorization (OAuth callback)" \
  --label "slice,polar-integration,api" \
  --body "## Task
Implement OAuth callback handling:
- GET /polar/callback endpoint
- Validate state parameter (CSRF)
- Exchange authorization code for tokens
- Encrypt and store tokens
- Transition connection to Connected
- Handle OAuth errors gracefully

## Acceptance Criteria
- [ ] Valid callback exchanges code for tokens
- [ ] State mismatch returns error (CSRF protection)
- [ ] Tokens are encrypted before storage
- [ ] Connection status transitions to Connected
- [ ] OAuth errors (denied, expired) handled gracefully
- [ ] Integration test covers happy path"

gh issue create --repo "$REPO" --title "Slice 9: GetPolarConnectionStatus feature" \
  --label "slice,polar-integration,api" \
  --body "## Task
Implement connection status query:
- GET /polar/status endpoint
- Return current connection state
- Last successful sync timestamp
- Last failure info (if applicable)

## Acceptance Criteria
- [ ] Returns current connection status
- [ ] Shows last sync time if connected
- [ ] Shows error details if failed
- [ ] Works for disconnected state (never connected)"

gh issue create --repo "$REPO" --title "Slice 9: DisconnectPolar feature" \
  --label "slice,polar-integration,api" \
  --body "## Task
Implement Polar disconnection:
- POST /polar/disconnect endpoint
- Revoke tokens with Polar API (best effort)
- Clear stored tokens
- Reset connection state to Disconnected
- Clear sync cursor

## Acceptance Criteria
- [ ] Tokens are revoked with Polar (best effort)
- [ ] Stored tokens are deleted
- [ ] Connection state becomes Disconnected
- [ ] Sync cursor is cleared
- [ ] Previously imported activities are NOT deleted"


gh issue create --repo "$REPO" --title "Slice 9: Polar token encryption and refresh" \
  --label "slice,polar-integration,persistence" \
  --body "## Task
Implement token security and lifecycle:
- Encrypt tokens at rest (AES-256-GCM or similar)
- Automatic token refresh when expired
- Refresh failure handling
- Token rotation support

## Acceptance Criteria
- [ ] Tokens encrypted in database (not plaintext)
- [ ] Encryption key managed via configuration
- [ ] Expired tokens trigger automatic refresh
- [ ] Refresh failure transitions to appropriate state
- [ ] Integration test verifies encryption round-trip"

echo ""
echo "--- Slice 10 Issues: Import New Polar Activities ---"

gh issue create --repo "$REPO" --title "Slice 10: Polar synchronization job" \
  --label "slice,polar-integration,worker" \
  --body "## Task
Implement background Polar sync worker:
- Periodic or triggered sync job
- Fetch exercises from Polar API (using cursor)
- Identify new/updated exercises
- Advance cursor only after successful processing
- Token refresh during sync if needed

## Acceptance Criteria
- [ ] Sync job fetches exercises since last cursor
- [ ] New exercises are identified
- [ ] Cursor advances only after successful processing
- [ ] Token refreshed if expired during sync
- [ ] Failed sync preserves cursor position
- [ ] Integration test with mocked Polar API"

gh issue create --repo "$REPO" --title "Slice 10: Create provider imports from Polar exercises" \
  --label "slice,polar-integration,importing" \
  --body "## Task
Implement Polar → Import pipeline integration:
- Map Polar exercise to import request
- Use provider activity ID for deduplication
- Route through standard import pipeline
- Handle exercise data format (FIT/TCX/GPS)

## Integration Rule
Polar context creates import requests — it does NOT directly create activities.

## Acceptance Criteria
- [ ] Polar exercises create standard import requests
- [ ] Provider ID used for duplicate detection
- [ ] Already-imported exercises are skipped
- [ ] Import uses standard pipeline (same as GPX upload)
- [ ] Provider source tracked in source revision"

gh issue create --repo "$REPO" --title "Slice 10: Source revision for updated Polar data" \
  --label "slice,recorded-activity,polar-integration" \
  --body "## Task
Implement new source revision creation for updated Polar data:
- When Polar provides updated data for existing activity
- Create new source revision (not overwrite)
- User's route corrections remain on published versions
- Track which revision is from which sync

## Key Invariant
New Polar data NEVER overwrites user corrections.

## Acceptance Criteria
- [ ] Updated Polar data creates new source revision
- [ ] Existing route versions are untouched
- [ ] User corrections on published versions preserved
- [ ] Source revision tracks provider and sync timestamp
- [ ] Integration test verifies non-destructive update"

echo ""
echo "==========================================="
echo "All issues and epics created successfully!"
echo "==========================================="
echo ""
echo "Summary:"
echo "  - 15 labels created"
echo "  - 11 epic tracking issues created"
echo "  - Implementation issues created for all 10 slices + infrastructure"
