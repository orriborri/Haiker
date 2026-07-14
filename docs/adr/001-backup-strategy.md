# ADR-001: Backup Strategy

**Status:** Accepted  
**Date:** 2024-01-15  
**Decision Makers:** Platform / Infrastructure team

---

## Context

Haiker runs a PostgreSQL database (PostGIS) and MinIO object storage as its primary data stores. A data loss event (hardware failure, accidental deletion, software bug, or security breach) without adequate backups would result in permanent loss of user data and service continuity.

### Requirements

- **RPO (Recovery Point Objective):** No more than 15 minutes of data loss for PostgreSQL. No more than 24 hours for object storage.
- **RTO (Recovery Time Objective):** Full service recovery within 1 hour for PostgreSQL, 2 hours for a complete disaster (both stores).
- **Encryption:** All backup data must be encrypted at rest to protect against unauthorized access to backup volumes.
- **Credential isolation:** Backup processes must use dedicated credentials that are separate from application credentials to limit blast radius.
- **Auditability:** Backup health must be continuously monitored with automated alerts on failure.
- **Retention:** Backups must be retained long enough for operational recovery but pruned automatically to manage storage costs.

---

## Decision

We adopt the following backup strategy:

### PostgreSQL: WAL Archiving + Base Backups

- **Continuous WAL archiving** provides point-in-time recovery (PITR) capability with a 15-minute RPO.
- **Daily base backups** via `pg_basebackup` provide the foundation for PITR. Base backups are compressed and encrypted with GPG (AES256 symmetric cipher) before storage.
- **Retention:** 7 days for both base backups and WAL archives. The most recent backup is never deleted regardless of age.

### MinIO: Versioning + Bucket Mirroring

- **Object versioning** is enabled on the backup MinIO instance to allow point-in-time object recovery.
- **Daily bucket mirroring** via `mc mirror` replicates all objects from the source MinIO instance to an independent backup MinIO instance.
- **Server-side encryption (SSE)** protects backup objects at rest using a dedicated encryption key.
- **Retention:** 30 days for old object versions.

### Independent Credentials

- A dedicated `backup_user` with only `REPLICATION` privilege is used for PostgreSQL backups (no DML access).
- A read-only MinIO service account (`backup-svc`) is used to read from the source instance.
- The backup MinIO instance has its own admin credentials, separate from the production MinIO instance.
- Compromise of application credentials does not grant access to backups, and vice versa.

### Encryption at Rest

- PostgreSQL base backups are encrypted with GPG using AES256 symmetric encryption before being written to disk.
- MinIO backup objects use server-side encryption with a dedicated key managed by the backup MinIO instance.

### Monitoring and Alerting

- A unified monitoring script (`scripts/backup-monitor.sh`) checks backup health every 5 minutes.
- Alert conditions are defined declaratively in `deploy/backup/monitoring/alert-rules.yml`.
- Webhook-based alerts (Slack, PagerDuty) are sent when thresholds are breached.
- Key thresholds: WAL age > 15 min (critical), base backup age > 24h (warning), storage > 80% (warning) / > 90% (critical).

---

## Alternatives Considered

### 1. Managed Backup Service (e.g., cloud-provider snapshots)

**Pros:** Fully managed, no operational overhead, automatic encryption.

**Rejected because:**
- Haiker is deployed on self-managed infrastructure (Docker Compose), not a managed cloud database.
- Introduces cloud provider lock-in.
- Less control over RPO and retention granularity.
- Does not support PITR to arbitrary timestamps.

### 2. pg_dump Only (Logical Backups)

**Pros:** Simple, portable, human-readable output.

**Rejected because:**
- Cannot achieve 15-minute RPO (pg_dump is a point-in-time snapshot, not continuous).
- Restore time for large databases is significantly longer than physical restore.
- No PITR capability (can only restore to the exact dump time).
- Higher storage requirements for frequent dumps.

### 3. No Encryption (Rely on Volume-Level Encryption)

**Pros:** Simpler backup scripts, no key management overhead.

**Rejected because:**
- Docker volumes may not have encryption enabled by default on all deployment targets.
- Defense in depth: application-level encryption protects against scenarios where volume access is compromised but encryption keys are not.
- Backup files may be copied to secondary locations (network transfers, offsite storage) where volume encryption does not apply.

### 4. Single Credential Set for App and Backup

**Pros:** Simpler credential management, fewer secrets to rotate.

**Rejected because:**
- A compromised application credential would also compromise backup integrity (attacker could delete or corrupt backups).
- Backup credentials need only narrow permissions (REPLICATION for PG, read-only for MinIO source).
- Principle of least privilege reduces blast radius.

---

## Consequences

### Positive

- **15-minute RPO** for PostgreSQL ensures minimal data loss in disaster scenarios.
- **PITR capability** allows precise recovery to any point within the retention window (7 days).
- **Independent credential model** limits the impact of any single credential compromise.
- **Encryption at rest** protects backup data even if storage volumes are exposed.
- **Automated monitoring** provides early warning of backup failures before they become recovery failures.
- **Self-contained infrastructure** (Docker Compose) allows the backup system to be deployed alongside the application without external dependencies.

### Negative

- **Operational complexity:** Multiple scripts, credentials, and volumes to manage. Mitigated by comprehensive documentation (this ADR, runbook, quick reference).
- **Key management responsibility:** Loss of the GPG encryption key means loss of existing encrypted backups. Mitigated by storing keys in a secrets manager and documenting rotation procedures.
- **Storage overhead:** Maintaining 7 days of base backups + WAL archives + 30 days of MinIO versions requires dedicated storage. Mitigated by automated retention enforcement.
- **Single-site limitation:** Current architecture stores backups on the same host. For geographic redundancy, an offsite replication strategy would need to be added as a future enhancement.

### Risks

- If WAL archiving silently fails, the RPO guarantee is broken. Mitigated by the `backup_age_critical` alert (WAL age > 15 min).
- If the encryption key is rotated incorrectly, old backups become inaccessible. Mitigated by the documented rotation procedure (keep old keys until retention expires).
