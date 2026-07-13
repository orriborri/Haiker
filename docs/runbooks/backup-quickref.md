# Backup Quick Reference

> One-page operational reference. For full procedures, see [backup-operations.md](./backup-operations.md).

---

## Check Backup Status

```bash
# Full health check (PostgreSQL + MinIO)
scripts/backup-monitor.sh

# PostgreSQL backup verification only
scripts/verify-backup.sh

# MinIO backup verification only
scripts/verify-minio-backup.sh
```

---

## Trigger Manual Backup

```bash
# PostgreSQL base backup
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  /scripts/backup-postgres.sh

# MinIO bucket mirror
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  /scripts/backup-minio.sh
```

---

## Restore PostgreSQL to Point in Time

```bash
# 1. Stop the application
docker compose stop app worker

# 2. Restore to a specific timestamp
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  /scripts/restore-postgres.sh \
  --target-time "2024-01-15T10:30:00+00:00"

# 3. Restart the application
docker compose start app worker
```

For automated (no prompt):

```bash
/scripts/restore-postgres.sh --target-time "2024-01-15T10:30:00+00:00" --yes
```

---

## Restore MinIO Objects

```bash
# Restore a single file
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc cp backup/haiker-artifacts/path/to/file source/haiker-artifacts/path/to/file

# Restore an entire bucket
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc mirror --preserve --overwrite backup/haiker-artifacts source/haiker-artifacts
```

---

## Run Retention Cleanup

```bash
# Enforce retention policy (remove expired backups)
scripts/backup-retention.sh

# Dry run (show what would be deleted)
DRY_RUN=true scripts/backup-retention.sh
```

---

## Rotate Credentials

```bash
# 1. Update .env.backup with new credentials
# 2. Restart affected services
docker compose -f deploy/backup/docker-compose.backup.yml restart backup-scheduler backup-verify

# 3. Verify
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  /scripts/backup-postgres.sh
```

---

## Key Thresholds

| Metric | Warning | Critical |
|--------|---------|----------|
| WAL archive age | - | > 15 min |
| Base backup age | > 24 hours | - |
| Storage capacity | >= 80% | >= 90% |
| Backup verification | exit code != 0 | exit code == 2 |

---

## Key File Paths

| File | Purpose |
|------|---------|
| `deploy/backup/docker-compose.backup.yml` | Backup infrastructure stack |
| `deploy/backup/env.backup.example` | Environment variable template |
| `scripts/backup-postgres.sh` | PostgreSQL backup script |
| `scripts/restore-postgres.sh` | PostgreSQL PITR restore |
| `scripts/verify-backup.sh` | PostgreSQL backup verification |
| `scripts/backup-minio.sh` | MinIO bucket mirror |
| `scripts/verify-minio-backup.sh` | MinIO backup verification |
| `scripts/backup-monitor.sh` | Unified monitoring + alerts |
| `scripts/backup-retention.sh` | Retention enforcement |
| `deploy/backup/monitoring/alert-rules.yml` | Alert rule definitions |
