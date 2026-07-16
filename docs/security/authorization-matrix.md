# Authorization Matrix

**Version:** 1.0  
**Date:** 2025-01-15  
**Authors:** Platform Security Team  

---

## Overview

This document maps every protected API operation to its authorization requirements and expected behavior for unauthenticated and cross-owner requests.

### Legend

- **Auth Required:** Whether the `AuthenticatedActor` extractor is applied
- **Ownership Check:** Whether the handler verifies the resource belongs to the requesting user
- **Cross-Owner Response:** HTTP status returned when an authenticated user accesses another user's resource
- **Unauthenticated Response:** HTTP status returned when no valid session/token is present
- **CSRF Required:** Whether `X-CSRF-Token` header is validated (state-changing methods)
- **Idempotency Key:** Whether `Idempotency-Key` header is required
- **Rate Limit Category:** Which token bucket category applies

---

## Authentication Endpoints

| Method | Path | Auth Required | Ownership Check | Cross-Owner Response | Unauthenticated Response | CSRF Required | Idempotency Key | Rate Limit Category |
|--------|------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| POST | `/auth/login` | No | N/A | N/A | N/A (public) | No | No | auth (5 RPM) |
| GET | `/auth/callback` | No | N/A | N/A | N/A (public) | No | No | auth (5 RPM) |
| POST | `/auth/logout` | Yes (session cookie) | N/A | N/A | 401 | Yes | No | auth (5 RPM) |
| GET | `/me` | Yes | N/A (returns own info) | N/A | 401 | No | No | reads (120 RPM) |

---

## Activity Endpoints

| Method | Path | Auth Required | Ownership Check | Cross-Owner Response | Unauthenticated Response | CSRF Required | Idempotency Key | Rate Limit Category |
|--------|------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| GET | `/v1/activities` | Yes | Implicit (query scoped to actor) | N/A | 401 | No | No | reads (120 RPM) |
| GET | `/v1/activities/{id}` | Yes | Yes | 404 Not Found | 401 | No | No | reads (120 RPM) |
| PATCH | `/v1/activities/{id}/title` | Yes | Yes | 404 Not Found | 401 | Yes | Yes | mutations (30 RPM) |
| DELETE | `/v1/activities/{id}` | Yes | Yes | 404 Not Found | 401 | Yes | Yes | mutations (30 RPM) |
| GET | `/v1/activities/{id}/recorded-route` | Yes | Yes | 404 Not Found | 401 | No | No | reads (120 RPM) |

---

## Import Endpoints

| Method | Path | Auth Required | Ownership Check | Cross-Owner Response | Unauthenticated Response | CSRF Required | Idempotency Key | Rate Limit Category |
|--------|------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| POST | `/v1/imports` | Yes | N/A (creates for actor) | N/A | 401 | Yes | Yes | imports (10 RPM) |
| GET | `/v1/imports/{id}` | Yes | Yes | 404 Not Found | 401 | No | No | reads (120 RPM) |
| POST | `/v1/imports/{id}/completion` | Yes | Yes | **403 Forbidden** | 401 | Yes | Yes | imports (10 RPM) |

### Finding: Import Completion BOLA

The `POST /v1/imports/{id}/completion` endpoint returns **403 Forbidden** for cross-owner requests instead of 404 Not Found. This discloses that the import resource exists but belongs to another user. See threat-model.md HIGH-002.

---

## Export Endpoints

| Method | Path | Auth Required | Ownership Check | Cross-Owner Response | Unauthenticated Response | CSRF Required | Idempotency Key | Rate Limit Category |
|--------|------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| POST | `/v1/activities/{id}/exports` | Yes | Yes (activity ownership) | 404 Not Found | 401 | Yes | Yes | exports (10 RPM) |
| GET | `/v1/exports/{id}` | Yes | Yes | 404 Not Found | 401 | No | No | reads (120 RPM) |
| GET | `/v1/exports/{id}/download` | Yes | Yes | 404 Not Found | 401 | No | No | exports (10 RPM) |

---

## Route Editing Endpoints

| Method | Path | Auth Required | Ownership Check | Cross-Owner Response | Unauthenticated Response | CSRF Required | Idempotency Key | Rate Limit Category |
|--------|------|:---:|:---:|:---:|:---:|:---:|:---:|:---:|
| POST | `/v1/activities/{id}/route-drafts` | Yes | Yes (activity ownership) | 404 Not Found | 401 | Yes | Yes | mutations (30 RPM) |
| GET | `/v1/route-drafts/{id}` | Yes | Yes | **403 Forbidden** | 401 | No | No | reads (120 RPM) |
| POST | `/v1/route-drafts/{id}/operations` | Yes | Yes | **403 Forbidden** | 401 | Yes | Yes | mutations (30 RPM) |
| POST | `/v1/route-drafts/{id}/undo` | Yes | Yes | **403 Forbidden** | 401 | Yes | Yes | mutations (30 RPM) |
| POST | `/v1/route-drafts/{id}/redo` | Yes | Yes | **403 Forbidden** | 401 | Yes | Yes | mutations (30 RPM) |
| POST | `/v1/route-drafts/{id}/reset` | Yes | Yes | **403 Forbidden** | 401 | Yes | Yes | mutations (30 RPM) |
| DELETE | `/v1/route-drafts/{id}` | Yes | Yes | **403 Forbidden** | 401 | Yes | Yes | mutations (30 RPM) |
| POST | `/v1/route-drafts/{id}/validation` | Yes | Yes | **403 Forbidden** | 401 | Yes | Yes | mutations (30 RPM) |
| POST | `/v1/route-drafts/{id}/publication` | Yes | Yes (via committer) | **403 Forbidden** | 401 | Yes | Yes (UUID format) | mutations (30 RPM) |

### Finding: Route Draft BOLA

All route-draft endpoints return **403 Forbidden** for cross-owner requests instead of 404 Not Found. This allows authenticated attackers to enumerate valid route-draft UUIDs belonging to other users. See threat-model.md HIGH-001.

**Affected handlers** (all in `crates/api/src/route_editing.rs`):
- `get_draft` (line 344)
- `post_apply_operation` (line 376)
- `post_undo` (line 536)
- `post_redo` (line 590)
- `post_reset` (line 646)
- `delete_draft` (line 717)
- `post_validate_draft` (line 762)
- `post_publish_draft` (line 984)

---

## Summary Statistics

| Metric | Count |
|--------|-------|
| Total protected operations | 24 |
| Require authentication | 22 |
| Public endpoints | 2 (login, callback) |
| Require CSRF token | 14 |
| Require Idempotency-Key | 12 |
| Non-disclosing cross-owner (404) | 12 |
| Disclosing cross-owner (403) | 9 |
| Operations with BOLA findings | 9 |

---

## Recommendations

1. **Standardize cross-owner responses to 404** across all ownership-checked endpoints to prevent resource enumeration.
2. **Add automated BOLA regression tests** that verify cross-owner requests receive 404 (not 403) for all ownership-checked endpoints.
3. **Document the public endpoint allowlist** in the route registration code to make accidental exposure visible in code review.
