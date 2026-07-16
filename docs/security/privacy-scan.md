# Privacy and Secret Scan Report

**Version:** 1.0  
**Date:** 2025-01-15  
**Authors:** Platform Security Team  
**Status:** Active  

---

## 1. Tracing Statement Review

All `tracing::*` calls in the codebase were reviewed for PII and secret leakage.

### Findings

| File | Line | Level | Data Logged | PII/Secret Risk | Status |
|------|------|-------|-------------|-----------------|--------|
| `crates/platform/src/auth_middleware.rs` | 106-108 | `warn` | `user_id` (UUID) | Low - user identifier only, dev-mode context | Acceptable |
| `crates/api/src/auth_handlers.rs` | (error paths) | `error` | Error details | None - no user PII in error messages | Pass |
| `crates/api/src/main.rs` | 262-265 | `error` | Configuration warning text | None - static warning message | Pass |

### Tracing Conventions Verified

- No GPS coordinates appear in any tracing spans
- No file content or GPX data appears in any tracing spans
- No authentication tokens or session values appear in tracing output
- User IDs (UUIDs) appear only in the dev-auth warning context (appropriate)
- No email addresses, names, or other personal data in tracing spans

---

## 2. Error Response Sanitization

### sanitize_failure_reason() - Imports

- **File:** `crates/api/src/imports.rs:111`
- **Behavior:** Compares failure reason (lowercased) against a whitelist of safe prefixes. Unknown reasons are replaced with "an internal error occurred".
- **Safe prefixes:** Known user-actionable messages (e.g., "unsupported file format", "file is empty", "parsing failed")
- **Blocked content:** SQL errors, file paths, connection strings, stack traces

### sanitize_failure_reason() - Exports

- **File:** `crates/api/src/exports.rs:72`
- **Behavior:** Same pattern as imports. Only known safe export failure prefixes pass through.
- **Safe prefixes:** Known export-specific failure messages (e.g., "generation failed: route has no coordinates")
- **Blocked content:** Same categories as imports

### RFC 9457 Problem Details

All API error responses use the Problem Details format (RFC 9457) which:
- Uses structured `type`, `title`, `status`, `detail` fields
- Never includes raw exception messages or stack traces
- Uses stable problem-type URIs (e.g., `/problems/upload-too-large`)

### Verification

Unit tests confirm:
- `sanitize_failure_reason_passes_through_known_safe_reasons` (`crates/api/src/imports.rs:1092`)
- `sanitize_failure_reason_replaces_sql_errors_with_generic_message` (`crates/api/src/imports.rs:1114`)
- Equivalent tests exist in `crates/api/src/exports.rs`

**Status: PASS** - Error responses do not leak internal details.

---

## 3. Audit Log Metadata Review

### Audit Store Contract

- **File:** `crates/platform/src/audit.rs:25-26`
- **Doc comment:** "The metadata must be privacy-safe (no coordinates, file content, or PII beyond user identifiers)."
- **Repeated at:** `crates/platform/src/audit.rs:56-57` (transactional variant)

### Audit Data Fields

| Field | Content | PII Classification |
|-------|---------|-------------------|
| Actor ID | User UUID | Pseudonymous identifier |
| Action | Operation name (e.g., "export.downloaded") | Not PII |
| Resource ID | Resource UUID | Not PII |
| Metadata | Action-specific key-value pairs | Must not contain coordinates/content |

### Export Download Audit

- **Verified:** The export download audit event records the download action and resource ID but does NOT log the presigned URL
- **Implementation:** `ExportAuditSink` trait (`crates/api/src/exports.rs:38-48`) abstracts audit recording
- **Test coverage:** Download handler tests verify Cache-Control headers but audit content is validated via the trait contract

**Status: PASS** - Audit logs follow the documented privacy contract.

---

## 4. Frontend localStorage Review

### Current State

- **File:** `frontend/src/api/client.ts:6`
- **Storage:** `localStorage.getItem("auth_token")` retrieves a token used in `Authorization: Bearer` header

### Risk Assessment

| Concern | Assessment |
|---------|-----------|
| XSS exfiltration | If an XSS vulnerability exists, an attacker could read the localStorage token |
| Token type | This appears to be used for the dev-auth Bearer UUID flow |
| Primary auth mechanism | Session-based via HttpOnly cookie (not accessible to JavaScript) |
| Severity | Medium - the HttpOnly session cookie is the real authentication mechanism; localStorage token is supplementary |

### Recommendation

- For production: Remove localStorage token usage; rely solely on HttpOnly session cookies
- The session cookie approach already provides XSS-resistant authentication
- If localStorage must be used, consider using sessionStorage (cleared on tab close) or in-memory state

**Status: MEDIUM FINDING** - See threat-model.md MEDIUM-003.

---

## 5. .env.example Review

### File: `.env.example`

| Variable | Value | Risk |
|----------|-------|------|
| `DEV_AUTH_ENABLED` | `false` | Safe - disabled by default |
| Database URL | Placeholder/localhost | Safe - not a real credential |
| S3 credentials | Placeholder values | Safe - not real secrets |
| OIDC settings | Placeholder values | Safe - not real secrets |

### Verification

- No real API keys, passwords, or tokens in `.env.example`
- All sensitive values use clear placeholder text
- Comments warn about production usage where appropriate (line 26: "WARNING: MUST NOT be set to true in production")

**Status: PASS** - No secrets in version-controlled configuration files.

---

## 6. Presigned URL Logging Review

### Verified Controls

| Check | Result | Evidence |
|-------|--------|----------|
| Presigned URL NOT in API JSON response body | PASS | Export response includes download metadata but not raw S3 URL |
| Presigned URL NOT in audit log | PASS | Audit sink records event without URL (`crates/platform/src/audit.rs` contract) |
| Presigned URL NOT in tracing spans | PASS | No `tracing::*` calls include presigned URLs |
| Cache-Control: no-store on download response | PASS | `crates/api/src/exports.rs:504` |
| Short TTL (300s) limits exposure window | PASS | `crates/api/src/exports.rs:61` |

**Status: PASS** - Presigned URLs are not persisted in any log, audit, or response beyond the immediate HTTP response to the authorized user.

---

## 7. Debug Derive Secret Exposure

### StorageConfig

- **File:** `crates/platform/src/config.rs:40`
- **Struct:** `StorageConfig` derives `Debug`
- **Sensitive fields:** `secret_access_key`
- **Risk:** If this struct is ever formatted with `{:?}` (debug print), the secret key appears in output
- **Current exposure:** No evidence of debug-printing this struct in production code paths, but the derive makes it possible

### OidcConfig

- **File:** `crates/platform/src/config.rs:53`
- **Struct:** `OidcConfig` derives `Debug`
- **Sensitive fields:** `client_secret`
- **Risk:** Same as StorageConfig

### Recommendation

Replace `#[derive(Debug)]` with a manual `impl Debug` that redacts sensitive fields:

```rust
impl fmt::Debug for StorageConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageConfig")
            .field("endpoint_url", &self.endpoint_url)
            .field("bucket_name", &self.bucket_name)
            .field("access_key_id", &self.access_key_id)
            .field("secret_access_key", &"[REDACTED]")
            .finish()
    }
}
```

**Status: MEDIUM FINDING** - See threat-model.md MEDIUM-001 and MEDIUM-002.

---

## 8. Summary of Findings

| ID | Category | Severity | Description | Status |
|----|----------|----------|-------------|--------|
| PRIV-001 | Frontend | Medium | localStorage stores auth_token accessible to XSS | Open |
| PRIV-002 | Config | Medium | StorageConfig #[derive(Debug)] could leak secret_access_key | Open |
| PRIV-003 | Config | Medium | OidcConfig #[derive(Debug)] could leak client_secret | Open |

### Items Verified Clean

| Category | Result |
|----------|--------|
| Tracing/logging statements | No PII or secrets found |
| Error responses | Properly sanitized via allowlist |
| Audit records | Follow privacy-safe contract (no coordinates/content/PII) |
| Presigned URLs | Not logged or cached |
| .env.example | No real secrets |
| Client bundles | No embedded secrets (API calls use runtime config) |

---

## 9. Recommendations

1. **Remove localStorage token storage** in production builds; rely on HttpOnly session cookies for authentication.
2. **Implement custom Debug** for `StorageConfig` and `OidcConfig` that redacts sensitive fields.
3. **Add CI lint rule** to detect `#[derive(Debug)]` on structs containing fields named `secret`, `password`, `key`, or `token`.
4. **Periodic review:** Re-run this scan quarterly or when new tracing/audit instrumentation is added.
5. **Consider structured logging policy:** Document which fields are safe to include in spans (user_id: yes, email: no, coordinates: no, tokens: no).
