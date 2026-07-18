# Secret Rotation Runbook

> **Owner:** Platform / Infrastructure team  
> **Last updated:** 2025-01-15  
> **Review cadence:** Quarterly (aligned with disaster recovery drill)

---

## Table of Contents

1. [Overview](#overview)
2. [Credential Inventory](#credential-inventory)
3. [Rotation Schedule](#rotation-schedule)
4. [Pre-Rotation Checklist](#pre-rotation-checklist)
5. [PostgreSQL Credentials](#postgresql-credentials)
6. [MinIO Credentials](#minio-credentials)
7. [OIDC Provider Credentials](#oidc-provider-credentials)
8. [Backup Encryption Keys](#backup-encryption-keys)
9. [Session Secrets](#session-secrets)
10. [Emergency Rotation](#emergency-rotation)
11. [Post-Rotation Verification](#post-rotation-verification)
12. [Contact and Escalation](#contact-and-escalation)

---

## Overview

This runbook documents credential rotation procedures for all secrets used by the Haiker platform. It covers both scheduled rotations and emergency rotations in the event of compromise.

### Key References

| Document | Purpose |
|----------|---------|
| [Backup Operations](backup-operations.md) | Backup credential details and rotation |
| [Incident Response](incident-response.md) | Incident process for credential compromise |
| `deploy/backup/env.backup.example` | Full credential inventory reference |
| `deploy/docker-compose.staging.yml` | Service dependency map |

### Impact Summary

| Credential Type | Services Affected | Expected Downtime |
|----------------|-------------------|-------------------|
| PostgreSQL (application) | API, Worker | Brief (service restart) |
| PostgreSQL (backup) | Backup scheduler, verifier | Brief (container restart) |
| MinIO (application) | API, Worker | Brief (service restart) |
| MinIO (backup) | Backup scheduler | Brief (container restart) |
| OIDC provider | API (authentication) | Brief (service restart) |
| Backup encryption key | Backup scheduler, verifier | None (new backups use new key) |
| Session secret | API | Session invalidation (users must re-login) |

---

## Credential Inventory

### Application Credentials

| Credential | Environment Variable | Used By | Storage |
|------------|---------------------|---------|---------|
| PostgreSQL username | `POSTGRES_USER` | API, Worker | `.env` / Secrets manager |
| PostgreSQL password | `POSTGRES_PASSWORD` | API, Worker | `.env` / Secrets manager |
| MinIO access key | `STORAGE_ACCESS_KEY_ID` | API, Worker | `.env` / Secrets manager |
| MinIO secret key | `STORAGE_SECRET_ACCESS_KEY` | API, Worker | `.env` / Secrets manager |
| OIDC client ID | `OIDC_CLIENT_ID` | API | `.env` / Secrets manager |
| OIDC client secret | `OIDC_CLIENT_SECRET` | API | `.env` / Secrets manager |
| Session signing secret | `SESSION_SECRET` | API | `.env` / Secrets manager |

### Backup Credentials

| Credential | Environment Variable | Used By | Storage |
|------------|---------------------|---------|---------|
| Backup PostgreSQL user | `BACKUP_POSTGRES_USER` | Backup scheduler | `.env.backup` / Secrets manager |
| Backup PostgreSQL password | `BACKUP_POSTGRES_PASSWORD` | Backup scheduler | `.env.backup` / Secrets manager |
| Backup encryption key | `BACKUP_ENCRYPTION_KEY` | Backup scheduler, verifier | `.env.backup` / Secrets manager |
| Backup MinIO root user | `BACKUP_MINIO_ROOT_USER` | Backup MinIO | `.env.backup` / Secrets manager |
| Backup MinIO root password | `BACKUP_MINIO_ROOT_PASSWORD` | Backup MinIO | `.env.backup` / Secrets manager |
| Source MinIO access key | `MINIO_SOURCE_ACCESS_KEY` | Backup scheduler | `.env.backup` / Secrets manager |
| Source MinIO secret key | `MINIO_SOURCE_SECRET_KEY` | Backup scheduler | `.env.backup` / Secrets manager |
| MinIO backup encryption key | `MINIO_BACKUP_ENCRYPTION_KEY` | Backup scheduler | `.env.backup` / Secrets manager |

### Infrastructure Credentials

| Credential | Environment Variable | Used By | Storage |
|------------|---------------------|---------|---------|
| Alert webhook URL | `ALERT_WEBHOOK_URL` | Backup monitor | `.env.backup` / Secrets manager |
| MinIO root user (source) | `MINIO_ROOT_USER` | MinIO server | `.env` / Secrets manager |
| MinIO root password (source) | `MINIO_ROOT_PASSWORD` | MinIO server | `.env` / Secrets manager |

---

## Rotation Schedule

| Credential Type | Rotation Cadence | Last Rotated | Next Due |
|----------------|-----------------|--------------|----------|
| PostgreSQL passwords | Every 90 days | [Record here] | [Record here] |
| MinIO access keys | Every 90 days | [Record here] | [Record here] |
| OIDC client secret | Every 180 days | [Record here] | [Record here] |
| Backup encryption key | Every 365 days | [Record here] | [Record here] |
| Session secret | Every 90 days | [Record here] | [Record here] |
| Alert webhook URL | On token expiry | [Record here] | [Record here] |

---

## Pre-Rotation Checklist

Before rotating any credential:

- [ ] Identify all services and scripts using the credential
- [ ] Verify you have access to the secrets manager
- [ ] Verify you can restart affected services
- [ ] Ensure a recent backup exists (`scripts/backup-monitor.sh`)
- [ ] Notify the team of the planned rotation window
- [ ] Have rollback plan ready (old credential value saved securely)
- [ ] Schedule rotation during low-traffic period (if possible)
- [ ] Confirm no active deployments or migrations in progress

---

## PostgreSQL Credentials

### Application Database Credentials

**Services affected:** `haiker-api`, `haiker-worker`

**Step 1: Generate a new password**

```bash
# Generate a strong random password (32 characters)
NEW_PG_PASSWORD=$(openssl rand -base64 32 | tr -d '=/+' | head -c 32)
echo "New password: ${NEW_PG_PASSWORD}"
```

**Step 2: Update PostgreSQL user password**

```bash
docker compose exec postgres psql -U haiker -d haiker -c "
  ALTER USER haiker WITH PASSWORD '${NEW_PG_PASSWORD}';
"
```

**Step 3: Update secrets manager and environment**

```bash
# Update secrets manager (example with a generic approach)
# Replace with your actual secrets manager CLI command
# aws secretsmanager update-secret --secret-id haiker/postgres-password --secret-string "${NEW_PG_PASSWORD}"

# Update the .env file
sed -i "s/^POSTGRES_PASSWORD=.*/POSTGRES_PASSWORD=${NEW_PG_PASSWORD}/" .env
```

**Step 4: Restart application services**

```bash
docker compose restart api worker
```

**Step 5: Verify connectivity**

```bash
# Check health endpoint
curl -sf http://localhost:3000/health | jq .

# Run smoke tests
scripts/smoke-test.sh
```

### Backup PostgreSQL Credentials

**Services affected:** `backup-scheduler`, `backup-verify`

**Step 1: Generate new password**

```bash
NEW_BACKUP_PG_PASSWORD=$(openssl rand -base64 32 | tr -d '=/+' | head -c 32)
echo "New password: ${NEW_BACKUP_PG_PASSWORD}"
```

**Step 2: Update the backup user password in PostgreSQL**

```bash
docker compose exec postgres psql -U haiker -d haiker -c "
  ALTER USER backup_user WITH PASSWORD '${NEW_BACKUP_PG_PASSWORD}';
"
```

**Step 3: Update backup environment**

```bash
sed -i "s/^BACKUP_POSTGRES_PASSWORD=.*/BACKUP_POSTGRES_PASSWORD=${NEW_BACKUP_PG_PASSWORD}/" deploy/backup/.env.backup
```

**Step 4: Restart backup services**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml restart backup-scheduler backup-verify
```

**Step 5: Verify**

```bash
# Trigger a test backup
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  /scripts/backup-postgres.sh

# Verify backup health
scripts/backup-monitor.sh
```

---

## MinIO Credentials

### Application MinIO Credentials

**Services affected:** `haiker-api`, `haiker-worker`

**Step 1: Generate new credentials**

```bash
NEW_MINIO_ACCESS_KEY=$(openssl rand -hex 16)
NEW_MINIO_SECRET_KEY=$(openssl rand -base64 32 | tr -d '=/+' | head -c 32)
echo "Access key: ${NEW_MINIO_ACCESS_KEY}"
echo "Secret key: ${NEW_MINIO_SECRET_KEY}"
```

**Step 2: Create new service account in MinIO**

```bash
docker compose exec minio mc admin user svcacct add source haiker-app \
  --access-key "${NEW_MINIO_ACCESS_KEY}" \
  --secret-key "${NEW_MINIO_SECRET_KEY}"
```

**Step 3: Update environment**

```bash
sed -i "s/^STORAGE_ACCESS_KEY_ID=.*/STORAGE_ACCESS_KEY_ID=${NEW_MINIO_ACCESS_KEY}/" .env
sed -i "s/^STORAGE_SECRET_ACCESS_KEY=.*/STORAGE_SECRET_ACCESS_KEY=${NEW_MINIO_SECRET_KEY}/" .env
```

**Step 4: Restart application**

```bash
docker compose restart api worker
```

**Step 5: Verify**

```bash
scripts/smoke-test.sh
curl -sf http://localhost:3000/health | jq .
```

**Step 6: Remove old service account (after verification)**

```bash
docker compose exec minio mc admin user svcacct rm source <OLD_ACCESS_KEY>
```

### Backup MinIO Credentials

**Services affected:** `minio-backup`, `minio-backup-scheduler`

**Step 1: Generate new credentials**

```bash
NEW_BACKUP_MINIO_USER=$(openssl rand -hex 8)
NEW_BACKUP_MINIO_PASSWORD=$(openssl rand -base64 32 | tr -d '=/+' | head -c 32)
```

**Step 2: Update environment**

```bash
sed -i "s/^BACKUP_MINIO_ROOT_USER=.*/BACKUP_MINIO_ROOT_USER=${NEW_BACKUP_MINIO_USER}/" deploy/backup/.env.backup
sed -i "s/^BACKUP_MINIO_ROOT_PASSWORD=.*/BACKUP_MINIO_ROOT_PASSWORD=${NEW_BACKUP_MINIO_PASSWORD}/" deploy/backup/.env.backup
```

**Step 3: Restart backup MinIO (requires full restart to pick up new root credentials)**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml down minio-backup minio-backup-scheduler
docker compose -f deploy/backup/docker-compose.backup.yml up -d minio-backup minio-backup-scheduler
```

**Step 4: Reconfigure mc aliases**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc alias set backup http://minio-backup:9000 "${NEW_BACKUP_MINIO_USER}" "${NEW_BACKUP_MINIO_PASSWORD}"
```

**Step 5: Verify**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc admin info backup

scripts/backup-monitor.sh
```

---

## OIDC Provider Credentials

**Services affected:** `haiker-api` (authentication)

**Impact:** Rotating the OIDC client secret invalidates the current client configuration. Users will need to re-authenticate.

**Step 1: Generate new client secret in the OIDC provider**

Access your OIDC provider admin console (Keycloak, Auth0, etc.) and regenerate the client secret for the Haiker application.

**Step 2: Update environment**

```bash
sed -i "s/^OIDC_CLIENT_SECRET=.*/OIDC_CLIENT_SECRET=<new-secret-from-provider>/" .env
```

**Step 3: Restart API**

```bash
docker compose restart api
```

**Step 4: Verify authentication flow**

```bash
# Check health endpoint (no auth required)
curl -sf http://localhost:3000/health | jq .

# Verify OIDC discovery endpoint is reachable
curl -sf "${OIDC_ISSUER_URL}/.well-known/openid-configuration" | jq .type

# Test a login flow (manual verification)
# Open the application in a browser and verify login works
```

**Step 5: Communicate to users if sessions were invalidated**

If the rotation invalidated existing sessions, notify users that they need to log in again.

---

## Backup Encryption Keys

### GPG Encryption Key (PostgreSQL Backups)

**Services affected:** `backup-scheduler`, `backup-verify`

> **WARNING:** Rotating the `BACKUP_ENCRYPTION_KEY` means old backups encrypted with the previous key require the old key to restore. Keep the old key until all backups encrypted with it have expired per the retention policy (`BACKUP_RETENTION_DAYS`, default 7 days).

**Step 1: Save the old key**

```bash
# Record the old key with the rotation date
echo "OLD KEY (rotated $(date -u +%Y-%m-%d)): $(grep BACKUP_ENCRYPTION_KEY deploy/backup/.env.backup | cut -d= -f2)" >> /secure/location/old-backup-keys.txt
```

**Step 2: Generate a new encryption key**

```bash
NEW_ENCRYPTION_KEY=$(openssl rand -base64 48 | tr -d '=/+' | head -c 48)
echo "New key: ${NEW_ENCRYPTION_KEY}"
```

**Step 3: Update environment**

```bash
sed -i "s/^BACKUP_ENCRYPTION_KEY=.*/BACKUP_ENCRYPTION_KEY=${NEW_ENCRYPTION_KEY}/" deploy/backup/.env.backup
```

**Step 4: Restart backup services**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml restart backup-scheduler backup-verify
```

**Step 5: Trigger a test backup with the new key**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  /scripts/backup-postgres.sh
```

**Step 6: Verify the new backup can be decrypted**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-verify \
  /scripts/verify-backup.sh
```

**Step 7: Record retention timeline**

The old key is needed until: `$(date -u -d "+${BACKUP_RETENTION_DAYS} days" +%Y-%m-%d)`

### MinIO Backup Encryption Key

**Step 1: Generate new key (minimum 32 characters)**

```bash
NEW_MINIO_ENC_KEY=$(openssl rand -base64 48 | tr -d '=/+' | head -c 32)
```

**Step 2: Update environment**

```bash
sed -i "s/^MINIO_BACKUP_ENCRYPTION_KEY=.*/MINIO_BACKUP_ENCRYPTION_KEY=${NEW_MINIO_ENC_KEY}/" deploy/backup/.env.backup
```

**Step 3: Restart backup scheduler**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml restart minio-backup-scheduler
```

**Step 4: Verify**

```bash
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  /scripts/backup-minio.sh

scripts/backup-monitor.sh
```

---

## Session Secrets

**Services affected:** `haiker-api`

**Impact:** Rotating the session secret invalidates ALL existing user sessions. All users will need to log in again.

**Step 1: Generate new session secret**

```bash
NEW_SESSION_SECRET=$(openssl rand -base64 64 | tr -d '=/+' | head -c 64)
echo "New secret: ${NEW_SESSION_SECRET}"
```

**Step 2: Update environment**

```bash
sed -i "s/^SESSION_SECRET=.*/SESSION_SECRET=${NEW_SESSION_SECRET}/" .env
```

**Step 3: Restart API**

```bash
docker compose restart api
```

**Step 4: Verify**

```bash
curl -sf http://localhost:3000/health | jq .
scripts/smoke-test.sh
```

**Step 5: Communicate session invalidation**

Notify users that they will need to log in again. This is expected behavior during a session secret rotation.

---

## Emergency Rotation

Use this procedure when a credential has been compromised or is suspected to be compromised.

### Immediate Actions (Do These First)

1. **Classify the incident** (see [incident-response.md](incident-response.md))
2. **Determine scope:** Which credential(s) are compromised?
3. **Assess impact:** What can an attacker do with this credential?
4. **Rotate immediately:** Do not wait for a maintenance window

### Emergency Rotation Procedure

```bash
# 1. Identify the compromised credential
# 2. Generate replacement immediately
NEW_VALUE=$(openssl rand -base64 32 | tr -d '=/+' | head -c 32)

# 3. Rotate at the source FIRST (e.g., change password in PostgreSQL)
# This immediately invalidates the compromised credential

# 4. Update all services that use it
# 5. Restart affected services
# 6. Verify functionality
```

### Per-Credential Emergency Steps

| Compromised Credential | Immediate Action | Blast Radius |
|-----------------------|------------------|--------------|
| `POSTGRES_PASSWORD` | Change in PostgreSQL immediately; attacker has full DB access | All data at risk |
| `STORAGE_ACCESS_KEY_ID` / `STORAGE_SECRET_ACCESS_KEY` | Revoke the access key in MinIO | File access at risk |
| `OIDC_CLIENT_SECRET` | Regenerate in OIDC provider | Authentication flow |
| `BACKUP_ENCRYPTION_KEY` | Rotate key; old backups still protected by old key | Future backups only |
| `SESSION_SECRET` | Rotate; all sessions invalidated | User sessions |
| `BACKUP_POSTGRES_PASSWORD` | Change in PostgreSQL | Backup access only |
| `ALERT_WEBHOOK_URL` | Regenerate webhook token | Alert delivery |

### Post-Emergency Actions

1. Investigate how the credential was compromised
2. Check audit logs for unauthorized access during the exposure window
3. Determine if any data was accessed or modified
4. Follow post-incident review process (see [incident-response.md](incident-response.md))

---

## Post-Rotation Verification

After any credential rotation (scheduled or emergency), perform these checks:

### Service Health

```bash
# API and Worker health
curl -sf http://localhost:3000/health | jq .
docker compose ps

# Smoke tests
scripts/smoke-test.sh
```

### Database Connectivity

```bash
# Verify application can connect
docker compose logs --tail=10 api | grep -i "database\|postgres\|connection"

# Verify backup can connect
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  pg_isready -h postgres -U backup_user
```

### Object Storage

```bash
# Verify application can access MinIO
docker compose logs --tail=10 api | grep -i "minio\|storage\|s3"

# Verify backup can access source MinIO
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc admin info source
```

### Backup System

```bash
# Full backup health check
scripts/backup-monitor.sh

# Trigger test backup
docker compose -f deploy/backup/docker-compose.backup.yml exec backup-scheduler \
  /scripts/backup-postgres.sh
```

### Authentication

```bash
# Verify OIDC is working (check API logs for auth errors)
docker compose logs --tail=20 api | grep -i "oidc\|auth\|token"
```

### Record the Rotation

Update the rotation schedule table (in this document or in your secrets manager):
- Record the rotation date
- Calculate the next rotation due date
- Archive the old credential securely (if needed for backup decryption)

---

## Contact and Escalation

| Role | Responsibility |
|------|---------------|
| Platform team | Credential management, rotation tooling |
| On-call engineer | Emergency rotations, service restarts |
| Security team | Compromise assessment, forensics |

### Escalation Path

1. **L1 (On-call):** Execute documented rotation procedures
2. **L2 (Platform):** Complex rotations, infrastructure credential changes
3. **L3 (Security):** Credential compromise investigation, scope assessment

### When to Escalate Immediately

- Credential compromise confirmed or suspected (escalate to Security)
- Rotation caused unexpected service failure
- Multiple credentials compromised simultaneously
- Unable to determine which services use a credential
