# Migration Recovery Runbook

> **Owner:** Platform / Infrastructure team  
> **Last updated:** 2025-01-15  
> **Review cadence:** Quarterly (aligned with disaster recovery drill)

---

## Table of Contents

1. [Overview](#overview)
2. [Prerequisites](#prerequisites)
3. [Diagnosing Migration Failures](#diagnosing-migration-failures)
4. [Rollback Procedures](#rollback-procedures)
5. [Forward-Fix Procedures](#forward-fix-procedures)
6. [Testing Migration Changes](#testing-migration-changes)
7. [Emergency Procedures](#emergency-procedures)
8. [Haiker Schema Reference](#haiker-schema-reference)
9. [Contact and Escalation](#contact-and-escalation)

---

## Overview

This runbook covers recovery procedures when SQLx database migrations fail during deployment or maintenance. Haiker uses SQLx with sequential migration files stored in `migrations/` and tracked in the `_sqlx_migrations` table.

### Key Tools

| Tool | Purpose |
|------|---------|
| `scripts/migration-recovery.sh status` | Show all applied migrations as JSON |
| `scripts/migration-recovery.sh verify` | Compare applied vs. on-disk migrations |
| `scripts/migration-recovery.sh rollback` | Roll back migrations (manual guidance if no .down.sql) |
| `scripts/migration-recovery.sh forward-fix` | Re-run pending migrations with pre-flight checks |
| `scripts/restore-postgres.sh` | Full PITR restore (emergency fallback) |
| `scripts/restore-drill.sh` | Test recovery in an isolated drill environment |

### How SQLx Tracks Migrations

SQLx maintains a `_sqlx_migrations` table with the following columns:

| Column | Type | Description |
|--------|------|-------------|
| `version` | BIGINT | Numeric version derived from filename prefix (YYYYMMDDHHMMSS) |
| `description` | TEXT | Description extracted from filename (after prefix, before .sql) |
| `installed_on` | TIMESTAMPTZ | Timestamp when the migration was applied |
| `success` | BOOLEAN | Whether the migration completed successfully |
| `checksum` | BYTEA | SHA-384 hash of the migration file content |
| `execution_time` | BIGINT | Time taken to execute (nanoseconds) |

---

## Prerequisites

Before performing any migration recovery:

- **Database access:** You need `psql` access to the target database with DDL privileges
- **Environment variables:** Ensure `PGHOST`, `PGPORT`, `PGUSER`, `PGPASSWORD`, `PGDATABASE` are configured (or `DATABASE_URL`)
- **Migration files:** The `migrations/` directory must be accessible from the working directory
- **Cargo + SQLx CLI:** Required for `forward-fix` subcommand (`cargo install sqlx-cli`)
- **Application stopped:** Ensure no application instances are writing to the database during recovery

### Default Connection Settings

| Variable | Default |
|----------|---------|
| `PGHOST` | localhost |
| `PGPORT` | 5432 |
| `PGUSER` | haiker |
| `PGPASSWORD` | haiker |
| `PGDATABASE` | haiker |

---

## Diagnosing Migration Failures

### Step 1: Check Current Migration State

```bash
scripts/migration-recovery.sh status
```

This outputs JSON with all applied migrations. Look for entries where `"success": false`.

### Step 2: Verify Consistency

```bash
scripts/migration-recovery.sh verify
```

This compares the database state against migration files on disk and reports:

- **Applied:** Migrations successfully recorded in the database
- **Pending:** Migration files that have not been applied yet
- **Failed:** Migrations marked as `success = false`
- **Checksum mismatches:** Migration files that have been modified after being applied
- **Orphaned:** Migrations in the database with no corresponding file on disk

### Step 3: Query the Database Directly

For detailed investigation, query `_sqlx_migrations` directly:

```sql
-- Show all migrations ordered by version
SELECT version, description, installed_on, success, execution_time
FROM _sqlx_migrations
ORDER BY version;

-- Show only failed migrations
SELECT version, description, installed_on
FROM _sqlx_migrations
WHERE success = false;

-- Show the last 5 applied migrations
SELECT version, description, installed_on, success
FROM _sqlx_migrations
ORDER BY version DESC
LIMIT 5;
```

### Step 4: Check PostgreSQL Logs

If a migration failed mid-execution, check the PostgreSQL logs for the specific error:

```bash
docker compose -f deploy/backup/docker-compose.backup.yml logs --tail=100 postgres | grep -i "error"
```

### Common Failure Symptoms

| Symptom | Likely Cause |
|---------|-------------|
| `success = false` in _sqlx_migrations | SQL syntax error or constraint violation |
| Checksum mismatch | Migration file was edited after being applied |
| Pending migrations not applying | Previous failed migration blocking the sequence |
| "relation already exists" error | Partial execution from a previous failed attempt |
| "column does not exist" error | Dependent migration was rolled back or never applied |

---

## Rollback Procedures

### Automated Rollback (with .down.sql files)

If `.down.sql` companion files exist for your migrations, use the automated rollback:

```bash
# Preview the rollback plan
scripts/migration-recovery.sh rollback --dry-run

# Roll back the last migration
scripts/migration-recovery.sh rollback --yes

# Roll back the last 3 migrations
scripts/migration-recovery.sh rollback --count 3 --yes
```

### Manual Rollback (without .down.sql files)

Since Haiker does not currently have `.down.sql` files, rollbacks must be performed manually. The `rollback` subcommand provides guidance:

```bash
# Get manual SQL guidance
scripts/migration-recovery.sh rollback --dry-run --count 1
```

After executing the manual SQL, remove the migration record:

```sql
DELETE FROM _sqlx_migrations WHERE version = <version>;
```

### Manual Rollback Patterns

#### Rolling Back ALTER TABLE ADD COLUMN

For migrations that add columns (e.g., `lifecycle_state`, `payload_hash`):

```sql
-- Rollback: 20250103000001_add_activity_lifecycle_state.sql
DROP INDEX IF EXISTS activity_catalog.idx_activities_lifecycle;
ALTER TABLE activity_catalog.activities DROP COLUMN IF EXISTS lifecycle_state;
DELETE FROM _sqlx_migrations WHERE version = 20250103000001;

-- Rollback: 20250105000001_add_payload_hash_to_imports.sql
ALTER TABLE imports.imports DROP COLUMN IF EXISTS payload_hash;
DELETE FROM _sqlx_migrations WHERE version = 20250105000001;
```

#### Rolling Back CREATE TABLE

For migrations that create tables (e.g., `route_editing` tables):

```sql
-- Rollback: 20250104000001_create_route_editing_tables.sql
-- Note: Drop dependent tables first (foreign key ordering)
DROP TABLE IF EXISTS route_editing.draft_operations CASCADE;
DROP TABLE IF EXISTS route_editing.drafts CASCADE;
DELETE FROM _sqlx_migrations WHERE version = 20250104000001;
```

#### Rolling Back CREATE INDEX

```sql
-- Drop an index by name
DROP INDEX IF EXISTS schema_name.index_name;
DELETE FROM _sqlx_migrations WHERE version = <version>;
```

#### Rolling Back CREATE SCHEMA

**WARNING:** Dropping a schema with `CASCADE` destroys ALL objects within it (tables, indexes, functions). Only use this if you are certain the schema and all its contents should be removed.

```sql
-- Rollback: 20250101000000_create_schemas.sql (DANGEROUS - destroys all data)
-- Only drop schemas that were created by this specific migration
-- and only if they contain no data you need to keep.
DROP SCHEMA IF EXISTS identity CASCADE;
DROP SCHEMA IF EXISTS activity_catalog CASCADE;
DROP SCHEMA IF EXISTS recorded_activity CASCADE;
DROP SCHEMA IF EXISTS route_editing CASCADE;
DROP SCHEMA IF EXISTS route_versioning CASCADE;
DROP SCHEMA IF EXISTS imports CASCADE;
DROP SCHEMA IF EXISTS exports CASCADE;
DROP SCHEMA IF EXISTS polar_integration CASCADE;
DROP SCHEMA IF EXISTS platform CASCADE;
DELETE FROM _sqlx_migrations WHERE version = 20250101000000;
```

### Rolling Back Multiple Migrations

When rolling back multiple migrations, always process them in **reverse chronological order** (newest first):

```sql
-- Example: Roll back the last 3 migrations in order
-- 1. Roll back 20250105000001 (add_payload_hash_to_imports)
ALTER TABLE imports.imports DROP COLUMN IF EXISTS payload_hash;
DELETE FROM _sqlx_migrations WHERE version = 20250105000001;

-- 2. Roll back 20250104000001 (create_route_editing_tables)
DROP TABLE IF EXISTS route_editing.draft_operations CASCADE;
DROP TABLE IF EXISTS route_editing.drafts CASCADE;
DELETE FROM _sqlx_migrations WHERE version = 20250104000001;

-- 3. Roll back 20250103000001 (add_activity_lifecycle_state)
DROP INDEX IF EXISTS activity_catalog.idx_activities_lifecycle;
ALTER TABLE activity_catalog.activities DROP COLUMN IF EXISTS lifecycle_state;
DELETE FROM _sqlx_migrations WHERE version = 20250103000001;
```

### Post-Rollback Verification

After any rollback, verify the state:

```bash
# Confirm the migration record was removed
scripts/migration-recovery.sh status

# Verify consistency
scripts/migration-recovery.sh verify

# Check that the schema change was actually reversed
psql -c "\d activity_catalog.activities"  # Should not show dropped column
psql -c "\dt route_editing.*"             # Should not show dropped tables
```

---

## Forward-Fix Procedures

Use forward-fix when migrations need to be re-applied after resolving an issue (e.g., fixing SQL syntax, resolving a constraint conflict).

### Step 1: Identify the Problem

```bash
scripts/migration-recovery.sh verify
```

Look for `"failed"` or `"pending"` entries.

### Step 2: Resolve the Issue

If a migration failed due to a SQL error:

1. **Fix the migration SQL** in `migrations/<filename>.sql`
2. **Remove the failed record** so SQLx will re-attempt it:

```sql
DELETE FROM _sqlx_migrations WHERE version = <version> AND success = false;
```

If the migration partially executed (e.g., created a table but failed on an index):

1. **Manually clean up** the partial changes:
   ```sql
   -- Example: drop partially created objects
   DROP TABLE IF EXISTS schema_name.table_name CASCADE;
   ```
2. **Remove the failed record:**
   ```sql
   DELETE FROM _sqlx_migrations WHERE version = <version>;
   ```
3. **Fix the migration SQL** if needed

### Step 3: Run Forward-Fix

```bash
# With confirmation prompt
scripts/migration-recovery.sh forward-fix

# Skip confirmation (for automated pipelines)
scripts/migration-recovery.sh forward-fix --yes
```

### Step 4: Verify

```bash
scripts/migration-recovery.sh verify
```

All migrations should show as "applied" with no pending or failed entries.

### Dealing with Checksum Mismatches

If you edited a migration file that was already applied (not recommended), you have two options:

**Option A: Update the checksum in the database** (if the change was cosmetic, e.g., comments only):

```sql
-- Recompute and update the checksum
-- Use: sha384sum migrations/<filename>.sql
UPDATE _sqlx_migrations
SET checksum = decode('<new_sha384_hex>', 'hex')
WHERE version = <version>;
```

**Option B: Remove and re-apply** (if the change was structural):

```sql
-- First, manually roll back the old version
-- Then remove the record
DELETE FROM _sqlx_migrations WHERE version = <version>;
-- Then run forward-fix to re-apply
```

---

## Testing Migration Changes

### Using the Drill Environment

Before applying migration changes to production, test them in the drill environment using `scripts/restore-drill.sh`:

```bash
# 1. Start a drill environment with a copy of production data
scripts/restore-drill.sh

# 2. Apply the migration recovery procedure in the drill
scripts/migration-recovery.sh verify
scripts/migration-recovery.sh forward-fix --yes

# 3. Run the full verification suite
scripts/migration-recovery.sh verify
scripts/smoke-test.sh
```

### Testing Rollback Procedures

Always test rollback SQL in the drill environment first:

```bash
# 1. Start drill environment
scripts/restore-drill.sh

# 2. Test the rollback SQL
psql -c "ALTER TABLE imports.imports DROP COLUMN IF EXISTS payload_hash;"
psql -c "DELETE FROM _sqlx_migrations WHERE version = 20250105000001;"

# 3. Verify the rollback worked
scripts/migration-recovery.sh verify

# 4. Re-apply the migration to confirm forward-fix works
scripts/migration-recovery.sh forward-fix --yes
```

### Pre-Deployment Checklist

Before deploying migrations to production:

- [ ] Migration tested in drill environment
- [ ] Rollback SQL written and tested
- [ ] Application compatible with both old and new schema (for zero-downtime deploys)
- [ ] Backup verified as recent (`scripts/verify-backup.sh`)
- [ ] Team notified of the deployment window

---

## Emergency Procedures

When migration recovery is not feasible (e.g., data corruption, multiple failed migrations with partial state), use Point-in-Time Recovery (PITR) as a last resort.

### When to Use PITR Instead of Migration Recovery

- Multiple migrations failed and left the database in an inconsistent state
- Manual rollback SQL would risk data loss
- The exact state of partial execution is unclear
- Time pressure requires the fastest path to a known-good state

### PITR Procedure

Use `scripts/restore-postgres.sh` to restore to a point before the failed migrations:

```bash
# 1. Stop the application
docker compose stop app worker

# 2. Identify the target time (just before migrations were attempted)
# Check deployment logs or _sqlx_migrations.installed_on for the timestamp

# 3. Run PITR restore
docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
  /scripts/restore-postgres.sh \
  --target-time "2025-01-15T10:00:00+00:00" \
  --yes

# 4. Verify the restore
scripts/migration-recovery.sh status

# 5. Re-apply migrations (after fixing the issue)
scripts/migration-recovery.sh forward-fix --yes

# 6. Restart the application
docker compose start app worker

# 7. Verify application health
scripts/smoke-test.sh
```

See [Backup Operations Runbook](backup-operations.md) for the full PITR procedure and additional context.

### Decision Matrix

| Situation | Recommended Action |
|-----------|-------------------|
| Single failed migration, SQL error clear | Fix SQL, forward-fix |
| Single failed migration, partial execution | Manual cleanup, then forward-fix |
| Multiple failed migrations, state unclear | PITR to pre-migration state |
| Checksum mismatch, cosmetic change | Update checksum in database |
| Checksum mismatch, structural change | Rollback and re-apply |
| Data corruption discovered | PITR to last known-good state |
| Production outage, fast recovery needed | PITR (fastest path to known-good) |

---

## Haiker Schema Reference

Understanding the Haiker schema structure is essential for writing correct rollback SQL.

### Schemas (Bounded Contexts)

| Schema | Purpose |
|--------|---------|
| `identity` | User accounts and sessions |
| `activity_catalog` | Activity definitions and metadata |
| `recorded_activity` | Recorded activity data |
| `route_editing` | Route drafts and editing operations |
| `route_versioning` | Published route versions |
| `imports` | Data import tracking |
| `exports` | Data export tracking |
| `polar_integration` | Polar API integration |
| `platform` | Platform-level tables (outbox, jobs, audit) |

### Migration Dependency Order

Migrations must be rolled back in reverse order. Key dependencies:

```
20250101000000_create_schemas          (all other migrations depend on these schemas)
20250101000001_create_platform_outbox   (platform schema)
20250101000002_create_job_queue         (platform schema)
20250101000003_create_identity_users    (identity schema)
20250101000004_create_identity_sessions (depends on identity.users)
20250101000005_create_platform_audit_log (platform schema)
20250101000006_enhance_job_queue        (depends on platform job queue)
20250102000001_create_imports_tables    (imports schema)
20250102000002_create_recorded_activity_tables (recorded_activity schema)
20250102000003_create_activity_catalog_tables  (activity_catalog schema)
20250103000001_add_activity_lifecycle_state    (depends on activity_catalog.activities)
20250104000001_create_route_editing_tables     (depends on identity.users)
20250105000001_add_payload_hash_to_imports     (depends on imports.imports)
```

### Common Rollback Examples

#### Drop a Recently Added Column

```sql
-- Safe: dropping a nullable column with no dependencies
ALTER TABLE imports.imports DROP COLUMN IF EXISTS payload_hash;
```

#### Drop a Column with an Index

```sql
-- Must drop index first, then column
DROP INDEX IF EXISTS activity_catalog.idx_activities_lifecycle;
ALTER TABLE activity_catalog.activities DROP COLUMN IF EXISTS lifecycle_state;
```

#### Drop Tables with Foreign Keys

```sql
-- Drop in reverse dependency order (child tables first)
DROP TABLE IF EXISTS route_editing.draft_operations CASCADE;
DROP TABLE IF EXISTS route_editing.drafts CASCADE;
```

---

## Contact and Escalation

| Role | Responsibility |
|------|---------------|
| Platform team | Migration tooling, recovery scripts |
| On-call engineer | Initial triage, run documented procedures |
| Database team | Complex schema decisions, data integrity |

### Escalation Path

1. **L1 (On-call):** Run `migration-recovery.sh status` and `verify`, attempt forward-fix
2. **L2 (Platform):** Manual rollback, investigate partial execution states
3. **L3 (Database team):** PITR decisions, complex multi-migration rollback, data recovery

### When to Escalate Immediately

- Any situation requiring PITR in production
- Data loss suspected from partial migration execution
- Multiple schemas affected by a failed migration
- Unsure whether rollback SQL is safe to execute
