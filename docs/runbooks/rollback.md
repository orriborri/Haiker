# Rollback Runbook

> **Owner:** Platform / Infrastructure team  
> **Last updated:** 2025-01-15  
> **Review cadence:** Quarterly (aligned with disaster recovery drill)

---

## Table of Contents

1. [Overview](#overview)
2. [Rollback Decision Tree](#rollback-decision-tree)
3. [Application Rollback](#application-rollback)
4. [Database Migration Rollback](#database-migration-rollback)
5. [Configuration Rollback](#configuration-rollback)
6. [Feature Flag Rollback](#feature-flag-rollback)
7. [Full Deployment Rollback](#full-deployment-rollback)
8. [Rollback Verification](#rollback-verification)
9. [When NOT to Rollback](#when-not-to-rollback)
10. [Contact and Escalation](#contact-and-escalation)

---

## Overview

This runbook covers procedures for rolling back application deployments, database migrations, and configuration changes for the Haiker platform. The goal is to restore service to a known-good state as quickly as possible.

### Key References

| Document | Purpose |
|----------|---------|
| [Migration Recovery](migration-recovery.md) | Detailed database migration rollback procedures |
| [Backup Operations](backup-operations.md) | PITR and data restoration |
| [Incident Response](incident-response.md) | When rollback is triggered by an incident |
| [Data Recovery](data-recovery.md) | When rollback alone does not restore data |

### Key Scripts

| Script | Purpose |
|--------|---------|
| `scripts/restore-postgres.sh` | Point-in-Time Recovery for PostgreSQL |
| `scripts/migration-recovery.sh` | Migration status, rollback, and forward-fix |
| `scripts/smoke-test.sh` | Post-rollback health verification |
| `scripts/backup-monitor.sh` | Verify backup health after rollback |

---

## Rollback Decision Tree

Use this decision tree to determine the appropriate rollback procedure:

```
Is the issue caused by new application code?
  YES -> Application Rollback (Section 3)
  NO  -> Is the issue caused by a database migration?
           YES -> Was data modified (not just schema)?
                    YES -> Consider PITR (see backup-operations.md)
                    NO  -> Database Migration Rollback (Section 4)
           NO  -> Is the issue caused by configuration changes?
                    YES -> Configuration Rollback (Section 5)
                    NO  -> Is the issue caused by multiple changes?
                             YES -> Full Deployment Rollback (Section 7)
                             NO  -> Investigate further (see incident-response.md)
```

### When to Rollback vs. Forward-Fix

| Scenario | Recommendation |
|----------|---------------|
| Bug is obvious, fix is simple, users not impacted | Forward-fix |
| Users actively impacted, root cause unclear | Rollback |
| Data corruption occurring | Rollback immediately |
| Performance degradation, cause unknown | Rollback |
| New feature has edge-case bug, core features work | Forward-fix |
| Migration partially applied, state unclear | Rollback (may need PITR) |

---

## Application Rollback

### Prerequisites

- Know the previous working image tag (from deployment history or container registry)
- Docker Compose access on the deployment host
- Ability to restart services

### Procedure

**Step 1: Identify the previous working image tag**

```bash
# Check current running image
docker compose ps --format json | jq -r '.[] | select(.Service == "api") | .Image'

# List recent image tags from registry (if using a registry)
# Or check deployment history / git log for the last known-good tag
git log --oneline --format="%h %s" -10
```

**Step 2: Stop the current deployment**

```bash
docker compose stop api worker
```

**Step 3: Update the image tag**

Edit `docker-compose.yml` or the environment file to reference the previous image:

```bash
# If using image tags in docker-compose.yml
export HAIKER_IMAGE_TAG="<previous-good-tag>"

# Or edit the compose file directly
# image: haiker:${HAIKER_IMAGE_TAG:-latest}
```

**Step 4: Deploy the previous version**

```bash
# Pull the previous image if needed
docker compose pull api worker

# Start services with the previous image
docker compose up -d api worker
```

**Step 5: Verify the rollback**

```bash
# Wait for services to be healthy
docker compose ps

# Run smoke tests
scripts/smoke-test.sh

# Check health endpoint
curl -sf http://localhost:3000/health | jq .

# Verify no errors in logs
docker compose logs --tail=50 api | grep -i error
docker compose logs --tail=50 worker | grep -i error
```

### Rollback with Docker Compose Build (Development/Staging)

If not using a registry and building images locally:

```bash
# Check out the previous known-good commit
git checkout <previous-commit-hash>

# Rebuild images
docker compose build api worker

# Restart services
docker compose up -d api worker

# Verify
scripts/smoke-test.sh
```

---

## Database Migration Rollback

For detailed migration rollback procedures, see [migration-recovery.md](migration-recovery.md). This section provides a quick decision framework.

### Quick Decision Tree

```
Did the migration complete successfully (success=true in _sqlx_migrations)?
  YES -> Is the application code compatible with the old schema?
           YES -> Roll back the migration SQL manually
           NO  -> Must rollback both application AND migration together (Section 7)
  NO  -> Was the migration partially executed?
           YES -> Check migration-recovery.md "Manual Rollback" section
           NO  -> Simply delete the failed record and fix the migration
```

### Quick Rollback Steps

**Step 1: Stop the application**

```bash
docker compose stop api worker
```

**Step 2: Check migration status**

```bash
scripts/migration-recovery.sh status
scripts/migration-recovery.sh verify
```

**Step 3: Determine rollback approach**

- If `.down.sql` files exist: `scripts/migration-recovery.sh rollback --yes`
- If no `.down.sql` files: write manual rollback SQL (see migration-recovery.md for patterns)

**Step 4: Execute the rollback**

```bash
# For manual rollback (example: rolling back a column addition)
docker compose exec postgres psql -U haiker -d haiker -c "
  ALTER TABLE imports.imports DROP COLUMN IF EXISTS payload_hash;
  DELETE FROM _sqlx_migrations WHERE version = 20250105000001;
"
```

**Step 5: Verify**

```bash
scripts/migration-recovery.sh verify
```

**Step 6: Restart application with compatible code**

```bash
docker compose up -d api worker
scripts/smoke-test.sh
```

### When to Use PITR Instead

Use Point-in-Time Recovery when:
- The migration modified existing data (UPDATE/DELETE on user data)
- Multiple migrations failed with unclear partial state
- Manual rollback SQL would risk data integrity

```bash
# PITR fallback (see backup-operations.md for full procedure)
docker compose stop api worker
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  /scripts/restore-postgres.sh \
  --target-time "<timestamp-before-migration>" \
  --yes
docker compose start api worker
scripts/smoke-test.sh
```

---

## Configuration Rollback

### Environment Variable Changes

**Step 1: Identify what changed**

```bash
# Check recent changes to .env files
git log --oneline -5 -- '*.env*' 'deploy/*.env*'

# Or compare current .env with the backup
diff .env .env.backup
```

**Step 2: Revert the configuration**

```bash
# If tracked in git
git checkout <previous-commit> -- .env

# If not tracked, restore from backup or secrets manager
# Copy the previous values from your secrets manager
```

**Step 3: Restart affected services**

```bash
# Determine which services use the changed variables
# API server configuration
docker compose restart api

# Worker configuration
docker compose restart worker

# Database connection changes require both
docker compose restart api worker

# MinIO/storage changes
docker compose restart api worker
```

**Step 4: Verify**

```bash
scripts/smoke-test.sh
curl -sf http://localhost:3000/health | jq .
```

### Docker Compose Configuration Changes

If `docker-compose.yml` or related compose files were modified:

```bash
# Revert the compose file
git checkout <previous-commit> -- docker-compose.yml

# Recreate affected services
docker compose up -d

# Verify
scripts/smoke-test.sh
```

---

## Feature Flag Rollback

> **Note:** Haiker MVP does not currently use a feature flag system. This section is included for future reference.

If feature flags are implemented in the future, the rollback procedure would be:

1. Identify the problematic feature flag
2. Toggle the flag to its previous state (disabled/enabled)
3. Verify the change takes effect (may require service restart depending on implementation)
4. Run smoke tests to confirm

For now, "feature rollback" means rolling back the application code that introduced the feature (see [Application Rollback](#application-rollback)).

---

## Full Deployment Rollback

Use this procedure when a deployment included both application code changes and database migrations that need to be reverted together.

### Prerequisites

- Know the previous working git commit or image tag
- Know which migrations were applied as part of the deployment
- Application must be stopped during the procedure

### Procedure

**Step 1: Stop the application**

```bash
docker compose stop api worker
```

**Step 2: Record the current state**

```bash
# Record current migration state
scripts/migration-recovery.sh status > /tmp/pre-rollback-migration-state.json

# Record current image/commit
git log --oneline -1
docker compose ps --format json > /tmp/pre-rollback-services.json
```

**Step 3: Roll back database migrations (newest first)**

```bash
# Identify migrations applied during this deployment
scripts/migration-recovery.sh status | jq '.[] | select(.installed_on > "DEPLOYMENT_START_TIME")'

# Roll back each migration in reverse order
# See migration-recovery.md for specific SQL patterns
docker compose exec postgres psql -U haiker -d haiker -c "
  -- Roll back migrations in reverse chronological order
  -- [Insert rollback SQL here - see migration-recovery.md]
"
```

**Step 4: Roll back application code**

```bash
# Check out the previous known-good commit
git checkout <previous-good-commit>

# Rebuild and deploy
docker compose build api worker
docker compose up -d api worker
```

**Step 5: Roll back configuration (if changed)**

```bash
# Restore previous configuration from version control or secrets manager
git checkout <previous-good-commit> -- .env deploy/
```

**Step 6: Comprehensive verification**

```bash
# Verify migration state
scripts/migration-recovery.sh verify

# Run smoke tests
scripts/smoke-test.sh

# Check health
curl -sf http://localhost:3000/health | jq .

# Verify backup monitoring
scripts/backup-monitor.sh

# Check for errors
docker compose logs --tail=100 api worker | grep -i "error\|panic\|fatal"
```

---

## Rollback Verification

After any rollback, perform these verification steps:

### Immediate Verification

```bash
# 1. Service health
docker compose ps
curl -sf http://localhost:3000/health | jq .

# 2. Smoke tests
scripts/smoke-test.sh

# 3. Error-free logs
docker compose logs --tail=50 api | grep -ic "error\|panic\|fatal"
docker compose logs --tail=50 worker | grep -ic "error\|panic\|fatal"

# 4. Database connectivity
docker compose exec postgres pg_isready -U haiker

# 5. Migration consistency
scripts/migration-recovery.sh verify
```

### Extended Verification (P1/P2 Incidents)

```bash
# 6. Verify backup system is healthy
scripts/backup-monitor.sh

# 7. Verify MinIO connectivity
docker compose exec api curl -sf http://minio:9000/minio/health/live

# 8. Check worker is processing jobs
docker compose logs --tail=20 worker | grep -i "processing\|completed"

# 9. Verify no data loss occurred
# (Query relevant tables to confirm expected data is present)
docker compose exec postgres psql -U haiker -d haiker -c "
  SELECT schemaname, relname, n_live_tup
  FROM pg_stat_user_tables
  ORDER BY schemaname, relname;
"
```

### Monitoring After Rollback

- Watch error rates for 30 minutes after rollback
- Verify alert channels are quiet
- Confirm no new incidents triggered by the rollback itself
- Check that background jobs (imports, exports) resume processing

---

## When NOT to Rollback

### Forward-Fix Scenarios

| Scenario | Why Not Rollback | Recommended Action |
|----------|-----------------|-------------------|
| Bug only affects edge case, <1% of users | Rollback disrupts all users | Fix and deploy forward |
| Migration added NOT NULL column with default | Rollback would lose the column but data is fine | Fix application code |
| New feature is broken but old features work | Rollback reverts all changes including good ones | Disable the broken feature, fix forward |
| Performance regression with clear cause | Root cause identified, fix is simple | Deploy the fix directly |
| Third-party API changed behavior | Our old code will still fail | Adapt code to new behavior |

### Situations Requiring Careful Judgment

| Scenario | Consideration |
|----------|--------------|
| Migration already ran DML on production data | Rollback loses those data changes; may need PITR |
| External systems consumed new API responses | Rolling back may break integrations |
| Users created data using new features | Rolling back loses that data |
| Rollback requires extended downtime | Forward-fix may be faster |

### Escalation Criteria

Escalate to L2/L3 before proceeding when:
- Unsure whether rollback will cause data loss
- Multiple interrelated changes make rollback complex
- External systems may be affected
- Rollback would require PITR (database team decision)

---

## Contact and Escalation

| Role | Responsibility |
|------|---------------|
| Platform team | Rollback tooling, deployment infrastructure |
| On-call engineer | Execute rollback procedures, verify results |
| Database team | Migration rollback decisions, PITR authorization |

### Escalation Path

1. **L1 (On-call):** Execute application rollback, simple config rollback
2. **L2 (Platform):** Complex multi-service rollback, deployment pipeline issues
3. **L3 (Database team):** Migration rollback with data implications, PITR decisions

### Communication

- Notify the team channel before starting any rollback
- Post results after rollback is verified
- Follow incident communication templates for P1/P2 (see [incident-response.md](incident-response.md))
