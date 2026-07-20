# API Design

## Standard

The Haiker public API conforms to **OpenAPI Specification 3.1.1**. The OpenAPI document is the authoritative external HTTP contract. Domain rules remain authoritative in the domain model; persisted state remains authoritative in the server database.

## Entry Document

```
openapi/openapi.yaml
```

```yaml
openapi: 3.1.1
info:
  title: Haiker API
  summary: Import, correct, version, and export hiking activities.
  version: 1.0.0
```

## Base Path

```
/v1
```

The following are distinct and must not be conflated:
- OpenAPI version: `3.1.1`
- API description version: `info.version`
- Public compatibility generation: `/v1`

## Resource Paths

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
GET    /v1/activities/{activityId}/route-comparison
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

## Operation IDs

Operation IDs are stable client-generation contracts. Examples:

- `listActivities`
- `getActivity`
- `startImport`
- `getImportStatus`
- `createRouteDraft`
- `applyRouteOperation`
- `publishRouteDraft`
- `requestGpxExport`

Never rename an operation ID without a breaking-change review.

## Error Contract

Use a single error envelope based on HTTP Problem Details semantics:

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

Rules:
- `type`: stable URI path identifying the problem kind
- `title`: human-readable summary (stable per type)
- `status`: HTTP status code (redundant with response, but included for log correlation)
- `code`: machine-readable error code (UPPER_SNAKE_CASE)
- `detail`: instance-specific explanation (may vary per occurrence)
- `requestId`: correlation ID for debugging
- Internal implementation details must NEVER leak through error responses

## Status Code Policy

| Code | Meaning |
|------|---------|
| 200 | Successful read or command with body |
| 201 | Resource created synchronously |
| 202 | Asynchronous import or export accepted |
| 204 | Successful deletion without body |
| 400 | Malformed request |
| 401 | Missing or invalid authentication |
| 403 | Authenticated but not authorized |
| 404 | Resource unavailable to the caller |
| 409 | Stale revision or idempotency conflict |
| 415 | Invalid upload media type |
| 422 | Domain validation failure |
| 429 | Rate limit exceeded |
| 500 | Unexpected failure |

## Pagination

Use **cursor-based pagination** consistently for all collection endpoints. Never use offset-based pagination.

## Geometry Media Type

Use `application/geo+json` for route geometry where practical.

GeoJSON coordinate order is always: `[longitude, latitude]`.

Domain objects that use named `latitude` and `longitude` fields must not be confused with GeoJSON array ordering.

## Request Validation

Request schemas should use:

```yaml
type: object
additionalProperties: false
```

Rust request DTOs should use `#[serde(deny_unknown_fields)]` where this matches the OpenAPI schema.

## Idempotency

Require `Idempotency-Key` header for:
- Starting imports
- Applying route operations
- Publishing a draft
- Requesting exports
- Manual synchronization

Behavior:
- Same key + same payload → replay original response
- Same key + different payload → return error
- Keys expire after a documented retention period

## Concurrency

Use explicit `expectedRevision` field for route draft mutations. A mismatch returns `409 Conflict`. No automatic merge in MVP.

## OpenAPI Repository Structure

```
openapi/
├── openapi.yaml            # Entry document
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

CI produces a bundled `dist/openapi.yaml`.

## OpenAPI CI Governance

Every pull request must:

1. Bundle the multi-file OpenAPI description
2. Validate against OpenAPI 3.1.1
3. Resolve every `$ref`
4. Enforce unique operation IDs
5. Run style linting
6. Detect breaking changes against the latest released contract
7. Generate the TypeScript client
8. Fail if generated artifacts are stale
9. Run Rust contract tests
10. Compile the frontend against the generated client

A change CANNOT merge when:
- The OpenAPI document is invalid
- A breaking change is unapproved
- A response or error is undocumented
- Rust behavior differs from the contract
- Generated clients are stale
