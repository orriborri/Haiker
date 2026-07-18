# Release Checklist - Haiker MVP

> **Owner:** Platform / Engineering team
> **Last updated:** 2025-01-20
> **Status:** Pre-Release

---

## Summary - Launch Readiness

| Category | PASS | BLOCK | PENDING | Status |
|----------|:----:|:-----:|:-------:|--------|
| Pre-Release Verification | 0 | 0 | 4 | PENDING |
| Security | 0 | 0 | 4 | PENDING |
| Reliability | 0 | 0 | 4 | PENDING |
| Operations | 0 | 0 | 4 | PENDING |
| Performance | 0 | 0 | 3 | PENDING |
| Compliance | 0 | 0 | 3 | PENDING |
| Deployment | 0 | 0 | 3 | PENDING |
| **TOTAL** | **0** | **0** | **25** | **PENDING** |

> Update this table as criteria are satisfied. Any BLOCK status prevents launch.
> All PENDING items must be resolved (PASS or accepted waiver) before Go decision.

---

## Table of Contents

1. [Pre-Release Verification](#1-pre-release-verification)
2. [Security](#2-security)
3. [Reliability](#3-reliability)
4. [Operations](#4-operations)
5. [Performance](#5-performance)
6. [Compliance](#6-compliance)
7. [Deployment](#7-deployment)
8. [Sign-Off](#8-sign-off)
9. [Launch Decision Matrix](#9-launch-decision-matrix)
10. [Post-Launch Monitoring](#10-post-launch-monitoring)

---

## 1. Pre-Release Verification

Functional criteria ensuring all MVP features are complete and tested.

| # | Criterion | Evidence Type | Owner Role | Status | Evidence / Block Reason |
|---|-----------|---------------|------------|--------|-------------------------|
| 1.1 | All MVP epics complete (A-G) | GitHub milestone 100% closed | Engineering Lead | PENDING | Epics: [A] Platform foundation, [B] Import GPX, [C] Activity library, [D] Route editor, [E] Route versions, [F] GPX export, [G] Production readiness |
| 1.2 | E2E production test passes | Automated test result (`tests/e2e-production.sh` exit 0) | Backend Lead | PENDING | Script: [`tests/e2e-production.sh`](../tests/e2e-production.sh) |
| 1.3 | Contract tests pass (OpenAPI validation) | CI workflow green (`.github/workflows/ci.yml` openapi job) | API Lead | PENDING | CI job: `openapi-validate` in [`ci.yml`](../.github/workflows/ci.yml) |
| 1.4 | Frontend build succeeds | CI workflow green (frontend build step) | Frontend Lead | PENDING | CI job: `build` step in [`ci.yml`](../.github/workflows/ci.yml) |

### Verification Commands

```bash
# Run E2E production test
TEST_AUTH_TOKEN="<token>" ./tests/e2e-production.sh

# Run all backend tests
cargo test --workspace

# Validate OpenAPI spec
npx @redocly/cli lint openapi/openapi.yaml

# Build frontend
cd frontend && pnpm build
```

---

## 2. Security

Security posture verification before exposing the application to users.

| # | Criterion | Evidence Type | Owner Role | Status | Evidence / Block Reason |
|---|-----------|---------------|------------|--------|-------------------------|
| 2.1 | Threat model reviewed and current | Document review sign-off | Security Lead | PENDING | Document: [`docs/security/threat-model.md`](security/threat-model.md) |
| 2.2 | Authorization matrix verified | Automated test result + manual review | Security Lead | PENDING | Document: [`docs/security/authorization-matrix.md`](security/authorization-matrix.md) |
| 2.3 | Secret rotation tested | Manual verification (runbook exercised) | Platform Lead | PENDING | Runbook: [`docs/runbooks/secret-rotation.md`](runbooks/secret-rotation.md) |
| 2.4 | No critical or high vulnerabilities | Dependency audit report (cargo audit, npm audit) | Security Lead | PENDING | Run `cargo audit` and `cd frontend && pnpm audit` |

### Verification Commands

```bash
# Dependency vulnerability scan (Rust)
cargo audit

# Dependency vulnerability scan (frontend)
cd frontend && pnpm audit --audit-level=high

# Verify authorization matrix coverage
cargo test --workspace -- authorization
```

### Key Security Documents

- [Threat Model](security/threat-model.md) - Attack vectors, mitigations, residual risks
- [Authorization Matrix](security/authorization-matrix.md) - Per-endpoint auth requirements
- [Privacy Scan](security/privacy-scan.md) - PII/secret exposure audit
- [Secret Rotation Runbook](runbooks/secret-rotation.md) - Credential lifecycle procedures

---

## 3. Reliability

Backup, recovery, and alerting readiness.

| # | Criterion | Evidence Type | Owner Role | Status | Evidence / Block Reason |
|---|-----------|---------------|------------|--------|-------------------------|
| 3.1 | Backup/restore drill passed | Drill report (filled [`docs/templates/drill-report-template.md`](templates/drill-report-template.md)) | Platform Lead | PENDING | Runbook: [`docs/runbooks/restore-drill.md`](runbooks/restore-drill.md) |
| 3.2 | RTO within target (4 hours) and RPO within target (PostgreSQL 15 min, MinIO 24h) | Drill timing measurements in report | Platform Lead | PENDING | Targets defined in [`docs/runbooks/restore-drill.md`](runbooks/restore-drill.md#rto-measurement-framework) |
| 3.3 | Alert rules configured and validated | Test script passes (`scripts/tests/test-alert-rules.sh` exit 0) | Platform Lead | PENDING | Alert rules: [`deploy/backup/monitoring/alert-rules.yml`](../deploy/backup/monitoring/alert-rules.yml), Test: [`scripts/tests/test-alert-rules.sh`](../scripts/tests/test-alert-rules.sh) |
| 3.4 | All operational runbooks complete and reviewed | Document review sign-off | Platform Lead | PENDING | Runbooks directory: [`docs/runbooks/`](runbooks/) |

### Verification Commands

```bash
# Validate alert rules
bash scripts/tests/test-alert-rules.sh

# Verify backup monitoring
scripts/backup-monitor.sh

# Run smoke tests
scripts/smoke-test.sh
```

### Runbook Inventory

| Runbook | Path | Coverage |
|---------|------|----------|
| Backup Operations | [`docs/runbooks/backup-operations.md`](runbooks/backup-operations.md) | PITR, MinIO restore, alert response |
| Backup Quick Reference | [`docs/runbooks/backup-quickref.md`](runbooks/backup-quickref.md) | One-page operations cheat sheet |
| Restore Drill | [`docs/runbooks/restore-drill.md`](runbooks/restore-drill.md) | Full drill procedure with pass/fail criteria |
| Migration Recovery | [`docs/runbooks/migration-recovery.md`](runbooks/migration-recovery.md) | Failed migration handling |
| Incident Response | [`docs/runbooks/incident-response.md`](runbooks/incident-response.md) | Classification, escalation, war room |
| Rollback | [`docs/runbooks/rollback.md`](runbooks/rollback.md) | Application, DB, configuration rollback |
| Secret Rotation | [`docs/runbooks/secret-rotation.md`](runbooks/secret-rotation.md) | Credential lifecycle |
| Data Recovery | [`docs/runbooks/data-recovery.md`](runbooks/data-recovery.md) | Single-record and bulk recovery |
| Dependency Outage | [`docs/runbooks/dependency-outage.md`](runbooks/dependency-outage.md) | External dependency failure handling |

---

## 4. Operations

Monitoring, on-call, and incident response readiness.

| # | Criterion | Evidence Type | Owner Role | Status | Evidence / Block Reason |
|---|-----------|---------------|------------|--------|-------------------------|
| 4.1 | Monitoring configured (health, metrics, logs) | Manual verification (dashboard screenshot or config review) | Platform Lead | PENDING | Health endpoint: `/health`, Metrics: `crates/platform/src/metrics.rs` |
| 4.2 | On-call rotation established | Documented schedule or PagerDuty configuration | Engineering Manager | PENDING | Configure via `ALERT_WEBHOOK_URL` per [`deploy/backup/env.backup.example`](../deploy/backup/env.backup.example) |
| 4.3 | Escalation paths documented | Document review | Engineering Manager | PENDING | Defined in [`docs/runbooks/incident-response.md`](runbooks/incident-response.md#escalation-matrix) |
| 4.4 | Incident response runbook exercised (tabletop or live) | Exercise report or meeting notes | Platform Lead | PENDING | Runbook: [`docs/runbooks/incident-response.md`](runbooks/incident-response.md) |

### Operational Readiness Checks

```bash
# Verify health endpoint responds
curl -sf http://localhost:3000/health | jq .

# Verify smoke test passes
scripts/smoke-test.sh

# Verify backup monitor runs cleanly
scripts/backup-monitor.sh

# Verify alert webhook is configured
grep -q "ALERT_WEBHOOK_URL" deploy/backup/env.backup.example
```

---

## 5. Performance

Load testing and resource behavior under sustained traffic.

| # | Criterion | Evidence Type | Owner Role | Status | Evidence / Block Reason |
|---|-----------|---------------|------------|--------|-------------------------|
| 5.1 | Load test results recorded | Load test report (tool output, e.g., k6, wrk, or hey) | Backend Lead | PENDING | Test against staging with representative traffic patterns |
| 5.2 | p99 latency within SLO (target: < 500ms for reads, < 2s for writes) | Load test percentile metrics | Backend Lead | PENDING | Measure: `GET /v1/activities`, `POST /v1/imports`, `POST /v1/exports` |
| 5.3 | No resource leaks under sustained load | Memory and connection pool metrics over 1h+ test | Platform Lead | PENDING | Monitor: Docker stats, PostgreSQL connection count, worker queue depth |

### Performance Test Commands

```bash
# Example load test (adjust tool and parameters)
# Read-heavy scenario
hey -n 1000 -c 50 -H "Authorization: Bearer <token>" http://localhost:3000/v1/activities

# Write scenario (import)
hey -n 100 -c 10 -m POST -H "Authorization: Bearer <token>" \
  -D crates/test_support/fixtures/valid_simple.gpx \
  http://localhost:3000/v1/imports

# Monitor resources during test
docker stats --no-stream
docker compose exec postgres psql -U haiker -c "SELECT count(*) FROM pg_stat_activity;"
```

---

## 6. Compliance

Privacy, data governance, and audit trail verification.

| # | Criterion | Evidence Type | Owner Role | Status | Evidence / Block Reason |
|---|-----------|---------------|------------|--------|-------------------------|
| 6.1 | Privacy scan completed (no PII in logs, no secrets in responses) | Document review | Security Lead | PENDING | Report: [`docs/security/privacy-scan.md`](security/privacy-scan.md) |
| 6.2 | Data retention policies enforced (backup retention, export expiry) | Configuration review + automated verification | Platform Lead | PENDING | Backup retention: `BACKUP_RETENTION_DAYS` in [`deploy/backup/env.backup.example`](../deploy/backup/env.backup.example); Export signed-URL expiry in application config |
| 6.3 | Audit logging verified (all state changes produce audit events) | Automated test result (`cargo test -- audit`) | Backend Lead | PENDING | Implementation: `crates/platform/src/` (outbox + audit), Tests: workspace audit tests |

### Compliance Verification

```bash
# Verify audit events are generated for state changes
cargo test --workspace -- audit

# Verify no PII in tracing output (review privacy scan)
# See: docs/security/privacy-scan.md

# Verify backup retention is configured
grep "BACKUP_RETENTION_DAYS" deploy/backup/env.backup.example
```

---

## 7. Deployment

Staging validation, rollback, and migration safety.

| # | Criterion | Evidence Type | Owner Role | Status | Evidence / Block Reason |
|---|-----------|---------------|------------|--------|-------------------------|
| 7.1 | Staging environment validated (full stack deployed and healthy) | Staging smoke test result | Platform Lead | PENDING | Staging compose: [`deploy/docker-compose.staging.yml`](../deploy/docker-compose.staging.yml), Test: `API_URL=<staging> scripts/smoke-test.sh` |
| 7.2 | Rollback procedure tested (application rollback verified) | Manual test report | Platform Lead | PENDING | Runbook: [`docs/runbooks/rollback.md`](runbooks/rollback.md) |
| 7.3 | Database migrations tested (apply + verify on staging data) | Migration verification output | Backend Lead | PENDING | Tool: `scripts/migration-recovery.sh verify`, Runbook: [`docs/runbooks/migration-recovery.md`](runbooks/migration-recovery.md) |

### Deployment Verification

```bash
# Deploy to staging
docker compose -f deploy/docker-compose.staging.yml up -d

# Run smoke tests against staging
API_URL="http://staging:3000" scripts/smoke-test.sh

# Verify migrations
scripts/migration-recovery.sh verify

# Test rollback procedure (on staging)
# Follow docs/runbooks/rollback.md Section 3 (Application Rollback)
```

---

## 8. Sign-Off

Each section requires explicit sign-off with evidence before the launch decision.

| Section | Evidence Link | Owner | Date | Status |
|---------|--------------|-------|------|--------|
| Pre-Release Verification | _Link to CI run or test report_ | _Name_ | _YYYY-MM-DD_ | LAUNCH_BLOCK: All epics must reach 100% completion |
| Security | _Link to audit report_ | _Name_ | _YYYY-MM-DD_ | LAUNCH_BLOCK: Threat model and auth matrix review required |
| Reliability | _Link to drill report_ | _Name_ | _YYYY-MM-DD_ | LAUNCH_BLOCK: Restore drill must pass within RTO target |
| Operations | _Link to on-call config_ | _Name_ | _YYYY-MM-DD_ | LAUNCH_BLOCK: On-call rotation and escalation paths required |
| Performance | _Link to load test results_ | _Name_ | _YYYY-MM-DD_ | LAUNCH_BLOCK: p99 latency must be within SLO |
| Compliance | _Link to privacy scan_ | _Name_ | _YYYY-MM-DD_ | LAUNCH_BLOCK: Privacy scan and audit logging verification required |
| Deployment | _Link to staging test results_ | _Name_ | _YYYY-MM-DD_ | LAUNCH_BLOCK: Staging validation and rollback test required |

### Sign-Off Process

1. Section owner completes all criteria within their section
2. Owner records evidence link (CI URL, report path, screenshot, or document reference)
3. Owner updates status to PASS with their name and date
4. If a criterion cannot be satisfied, owner sets LAUNCH_BLOCK with:
   - Reason the criterion is not met
   - Remediation plan with target date
   - Whether a waiver is being requested
5. All sections must show PASS before proceeding to Launch Decision

### LAUNCH_BLOCK Resolution

To resolve a LAUNCH_BLOCK:

1. Complete the remediation described in the block reason
2. Provide updated evidence
3. Have the section owner verify and update status to PASS
4. Update the summary table counts

---

## 9. Launch Decision Matrix

### Go/No-Go Criteria

| Condition | Decision |
|-----------|----------|
| All 25 criteria are PASS | **GO** - proceed with production launch |
| Any criterion is BLOCK | **NO-GO** - remediation required before re-evaluation |
| Any criterion is PENDING | **INCOMPLETE** - review not finished, cannot make launch decision |

### Minimum Launch Requirements (Non-Negotiable)

These criteria cannot be waived under any circumstances:

1. E2E production test passes (1.2)
2. No critical or high vulnerabilities (2.4)
3. Backup/restore drill passed within RTO (3.1, 3.2)
4. Alert rules configured and validated (3.3)
5. Incident response runbook exists and is exercised (4.4)
6. Staging environment validated (7.1)

### Launch Block Escalation

| Level | Authority | Conditions |
|-------|-----------|------------|
| Section Owner | Can resolve blocks within their section | Standard remediation completed |
| Engineering Lead | Can grant temporary waiver for non-critical items | Documented risk acceptance with remediation timeline (max 30 days) |
| CTO / VP Engineering | Can override any block except minimum launch requirements | Documented business justification with explicit risk acceptance |

### Escalation Process

1. Section owner identifies a block they cannot resolve before the target launch date
2. Owner documents: the block, why it cannot be resolved in time, the risk if launched without resolution, and the remediation timeline
3. Owner presents to Engineering Lead for waiver consideration
4. If waiver is granted, the criterion is marked as `WAIVED` with the approver name, date, and remediation deadline
5. Waived items must be resolved within 30 days of launch or escalated to CTO

---

## 10. Post-Launch Monitoring

### First 24 Hours

| Check | Frequency | Owner | Action if Abnormal |
|-------|-----------|-------|--------------------|
| API error rate (5xx) | Every 15 minutes | On-call | If > 1% of requests, initiate incident per [`incident-response.md`](runbooks/incident-response.md) |
| API p99 latency | Every 15 minutes | On-call | If > 2s sustained for 10 min, investigate |
| Worker queue depth | Every 15 minutes | On-call | If backlog > 100 jobs for 30 min, investigate |
| Database connection pool | Every 30 minutes | On-call | If > 80% utilization, alert platform team |
| Backup health | Every hour | On-call | Run `scripts/backup-monitor.sh`; if unhealthy, follow backup-operations runbook |
| Disk space (all volumes) | Every hour | On-call | If > 85%, alert platform team |
| User-reported issues | Continuous | Support | Triage and escalate per severity |
| Authentication flow | Every hour | On-call | Test login flow; if broken, escalate immediately |

### First 24-Hour Checklist

- [ ] All alert channels are receiving notifications
- [ ] At least one successful backup has completed since launch
- [ ] No P1 or P2 incidents have occurred
- [ ] Error rate has remained below 1%
- [ ] No unexpected resource growth (memory, connections, disk)
- [ ] Import and export flows are completing successfully
- [ ] User feedback channel is monitored

### First Week

| Check | Frequency | Owner | Action if Abnormal |
|-------|-----------|-------|--------------------|
| Daily backup verification | Daily | Platform | Verify via `scripts/backup-monitor.sh`; investigate any gaps |
| Resource trend analysis | Daily | Platform | Review Docker stats trends; plan capacity if growth exceeds projection |
| Error log review | Daily | Backend | Review and categorize new error patterns; file issues for recurring errors |
| Performance trend | Daily | Backend | Compare p50/p95/p99 latency with pre-launch baseline |
| Queue processing health | Daily | Backend | Verify import/export completion rates and failure rates |
| Security log review | Daily | Security | Review authentication failures, rate limit hits, unusual access patterns |
| User feedback triage | Daily | Product | Categorize and prioritize user-reported issues |

### First Week Checklist

- [ ] 7 consecutive successful daily backups completed
- [ ] No unresolved P1/P2 incidents
- [ ] Resource utilization trends are stable or within projections
- [ ] No new critical/high vulnerabilities discovered
- [ ] All user-reported issues triaged and prioritized
- [ ] Post-launch retrospective scheduled (end of first week)
- [ ] On-call handoff completed at least once successfully
- [ ] Alert tuning completed (false positives addressed)

### Post-Launch Retrospective (End of Week 1)

Schedule a retrospective to cover:

1. What went well during launch and first week
2. What issues were encountered and how they were resolved
3. What monitoring gaps were identified
4. What runbook updates are needed based on real-world experience
5. Action items for the next sprint (performance, reliability, operational improvements)

---

## Appendix: Evidence Templates

### Test Result Evidence Format

```
Criterion: [Number and description]
Date: YYYY-MM-DD
Runner: [CI URL or operator name]
Result: PASS / FAIL
Command: [exact command run]
Output summary: [key output lines or link to full log]
```

### Document Review Evidence Format

```
Criterion: [Number and description]
Document: [path to reviewed document]
Reviewer: [name]
Date: YYYY-MM-DD
Finding: No issues / [list of issues found]
Sign-off: [Approved / Needs revision]
```

### Drill Report Evidence Format

```
Criterion: [Number and description]
Date: YYYY-MM-DD
Report: [link to filled drill-report-template.md]
RTO measured: [minutes]
RPO measured: [minutes]
Result: PASS / FAIL
Approvers: [names]
```
