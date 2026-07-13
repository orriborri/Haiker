#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Backup Retention Enforcement Script
# Removes old backup files according to retention policy while ensuring the
# most recent backup is NEVER deleted regardless of age.
# Covers: PostgreSQL base backups, encrypted WAL archives, MinIO snapshots.
# =============================================================================

# --- Configuration (from environment) ---
BACKUP_STORAGE_PATH="${BACKUP_STORAGE_PATH:-/backups}"
WAL_ARCHIVE_PATH="${WAL_ARCHIVE_PATH:-/var/lib/postgresql/wal_archive}"
MINIO_BACKUP_PATH="${MINIO_BACKUP_PATH:-/minio-backups}"
BACKUP_RETENTION_DAYS="${BACKUP_RETENTION_DAYS:-7}"
MINIO_BACKUP_RETENTION_DAYS="${MINIO_BACKUP_RETENTION_DAYS:-30}"
RETENTION_LOG_FILE="${RETENTION_LOG_FILE:-/var/log/backup-retention.log}"
DRY_RUN="${DRY_RUN:-false}"

TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
DELETION_COUNT=0

# --- Helpers ---
log() {
    local message="[${TIMESTAMP}] $1"
    echo "${message}" >&2
    if [[ -n "${RETENTION_LOG_FILE}" ]]; then
        mkdir -p "$(dirname "${RETENTION_LOG_FILE}")" 2>/dev/null || true
        echo "${message}" >> "${RETENTION_LOG_FILE}" 2>/dev/null || true
    fi
}

log_deletion() {
    local file_path="$1"
    local file_type="$2"
    local age_days="$3"
    log "DELETED: type=${file_type} path=${file_path} age_days=${age_days}"
    DELETION_COUNT=$((DELETION_COUNT + 1))
}

# --- Safety: find the newest file in a set ---
get_newest_file() {
    local search_path="$1"
    local pattern="$2"

    find "${search_path}" -name "${pattern}" -type f 2>/dev/null \
        | xargs ls -t 2>/dev/null \
        | head -n1
}

# --- Remove old files, keeping the newest ---
enforce_retention() {
    local search_path="$1"
    local pattern="$2"
    local retention_days="$3"
    local file_type="$4"

    if [[ ! -d "${search_path}" ]]; then
        log "INFO: Path does not exist, skipping: ${search_path}"
        return
    fi

    local file_count
    file_count=$(find "${search_path}" -name "${pattern}" -type f 2>/dev/null | wc -l | tr -d ' ')

    if [[ "${file_count}" -eq 0 ]]; then
        log "INFO: No ${file_type} files found in ${search_path}"
        return
    fi

    # Identify the newest file (NEVER delete this)
    local newest_file
    newest_file=$(get_newest_file "${search_path}" "${pattern}")

    if [[ -z "${newest_file}" ]]; then
        log "INFO: Could not determine newest ${file_type} file, skipping"
        return
    fi

    log "INFO: Newest ${file_type} file (protected): ${newest_file}"
    log "INFO: Enforcing ${retention_days}-day retention on ${file_count} ${file_type} files in ${search_path}"

    local now_epoch
    now_epoch=$(date +%s)
    local retention_seconds=$((retention_days * 86400))

    while IFS= read -r file; do
        [[ -z "${file}" ]] && continue

        # SAFETY: Never delete the most recent backup
        if [[ "${file}" == "${newest_file}" ]]; then
            continue
        fi

        local file_epoch
        if stat --version >/dev/null 2>&1; then
            file_epoch=$(stat -c %Y "${file}")
        else
            file_epoch=$(stat -f %m "${file}")
        fi

        local age_seconds=$((now_epoch - file_epoch))
        local age_days=$((age_seconds / 86400))

        if [[ ${age_seconds} -gt ${retention_seconds} ]]; then
            if [[ "${DRY_RUN}" == "true" ]]; then
                log "DRY-RUN: Would delete ${file_type}: ${file} (age: ${age_days} days)"
            else
                rm -f "${file}"
                log_deletion "${file}" "${file_type}" "${age_days}"
            fi
        fi
    done < <(find "${search_path}" -name "${pattern}" -type f 2>/dev/null)
}

# --- WAL retention tied to oldest retained base backup ---
# Instead of a fixed day threshold, WAL segments are retained as long as the
# oldest base backup needs them for PITR. Any WAL file older than the oldest
# retained base backup is safe to remove.
enforce_wal_retention() {
    local wal_path="$1"
    local base_backup_path="$2"

    if [[ ! -d "${wal_path}" ]]; then
        log "INFO: WAL path does not exist, skipping: ${wal_path}"
        return
    fi

    local wal_count
    wal_count=$(find "${wal_path}" -type f 2>/dev/null | wc -l | tr -d ' ')

    if [[ "${wal_count}" -eq 0 ]]; then
        log "INFO: No WAL archive files found in ${wal_path}"
        return
    fi

    # Find the oldest retained base backup to use as the WAL retention boundary
    local oldest_base_backup=""
    local oldest_base_epoch=""

    if [[ -d "${base_backup_path}" ]]; then
        oldest_base_backup=$(find "${base_backup_path}" -name "*.tar.gz.gpg" -type f 2>/dev/null \
            | xargs ls -tr 2>/dev/null \
            | head -n1)
    fi

    if [[ -z "${oldest_base_backup}" ]]; then
        log "INFO: No base backups found; falling back to BACKUP_RETENTION_DAYS (${BACKUP_RETENTION_DAYS}) for WAL retention"
        enforce_retention "${wal_path}" "*" "${BACKUP_RETENTION_DAYS}" "wal_archive"
        return
    fi

    # Get the modification time of the oldest base backup
    if stat --version >/dev/null 2>&1; then
        oldest_base_epoch=$(stat -c %Y "${oldest_base_backup}")
    else
        oldest_base_epoch=$(stat -f %m "${oldest_base_backup}")
    fi

    log "INFO: Oldest retained base backup: ${oldest_base_backup}"
    log "INFO: WAL retention boundary epoch: ${oldest_base_epoch}"
    log "INFO: Enforcing WAL retention (keeping all WAL newer than oldest base backup) on ${wal_count} files"

    # Identify the newest WAL file (NEVER delete this)
    local newest_wal
    newest_wal=$(get_newest_file "${wal_path}" "*")

    local now_epoch
    now_epoch=$(date +%s)

    while IFS= read -r file; do
        [[ -z "${file}" ]] && continue

        # SAFETY: Never delete the newest WAL file
        if [[ -n "${newest_wal}" && "${file}" == "${newest_wal}" ]]; then
            continue
        fi

        local file_epoch
        if stat --version >/dev/null 2>&1; then
            file_epoch=$(stat -c %Y "${file}")
        else
            file_epoch=$(stat -f %m "${file}")
        fi

        # Delete WAL files older than the oldest retained base backup
        if [[ ${file_epoch} -lt ${oldest_base_epoch} ]]; then
            local age_days=$(( (now_epoch - file_epoch) / 86400 ))
            if [[ "${DRY_RUN}" == "true" ]]; then
                log "DRY-RUN: Would delete wal_archive: ${file} (age: ${age_days} days, older than oldest base backup)"
            else
                rm -f "${file}"
                log_deletion "${file}" "wal_archive" "${age_days}"
            fi
        fi
    done < <(find "${wal_path}" -type f 2>/dev/null)
}

# --- Main ---
main() {
    log "=== Backup Retention Enforcement Started ==="
    log "INFO: PostgreSQL retention: ${BACKUP_RETENTION_DAYS} days"
    log "INFO: MinIO retention: ${MINIO_BACKUP_RETENTION_DAYS} days"

    if [[ "${DRY_RUN}" == "true" ]]; then
        log "INFO: DRY RUN MODE - no files will be deleted"
    fi

    # 1. PostgreSQL base backups (encrypted tar.gz.gpg)
    enforce_retention \
        "${BACKUP_STORAGE_PATH}" \
        "*.tar.gz.gpg" \
        "${BACKUP_RETENTION_DAYS}" \
        "pg_base_backup"

    # 2. Encrypted WAL archives
    # WAL retention is tied to the oldest retained base backup, not to a fixed
    # day count. This ensures WAL segments needed for PITR from the oldest valid
    # base backup are never deleted prematurely.
    enforce_wal_retention \
        "${WAL_ARCHIVE_PATH}" \
        "${BACKUP_STORAGE_PATH}"

    # 3. MinIO backup snapshots
    enforce_retention \
        "${MINIO_BACKUP_PATH}" \
        "*" \
        "${MINIO_BACKUP_RETENTION_DAYS}" \
        "minio_snapshot"

    log "=== Backup Retention Enforcement Completed ==="
    log "INFO: Total files deleted: ${DELETION_COUNT}"

    # Output structured JSON summary
    cat <<EOF
{
  "timestamp": "${TIMESTAMP}",
  "dry_run": ${DRY_RUN},
  "files_deleted": ${DELETION_COUNT},
  "retention_policy": {
    "postgresql_days": ${BACKUP_RETENTION_DAYS},
    "minio_days": ${MINIO_BACKUP_RETENTION_DAYS}
  },
  "paths_scanned": [
    "${BACKUP_STORAGE_PATH}",
    "${WAL_ARCHIVE_PATH}",
    "${MINIO_BACKUP_PATH}"
  ]
}
EOF
}

main "$@"
