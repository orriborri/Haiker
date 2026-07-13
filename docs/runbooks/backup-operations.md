# Backup Operations Runbook

> **Owner:** Platform / Infrastructure team  
> **Last updated:** 2024-01-15  
> **Review cadence:** Quarterly (aligned with disaster recovery drill)

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Daily Operations](#daily-operations)
3. [PostgreSQL PITR Procedure](#postgresql-pitr-procedure)
4. [MinIO Restoration Procedure](#minio-restoration-procedure)
5. [Disaster Recovery](#disaster-recovery)
6. [Alert Response](#alert-response)
7. [Troubleshooting](#troubleshooting)
8. [Testing Backup Restoration](#testing-backup-restoration)
9. [Access and Credentials Management](#access-and-credentials-management)
10. [Contact and Ownership](#contact-and-ownership)

---

## Architecture Overview

### What Is Backed Up

| Component | Method | Schedule | Retention | Encryption |
|-----------|--------|----------|-----------|------------|
| PostgreSQL (data) | `pg_basebackup` + WAL archiving | Base: daily 02:00 UTC, WAL: continuous | 7 days (base), 7 days (WAL) | GPG AES256 symmetric |
| MinIO (object storage) | `mc mirror` bucket replication | Daily 03:00 UTC | 30 days (versioned) | Server-side encryption (SSE) |

### Where Backups Are Stored

- **PostgreSQL base backups:** `postgres_backups` Docker volume (mounted at `/backups` inside containers)
- **PostgreSQL WAL archive:** `postgres_wal_archive` Docker volume (mounted at `/var/lib/postgresql/wal_archive`)
- **MinIO backups:** `minio_backups` Docker volume on the backup MinIO instance

### Recovery Point Objectives (RPO)

- **PostgreSQL:** 15 minutes (WAL archiving interval)
- **MinIO:** 24 hours (daily mirror schedule)

### Key Infrastructure Files

- Docker Compose: `deploy/backup/docker-compose.backup.yml`
- Environment config: `deploy/backup/env.backup.example`
- PostgreSQL WAL config: `deploy/backup/postgresql/postgresql-backup.conf`
- Alert rules: `deploy/backup/monitoring/alert-rules.yml`
- MinIO policy setup: `deploy/backup/minio/setup-backup-policy.sh`

### Backup Scripts

| Script | Purpose |
|--------|---------|
| `scripts/backup-postgres.sh` | Full PostgreSQL base backup with GPG encryption |
| `scripts/restore-postgres.sh` | Point-in-Time Recovery restore |
| `scripts/verify-backup.sh` | PostgreSQL backup health verification |
| `scripts/backup-minio.sh` | MinIO bucket mirroring to backup instance |
| `scripts/verify-minio-backup.sh` | MinIO backup verification |
| `scripts/backup-monitor.sh` | Unified monitoring with webhook alerts |
| `scripts/backup-retention.sh` | Retention policy enforcement |

---

## Daily Operations

### Verify Backups Are Current

Run the verification script to check overall backup health:

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-verify \
  /scripts/verify-backup.sh
```

This returns a JSON report with checks for:
- Base backup age (threshold: 24 hours)
- WAL archive age (threshold: 15 minutes)
- Backup file integrity (GPG decryption + tar validation)
- Storage capacity

### Check Monitoring Status

Run the unified monitor:

```bash
scripts/backup-monitor.sh
```

This calls both PostgreSQL and MinIO verification scripts, evaluates alert rules, and sends webhook notifications if any thresholds are breached.

### View Latest Backup Timestamps

```bash
# List PostgreSQL backups (most recent first)
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  find /backups -name "*.tar.gz.gpg" -type f | sort -r | head -5

# Check MinIO backup status
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc ls --recursive backup/haiker-artifacts | tail -5
```

### Trigger Manual Backup

```bash
# PostgreSQL manual backup
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  /scripts/backup-postgres.sh

# MinIO manual backup
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  /scripts/backup-minio.sh
```

---

## PostgreSQL PITR Procedure

Point-in-Time Recovery allows restoring the database to any point within the WAL retention window (up to 7 days).

### Prerequisites

- Access to the backup volumes (`postgres_backups`, `postgres_wal_archive`)
- The `BACKUP_ENCRYPTION_KEY` used to encrypt the backup
- The target recovery timestamp in ISO 8601 format

### Step-by-Step Procedure

**1. Stop the application to prevent new writes:**

```bash
docker compose stop app worker
```

**2. Identify the target recovery time:**

Determine the timestamp you want to recover to (e.g., just before a data corruption event).

**3. List available backups:**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  find /backups -name "*.tar.gz.gpg" -type f | sort -r
```

Choose a base backup that was taken BEFORE your target recovery time.

**4. Run the restore script:**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  /scripts/restore-postgres.sh \
  --target-time "2024-01-15T10:30:00+00:00" \
  --backup-file /backups/20240115T020000Z.tar.gz.gpg
```

For automated recovery (no confirmation prompt):

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  /scripts/restore-postgres.sh \
  --target-time "2024-01-15T10:30:00+00:00" \
  --yes
```

**5. Verify the recovery:**

```bash
# Check PostgreSQL is accepting connections
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  pg_isready

# Check the database is no longer in recovery mode
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  psql -U haiker -c "SELECT pg_is_in_recovery();"
```

**6. Restart the application:**

```bash
docker compose start app worker
```

**7. Verify application health:**

```bash
scripts/smoke-test.sh
```

### What the Restore Script Does

1. Stops PostgreSQL gracefully
2. Clears the existing `PGDATA` directory
3. Decrypts the selected backup with GPG
4. Extracts the base backup to `PGDATA`
5. Configures `recovery.signal` with the target time and WAL restore command
6. Starts PostgreSQL in recovery mode
7. Waits for WAL replay to reach the target timestamp
8. Promotes the database to primary (read-write)

---

## MinIO Restoration Procedure

### Restore Specific Objects

MinIO backup uses versioning. To restore specific objects:

```bash
# List versions of a specific object
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc ls --versions backup/haiker-artifacts/path/to/object

# Copy an object back from the backup instance
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc cp backup/haiker-artifacts/path/to/object source/haiker-artifacts/path/to/object
```

### Full Bucket Restoration

To restore an entire bucket from the backup MinIO instance:

```bash
# Mirror from backup back to source
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc mirror --preserve --overwrite backup/haiker-artifacts source/haiker-artifacts
```

### Restore to a Point in Time (Version-Based)

Since the backup MinIO instance has versioning enabled, you can list and restore older versions:

```bash
# List all versions of objects in a bucket
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc ls --versions --recursive backup/haiker-uploads

# Restore a specific version
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc cp --version-id <VERSION_ID> backup/haiker-uploads/file.pdf source/haiker-uploads/file.pdf
```

---

## Disaster Recovery

This section covers full recovery when both PostgreSQL and MinIO data are lost.

### Full Disaster Recovery Procedure

**Phase 1: Infrastructure**

1. Provision new infrastructure (if hardware failure)
2. Deploy the backup Docker Compose stack:

```bash
cd deploy/backup
cp env.backup.example .env.backup
# Fill in production values from secrets manager
docker compose -f docker-compose.backup.yml up -d postgres minio-backup
```

**Phase 2: PostgreSQL Recovery**

3. Restore PostgreSQL from the latest backup:

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  /scripts/restore-postgres.sh \
  --target-time "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --yes
```

4. Verify database health:

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  psql -U haiker -c "SELECT count(*) FROM pg_stat_user_tables;"
```

**Phase 3: MinIO Recovery**

5. Restore MinIO buckets from the backup instance:

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc mirror --preserve --overwrite backup/haiker-artifacts source/haiker-artifacts

docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc mirror --preserve --overwrite backup/haiker-uploads source/haiker-uploads
```

6. Verify object counts:

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  /scripts/verify-minio-backup.sh
```

**Phase 4: Application Recovery**

7. Start the full application stack:

```bash
docker compose up -d
```

8. Run smoke tests:

```bash
scripts/smoke-test.sh
```

9. Verify monitoring is green:

```bash
scripts/backup-monitor.sh
```

### Recovery Time Expectations

| Component | Expected Recovery Time |
|-----------|----------------------|
| PostgreSQL (latest backup) | 5-30 minutes (depends on database size and WAL volume) |
| MinIO (full bucket restore) | 10-60 minutes (depends on object count and total size) |
| Full stack verification | 5-10 minutes |

---

## Alert Response

Each alert rule maps to a specific condition and remediation procedure. Alert rules are defined in `deploy/backup/monitoring/alert-rules.yml`.

### backup_age_critical

**Condition:** WAL archive age exceeds 15 minutes (RPO threshold).

**Severity:** Critical

**Remediation:**
1. Check that the PostgreSQL WAL archiving process is running:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
     psql -U haiker -c "SELECT * FROM pg_stat_archiver;"
   ```
2. Check for WAL archive failures:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
     psql -U haiker -c "SELECT last_failed_wal, last_failed_time FROM pg_stat_archiver;"
   ```
3. Verify the WAL archive volume has free space:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
     df -h /var/lib/postgresql/wal_archive
   ```
4. If the archive disk is full, run retention cleanup:
   ```bash
   scripts/backup-retention.sh
   ```
5. Restart the PostgreSQL container if archiving is stuck:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml restart postgres
   ```

### backup_age_warning

**Condition:** Base backup age exceeds 24 hours.

**Severity:** Warning

**Remediation:**
1. Check if the backup scheduler is running:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml ps backup-scheduler
   ```
2. Check scheduler logs:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml logs --tail=50 backup-scheduler
   ```
3. Trigger a manual backup:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     /scripts/backup-postgres.sh
   ```
4. If the manual backup fails, check connectivity to PostgreSQL:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     pg_isready -h postgres -U backup_user
   ```

### backup_failure

**Condition:** Backup verification script returned a non-zero exit code.

**Severity:** Critical

**Remediation:**
1. Run the verification script manually and inspect output:
   ```bash
   scripts/verify-backup.sh
   ```
2. Check for GPG decryption failures (indicates key mismatch or corrupt backup):
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-verify \
     gpg --batch --yes --decrypt --passphrase "${BACKUP_ENCRYPTION_KEY}" \
     "$(find /backups -name '*.tar.gz.gpg' | sort -r | head -1)" | tar -tzf - > /dev/null
   ```
3. If the latest backup is corrupt, trigger a new backup immediately:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     /scripts/backup-postgres.sh
   ```
4. If MinIO verification failed, check connectivity between source and backup instances:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
     mc admin info source
   ```

### capacity_warning

**Condition:** Backup storage utilization is at or above 80%.

**Severity:** Warning

**Remediation:**
1. Check current disk usage:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     df -h /backups
   ```
2. Run retention enforcement to remove old backups:
   ```bash
   scripts/backup-retention.sh
   ```
3. Review retention policy - consider shortening `BACKUP_RETENTION_DAYS` if growth is expected.
4. If volume is too small, extend the Docker volume or attach additional storage.

### capacity_critical

**Condition:** Backup storage utilization is at or above 90%.

**Severity:** Critical

**Remediation:**
1. Immediately run retention enforcement:
   ```bash
   scripts/backup-retention.sh
   ```
2. If retention cannot free sufficient space, manually remove oldest backups:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     find /backups -name "*.tar.gz.gpg" | sort | head -3 | xargs rm -v
   ```
3. Extend storage capacity (resize volume or add disk).
4. Reduce retention period temporarily if needed.

### retention_violation

**Condition:** Backups older than the retention policy still exist, or no current backup is found.

**Severity:** Warning

**Remediation:**
1. Run the retention enforcement script:
   ```bash
   scripts/backup-retention.sh
   ```
2. If no backups exist at all (critical variant), trigger a new backup immediately:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     /scripts/backup-postgres.sh
   ```
3. Inspect retention script logs for errors:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     cat /var/log/backup-retention.log
   ```

---

## Troubleshooting

### GPG Decryption Fails

**Symptoms:** Restore script fails with "gpg: decryption failed: Bad session key"

**Cause:** Wrong `BACKUP_ENCRYPTION_KEY` or corrupt backup file.

**Resolution:**
1. Verify the encryption key matches the one used during backup.
2. Check that the `.env.backup` file has the correct `BACKUP_ENCRYPTION_KEY`.
3. If the key is lost, the backup is unrecoverable. Trigger a new backup with the correct key.

### pg_basebackup Connection Refused

**Symptoms:** Backup script fails with "could not connect to server"

**Cause:** PostgreSQL is not accepting replication connections from the backup user.

**Resolution:**
1. Verify the `pg_hba_backup.conf` allows the backup user replication access.
2. Check that the backup user exists with REPLICATION privilege:
   ```sql
   SELECT rolname, rolreplication FROM pg_roles WHERE rolname = 'backup_user';
   ```
3. Verify the PostgreSQL `max_wal_senders` is not exhausted:
   ```sql
   SELECT count(*) FROM pg_stat_replication;
   SHOW max_wal_senders;
   ```

### WAL Archive Disk Full

**Symptoms:** PostgreSQL logs "could not archive WAL segment" errors.

**Cause:** WAL archive volume is full.

**Resolution:**
1. Run retention cleanup:
   ```bash
   scripts/backup-retention.sh
   ```
2. Check if there is a backlog of WAL files in `pg_wal`:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
     ls /var/lib/postgresql/data/pg_wal | wc -l
   ```
3. If the backlog is large, consider reducing `wal_keep_size` in PostgreSQL config.

### MinIO Mirror Fails

**Symptoms:** `backup-minio.sh` reports "ERROR: Failed to mirror bucket"

**Cause:** Network connectivity or credential issues between source and backup MinIO.

**Resolution:**
1. Verify aliases are configured:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
     mc alias list
   ```
2. Test connectivity:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
     mc admin info source
   docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
     mc admin info backup
   ```
3. Verify credentials in `.env.backup` match those configured on the MinIO instances.

### Restore Script Hangs at "Waiting for recovery"

**Symptoms:** `restore-postgres.sh` reaches the recovery wait phase but never completes.

**Cause:** Missing WAL segments between the base backup and the target time.

**Resolution:**
1. Check PostgreSQL logs for recovery errors:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml logs postgres | tail -50
   ```
2. Verify all required WAL segments exist in the archive:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
     ls /var/lib/postgresql/wal_archive/
   ```
3. If WAL segments are missing, choose a more recent base backup or a target time closer to the backup timestamp.

### Backup Container Keeps Restarting

**Symptoms:** `backup-scheduler` or `backup-verify` container is in a restart loop.

**Resolution:**
1. Check container logs:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml logs --tail=100 backup-scheduler
   ```
2. Verify required environment variables are set (especially `BACKUP_ENCRYPTION_KEY`).
3. Check that the health check script exists and is executable:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     ls -la /scripts/healthcheck.sh
   ```

---

## Testing Backup Restoration

### Quarterly Drill Procedure

A disaster recovery drill should be performed every quarter to verify:
- Backups can be successfully restored
- Recovery procedures are current and accurate
- Team members are familiar with the process

**Drill Steps:**

1. **Preparation (day before):**
   - Notify stakeholders of the planned drill
   - Ensure you have access to all required credentials
   - Spin up an isolated test environment (separate Docker network)

2. **Execute PostgreSQL PITR:**
   ```bash
   # In the test environment
   docker compose -f deploy/backup/docker-compose.backup.yml \
     -p backup-drill \
     up -d postgres

   # Restore to a random point within the last 24 hours
   TARGET=$(date -u -d "12 hours ago" +%Y-%m-%dT%H:%M:%SZ)
   docker compose -f deploy/backup/docker-compose.backup.yml \
     -p backup-drill \
     exec postgres /scripts/restore-postgres.sh --target-time "${TARGET}" --yes
   ```

3. **Execute MinIO restore:**
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml \
     -p backup-drill \
     exec minio-backup-scheduler \
     mc mirror --preserve backup/haiker-artifacts drill/haiker-artifacts
   ```

4. **Validate data integrity:**
   - Connect to the restored PostgreSQL and run sample queries
   - Verify object counts in the restored MinIO buckets
   - Run the application smoke tests against the restored data

5. **Document results:**
   - Record the time taken for each recovery step
   - Note any issues encountered
   - Update this runbook if procedures have changed

6. **Cleanup:**
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml -p backup-drill down -v
   ```

---

## Access and Credentials Management

### Credential Inventory

| Credential | Purpose | Storage |
|------------|---------|---------|
| `BACKUP_POSTGRES_USER` / `BACKUP_POSTGRES_PASSWORD` | PostgreSQL replication access | Secrets manager |
| `BACKUP_ENCRYPTION_KEY` | GPG encryption/decryption of PostgreSQL backups | Secrets manager |
| `BACKUP_MINIO_ROOT_USER` / `BACKUP_MINIO_ROOT_PASSWORD` | Backup MinIO instance admin | Secrets manager |
| `MINIO_SOURCE_ACCESS_KEY` / `MINIO_SOURCE_SECRET_KEY` | Read access to source MinIO | Secrets manager |
| `MINIO_BACKUP_ENCRYPTION_KEY` | Server-side encryption for MinIO backup objects | Secrets manager |

### Credential Isolation

- Backup credentials are **independent** from application credentials
- The backup PostgreSQL user has `REPLICATION` privilege only (no DML access)
- The MinIO source access key is a service account with read-only bucket access
- Loss of backup credentials does not compromise the application
- Loss of application credentials does not compromise backups

### Credential Rotation Procedure

1. Generate a new credential value
2. Update the secrets manager
3. Update the `.env.backup` file on the deployment host
4. Restart the affected backup services:
   ```bash
   # For PostgreSQL backup credentials
   docker compose -f deploy/backup/docker-compose.backup.yml restart backup-scheduler backup-verify

   # For MinIO backup credentials
   docker compose -f deploy/backup/docker-compose.backup.yml restart minio-backup minio-backup-scheduler
   ```
5. Trigger a manual backup to verify the new credentials work:
   ```bash
   docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
     /scripts/backup-postgres.sh
   ```
6. Verify the backup monitor is green:
   ```bash
   scripts/backup-monitor.sh
   ```

### GPG Key Rotation

> **WARNING:** Rotating the `BACKUP_ENCRYPTION_KEY` means old backups encrypted with the previous key require the old key to restore. Keep old keys until all backups encrypted with them have expired per the retention policy.

1. Store the old key securely (label with the rotation date)
2. Generate a new key and update the secrets manager
3. Update `.env.backup` with the new `BACKUP_ENCRYPTION_KEY`
4. Restart backup services
5. After `BACKUP_RETENTION_DAYS` (7 days), the old key is no longer needed for operational restores

---

## Contact and Ownership

| Role | Responsibility |
|------|---------------|
| Platform team | Backup infrastructure, scripts, and monitoring |
| On-call engineer | Alert response and initial triage |
| Database team | PostgreSQL-specific recovery decisions |

### Escalation Path

1. **L1 (On-call):** Respond to alerts, run documented remediation steps
2. **L2 (Platform):** Investigate persistent issues, manual recovery
3. **L3 (Database team):** Complex PITR scenarios, data integrity decisions

### Communication Channels

- Alerts: Configured via `ALERT_WEBHOOK_URL` (Slack/PagerDuty)
- Alert channel: Configurable via `ALERT_CHANNEL` (default: `#backup-alerts`)
- Incident management: Follow standard incident process
