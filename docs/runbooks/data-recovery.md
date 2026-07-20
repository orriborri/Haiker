# Data Recovery Runbook

> **Owner:** Platform / Infrastructure team  
> **Last updated:** 2025-01-15  
> **Review cadence:** Quarterly (aligned with disaster recovery drill)

---

## Table of Contents

1. [Overview](#overview)
2. [Scope of Recoverable Data](#scope-of-recoverable-data)
3. [Data Loss Assessment Checklist](#data-loss-assessment-checklist)
4. [Single-Record Recovery](#single-record-recovery)
5. [Bulk Data Recovery (PITR)](#bulk-data-recovery-pitr)
6. [Object Storage Recovery](#object-storage-recovery)
7. [Recovery from Application Bugs](#recovery-from-application-bugs)
8. [Communication with Affected Users](#communication-with-affected-users)
9. [Contact and Escalation](#contact-and-escalation)

---

## Overview

This runbook covers procedures for recovering lost or corrupted data in the Haiker platform. It addresses single-record fixes, bulk recovery via Point-in-Time Recovery (PITR), object storage recovery, and corrective actions for application bug-induced data issues.

### Key References

| Document | Purpose |
|----------|---------|
| [Backup Operations](backup-operations.md) | Infrastructure-level backup and PITR procedures |
| [Restore Drill](restore-drill.md) | Recovery testing procedures |
| [Migration Recovery](migration-recovery.md) | Migration-related data issues |
| [Incident Response](incident-response.md) | Incident classification for data loss events |

### Key Scripts

| Script | Purpose |
|--------|---------|
| `scripts/restore-postgres.sh` | Point-in-Time Recovery for PostgreSQL |
| `scripts/verify-backup.sh` | Verify backup integrity before recovery |
| `scripts/smoke-test.sh` | Post-recovery health verification |
| `scripts/backup-monitor.sh` | Verify backup system health |

### Recovery Point Objectives

| Data Type | RPO | Method |
|-----------|-----|--------|
| PostgreSQL (all schemas) | 15 minutes | WAL archiving + PITR |
| MinIO objects | 24 hours | Daily mirror with versioning |
| Audit log | 15 minutes | Stored in PostgreSQL (platform schema) |

---

## Scope of Recoverable Data

### PostgreSQL Data (PITR-recoverable)

All data in these schemas can be recovered to any point within the 7-day WAL retention window:

| Schema | Key Tables | Recovery Notes |
|--------|-----------|----------------|
| `identity` | users, sessions | User accounts and active sessions |
| `activity_catalog` | activities | Activity definitions and metadata |
| `recorded_activity` | recorded_activities | User-recorded activity data |
| `route_editing` | drafts, draft_operations | In-progress route edits |
| `route_versioning` | route_versions | Published route versions |
| `imports` | imports | Import job tracking and status |
| `exports` | exports | Export job tracking and status |
| `polar_integration` | sync_state | Polar API sync state |
| `platform` | outbox, job_queue, audit_log | Platform infrastructure tables |

### MinIO Object Storage (version-recoverable)

| Bucket | Contents | Recovery Notes |
|--------|----------|----------------|
| `haiker-artifacts` | Generated exports, processed files | Recoverable from backup MinIO (versioned) |
| `haiker-uploads` | User-uploaded files (GPX, images) | Recoverable from backup MinIO (versioned) |

### Non-Recoverable Data

| Data Type | Why Not Recoverable | Mitigation |
|-----------|-------------------|------------|
| In-flight requests at crash time | Not yet persisted | Clients should retry |
| Active WebSocket state | In-memory only | Reconnection rebuilds state |
| Temporary processing files | Ephemeral by design | Re-trigger the job |

---

## Data Loss Assessment Checklist

Before beginning any recovery, assess the scope and impact:

- [ ] **What data is affected?** (Which schema, tables, rows)
- [ ] **When did the loss occur?** (Timestamp for PITR targeting)
- [ ] **How many users are affected?**
- [ ] **Is data loss ongoing?** (Stop the source if still occurring)
- [ ] **What caused the loss?** (Bug, migration, manual error, corruption)
- [ ] **Is the data fully lost or corrupted?** (Corrupted may be partially salvageable)
- [ ] **Do we have a recent backup?** (Run `scripts/verify-backup.sh`)
- [ ] **What is the RPO gap?** (Data between last backup and the event)
- [ ] **Can affected users re-create the data?** (User-generated content vs. system-generated)
- [ ] **Is there an audit trail?** (Check `platform.audit_log`)

### Severity Assessment

| Scope | Severity | Action |
|-------|----------|--------|
| Single user, single record | P3 | Single-record recovery |
| Multiple users, specific feature | P2 | Targeted bulk recovery |
| Wide data corruption across schemas | P1 | Full PITR |
| Data loss with no backup coverage | P1 | Escalate immediately |

---

## Single-Record Recovery

Use this procedure when a small number of specific records need to be restored or corrected.

### From Audit Log

If the `platform.audit_log` table captured the original values:

```bash
# Find the audit entry for the affected record
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT id, action, entity_type, entity_id, old_value, new_value, created_at
  FROM platform.audit_log
  WHERE entity_id = '<affected-record-id>'
  ORDER BY created_at DESC
  LIMIT 10;
"
```

If the old value is available, reconstruct the record:

```bash
# Example: restore a deleted activity
docker compose exec postgres psql -U haiker -d haiker -c "
  -- Use the old_value from audit_log to reconstruct
  INSERT INTO activity_catalog.activities (id, name, description, created_at, updated_at)
  VALUES ('<id>', '<name>', '<description>', '<original_created_at>', now())
  ON CONFLICT (id) DO UPDATE SET
    name = EXCLUDED.name,
    description = EXCLUDED.description,
    updated_at = EXCLUDED.updated_at;
"
```

### From PITR in a Drill Environment

When the audit log does not contain sufficient data, use PITR in an isolated environment to extract the record:

**Step 1: Start a drill environment with a point-in-time restore**

```bash
# Restore to a time when the record existed
docker compose -f deploy/backup/docker-compose.backup.yml \
  -p recovery-drill \
  up -d postgres

docker compose -f deploy/backup/docker-compose.backup.yml \
  -p recovery-drill \
  exec postgres /scripts/restore-postgres.sh \
  --target-time "<timestamp-before-data-loss>" \
  --yes
```

**Step 2: Extract the record from the recovered database**

```bash
# Export the specific record(s)
docker compose -f deploy/backup/docker-compose.backup.yml \
  -p recovery-drill \
  exec postgres psql -U haiker -d haiker -c "
    COPY (
      SELECT * FROM <schema>.<table> WHERE id = '<record-id>'
    ) TO STDOUT WITH CSV HEADER;
  " > /tmp/recovered-record.csv
```

**Step 3: Import into the production database**

```bash
# Review the data first
cat /tmp/recovered-record.csv

# Import (adjust the COPY command for your table structure)
docker compose exec postgres psql -U haiker -d haiker -c "
  COPY <schema>.<table> FROM STDIN WITH CSV HEADER;
" < /tmp/recovered-record.csv
```

**Step 4: Clean up the drill environment**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml -p recovery-drill down -v
```

### Direct SQL Fix

For known data corrections (e.g., wrong value due to a bug):

```bash
# Always wrap in a transaction for safety
docker compose exec postgres psql -U haiker -d haiker -c "
  BEGIN;

  -- Show current state
  SELECT * FROM <schema>.<table> WHERE id = '<record-id>';

  -- Apply the fix
  UPDATE <schema>.<table>
  SET <column> = '<correct-value>', updated_at = now()
  WHERE id = '<record-id>';

  -- Verify
  SELECT * FROM <schema>.<table> WHERE id = '<record-id>';

  COMMIT;
"
```

---

## Bulk Data Recovery (PITR)

Use PITR when data corruption or loss is widespread and single-record recovery is impractical.

### When to Use PITR

- Multiple tables or schemas affected
- Large number of records corrupted
- Exact scope of damage is unclear
- Time pressure requires fastest path to known-good state

### Prerequisites

- Application must be stopped during PITR
- Recent backup must exist (verify first)
- Target recovery time must be determined

### Procedure

**Step 1: Stop the application**

```bash
docker compose stop api worker
```

**Step 2: Verify backup availability**

```bash
scripts/verify-backup.sh

# List available backups
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  find /backups -name "*.tar.gz.gpg" -type f | sort -r | head -5
```

**Step 3: Determine target recovery time**

The target time should be just before the data loss event occurred:

```bash
# Check audit log for the earliest sign of the problem
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT min(created_at) as earliest_problem
  FROM platform.audit_log
  WHERE action = '<problematic-action>'
  AND created_at > '<approximate-start-time>';
"
```

**Step 4: Execute PITR**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  /scripts/restore-postgres.sh \
  --target-time "<target-recovery-time>" \
  --yes
```

**Step 5: Verify recovery**

```bash
# Check database is accepting connections
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  pg_isready

# Verify data is present
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT schemaname, relname, n_live_tup
  FROM pg_stat_user_tables
  ORDER BY schemaname, relname;
"
```

**Step 6: Restart application**

```bash
docker compose start api worker
scripts/smoke-test.sh
```

**Step 7: Assess the RPO gap**

Any data created between the target recovery time and the PITR execution is lost. Determine:
- What users were active during that window
- What data they may have created
- Whether they can re-create it
- Whether notification is needed

See [Backup Operations - PostgreSQL PITR Procedure](backup-operations.md#postgresql-pitr-procedure) for the full procedure with additional detail.

---

## Object Storage Recovery

### Single File Recovery

Recover a specific file from the backup MinIO instance:

```bash
# List versions of the file
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc ls --versions backup/haiker-uploads/path/to/file.gpx

# Copy the desired version back to source
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc cp backup/haiker-uploads/path/to/file.gpx source/haiker-uploads/path/to/file.gpx
```

### Recover a Specific Version

```bash
# List all versions with version IDs
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc ls --versions backup/haiker-uploads/path/to/file.gpx

# Restore a specific version by ID
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc cp --version-id <VERSION_ID> \
  backup/haiker-uploads/path/to/file.gpx \
  source/haiker-uploads/path/to/file.gpx
```

### Bucket-Level Recovery

Restore an entire bucket from the backup:

```bash
# Full bucket restore (overwrites existing objects)
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc mirror --preserve --overwrite backup/haiker-uploads source/haiker-uploads

# Verify object counts
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc ls --recursive --summarize backup/haiker-uploads
```

### Point-in-Time Recovery (Version-Based)

Since the backup MinIO instance has versioning enabled, you can list and selectively restore older versions:

```bash
# List all versions of all objects in a bucket (or subdirectory)
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc ls --versions --recursive backup/haiker-uploads/user-data/

# Identify versions from before the data loss event
# (Compare version timestamps to the incident time)

# Restore specific versions
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc cp --version-id <VERSION_ID> \
  backup/haiker-uploads/user-data/file.gpx \
  source/haiker-uploads/user-data/file.gpx
```

### Recovery Limitations

- MinIO backup RPO is 24 hours (daily mirror schedule)
- Objects uploaded after the last mirror and before the incident are not in the backup
- Versioned recovery requires knowing the correct version ID

---

## Recovery from Application Bugs

When an application bug has written incorrect data, follow this process to identify affected records and craft corrective queries.

### Step 1: Identify the Bug and Its Data Impact

```bash
# Determine when the buggy code was deployed
git log --oneline --since="2025-01-10" --until="2025-01-15" -- crates/

# Check which endpoints/jobs are affected
# Review the bug fix PR/commit for clues about data impact
```

### Step 2: Identify Affected Records

```bash
# Query for records modified during the bug window
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT id, updated_at, <affected_columns>
  FROM <schema>.<table>
  WHERE updated_at BETWEEN '<bug-deploy-time>' AND '<bug-fix-time>'
  AND <condition-identifying-buggy-data>;
"
```

### Step 3: Determine the Correct Values

**Option A: From audit log**

```bash
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT entity_id, old_value, new_value, created_at
  FROM platform.audit_log
  WHERE entity_type = '<affected-entity>'
  AND created_at BETWEEN '<bug-deploy-time>' AND '<bug-fix-time>'
  ORDER BY created_at;
"
```

**Option B: From PITR drill environment**

See [Single-Record Recovery: From PITR in a Drill Environment](#from-pitr-in-a-drill-environment) above.

**Option C: From application logic**

If the correct value can be recomputed (e.g., a derived field), compute it from the source data.

### Step 4: Craft and Test Corrective Query

```bash
# ALWAYS test in the drill environment first
docker compose -f deploy/backup/docker-compose.backup.yml \
  -p recovery-drill \
  exec postgres psql -U haiker -d haiker -c "
    BEGIN;

    -- Show affected records (dry run)
    SELECT count(*) as affected_count
    FROM <schema>.<table>
    WHERE <condition-identifying-buggy-data>;

    -- Apply correction
    UPDATE <schema>.<table>
    SET <column> = <correct-expression>, updated_at = now()
    WHERE <condition-identifying-buggy-data>;

    -- Verify
    SELECT count(*) as still_affected
    FROM <schema>.<table>
    WHERE <condition-identifying-buggy-data>;

    ROLLBACK; -- Use COMMIT only after review
"
```

### Step 5: Apply to Production

```bash
docker compose exec postgres psql -U haiker -d haiker -c "
  BEGIN;

  -- Count affected
  SELECT count(*) FROM <schema>.<table> WHERE <condition>;

  -- Apply fix
  UPDATE <schema>.<table>
  SET <column> = <correct-expression>, updated_at = now()
  WHERE <condition>;

  -- Verify count matches expected
  SELECT count(*) FROM <schema>.<table> WHERE <verification-condition>;

  COMMIT;
"
```

### Step 6: Verify

```bash
# Application-level verification
scripts/smoke-test.sh

# Verify affected users can access their data
# (manual spot-check of a few records)
```

---

## Communication with Affected Users

### When to Notify Users

| Scenario | Notification Required | Template |
|----------|----------------------|----------|
| Data fully recovered, no user action needed | Optional (transparency) | Recovery Complete |
| Data recovered with RPO gap (some data lost) | Required | Partial Recovery |
| Data unrecoverable | Required | Data Loss |
| Recovery required user action (re-upload, etc.) | Required | Action Required |

### Communication Templates

**Recovery Complete (no data loss):**

```
Subject: Service Recovery Complete - No Action Required

We experienced a brief data issue on [date] that has been fully resolved.
All your data has been recovered and is intact. No action is required on your part.

We apologize for any inconvenience.
```

**Partial Recovery (some data lost):**

```
Subject: Service Recovery - Potential Data Impact

We experienced a data issue on [date] between [start time] and [end time] UTC.
We have recovered data up to [recovery point time] UTC.

If you made changes between [recovery point] and [incident time], those changes
may need to be re-entered. Specifically:
- [Description of what data may be affected]

We sincerely apologize for the inconvenience. If you have questions or need
assistance, please contact support.
```

**Action Required:**

```
Subject: Action Required - Data Recovery

We experienced a data issue affecting [description]. While most data has been
recovered, the following requires your attention:

- [Specific action the user needs to take]
- [E.g., "Please re-upload files submitted between X and Y"]

Steps to verify your data:
1. [Step 1]
2. [Step 2]

If you encounter any issues, please contact support.
```

---

## Contact and Escalation

| Role | Responsibility |
|------|---------------|
| Platform team | Recovery tooling, PITR execution |
| On-call engineer | Initial assessment, single-record fixes |
| Database team | Bulk recovery decisions, complex SQL corrections |

### Escalation Path

1. **L1 (On-call):** Assess scope, perform single-record recovery, escalate if bulk
2. **L2 (Platform):** Execute PITR, coordinate multi-system recovery
3. **L3 (Database team):** Complex data corrections, cross-schema recovery, data integrity validation

### When to Escalate Immediately

- Data loss affecting more than 10 users
- Corruption detected in multiple schemas
- RPO gap cannot be determined
- Recovery requires modifying data in the identity schema (user accounts)
- Uncertainty about whether the fix is correct
