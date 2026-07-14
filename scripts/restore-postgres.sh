#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# PostgreSQL Point-in-Time Recovery (PITR) Restore Script
# Restores from a base backup and replays WAL to a specified target timestamp.
#
# USAGE CONTEXT:
# This script requires pg_ctl, psql, and direct access to PGDATA. It is designed
# to run INSIDE the postgres container, not as a standalone service.
#
# To perform a restore:
#   docker compose -f deploy/backup/docker-compose.backup.yml exec postgres bash
#   /scripts/restore-postgres.sh --target-time "2024-01-15T10:30:00Z" --yes
#
# Or from the host (if the script is mounted):
#   docker compose -f deploy/backup/docker-compose.backup.yml exec postgres \
#     /scripts/restore-postgres.sh --target-time "2024-01-15T10:30:00Z" --yes
#
# See docs/runbooks/backup-operations.md for the full disaster recovery procedure.
# =============================================================================

# --- Configuration (from environment) ---
BACKUP_STORAGE_PATH="${BACKUP_STORAGE_PATH:-/backups}"
BACKUP_ENCRYPTION_KEY="${BACKUP_ENCRYPTION_KEY:?BACKUP_ENCRYPTION_KEY must be set}"
PGDATA="${PGDATA:-/var/lib/postgresql/data}"
WAL_ARCHIVE_PATH="${WAL_ARCHIVE_PATH:-/var/lib/postgresql/wal_archive}"
POSTGRES_USER="${POSTGRES_USER:-postgres}"

# --- Logging helpers ---
log_info() { echo "{\"level\":\"info\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}"; }
log_error() { echo "{\"level\":\"error\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}" >&2; }

# --- Usage ---
usage() {
    cat <<EOF
Usage: $0 --target-time <timestamp> [--backup-file <path>] [--yes]

Options:
  --target-time   Recovery target timestamp (ISO 8601 format, e.g., 2024-01-15T10:30:00Z)
  --backup-file   Path to the encrypted backup file (.tar.gz.gpg). If omitted, the latest backup is used.
  --yes           Skip confirmation prompt (for automated recovery).

Example:
  $0 --target-time "2024-01-15T10:30:00+00:00" --backup-file /backups/20240115T060000Z.tar.gz.gpg
EOF
    exit 1
}

# --- Parse arguments ---
TARGET_TIME=""
BACKUP_FILE=""
SKIP_CONFIRM=false

while [[ $# -gt 0 ]]; do
    case "$1" in
        --target-time)
            TARGET_TIME="$2"
            shift 2
            ;;
        --backup-file)
            BACKUP_FILE="$2"
            shift 2
            ;;
        --yes)
            SKIP_CONFIRM=true
            shift
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            ;;
    esac
done

if [[ -z "${TARGET_TIME}" ]]; then
    log_error "Missing required parameter: --target-time"
    usage
fi

# --- Locate backup file ---
if [[ -z "${BACKUP_FILE}" ]]; then
    log_info "No backup file specified, searching for latest backup"
    BACKUP_FILE=$(find "${BACKUP_STORAGE_PATH}" -name "*.tar.gz.gpg" -type f | sort -r | head -n1)
    if [[ -z "${BACKUP_FILE}" ]]; then
        log_error "No backup files found in ${BACKUP_STORAGE_PATH}"
        exit 1
    fi
    log_info "Using latest backup: ${BACKUP_FILE}"
fi

if [[ ! -f "${BACKUP_FILE}" ]]; then
    log_error "Backup file not found: ${BACKUP_FILE}"
    exit 1
fi

# --- Safety confirmation ---
if [[ "${SKIP_CONFIRM}" != "true" ]]; then
    echo ""
    echo "========================================"
    echo " PostgreSQL PITR Restore"
    echo "========================================"
    echo ""
    echo "  Backup file:  ${BACKUP_FILE}"
    echo "  Target time:  ${TARGET_TIME}"
    echo "  PGDATA:       ${PGDATA}"
    echo "  WAL archive:  ${WAL_ARCHIVE_PATH}"
    echo ""
    echo "  WARNING: This will DESTROY the current database contents!"
    echo ""
    read -r -p "  Type 'RESTORE' to confirm: " CONFIRMATION
    if [[ "${CONFIRMATION}" != "RESTORE" ]]; then
        log_info "Restore cancelled by user"
        exit 0
    fi
fi

# --- Step 1: Stop PostgreSQL ---
log_info "Stopping PostgreSQL"
if pg_isready -q 2>/dev/null; then
    pg_ctl -D "${PGDATA}" stop -m fast || true
fi

# Wait for shutdown
for i in $(seq 1 30); do
    if ! pg_isready -q 2>/dev/null; then
        break
    fi
    sleep 1
done

log_info "PostgreSQL stopped"

# --- Step 2: Clear existing data directory ---
log_info "Clearing existing data directory: ${PGDATA}"
rm -rf "${PGDATA:?}"/*

# --- Step 3: Decrypt and extract backup ---
log_info "Decrypting and extracting backup: ${BACKUP_FILE}"
TEMP_TAR=$(mktemp /tmp/restore-XXXXXX.tar.gz)
trap 'rm -f "${TEMP_TAR}"' EXIT

gpg --batch --yes --decrypt \
    --passphrase "${BACKUP_ENCRYPTION_KEY}" \
    --output "${TEMP_TAR}" \
    "${BACKUP_FILE}"

tar -xzf "${TEMP_TAR}" -C "${PGDATA}" --strip-components=1

log_info "Backup extracted to ${PGDATA}"

# --- Step 4: Configure recovery ---
log_info "Configuring Point-in-Time Recovery to target: ${TARGET_TIME}"

# PostgreSQL 12+ uses recovery signal files instead of recovery.conf
touch "${PGDATA}/recovery.signal"

# Append recovery settings to the existing postgresql.auto.conf from the extracted
# backup (preserving any GUC values previously set via ALTER SYSTEM).
cat >> "${PGDATA}/postgresql.auto.conf" <<RECOVERY_CONF

# Recovery configuration - auto-generated by restore script
restore_command = 'cp ${WAL_ARCHIVE_PATH}/%f %p'
recovery_target_time = '${TARGET_TIME}'
recovery_target_action = 'promote'
RECOVERY_CONF

# --- Step 5: Set ownership ---
log_info "Setting file ownership"
chown -R "${POSTGRES_USER}:${POSTGRES_USER}" "${PGDATA}" 2>/dev/null || true

# --- Step 6: Start PostgreSQL in recovery mode ---
log_info "Starting PostgreSQL in recovery mode"
pg_ctl -D "${PGDATA}" start -w -t 300

# --- Step 7: Validate recovery ---
log_info "Waiting for recovery to complete"
RECOVERY_TIMEOUT=300
ELAPSED=0

while [[ ${ELAPSED} -lt ${RECOVERY_TIMEOUT} ]]; do
    if pg_isready -q 2>/dev/null; then
        IN_RECOVERY=$(psql -U "${POSTGRES_USER}" -tAc "SELECT pg_is_in_recovery();" 2>/dev/null || echo "true")
        if [[ "${IN_RECOVERY}" == "f" ]] || [[ "${IN_RECOVERY}" == "false" ]]; then
            log_info "Recovery completed successfully. Database promoted to primary."
            break
        fi
    fi
    sleep 5
    ELAPSED=$((ELAPSED + 5))
done

if [[ ${ELAPSED} -ge ${RECOVERY_TIMEOUT} ]]; then
    log_error "Recovery did not complete within ${RECOVERY_TIMEOUT} seconds. Manual intervention may be required."
    exit 1
fi

log_info "PITR restore finished. Database recovered to: ${TARGET_TIME}"
