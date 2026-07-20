# Dependency Outage Runbook

> **Owner:** Platform / Infrastructure team  
> **Last updated:** 2025-01-15  
> **Review cadence:** Quarterly (aligned with disaster recovery drill)

---

## Table of Contents

1. [Overview](#overview)
2. [External Dependency Inventory](#external-dependency-inventory)
3. [PostgreSQL Outage](#postgresql-outage)
4. [MinIO / Object Storage Outage](#minio--object-storage-outage)
5. [OIDC Provider Outage](#oidc-provider-outage)
6. [Polar Integration Outage](#polar-integration-outage)
7. [Alert Webhook Outage](#alert-webhook-outage)
8. [Recovery Verification](#recovery-verification)
9. [Post-Outage Review](#post-outage-review)
10. [Contact and Escalation](#contact-and-escalation)

---

## Overview

This runbook documents detection, graceful degradation, and recovery procedures for each external dependency of the Haiker platform. Use this when a dependency becomes unavailable or exhibits degraded performance.

### Key References

| Document | Purpose |
|----------|---------|
| [Incident Response](incident-response.md) | Incident classification and communication |
| [Backup Operations](backup-operations.md) | PostgreSQL and MinIO backup/recovery |
| [Rollback](rollback.md) | Application rollback if deployment caused the outage |
| [Secret Rotation](secret-rotation.md) | If credential issues cause connectivity failures |

### Key Scripts

| Script | Purpose |
|--------|---------|
| `scripts/smoke-test.sh` | Full system health verification |
| `scripts/backup-monitor.sh` | Backup system health check |

---

## External Dependency Inventory

| Dependency | Port/Endpoint | Used By | Criticality |
|------------|---------------|---------|-------------|
| PostgreSQL (PostGIS) | `postgres:5432` | API, Worker | Critical (total outage without it) |
| MinIO (S3-compatible) | `minio:9000` | API, Worker | High (file operations fail) |
| OIDC Provider | External URL (configured via `OIDC_ISSUER_URL`) | API | High (no new logins) |
| Polar API | `https://www.polaraccesslink.com` | Worker (polar_integration) | Medium (sync stale) |
| Alert Webhook | Configured via `ALERT_WEBHOOK_URL` | Backup monitor | Low (alerts not delivered) |

### Dependency Health Check Commands

```bash
# PostgreSQL
docker compose exec postgres pg_isready -U haiker

# MinIO
curl -sf http://localhost:9000/minio/health/live

# OIDC Provider
curl -sf "${OIDC_ISSUER_URL}/.well-known/openid-configuration" | jq .issuer

# Polar API (basic connectivity)
curl -sf -o /dev/null -w "%{http_code}" https://www.polaraccesslink.com/v3

# Alert Webhook
curl -sf -o /dev/null -w "%{http_code}" -X POST "${ALERT_WEBHOOK_URL}" -d '{"text":"health check"}'
```

---

## PostgreSQL Outage

### Symptoms

- API returns 5xx errors on all database-dependent endpoints
- Health check at `/health` returns unhealthy database status
- Worker logs show connection pool errors
- `pg_isready` returns non-zero exit code

### Detection

```bash
# Check PostgreSQL readiness
docker compose exec postgres pg_isready -U haiker

# Check connection count and state
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT state, count(*) FROM pg_stat_activity GROUP BY state;
"

# Check API health endpoint
curl -sf http://localhost:3000/health | jq .

# Check for connection pool exhaustion in API logs
docker compose logs --tail=50 api | grep -i "pool\|connection\|timeout"
```

### Immediate Actions

**If PostgreSQL container is stopped/crashed:**

```bash
# Check container status
docker compose ps postgres

# Restart PostgreSQL
docker compose restart postgres

# Wait for it to be ready
docker compose exec postgres pg_isready -U haiker
# Retry a few times if not immediately ready
```

**If PostgreSQL is running but not accepting connections:**

```bash
# Check PostgreSQL logs for errors
docker compose logs --tail=100 postgres | grep -i "error\|fatal\|panic"

# Check disk space (common cause of PostgreSQL shutdown)
docker compose exec postgres df -h /var/lib/postgresql/data

# Check max_connections
docker compose exec postgres psql -U haiker -d haiker -c "
  SHOW max_connections;
  SELECT count(*) FROM pg_stat_activity;
"

# If connections are exhausted, terminate idle connections
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT pg_terminate_backend(pid)
  FROM pg_stat_activity
  WHERE state = 'idle'
  AND query_start < now() - interval '10 minutes'
  AND usename != 'haiker';
"
```

**If PostgreSQL disk is full:**

```bash
# Check what is using space
docker compose exec postgres du -sh /var/lib/postgresql/data/*

# If WAL files are accumulating
docker compose exec postgres ls /var/lib/postgresql/data/pg_wal | wc -l

# Force a checkpoint to allow WAL cleanup
docker compose exec postgres psql -U haiker -d haiker -c "CHECKPOINT;"
```

### Graceful Degradation

When PostgreSQL is unavailable:
- **API:** All endpoints requiring data return 503 Service Unavailable
- **Worker:** Job processing stops; jobs remain in queue for retry
- **What still works:** Nothing meaningful (PostgreSQL is critical)

### Recovery Steps

```bash
# 1. Identify and fix the root cause (see immediate actions above)

# 2. Verify PostgreSQL is accepting connections
docker compose exec postgres pg_isready -U haiker

# 3. Restart application services (to reset connection pools)
docker compose restart api worker

# 4. Verify application health
scripts/smoke-test.sh

# 5. Check for data integrity
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT schemaname, relname, n_live_tup
  FROM pg_stat_user_tables
  ORDER BY schemaname, relname;
"

# 6. Verify backup system reconnected
scripts/backup-monitor.sh
```

### Escalation

- If PostgreSQL will not start after restart: escalate to L2 (Platform)
- If data corruption suspected: escalate to L3 (Database team)
- If disk full and cannot be resolved: escalate to L2 (Platform)
- If connection pool exhaustion keeps recurring: escalate to L2 (Platform)

---

## MinIO / Object Storage Outage

### Symptoms

- File upload requests fail with errors
- File download requests return 500/503
- Export generation fails
- Import processing fails (cannot store artifacts)
- Health check shows storage as unhealthy

### Detection

```bash
# Check MinIO health
curl -sf http://localhost:9000/minio/health/live
echo "Exit code: $?"

# Check MinIO container status
docker compose ps minio

# Check MinIO logs
docker compose logs --tail=50 minio | grep -i "error\|fatal"

# Check disk space for MinIO data volume
docker compose exec minio df -h /data

# Test object access from application perspective
docker compose exec api curl -sf http://minio:9000/minio/health/live
```

### Immediate Actions

**If MinIO container is stopped/crashed:**

```bash
# Restart MinIO
docker compose restart minio

# Wait for health check to pass
sleep 5
curl -sf http://localhost:9000/minio/health/live
```

**If MinIO is running but returning errors:**

```bash
# Check MinIO admin info
docker compose exec minio mc admin info local

# Check disk usage
docker compose exec minio df -h /data

# If disk is full, identify large or temporary files
docker compose exec minio du -sh /data/*
```

**If credential mismatch (after rotation):**

```bash
# Verify credentials match between .env and MinIO
docker compose exec minio mc admin user list local

# See secret-rotation.md for credential verification procedures
```

### Graceful Degradation

When MinIO is unavailable:
- **What fails:** File uploads, file downloads, export generation, import artifact storage
- **What still works:** API endpoints not involving file operations, route editing, activity catalog browsing (if data is in PostgreSQL), user authentication
- **Worker behavior:** Import/export jobs will fail and be retried when MinIO returns

### Recovery Steps

```bash
# 1. Fix the root cause (restart, disk space, credentials)

# 2. Verify MinIO is healthy
curl -sf http://localhost:9000/minio/health/live

# 3. Restart application services (to reset storage client connections)
docker compose restart api worker

# 4. Verify file operations work
scripts/smoke-test.sh

# 5. Check for failed jobs that need retry
docker compose logs --tail=50 worker | grep -i "retry\|failed\|queue"

# 6. Verify backup MinIO connectivity
docker compose -f deploy/backup/docker-compose.backup.yml exec minio-backup-scheduler \
  mc admin info source
```

### Escalation

- If MinIO data volume is corrupted: escalate to L2 (Platform)
- If MinIO will not start: escalate to L2 (Platform)
- If data loss in MinIO: see [data-recovery.md](data-recovery.md#object-storage-recovery)

---

## OIDC Provider Outage

### Symptoms

- New login attempts fail (redirect to OIDC provider times out or errors)
- Token refresh fails for existing sessions (sessions expire without renewal)
- API returns 401 for previously authenticated users (if token cannot be validated)
- Application logs show OIDC discovery or token validation errors

### Detection

```bash
# Check OIDC discovery endpoint
curl -sf "${OIDC_ISSUER_URL}/.well-known/openid-configuration" | jq .issuer
echo "Exit code: $?"

# Check OIDC provider response time
curl -o /dev/null -sf -w "HTTP %{http_code} in %{time_total}s\n" \
  "${OIDC_ISSUER_URL}/.well-known/openid-configuration"

# Check API logs for auth errors
docker compose logs --tail=50 api | grep -i "oidc\|auth\|token\|unauthorized"

# Check if existing sessions still work (token validation may use cached JWKS)
curl -sf http://localhost:3000/health | jq .
```

### Immediate Actions

**If OIDC provider is completely unreachable:**

```bash
# Verify it is not a network issue on our side
# Try from different network paths
curl -v "${OIDC_ISSUER_URL}/.well-known/openid-configuration" 2>&1 | head -20

# Check DNS resolution
nslookup $(echo "${OIDC_ISSUER_URL}" | sed 's|https://||' | sed 's|/.*||')

# Check if our firewall/proxy is blocking
docker compose exec api curl -sf "${OIDC_ISSUER_URL}/.well-known/openid-configuration"
```

**If OIDC provider returns errors (5xx):**

This is on the provider side. Check their status page and wait for recovery.

### Graceful Degradation

When the OIDC provider is unavailable:
- **What fails:** New logins, token refresh
- **What still works:** 
  - Existing sessions with valid (non-expired) tokens continue working if JWKS keys are cached
  - All non-authenticated endpoints (health check, public API)
  - Background worker operations (do not require user auth)
- **Duration of grace period:** Until existing tokens expire (typically 15-60 minutes depending on token lifetime configuration)

### Recovery Steps

```bash
# 1. Monitor OIDC provider status page for resolution

# 2. Once provider is back, verify discovery endpoint
curl -sf "${OIDC_ISSUER_URL}/.well-known/openid-configuration" | jq .issuer

# 3. Restart API to refresh JWKS cache (if tokens were failing validation)
docker compose restart api

# 4. Verify login flow works
# (Manual: open application in browser and test login)

# 5. Check for users who had sessions expire during the outage
docker compose logs --tail=100 api | grep -i "401\|unauthorized" | wc -l
```

### Escalation

- If OIDC provider is down for more than 1 hour: escalate to L2 (Platform) to consider emergency authentication bypass
- If credential mismatch suspected: see [secret-rotation.md](secret-rotation.md#oidc-provider-credentials)
- If OIDC provider is a self-hosted instance: escalate to whoever manages it

---

## Polar Integration Outage

### Symptoms

- Polar sync jobs failing in worker logs
- Sync data becoming stale (last sync time not updating)
- Worker logs show HTTP errors or timeouts for Polar API calls

### Detection

```bash
# Check Polar API reachability
curl -sf -o /dev/null -w "HTTP %{http_code} in %{time_total}s\n" \
  https://www.polaraccesslink.com/v3

# Check worker logs for Polar errors
docker compose logs --tail=100 worker | grep -i "polar\|accesslink"

# Check job queue for failed Polar sync jobs
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT id, status, last_error, updated_at
  FROM platform.job_queue
  WHERE job_type LIKE '%polar%'
  ORDER BY updated_at DESC
  LIMIT 10;
"
```

### Immediate Actions

**If Polar API is unreachable:**

```bash
# This is an external service - check their status
curl -v https://www.polaraccesslink.com/v3 2>&1 | head -20

# No immediate fix available - Polar controls their API availability
```

**If our Polar credentials expired:**

```bash
# Check if the error is authentication-related
docker compose logs --tail=20 worker | grep -i "polar.*401\|polar.*403\|polar.*unauthorized"

# If yes, re-authenticate with Polar
# (Follow Polar API documentation for token refresh)
```

### Graceful Degradation

When the Polar API is unavailable:
- **What fails:** New sync data from Polar devices is not fetched
- **What still works:** 
  - All other application features (routes, activities, imports, exports)
  - Previously synced Polar data remains available
  - Users can still record activities manually
- **User impact:** Low - data is stale but core functionality works

### Recovery Steps

```bash
# 1. Wait for Polar API to recover (external dependency)

# 2. Verify connectivity
curl -sf -o /dev/null -w "%{http_code}" https://www.polaraccesslink.com/v3

# 3. Restart worker to retry failed jobs
docker compose restart worker

# 4. Verify sync is resuming
docker compose logs --tail=20 worker | grep -i "polar"

# 5. Check that stale sync jobs are being processed
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT id, status, updated_at
  FROM platform.job_queue
  WHERE job_type LIKE '%polar%'
  ORDER BY updated_at DESC
  LIMIT 5;
"
```

### Escalation

- Extended Polar outage (>24 hours): escalate to L2 (Platform) for user communication
- If the issue is on our side (credential/config): see [secret-rotation.md](secret-rotation.md)

---

## Alert Webhook Outage

### Symptoms

- Alert notifications not being delivered (Slack channel quiet despite known issues)
- `backup-monitor.sh` reports webhook delivery failures
- Curl to webhook URL times out or returns non-2xx

### Detection

```bash
# Test webhook delivery
curl -sf -o /dev/null -w "HTTP %{http_code}\n" -X POST "${ALERT_WEBHOOK_URL}" \
  -H "Content-Type: application/json" \
  -d '{"text":"Webhook health check test"}'

# Check backup monitor logs for delivery failures
docker compose logs --tail=50 backup-monitor 2>/dev/null || \
  grep -i "webhook\|alert\|delivery" /var/log/backup-monitor.log 2>/dev/null
```

### Immediate Actions

```bash
# If webhook URL has changed (Slack regenerated it)
# Update the webhook URL in the environment
sed -i "s|^ALERT_WEBHOOK_URL=.*|ALERT_WEBHOOK_URL=<new-url>|" deploy/backup/.env.backup

# Restart monitoring service
docker compose -f deploy/backup/docker-compose.backup.yml restart backup-monitor 2>/dev/null
```

### Graceful Degradation

When the alert webhook is unavailable:
- **What fails:** Alert notifications are not delivered to the team channel
- **What still works:** Everything else - the application, backups, monitoring checks all continue to run
- **Risk:** Team may not be notified of actual issues until webhook is restored
- **Mitigation:** Manual monitoring checks until webhook is restored

### Recovery Steps

```bash
# 1. Fix webhook URL or wait for provider recovery

# 2. Test delivery
curl -sf -X POST "${ALERT_WEBHOOK_URL}" \
  -H "Content-Type: application/json" \
  -d '{"text":"Alert webhook recovered - test notification"}'

# 3. Run backup monitor to flush any pending alerts
scripts/backup-monitor.sh

# 4. Review if any alerts were missed during the outage
scripts/verify-backup.sh
```

### Escalation

- Not typically escalated unless combined with other outages
- If webhook provider is permanently unavailable: escalate to L2 to configure alternative

---

## Recovery Verification

After any dependency recovers, perform these verification steps:

### Universal Verification

```bash
# Full system health check
scripts/smoke-test.sh

# API health (includes dependency checks)
curl -sf http://localhost:3000/health | jq .

# Check all containers are healthy
docker compose ps

# Check for error logs after recovery
docker compose logs --since="5m" api worker | grep -i "error\|panic\|fatal"
```

### Per-Dependency Verification

| Dependency | Verification Command |
|------------|---------------------|
| PostgreSQL | `docker compose exec postgres pg_isready -U haiker` |
| MinIO | `curl -sf http://localhost:9000/minio/health/live` |
| OIDC | `curl -sf "${OIDC_ISSUER_URL}/.well-known/openid-configuration" \| jq .issuer` |
| Polar API | `curl -sf -o /dev/null -w "%{http_code}" https://www.polaraccesslink.com/v3` |
| Alert Webhook | `curl -sf -o /dev/null -w "%{http_code}" -X POST "${ALERT_WEBHOOK_URL}" -d '{}'` |

### Check for Cascading Effects

After a dependency outage resolves, verify:

```bash
# Worker job queue - check for backed-up jobs
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT status, count(*) FROM platform.job_queue GROUP BY status;
"

# Backup system health
scripts/backup-monitor.sh

# Verify no data was lost during the outage
# (Application should not have written data if the dependency was down)
```

---

## Post-Outage Review

After any dependency outage lasting more than 15 minutes, conduct a brief review:

### Review Checklist

- [ ] What was the root cause of the outage?
- [ ] How was it detected? (Monitoring alert? User report? Manual check?)
- [ ] How long until detection? Could we detect earlier?
- [ ] What was the user impact during the outage?
- [ ] Did graceful degradation work as expected?
- [ ] Were any alerts missed (webhook outage)?
- [ ] Do we need additional monitoring for this dependency?
- [ ] Is there an alternative or failover we should implement?
- [ ] Should we adjust timeouts or retry logic?
- [ ] Were runbook procedures accurate and sufficient?

### Improvements to Consider

| Dependency | Possible Improvements |
|------------|----------------------|
| PostgreSQL | Connection pool tuning, read replicas, disk space alerting |
| MinIO | Multi-node cluster, cross-region replication |
| OIDC | JWKS cache TTL extension, emergency auth bypass |
| Polar API | Longer retry intervals, circuit breaker pattern |
| Alert Webhook | Backup notification channel (email, SMS) |

---

## Contact and Escalation

| Role | Responsibility |
|------|---------------|
| Platform team | Infrastructure dependencies, failover configuration |
| On-call engineer | Initial detection, documented remediation |
| Database team | PostgreSQL-specific issues |
| Security team | OIDC provider issues (if self-hosted) |

### Escalation Path

1. **L1 (On-call):** Detect outage, run immediate actions, communicate status
2. **L2 (Platform):** Investigate complex dependency issues, coordinate with providers
3. **L3 (Specialist):** Database team for PostgreSQL, Security for OIDC

### External Provider Contacts

| Provider | Status Page | Support Contact |
|----------|-------------|-----------------|
| OIDC Provider | [Configure status page URL] | [Configure contact] |
| Polar | [Check Polar developer docs] | [developer.polar.com] |
| Slack (webhooks) | status.slack.com | N/A |
