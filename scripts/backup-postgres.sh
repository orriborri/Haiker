#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# PostgreSQL Backup Script
# Performs a full base backup with GPG encryption and retention management.
# Designed to run on a schedule (e.g., daily via cron/ofelia).
# =============================================================================

# --- Configuration (from environment) ---
BACKUP_POSTGRES_HOST="${BACKUP_POSTGRES_HOST:-postgres}"
BACKUP_POSTGRES_PORT="${BACKUP_POSTGRES_PORT:-5432}"
BACKUP_POSTGRES_USER="${BACKUP_POSTGRES_USER:?BACKUP_POSTGRES_USER must be set}"
BACKUP_POSTGRES_PASSWORD="${BACKUP_POSTGRES_PASSWORD:?BACKUP_POSTGRES_PASSWORD must be set}"
BACKUP_STORAGE_PATH="${BACKUP_STORAGE_PATH:-/backups}"
BACKUP_ENCRYPTION_KEY="${BACKUP_ENCRYPTION_KEY:?BACKUP_ENCRYPTION_KEY must be set}"
BACKUP_RETENTION_DAYS="${BACKUP_RETENTION_DAYS:-7}"
BACKUP_LABEL="${BACKUP_LABEL:-haiker_backup}"

# --- Derived variables ---
TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
BACKUP_DIR="${BACKUP_STORAGE_PATH}/${TIMESTAMP}"
BACKUP_TARBALL="${BACKUP_STORAGE_PATH}/${TIMESTAMP}.tar.gz"
BACKUP_ENCRYPTED="${BACKUP_TARBALL}.gpg"

# --- Logging helpers ---
log_info() { echo "{\"level\":\"info\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}"; }
log_error() { echo "{\"level\":\"error\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}" >&2; }

# --- Cleanup function ---
cleanup() {
    log_info "Cleaning up intermediate files"
    rm -rf "${BACKUP_DIR}" "${BACKUP_TARBALL}" 2>/dev/null || true
}
trap cleanup EXIT

# --- Step 1: Perform pg_basebackup ---
log_info "Starting pg_basebackup to ${BACKUP_DIR}"

export PGPASSWORD="${BACKUP_POSTGRES_PASSWORD}"
pg_basebackup \
    --host="${BACKUP_POSTGRES_HOST}" \
    --port="${BACKUP_POSTGRES_PORT}" \
    --username="${BACKUP_POSTGRES_USER}" \
    --pgdata="${BACKUP_DIR}" \
    --format=plain \
    --wal-method=stream \
    --checkpoint=fast \
    --label="${BACKUP_LABEL}_${TIMESTAMP}" \
    --progress

log_info "pg_basebackup completed successfully"

# --- Step 2: Compress the backup ---
log_info "Compressing backup to ${BACKUP_TARBALL}"
tar -czf "${BACKUP_TARBALL}" -C "${BACKUP_STORAGE_PATH}" "${TIMESTAMP}"

# --- Step 3: Encrypt with GPG ---
log_info "Encrypting backup with GPG"
gpg --batch --yes --symmetric \
    --cipher-algo AES256 \
    --passphrase "${BACKUP_ENCRYPTION_KEY}" \
    --output "${BACKUP_ENCRYPTED}" \
    "${BACKUP_TARBALL}"

log_info "Backup encrypted: ${BACKUP_ENCRYPTED}"

# --- Step 4: Verify backup integrity ---
log_info "Verifying backup integrity"
gpg --batch --yes --decrypt \
    --passphrase "${BACKUP_ENCRYPTION_KEY}" \
    "${BACKUP_ENCRYPTED}" | tar -tzf - > /dev/null

log_info "Backup integrity verified"

# --- Step 5: Calculate and log backup metadata ---
BACKUP_SIZE=$(stat -c%s "${BACKUP_ENCRYPTED}" 2>/dev/null || stat -f%z "${BACKUP_ENCRYPTED}" 2>/dev/null || echo "unknown")
log_info "Backup completed: file=${BACKUP_ENCRYPTED}, size=${BACKUP_SIZE}, timestamp=${TIMESTAMP}"

# --- Retention is handled by scripts/backup-retention.sh ---
# Retention enforcement (with newest-file protection) is delegated to the dedicated
# backup-retention.sh script, which should be scheduled independently.

log_info "Backup process finished successfully"
