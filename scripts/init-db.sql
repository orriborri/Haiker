-- Enable PostGIS extension
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS postgis_topology;

-- Create dedicated backup user with REPLICATION privilege.
-- This user has minimal permissions: it can only perform base backups
-- and stream WAL for archiving. It cannot DROP objects or read application data.
--
-- IMPORTANT: The password below ('backup_secure_password') is a PLACEHOLDER for
-- initial development/testing only. In production, you MUST either:
--   1. Replace this script with an entrypoint that reads BACKUP_POSTGRES_PASSWORD
--      from the environment and executes CREATE ROLE dynamically, OR
--   2. Change this password to match your env.backup BACKUP_POSTGRES_PASSWORD value
--      before first deployment.
-- Failure to synchronize this password with the BACKUP_POSTGRES_PASSWORD environment
-- variable in docker-compose.backup.yml will cause backup authentication failures.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_catalog.pg_roles WHERE rolname = 'backup_user') THEN
        CREATE ROLE backup_user WITH LOGIN REPLICATION PASSWORD 'backup_secure_password';
    END IF;
END
$$;

-- Grant only the minimum required permissions for pg_basebackup
-- pg_read_all_settings allows reading server config (needed for backup metadata)
GRANT pg_read_all_settings TO backup_user;

-- Explicitly do NOT grant any data access privileges
-- The backup user connects only for replication protocol (pg_basebackup)
-- and does not need SELECT/INSERT/UPDATE/DELETE on any application tables.
REVOKE ALL ON SCHEMA public FROM backup_user;
