#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# PostgreSQL Backup Verification Script
# Checks backup health: age, integrity, and storage capacity.
# Outputs structured JSON for monitoring integration.
# =============================================================================

# --- Configuration (from environment) ---
BACKUP_STORAGE_PATH="${BACKUP_STORAGE_PATH:-/backups}"
WAL_ARCHIVE_PATH="${WAL_ARCHIVE_PATH:-/var/lib/postgresql/wal_archive}"
BACKUP_ENCRYPTION_KEY="${BACKUP_ENCRYPTION_KEY:-}"
BACKUP_RPO_BASE_HOURS="${BACKUP_RPO_BASE_HOURS:-24}"
BACKUP_RPO_WAL_MINUTES="${BACKUP_RPO_WAL_MINUTES:-15}"
BACKUP_STORAGE_WARN_PERCENT="${BACKUP_STORAGE_WARN_PERCENT:-80}"

# --- State tracking ---
OVERALL_STATUS="healthy"
CHECKS=()

# --- Helpers ---
log_info() { echo "{\"level\":\"info\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}" >&2; }

add_check() {
    local name="$1"
    local status="$2"
    local detail="$3"
    CHECKS+=("{\"name\":\"${name}\",\"status\":\"${status}\",\"detail\":\"${detail}\"}")
    if [[ "${status}" == "critical" ]] || [[ "${status}" == "warning" && "${OVERALL_STATUS}" == "healthy" ]]; then
        OVERALL_STATUS="${status}"
    fi
}

# --- Check 1: Base backup age ---
check_base_backup_age() {
    log_info "Checking base backup age (threshold: ${BACKUP_RPO_BASE_HOURS}h)"

    LATEST_BACKUP=$(find "${BACKUP_STORAGE_PATH}" -name "*.tar.gz.gpg" -type f 2>/dev/null | sort -r | head -n1)

    if [[ -z "${LATEST_BACKUP}" ]]; then
        add_check "base_backup_age" "critical" "No base backups found in ${BACKUP_STORAGE_PATH}"
        return
    fi

    # Get file modification time in epoch seconds
    if stat --version >/dev/null 2>&1; then
        # GNU stat
        BACKUP_EPOCH=$(stat -c %Y "${LATEST_BACKUP}")
    else
        # BSD stat (macOS)
        BACKUP_EPOCH=$(stat -f %m "${LATEST_BACKUP}")
    fi

    NOW_EPOCH=$(date +%s)
    AGE_SECONDS=$((NOW_EPOCH - BACKUP_EPOCH))
    AGE_HOURS=$((AGE_SECONDS / 3600))
    THRESHOLD_SECONDS=$((BACKUP_RPO_BASE_HOURS * 3600))

    if [[ ${AGE_SECONDS} -gt ${THRESHOLD_SECONDS} ]]; then
        add_check "base_backup_age" "critical" "Latest backup is ${AGE_HOURS}h old (threshold: ${BACKUP_RPO_BASE_HOURS}h). File: $(basename "${LATEST_BACKUP}")"
    else
        add_check "base_backup_age" "healthy" "Latest backup is ${AGE_HOURS}h old. File: $(basename "${LATEST_BACKUP}")"
    fi
}

# --- Check 2: WAL archive age ---
check_wal_archive_age() {
    log_info "Checking WAL archive age (threshold: ${BACKUP_RPO_WAL_MINUTES}min)"

    if [[ ! -d "${WAL_ARCHIVE_PATH}" ]]; then
        add_check "wal_archive_age" "critical" "WAL archive directory does not exist: ${WAL_ARCHIVE_PATH}"
        return
    fi

    LATEST_WAL=$(find "${WAL_ARCHIVE_PATH}" -type f 2>/dev/null | sort -r | head -n1)

    if [[ -z "${LATEST_WAL}" ]]; then
        add_check "wal_archive_age" "warning" "No WAL segments found in ${WAL_ARCHIVE_PATH}"
        return
    fi

    if stat --version >/dev/null 2>&1; then
        WAL_EPOCH=$(stat -c %Y "${LATEST_WAL}")
    else
        WAL_EPOCH=$(stat -f %m "${LATEST_WAL}")
    fi

    NOW_EPOCH=$(date +%s)
    AGE_SECONDS=$((NOW_EPOCH - WAL_EPOCH))
    AGE_MINUTES=$((AGE_SECONDS / 60))
    THRESHOLD_SECONDS=$((BACKUP_RPO_WAL_MINUTES * 60))

    if [[ ${AGE_SECONDS} -gt ${THRESHOLD_SECONDS} ]]; then
        add_check "wal_archive_age" "critical" "Latest WAL segment is ${AGE_MINUTES}min old (threshold: ${BACKUP_RPO_WAL_MINUTES}min)"
    else
        add_check "wal_archive_age" "healthy" "Latest WAL segment is ${AGE_MINUTES}min old"
    fi
}

# --- Check 3: Backup file integrity (GPG verification) ---
check_backup_integrity() {
    log_info "Checking backup file integrity"

    if [[ -z "${BACKUP_ENCRYPTION_KEY}" ]]; then
        add_check "backup_integrity" "warning" "BACKUP_ENCRYPTION_KEY not set, cannot verify encrypted backups"
        return
    fi

    LATEST_BACKUP=$(find "${BACKUP_STORAGE_PATH}" -name "*.tar.gz.gpg" -type f 2>/dev/null | sort -r | head -n1)

    if [[ -z "${LATEST_BACKUP}" ]]; then
        add_check "backup_integrity" "critical" "No backup files to verify"
        return
    fi

    # Try to decrypt and list contents (validates GPG encryption and tar structure)
    if gpg --batch --yes --decrypt --passphrase "${BACKUP_ENCRYPTION_KEY}" "${LATEST_BACKUP}" 2>/dev/null | tar -tzf - > /dev/null 2>&1; then
        add_check "backup_integrity" "healthy" "Latest backup passes GPG decryption and tar integrity check"
    else
        add_check "backup_integrity" "critical" "Latest backup failed integrity check: $(basename "${LATEST_BACKUP}")"
    fi
}

# --- Check 4: Storage capacity ---
check_storage_capacity() {
    log_info "Checking storage capacity (warn threshold: ${BACKUP_STORAGE_WARN_PERCENT}%)"

    if [[ ! -d "${BACKUP_STORAGE_PATH}" ]]; then
        add_check "storage_capacity" "critical" "Backup storage path does not exist: ${BACKUP_STORAGE_PATH}"
        return
    fi

    # Get usage percentage (works on both Linux and macOS)
    USAGE_LINE=$(df "${BACKUP_STORAGE_PATH}" | tail -1)
    USAGE_PERCENT=$(echo "${USAGE_LINE}" | awk '{print $5}' | tr -d '%')

    if [[ ${USAGE_PERCENT} -ge 95 ]]; then
        add_check "storage_capacity" "critical" "Storage is ${USAGE_PERCENT}% full"
    elif [[ ${USAGE_PERCENT} -ge ${BACKUP_STORAGE_WARN_PERCENT} ]]; then
        add_check "storage_capacity" "warning" "Storage is ${USAGE_PERCENT}% full (warn threshold: ${BACKUP_STORAGE_WARN_PERCENT}%)"
    else
        add_check "storage_capacity" "healthy" "Storage is ${USAGE_PERCENT}% full"
    fi
}

# --- Run all checks ---
check_base_backup_age
check_wal_archive_age
check_backup_integrity
check_storage_capacity

# --- Output structured JSON ---
CHECKS_JSON=$(printf '%s,' "${CHECKS[@]}" | sed 's/,$//')
BACKUP_COUNT=$(find "${BACKUP_STORAGE_PATH}" -name "*.tar.gz.gpg" -type f 2>/dev/null | wc -l | tr -d ' ')

cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "${OVERALL_STATUS}",
  "backup_count": ${BACKUP_COUNT},
  "checks": [${CHECKS_JSON}]
}
EOF

# Exit with appropriate code for monitoring
case "${OVERALL_STATUS}" in
    healthy)  exit 0 ;;
    warning)  exit 1 ;;
    critical) exit 2 ;;
    *)        exit 3 ;;
esac
