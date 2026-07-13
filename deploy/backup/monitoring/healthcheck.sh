#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Backup System Healthcheck
# Lightweight Docker HEALTHCHECK script.
# Returns exit code 0 if operational, 1 if not.
# Checks: scheduler process running, last backup within expected window.
# =============================================================================

# --- Configuration ---
BACKUP_STORAGE_PATH="${BACKUP_STORAGE_PATH:-/backups}"
WAL_ARCHIVE_PATH="${WAL_ARCHIVE_PATH:-/var/lib/postgresql/wal_archive}"
HEALTHCHECK_MAX_BACKUP_AGE_HOURS="${HEALTHCHECK_MAX_BACKUP_AGE_HOURS:-25}"
HEALTHCHECK_MAX_WAL_AGE_MINUTES="${HEALTHCHECK_MAX_WAL_AGE_MINUTES:-30}"

# --- Check 1: Verify scheduler process is running ---
check_scheduler_running() {
    # Check for cron or the sleep-loop backup process
    if pgrep -x "cron" >/dev/null 2>&1 || pgrep -x "crond" >/dev/null 2>&1; then
        return 0
    fi

    # Fallback: check if our main entrypoint process is alive (PID 1)
    if kill -0 1 >/dev/null 2>&1; then
        return 0
    fi

    return 1
}

# --- Check 2: Verify last backup is within expected window ---
check_backup_freshness() {
    local search_path="$1"
    local max_age_seconds="$2"
    local pattern="${3:-*}"

    if [[ ! -d "${search_path}" ]]; then
        # If the path does not exist, skip (may be a MinIO-only container)
        return 0
    fi

    local latest_file
    latest_file=$(find "${search_path}" -name "${pattern}" -type f 2>/dev/null | sort -r | head -n1)

    if [[ -z "${latest_file}" ]]; then
        # No backup files found - unhealthy
        return 1
    fi

    local file_epoch now_epoch age_seconds
    if stat --version >/dev/null 2>&1; then
        file_epoch=$(stat -c %Y "${latest_file}")
    else
        file_epoch=$(stat -f %m "${latest_file}")
    fi

    now_epoch=$(date +%s)
    age_seconds=$((now_epoch - file_epoch))

    if [[ ${age_seconds} -gt ${max_age_seconds} ]]; then
        return 1
    fi

    return 0
}

# --- Run checks ---
HEALTHY=true

if ! check_scheduler_running; then
    echo "UNHEALTHY: scheduler process not running"
    HEALTHY=false
fi

# Check base backup freshness (hours)
MAX_BACKUP_AGE_SECONDS=$((HEALTHCHECK_MAX_BACKUP_AGE_HOURS * 3600))
if ! check_backup_freshness "${BACKUP_STORAGE_PATH}" "${MAX_BACKUP_AGE_SECONDS}" "*.tar.gz.gpg"; then
    echo "UNHEALTHY: base backup too old or missing"
    HEALTHY=false
fi

# Check WAL archive freshness (minutes) - only if WAL path exists
if [[ -d "${WAL_ARCHIVE_PATH}" ]]; then
    MAX_WAL_AGE_SECONDS=$((HEALTHCHECK_MAX_WAL_AGE_MINUTES * 60))
    if ! check_backup_freshness "${WAL_ARCHIVE_PATH}" "${MAX_WAL_AGE_SECONDS}"; then
        echo "UNHEALTHY: WAL archive too old or missing"
        HEALTHY=false
    fi
fi

if [[ "${HEALTHY}" == "true" ]]; then
    echo "OK"
    exit 0
else
    exit 1
fi
