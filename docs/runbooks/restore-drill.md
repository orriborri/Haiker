# Restore Drill Runbook

> **Owner:** Platform / Infrastructure team
> **Last updated:** 2025-01-20
> **Review cadence:** Quarterly (minimum), or after any significant infrastructure change

---

## Table of Contents

1. [Purpose and Scope](#purpose-and-scope)
2. [Prerequisites](#prerequisites)
3. [Drill Execution Steps](#drill-execution-steps)
4. [RTO Measurement Framework](#rto-measurement-framework)
5. [RPO Measurement](#rpo-measurement)
6. [Integrity Verification Checklist](#integrity-verification-checklist)
7. [Pass/Fail Criteria](#passfail-criteria)
8. [Gap and Remediation Tracking](#gap-and-remediation-tracking)
9. [Post-Drill Review Process](#post-drill-review-process)
10. [Approval Workflow](#approval-workflow)
11. [Cross-References](#cross-references)

---

## Purpose and Scope

### What the Drill Proves

A restore drill validates the end-to-end recoverability of the Haiker platform by exercising the full disaster recovery pipeline in an isolated environment. Specifically, it proves:

- PostgreSQL Point-in-Time Recovery (PITR) backups can be restored to a target timestamp
- MinIO object storage can be mirrored from the backup instance
- Database migrations are consistent between the restored state and the codebase
- Cross-context referential integrity is intact after recovery
- The application can start and pass smoke tests against restored data
- The overall process completes within the defined Recovery Time Objective (RTO)

### Cadence

| Frequency | Trigger |
|-----------|---------|
| Quarterly | Scheduled (aligned with backup operations review) |
| Ad-hoc | After major infrastructure changes, migration schema changes, or backup pipeline modifications |
| Pre-launch | Required for launch readiness sign-off |

### Scope Boundaries

The drill covers:

- All bounded contexts: identity, activity_catalog, recorded_activity, route_editing, route_versioning, imports, exports, platform
- PostgreSQL PITR with WAL replay
- MinIO bucket restoration (haiker-artifacts, haiker-uploads)
- Migration state verification
- Cross-schema referential integrity
- Application health (smoke tests)

The drill does NOT cover:

- Network-level failover or DNS propagation
- Load balancer reconfiguration
- External service dependency recovery (third-party APIs)
- Client-side cache invalidation

---

## Prerequisites

### Credentials and Access

| Credential | Purpose | Source |
|------------|---------|--------|
| `BACKUP_ENCRYPTION_KEY` | Decrypt PostgreSQL base backups (GPG AES256) | Secrets manager |
| `BACKUP_POSTGRES_USER` / `BACKUP_POSTGRES_PASSWORD` | PostgreSQL replication access | Secrets manager |
| `BACKUP_MINIO_ROOT_USER` / `BACKUP_MINIO_ROOT_PASSWORD` | Backup MinIO admin access | Secrets manager |
| `MINIO_SOURCE_ACCESS_KEY` / `MINIO_SOURCE_SECRET_KEY` | Read access to source MinIO | Secrets manager |

### Environment Requirements

- **Isolated Docker environment:** The drill runs in a separate Docker Compose project (default: `haiker-drill`) to avoid impacting production
- **Docker and Docker Compose:** Available on the operator's workstation or dedicated drill host
- **Sufficient disk space:** At least 2x the current database size for restore + WAL replay
- **Network access:** To the backup storage volumes (or mounted Docker volumes)
- **Repository checkout:** Current version of the codebase with all migration files

### Tools

| Tool | Purpose |
|------|---------|
| `docker` / `docker compose` | Container orchestration for the isolated drill environment |
| `psql` | PostgreSQL client for verification queries |
| `mc` (MinIO Client) | Object storage verification |
| `gpg` | Backup decryption |
| `bash` (4.0+) | Script execution |

### Pre-Drill Checklist

- [ ] Verify backup credentials are accessible from secrets manager
- [ ] Confirm backup volumes are mounted and accessible
- [ ] Verify no other drill environment is running (`docker compose -p haiker-drill ps`)
- [ ] Notify stakeholders of the planned drill window
- [ ] Ensure the operator has a copy of `deploy/backup/docker-compose.backup.yml`
- [ ] Confirm the target recovery timestamp is within the WAL retention window (7 days)

---

## Drill Execution Steps

The following manual walkthrough parallels what `scripts/restore-drill.sh` automates. Use these steps when running a drill manually or when debugging an automated drill failure.

### Phase 1: Environment Setup

**Goal:** Spin up an isolated Docker Compose project with PostgreSQL and MinIO services.

```bash
# Set the drill project name to isolate from production
export DRILL_PROJECT_NAME="haiker-drill"
export DRILL_COMPOSE_FILE="deploy/backup/docker-compose.backup.yml"

# Ensure no leftover drill environment
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" down -v 2>/dev/null || true

# Start required services
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" up -d postgres minio-backup

# Wait for PostgreSQL readiness (up to 120 seconds)
until docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" exec -T postgres pg_isready -q; do
    sleep 2
done

# Wait for MinIO readiness (up to 60 seconds)
until docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" exec -T minio-backup \
    curl -sf http://localhost:9000/minio/health/live; do
    sleep 2
done
```

**Success indicator:** Both PostgreSQL and MinIO respond to health checks.

### Phase 2: PostgreSQL PITR Restore

**Goal:** Restore the PostgreSQL database to a specific point in time using `pg_basebackup` + WAL replay.

```bash
# Choose a target time (e.g., 12 hours ago)
TARGET_TIME=$(date -u -d '12 hours ago' +%Y-%m-%dT%H:%M:%SZ)

# Execute the PITR restore
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" exec -T postgres \
    /scripts/restore-postgres.sh \
    --target-time "${TARGET_TIME}" \
    --yes
```

**What happens internally:**

1. PostgreSQL is stopped gracefully
2. Existing PGDATA is cleared
3. The selected base backup is decrypted (GPG) and extracted
4. `recovery.signal` is configured with the target timestamp and WAL restore command
5. PostgreSQL starts in recovery mode and replays WAL segments
6. Once the target timestamp is reached, the database is promoted to primary (read-write)

**Success indicator:** `pg_isready` returns success and `SELECT pg_is_in_recovery()` returns `false`.

### Phase 3: MinIO Object Restore

**Goal:** Mirror object storage data from the backup MinIO instance to the drill environment.

```bash
# Mirror artifact bucket
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" exec -T minio-backup-scheduler \
    mc mirror --preserve --overwrite backup/haiker-artifacts drill/haiker-artifacts

# Mirror uploads bucket
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" exec -T minio-backup-scheduler \
    mc mirror --preserve --overwrite backup/haiker-uploads drill/haiker-uploads
```

**Success indicator:** `mc mirror` completes without errors; object counts match source.

### Phase 4: Migration Verification

**Goal:** Confirm all expected database migrations are applied in the restored database.

```bash
# Query applied migrations
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" exec -T postgres \
    psql -U haiker -d haiker -t -A -c \
    "SELECT version FROM _sqlx_migrations ORDER BY version;"

# Compare against migration files on disk
ls migrations/*.sql | wc -l
```

Alternatively, use the migration recovery script:

```bash
scripts/migration-recovery.sh verify
```

**Success indicator:** All migration files on disk have corresponding successful entries in `_sqlx_migrations`. Zero pending or failed migrations.

### Phase 5: Integrity Checks

**Goal:** Verify cross-context referential integrity across all bounded context schemas.

```bash
# Determine the drill PostgreSQL port
DRILL_PG_PORT=$(docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" \
    port postgres 5432 | sed 's/.*://')

# Run the integrity check script
PGHOST="localhost" \
PGPORT="${DRILL_PG_PORT}" \
PGUSER="haiker" \
PGPASSWORD="haiker" \
PGDATABASE="haiker" \
    scripts/integrity-check.sh
```

**Success indicator:** JSON output shows `"status": "healthy"` with all checks passing.

### Phase 6: Application Startup and Smoke Tests

**Goal:** Verify the application can start and respond to basic API requests.

```bash
# If an API service is available in the drill compose file:
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" up -d api

# Wait for the API to become available
API_PORT=$(docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" \
    port api 3000 | sed 's/.*://')

# Run smoke tests
API_URL="http://localhost:${API_PORT}" scripts/smoke-test.sh
```

If no API service is available, verify basic database connectivity:

```bash
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" exec -T postgres \
    psql -U haiker -d haiker -c "SELECT 1;"
```

**Success indicator:** Smoke tests pass or basic connectivity check succeeds.

### Phase 7: Cleanup

**Goal:** Tear down the isolated drill environment and release resources.

```bash
# Remove all drill containers and volumes
docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" down -v
```

To keep the environment running for debugging (e.g., investigating a failure):

```bash
# Skip cleanup - remember to clean up manually later
echo "Drill environment '${DRILL_PROJECT_NAME}' left running for debugging"
echo "Clean up with: docker compose -f ${DRILL_COMPOSE_FILE} -p ${DRILL_PROJECT_NAME} down -v"
```

---

## RTO Measurement Framework

### Phase Definitions and Timing

Recovery Time Objective (RTO) is measured as the total elapsed wall-clock time from the start of the first recovery action to the completion of all verification steps.

| Phase | Description | Expected Duration |
|-------|-------------|-------------------|
| Infrastructure Provisioning (Setup) | Spin up isolated environment, wait for service health | 1-3 minutes |
| PostgreSQL PITR | Decrypt backup, restore base, replay WAL to target | 5-30 minutes |
| MinIO Restore | Mirror buckets from backup instance | 10-60 minutes |
| Migration Verification | Validate applied migrations match codebase | 1-2 minutes |
| Integrity Checks | Cross-context referential integrity queries | 2-5 minutes |
| Application Startup | Start API service, wait for readiness | 1-3 minutes |
| Smoke Tests | Run health checks against restored environment | 1-3 minutes |

### Timing Methodology

Each phase is timed with start and end timestamps:

```
Phase Start: $(date +%s) at phase entry
Phase End:   $(date +%s) at phase exit
Phase Duration = End - Start (seconds)
```

The automated drill script (`scripts/restore-drill.sh`) captures these timestamps automatically and includes them in the JSON report.

### Total RTO Formula

```
Total RTO = Sum of all phase durations
          = T(setup) + T(pg_restore) + T(minio_restore) + T(migration_verify)
            + T(integrity_check) + T(smoke_test)
```

Note: The cleanup phase is NOT included in RTO calculation since it occurs after recovery is complete.

### Provisional Target: 4 Hours (14,400 seconds)

The 4-hour RTO target is derived from the sum of expected worst-case component recovery times with a safety buffer:

| Component | Worst Case | Justification |
|-----------|-----------|---------------|
| Infrastructure Provisioning | 5 minutes | Container startup with health checks |
| PostgreSQL PITR | 60 minutes | Large database + full WAL retention replay |
| MinIO Restore | 90 minutes | Full bucket mirror with many objects |
| Migration Verification | 5 minutes | Sequential file comparison |
| Integrity Checks | 10 minutes | Complex cross-schema queries on large dataset |
| Application Startup | 5 minutes | Cold start with connection pool init |
| Smoke Tests | 5 minutes | Full API endpoint validation |
| **Subtotal** | **180 minutes (3 hours)** | |
| Safety Buffer (33%) | 60 minutes | Accounts for retries, network latency, operator overhead |
| **Total Target** | **240 minutes (4 hours)** | |

### Handling Phase Overruns

If any phase exceeds its expected duration:

1. **During automated drill:** The script continues but marks the overrun in the report. The overall drill will be flagged if total RTO exceeds the target.
2. **During manual drill:** Document the overrun, the cause, and whether it would be acceptable in a real disaster scenario.
3. **Post-drill action:** Any phase that exceeds 2x its expected duration should generate a gap in the remediation tracking table.
4. **Overall timeout:** The automated drill has a configurable timeout (`DRILL_TIMEOUT_SECONDS`, default: 3600s). If the drill has not completed within this window, it is terminated and marked as failed.

---

## RPO Measurement

### Recovery Point Objective Definition

RPO measures the maximum acceptable data loss window. For Haiker:

| Component | RPO Target | Mechanism |
|-----------|-----------|-----------|
| PostgreSQL | 15 minutes | Continuous WAL archiving at 15-minute intervals |
| MinIO | 24 hours | Daily `mc mirror` schedule (03:00 UTC) |

### Verifying the Data Loss Window

After a restore, verify how much data was potentially lost:

**PostgreSQL RPO Verification:**

```sql
-- Last WAL segment replayed timestamp
SELECT pg_last_xact_replay_timestamp();

-- Compare with restore target
-- Data loss window = Target Time - Last WAL Timestamp (should be < 15 minutes)
SELECT now() - pg_last_xact_replay_timestamp() AS recovery_lag;
```

**MinIO RPO Verification:**

```bash
# Check the timestamp of the most recent object in the backup bucket
mc stat backup/haiker-artifacts --recursive | sort -k 4 | tail -1

# Compare with the current time
# Data loss window = Current Time - Last Mirror Completion
```

### RPO Assessment in Drill Report

Record the following in the drill report:

| Metric | Value |
|--------|-------|
| Last WAL Timestamp (PostgreSQL) | Timestamp from `pg_last_xact_replay_timestamp()` |
| Restore Target Timestamp | The `--target-time` value used |
| PostgreSQL Data Loss Window | Target - Last WAL (should be < 15 min) |
| Last MinIO Sync Timestamp | Most recent object modification time in backup |
| MinIO Data Loss Window | Current Time - Last Mirror (should be < 24h) |

If either data loss window exceeds the RPO target, the drill should be flagged and the cause investigated.

---

## Integrity Verification Checklist

The following checks are executed by `scripts/integrity-check.sh` and verify cross-context referential coherence. All checks must report zero violations for a healthy restore.

| # | Check | Query Description | Expected Healthy Outcome |
|---|-------|-------------------|--------------------------|
| 1 | Orphan Activities | Activities in `activity_catalog.activities` with `owner_id` not found in `identity.users` | 0 orphan activities |
| 2 | Dangling Imports (owners) | Imports in `imports.imports` with `owner_id` not found in `identity.users` | 0 dangling import owners |
| 3 | Dangling Imports (artifacts) | Imports with `source_artifact_id` not found in `recorded_activity.source_artifacts` | 0 dangling import artifacts |
| 4 | Dangling Imports (activities) | Imports with `activity_id` not found in `activity_catalog.activities` | 0 dangling import activities |
| 5 | Orphan Tracks | Recorded tracks in `recorded_activity.recorded_tracks` with `source_revision_id` not found in `recorded_activity.source_revisions` | 0 orphan tracks |
| 6 | Invalid Draft References (owners) | Route drafts in `route_editing.drafts` with `owner_id` not found in `identity.users` | 0 invalid draft owner references |
| 7 | Invalid Draft References (activities) | Route drafts with `activity_id` not found in `activity_catalog.activities` | 0 invalid draft activity references |
| 8 | Broken Version Pointers | Activities with `current_route_version_id` not found in `route_versioning.route_versions` | 0 broken version pointers |
| 9 | Unknown Audit Actors | Audit log entries in `platform.audit_log` with `actor_id` not found in `identity.users` | 0 unknown actors (or only system/service actors) |
| 10 | MinIO Artifact Existence | Sampled `object_storage_key` values from `recorded_activity.source_artifacts` checked against MinIO bucket | All sampled keys exist in MinIO |

### Interpreting Results

- **All checks healthy:** Restore has full cross-context coherence.
- **Audit actor warnings:** May be acceptable if the only unresolved actors are system-level service accounts (not real user IDs). Document in the drill report.
- **MinIO check skipped:** If `mc` is not available or the MinIO alias is not configured, this check is skipped with a warning. This is acceptable for database-only drills but should be resolved for full drills.
- **Any critical finding:** The drill fails. Investigate the root cause (partial restore, WAL gap, schema mismatch).

---

## Pass/Fail Criteria

### Drill Passes When

All of the following conditions are met:

| Criterion | Measurement |
|-----------|-------------|
| PostgreSQL PITR completes successfully | `pg_isready` returns success, `pg_is_in_recovery()` is false |
| MinIO restore completes | `mc mirror` exits 0 or objects are verified |
| All migrations are applied | `scripts/migration-recovery.sh verify` reports zero pending/failed |
| Integrity checks pass | `scripts/integrity-check.sh` reports `"status": "healthy"` |
| Smoke tests pass | `scripts/smoke-test.sh` exits 0 |
| Total RTO is within target | Total elapsed time <= 14,400 seconds (4 hours) |
| RPO is within targets | PostgreSQL data loss < 15 min, MinIO data loss < 24h |

### Drill Fails When

Any of the following conditions occur:

| Condition | Severity | Action |
|-----------|----------|--------|
| PostgreSQL restore does not complete | Critical | Investigate backup integrity, WAL availability |
| Integrity checks report critical violations | Critical | Investigate schema coherence, partial restore |
| Total RTO exceeds target (4 hours) | High | Identify bottleneck phase, plan optimization |
| Smoke tests fail | High | Investigate application configuration, missing data |
| Migration verification shows missing migrations | Medium | Investigate restore point vs. codebase version alignment |
| MinIO restore has errors but non-critical | Medium | Document gap, verify manual recovery path |
| Audit actor warnings (system accounts only) | Low | Document as acceptable, no action needed |

### Exit Codes (Automated Drill)

| Exit Code | Meaning |
|-----------|---------|
| 0 | Drill passed (all criteria met) |
| 1 | Drill completed with warnings (non-critical issues) |
| 2 | Drill failed (one or more critical criteria not met) |

---

## Gap and Remediation Tracking

Use the following table format to record gaps discovered during the drill. Each gap represents something that was missing, broken, or suboptimal during the recovery process.

### Gap Tracking Table

| Gap | Severity | Owner | Remediation | Target Date | Status |
|-----|----------|-------|-------------|-------------|--------|
| _Description of what failed or was missing_ | critical / high / medium / low | _Team or person responsible_ | _Specific fix or improvement_ | _YYYY-MM-DD_ | open / in-progress / resolved |

### Severity Definitions

| Severity | Definition | Example |
|----------|-----------|---------|
| Critical | Recovery cannot complete without this fix | Backup decryption key unavailable, WAL segments missing |
| High | Recovery completes but with unacceptable data loss or RTO breach | PITR takes 3x expected time, integrity check finds orphaned records |
| Medium | Recovery completes but with degraded confidence | MinIO check skipped, one smoke test endpoint unavailable |
| Low | Minor improvement opportunity | Documentation unclear, operator had to look up a command |

### Process for Recording Gaps

1. During the drill, note any deviation from the expected procedure or outcome
2. After the drill, categorize each deviation as a gap with severity
3. Assign an owner and define a specific remediation action
4. Set a target resolution date (before the next scheduled drill)
5. Track status updates in subsequent drill reviews

### Launch Readiness Gate

All critical and high severity gaps MUST be resolved before launch sign-off. Medium severity gaps must have an approved remediation plan with a target date. Low severity gaps are tracked but do not block launch.

---

## Post-Drill Review Process

### Debrief Steps

Conduct a debrief within 48 hours of the drill completion:

1. **Gather participants:** All operators who participated in the drill, plus the platform team lead and on-call lead
2. **Review the drill report:** Walk through each phase, noting timing, status, and any anomalies
3. **Identify gaps:** Document anything that was harder than expected, took longer than expected, or failed
4. **Categorize findings:** Assign severity, owner, and remediation to each gap
5. **Update documentation:** If any procedure steps were unclear or incorrect, update this runbook immediately
6. **Record lessons learned:** Capture anything that should change in the drill process itself
7. **Schedule follow-ups:** Set calendar reminders for remediation target dates

### Documentation Deliverables

After each drill, produce the following:

| Document | Location | Responsibility |
|----------|----------|----------------|
| Drill Report (filled template) | `docs/templates/drill-report-template.md` (filled copy stored per team process) | Drill operator |
| Gap Tracking Updates | Linked from drill report | Platform team lead |
| Runbook Updates | `docs/runbooks/restore-drill.md` | Drill operator + reviewer |
| Script Fixes (if any) | PRs to relevant scripts | Assigned developer |

### Review Frequency

| Review Type | Frequency | Participants |
|-------------|-----------|--------------|
| Drill execution | Quarterly | Drill operator (rotating) |
| Debrief meeting | Within 48h of drill | All participants + leads |
| Gap status review | Monthly | Platform team |
| Runbook accuracy review | Quarterly (with drill) | Platform team lead |

---

## Approval Workflow

### Sign-Off Requirements

The restore drill must be approved by the following roles before it can be used as evidence of launch readiness:

| Role | Responsibility | Sign-Off Condition |
|------|---------------|-------------------|
| Platform Team Lead | Owns backup infrastructure and drill procedures | Drill passed with no unresolved critical/high gaps |
| On-Call Lead | Responsible for incident response capability | Team is trained on procedures, runbook is current |

### Approval Process

1. **Drill completion:** Operator fills in the drill report template with all results
2. **Gap review:** Platform team lead reviews all gaps and confirms remediation plan
3. **Sign-off request:** Operator submits the completed drill report for sign-off
4. **Review and approve:** Each approver reviews the report and either:
   - Approves (signs the report with date and name)
   - Requests changes (specifies what must be resolved before re-submission)
5. **Launch readiness:** Both sign-offs on the same drill report constitute launch readiness evidence

### Conditions for Launch Readiness

All of the following must be true:

- [ ] Most recent drill passed (exit code 0 or 1 with documented acceptable warnings)
- [ ] Total RTO was within the 4-hour target
- [ ] All critical and high severity gaps are resolved
- [ ] Medium severity gaps have approved remediation plans
- [ ] Both Platform Team Lead and On-Call Lead have signed off
- [ ] Drill was conducted within the last 90 days

### Re-Drill Triggers

A new drill is required (invalidating previous sign-off) when:

- Major infrastructure changes are deployed (new hosting, storage migration)
- Backup pipeline is significantly modified
- New bounded contexts or schemas are added
- More than 90 days have elapsed since the last approved drill

---

## Cross-References

### Related Documentation

| Document | Path | Description |
|----------|------|-------------|
| Backup Operations Runbook | [docs/runbooks/backup-operations.md](../runbooks/backup-operations.md) | Full PITR procedure, MinIO restoration, alert response |
| Backup Quick Reference | [docs/runbooks/backup-quickref.md](../runbooks/backup-quickref.md) | One-page backup operations cheat sheet |

### Related Scripts

| Script | Path | Description |
|--------|------|-------------|
| Restore Drill (automated) | [scripts/restore-drill.sh](../../scripts/restore-drill.sh) | Automated drill orchestration with JSON report output |
| Integrity Check | [scripts/integrity-check.sh](../../scripts/integrity-check.sh) | Cross-context referential integrity validation |
| Migration Recovery | [scripts/migration-recovery.sh](../../scripts/migration-recovery.sh) | Migration status, verify, rollback, and forward-fix |
| PostgreSQL Restore | [scripts/restore-postgres.sh](../../scripts/restore-postgres.sh) | Point-in-Time Recovery execution |
| MinIO Backup | [scripts/backup-minio.sh](../../scripts/backup-minio.sh) | MinIO bucket mirroring |
| Smoke Test | [scripts/smoke-test.sh](../../scripts/smoke-test.sh) | API health check validation |

### Templates

| Template | Path | Description |
|----------|------|-------------|
| Drill Report Template | [docs/templates/drill-report-template.md](../templates/drill-report-template.md) | Fill-in template for recording drill results |

### Infrastructure

| Resource | Path | Description |
|----------|------|-------------|
| Backup Docker Compose | [deploy/backup/docker-compose.backup.yml](../../deploy/backup/docker-compose.backup.yml) | Docker Compose overlay for backup infrastructure |
| Backup Environment Example | [deploy/backup/env.backup.example](../../deploy/backup/env.backup.example) | Environment variable reference for backup services |
