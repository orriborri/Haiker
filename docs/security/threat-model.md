# Threat Model

**Version:** 1.0  
**Date:** 2025-01-15  
**Authors:** Platform Security Team  
**Status:** Active  

---

## 1. Scope and Assets

### In-Scope Assets

| Asset | Description | Sensitivity |
|-------|-------------|-------------|
| User sessions | HTTP-only session cookies, CSRF tokens | High |
| GPX route data | GPS coordinates, timestamps, user activity | High (PII) |
| Activity metadata | Titles, descriptions, filenames | Medium |
| Upload artifacts | Raw GPX files in object storage | High |
| Export artifacts | Generated GPX files with presigned URLs | High |
| Configuration secrets | OIDC client secret, S3 credentials, DB connection | Critical |
| User identifiers | User IDs (UUIDs), email addresses (via OIDC) | Medium |

### In-Scope Attack Vectors

1. Broken Object-Level Authorization (BOLA)
2. XML/GPX injection
3. Metadata XSS
4. Signed-link leakage
5. Session and CSRF attacks
6. Upload abuse
7. Resource exhaustion
8. Duplicate/Replay attacks
9. Stale writes
10. Log/telemetry/error exposure
11. Secret rotation failures
12. Private-by-default route bypass

### Out of Scope

- Physical infrastructure attacks
- Social engineering of end users
- Denial-of-service at network layer (handled by infrastructure/CDN)
- OIDC provider compromise (Auth0 responsibility)

---

## 2. Authentication Architecture

### Overview

The system uses a layered authentication architecture:

1. **Primary:** Session-based authentication via HTTP-only cookies (`AuthSession` extractor in `crates/platform/src/auth_middleware.rs`)
2. **OIDC Integration:** Auth0 as identity provider (`crates/platform/src/oidc.rs`)
3. **Development Bypass:** `DEV_AUTH_ENABLED` environment variable allows raw UUID Bearer tokens (`crates/platform/src/auth_middleware.rs:32`)

### Session Flow

1. User initiates login via `POST /auth/login` (redirects to Auth0)
2. Auth0 callback at `GET /auth/callback` validates tokens
3. Server generates UUID session token, stores SHA-256 hash in DB (`crates/platform/src/session.rs:144-147`)
4. Raw token sent as HTTP-only cookie; never stored server-side in plaintext
5. Subsequent requests validated by hashing cookie value and looking up in DB

### DEV_AUTH_ENABLED Risk

- **Location:** `crates/platform/src/auth_middleware.rs:32`
- **Behavior:** When enabled, accepts `Authorization: Bearer <UUID>` as direct user identity
- **Cached:** Value read once via `OnceLock`, cannot be toggled at runtime
- **Startup Warning:** `crates/api/src/main.rs:262-266` logs `SECURITY RISK` error when both OIDC and dev auth are active
- **Mitigation:** `.env.example` defaults to `DEV_AUTH_ENABLED=false` (line 27); deployment manifests must not set this

---

## 3. Broken Object-Level Authorization (BOLA)

### Analysis by Endpoint

| Endpoint | Cross-Owner Response | Disclosing? | Risk |
|----------|---------------------|-------------|------|
| `GET /v1/activities/{id}` | 404 Not Found | No | Low |
| `PATCH /v1/activities/{id}/title` | 404 Not Found | No | Low |
| `DELETE /v1/activities/{id}` | 404 Not Found | No | Low |
| `GET /v1/activities/{id}/recorded-route` | 404 Not Found | No | Low |
| `GET /v1/imports/{id}` | 404 Not Found | No | Low |
| `POST /v1/imports/{id}/completion` | **403 Forbidden** | **Yes** | **High** |
| `GET /v1/exports/{id}` | 404 Not Found | No | Low |
| `GET /v1/exports/{id}/download` | 404 Not Found | No | Low |
| `GET /v1/route-drafts/{id}` | **403 Forbidden** | **Yes** | **High** |
| `POST /v1/route-drafts/{id}/operations` | **403 Forbidden** | **Yes** | **High** |
| `POST /v1/route-drafts/{id}/undo` | **403 Forbidden** | **Yes** | **High** |
| `POST /v1/route-drafts/{id}/redo` | **403 Forbidden** | **Yes** | **High** |
| `POST /v1/route-drafts/{id}/reset` | **403 Forbidden** | **Yes** | **High** |
| `DELETE /v1/route-drafts/{id}` | **403 Forbidden** | **Yes** | **High** |
| `POST /v1/route-drafts/{id}/validation` | **403 Forbidden** | **Yes** | **High** |

### Findings

**HIGH-001: Route draft endpoints disclose resource existence via 403**

- **File:** `crates/api/src/route_editing.rs` lines 357-361, 486-490, 553-557, 607-611, 663-667
- **Impact:** An attacker can enumerate valid route-draft UUIDs belonging to other users by observing 403 vs 404 responses
- **Recommendation:** Return 404 for cross-owner access (consistent with activities/imports/exports pattern)

**HIGH-002: Import completion discloses resource existence via 403**

- **File:** `crates/api/src/imports.rs` lines 132-139
- **Impact:** An attacker can determine if an import UUID belongs to another user
- **Recommendation:** Return 404 for cross-owner access

---

## 4. XML/GPX Injection Risks

### Attack Surface

The import pipeline accepts user-supplied XML (GPX 1.0/1.1) files.

### Current Mitigations

| Control | Implementation |
|---------|---------------|
| Content-type validation | Only `application/gpx+xml` or `application/xml` accepted (`crates/api/src/imports.rs:154`) |
| File size limit | 50 MB maximum (`crates/api/src/imports.rs:145`) |
| Async processing | GPX parsing happens in background worker, not in request handler |
| Checksum verification | SHA-256 checksum validated before parsing (`crates/app/src/imports/orchestrator.rs:137-141`) |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| XML External Entity (XXE) attacks | Medium | Depends on XML parser configuration; Rust XML parsers typically do not resolve external entities by default |
| Billion Laughs (entity expansion DoS) | Low | Most Rust XML parsers limit entity expansion depth |
| Malformed XML causing parser panic | Low | Parsing runs async; failures are caught and recorded |

---

## 5. Metadata XSS

### Attack Surface

User-controlled strings that may be rendered in the frontend:
- Activity titles (via `PATCH /v1/activities/{id}/title`)
- Import filenames (via `POST /v1/imports` body)
- Failure reason messages

### Current Mitigations

- Backend serves data as JSON (Content-Type: application/json) - browsers do not execute scripts in JSON responses
- React/Vite frontend uses JSX which auto-escapes interpolated values by default
- Failure reasons are sanitized via `sanitize_failure_reason()` (`crates/api/src/imports.rs:111`, `crates/api/src/exports.rs:72`)

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| Stored XSS if frontend uses `dangerouslySetInnerHTML` with user content | Medium | No evidence of this pattern currently; requires ongoing review |
| Title/filename used in download `Content-Disposition` without sanitization | Low | Exports use server-generated filenames |

---

## 6. Signed-Link Leakage

### Presigned URL Security

| Control | Implementation | File |
|---------|---------------|------|
| Short TTL | 300 seconds (5 minutes) | `crates/api/src/exports.rs:61` |
| Cache-Control: no-store | Prevents browser/proxy caching of URL | `crates/api/src/exports.rs:504` |
| URL not in API response | Object storage key not exposed in JSON | Verified via test assertions |
| URL not in audit log | Download events recorded without the presigned URL | `crates/platform/src/audit.rs` ExportAuditSink abstraction |
| Auth required | Download endpoint requires `AuthenticatedActor` + ownership check | `crates/api/src/exports.rs` |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| URL leaked via browser history/Referer header during 5-min window | Low | Acceptable given short TTL and no-store header |
| URL logged by reverse proxy access logs | Low | Operational concern; proxy config should avoid query-string logging |

---

## 7. Session and CSRF

### Session Token Security

| Property | Implementation | File |
|----------|---------------|------|
| Token format | UUID v4 (122 bits entropy) | `crates/platform/src/session.rs` |
| Storage | SHA-256 hash in PostgreSQL | `crates/platform/src/session.rs:144-147` |
| Cookie flags | HttpOnly, SameSite=Lax, Path=/, Max-Age=7d | `crates/api/src/auth_handlers.rs:278-284` |
| Secure flag | Conditional on HTTPS redirect URI | `crates/api/src/auth_handlers.rs:278` |
| Revocation | On logout; expired sessions cleaned up | Session store implementation |

### CSRF Protection

| Property | Implementation |
|----------|---------------|
| Token delivery | Returned to client on login response |
| Validation | `X-CSRF-Token` header required on POST/PUT/PATCH/DELETE |
| Scope | State-changing methods only |
| Cookie interaction | SameSite=Lax prevents cross-origin POST with cookies |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| SameSite=Lax allows top-level navigation GET with cookies (not exploitable since GETs are safe) | Low | Acceptable |
| 7-day session lifetime may be too long for sensitive operations | Low | Acceptable for MVP; consider step-up auth later |

---

## 8. Upload Abuse

### Controls

| Control | Value | File |
|---------|-------|------|
| Max file size | 50 MB | `crates/api/src/imports.rs:145` |
| Content-type whitelist | `application/gpx+xml`, `application/xml` | `crates/api/src/imports.rs:154` |
| Per-user daily upload quota | Configurable | Import domain logic |
| Checksum verification | SHA-256 computed and verified on completion | `crates/app/src/imports/orchestrator.rs:137-141` |
| Duplicate detection | Checksum-based deduplication per owner | Import orchestrator |
| Queue backpressure | 429 Too Many Requests with Retry-After | Import queue handler |
| Rate limiting | 10 RPM on import routes | `crates/platform/src/rate_limit.rs:56` |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| ZIP bomb (compressed XML) | Low | GPX files are typically uncompressed; file size limit applies to raw upload |
| Slowloris-style upload (very slow byte stream) | Low | Handled by HTTP server timeouts |

---

## 9. Resource Exhaustion

### Rate Limiting

| Category | Limit (RPM) | Scope | File |
|----------|-------------|-------|------|
| Auth (login/register) | 5 | Per-actor or per-IP | `crates/platform/src/rate_limit.rs:55` |
| Imports | 10 | Per-actor or per-IP | `crates/platform/src/rate_limit.rs:56` |
| Mutations (create/update/delete) | 30 | Per-actor or per-IP | `crates/platform/src/rate_limit.rs:57` |
| Reads | 120 | Per-actor or per-IP | `crates/platform/src/rate_limit.rs:58` |
| Exports | 10 | Per-actor or per-IP | `crates/platform/src/rate_limit.rs:59` |

### Bucket Management

- **Eviction threshold:** 10,000 entries (`crates/platform/src/rate_limit.rs:255`)
- **Stale age:** 120 seconds (`crates/platform/src/rate_limit.rs:259`)
- **Eviction trigger:** When bucket map exceeds threshold, entries older than stale age are removed

### Other Controls

| Control | Description |
|---------|-------------|
| Job queue backpressure | Import/export jobs return 429 when queue is saturated |
| Idempotency-Key requirement | Prevents accidental duplicate mutations |
| Pagination | List endpoints use cursor or offset-based pagination |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| IP-based rate limiting can be bypassed with rotating IPs | Low | Acceptable for MVP; consider fingerprinting later |
| Memory growth from bucket map before eviction threshold | Low | Bounded at 10,000 entries with periodic cleanup |

---

## 10. Duplicate/Replay Attacks

### Idempotency Controls

| Control | Implementation |
|---------|---------------|
| Idempotency-Key header | Required on all mutating endpoints |
| Payload mismatch detection | Same key + different payload returns 409 Conflict |
| UUID format enforcement | Publication endpoint requires UUID-format idempotency key |
| Response replay | Stored responses returned for duplicate keys |

### Optimistic Concurrency Control (OCC)

| Control | Implementation |
|---------|---------------|
| `expectedRevision` parameter | Route draft operations require current revision |
| Stale write detection | Revision mismatch returns 409 Conflict |
| Atomic publish | Publication uses locked row read to eliminate TOCTOU |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| Idempotency key storage growth over time | Low | Keys should have TTL-based cleanup |
| Clock skew between replicas could affect TTL enforcement | Low | Single-writer for now |

---

## 11. Stale Writes

### Optimistic Concurrency via expectedRevision

- Route draft operations (`post_apply_operation`, `post_undo`, `post_redo`, `post_reset`) require `expectedRevision`
- Mismatches produce 409 Conflict with clear error messaging
- Publication endpoint (`post_publish_draft`) uses locked row to eliminate race conditions between read-check-write (`crates/api/src/route_editing.rs:984`)

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| No OCC on activity title updates (last-write-wins) | Low | Acceptable for single-user-per-activity model |

---

## 12. Log/Telemetry/Error Exposure

### Error Response Sanitization

| Control | File | Line |
|---------|------|------|
| `sanitize_failure_reason()` for imports | `crates/api/src/imports.rs` | 111 |
| `sanitize_failure_reason()` for exports | `crates/api/src/exports.rs` | 72 |
| RFC 9457 Problem Details format | All error responses | - |
| No SQL errors in responses | Only known-safe prefixes pass through | Verified via tests |
| No file paths in responses | Generic message for unknown errors | Verified via tests |
| No connection strings in responses | Generic message for unknown errors | Verified via tests |

### Tracing/Logging

| Statement | Location | Data Logged | Risk |
|-----------|----------|-------------|------|
| DEV_AUTH warning | `crates/platform/src/auth_middleware.rs:106-108` | `user_id` | Low (dev only, appropriate warning) |
| Auth error | `crates/api/src/auth_handlers.rs` | Error details, no user PII | Low |
| Audit append | `crates/platform/src/audit.rs:25-26` | User ID, action, resource (no coordinates/file content/PII) | Low |
| Export download | Export audit sink | Event recorded WITHOUT presigned URL | Low |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| Third-party tracing subscribers could forward spans to external services | Low | Operational concern; review subscriber config |

---

## 13. Secret Rotation

### Secrets Inventory

| Secret | Storage | Rotation Mechanism | File |
|--------|---------|-------------------|------|
| OIDC client secret | Environment variable | Auth0 dashboard rotation | `crates/platform/src/oidc.rs` |
| S3 access key / secret key | Environment variable | Object storage provider rotation | `crates/platform/src/config.rs:40-50` |
| Database connection URL | Environment variable | Password rotation + connection string update | `crates/platform/src/config.rs:31-36` |
| Session signing (token hashing) | Algorithmic (SHA-256) | No secret key; tokens are random UUIDs | `crates/platform/src/session.rs` |

### Findings

**MEDIUM-001: StorageConfig derives Debug - could leak secret_access_key**

- **File:** `crates/platform/src/config.rs:40`
- **Impact:** If `StorageConfig` is debug-printed (e.g., via `tracing::debug!(?config)` or panic messages), the `secret_access_key` field would be exposed in logs
- **Recommendation:** Implement custom `Debug` that redacts sensitive fields, or use `secrecy::Secret<String>` wrapper

**MEDIUM-002: OidcConfig derives Debug - could leak client_secret**

- **File:** `crates/platform/src/config.rs:53`
- **Impact:** If `OidcConfig` is debug-printed, the `client_secret` field would be exposed in logs
- **Recommendation:** Same as MEDIUM-001

---

## 14. Private-by-Default Routes

### Enforcement

All API endpoints require the `AuthenticatedActor` extractor, which:
1. Checks for valid session cookie (SHA-256 hash lookup in DB)
2. Falls back to DEV_AUTH Bearer token if `DEV_AUTH_ENABLED=true`
3. Returns 401 Unauthorized if neither authentication method succeeds

### Public Endpoints (by design)

| Endpoint | Purpose |
|----------|---------|
| `POST /auth/login` | Initiates OIDC flow |
| `GET /auth/callback` | OIDC callback (validates state/nonce) |
| Health check endpoints | Infrastructure liveness/readiness |

### Residual Risks

| Risk | Severity | Status |
|------|----------|--------|
| New endpoints may forget to include `AuthenticatedActor` extractor | Low | Mitigated by Axum type system (extractor must be declared) |
| DEV_AUTH bypass in production | Critical (if enabled) | Startup warning + .env.example default prevents this |

---

## 15. Findings Summary

| ID | Severity | Category | Description | File | Status | Owner | Target Date |
|----|----------|----------|-------------|------|--------|-------|-------------|
| HIGH-001 | High | BOLA | Route draft endpoints return 403 for cross-owner (discloses resource existence) | `crates/api/src/route_editing.rs:357-667` | Open | platform-team | 2025-02-14 |
| HIGH-002 | High | BOLA | Import completion returns 403 for cross-owner (discloses resource existence) | `crates/api/src/imports.rs:132-139` | Open | platform-team | 2025-02-14 |
| MEDIUM-001 | Medium | Secret Exposure | StorageConfig derives Debug, could leak secret_access_key in logs | `crates/platform/src/config.rs:40` | Open | platform-team | 2025-02-14 |
| MEDIUM-002 | Medium | Secret Exposure | OidcConfig derives Debug, could leak client_secret in logs | `crates/platform/src/config.rs:53` | Open | platform-team | 2025-02-14 |
| MEDIUM-003 | Medium | XSS | Frontend stores auth_token in localStorage (XSS exfiltration risk) | `frontend/src/api/client.ts:6` | Accepted | platform-team | 2025-02-14 |
| LOW-001 | Low | Session | 7-day session Max-Age without step-up authentication for sensitive ops | `crates/api/src/auth_handlers.rs:284` | Accepted | platform-team | - |
| LOW-002 | Low | Rate Limit | IP-based limiting bypassable with rotating IPs | `crates/platform/src/rate_limit.rs` | Accepted | platform-team | - |

### Severity Definitions

- **Critical:** Immediate exploitation possible, data breach or full system compromise
- **High:** Exploitable vulnerability requiring targeted effort, information disclosure enabling further attacks
- **Medium:** Weakness that increases attack surface but requires additional conditions
- **Low:** Defense-in-depth gap; acceptable risk for current threat profile
