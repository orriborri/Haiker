#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# MinIO Backup Verification Script
# Validates replication status between source and backup MinIO instances.
# Outputs structured JSON status for monitoring integration.
# =============================================================================

# --- Configuration (from environment) ---
MINIO_SOURCE_ALIAS="${MINIO_SOURCE_ALIAS:-source}"
MINIO_BACKUP_ALIAS="${MINIO_BACKUP_ALIAS:-backup}"
SOURCE_ENDPOINT="${MINIO_SOURCE_ENDPOINT:-http://minio:9000}"
BACKUP_ENDPOINT="${BACKUP_MINIO_ENDPOINT:-http://minio-backup:9000}"
SOURCE_ACCESS_KEY="${MINIO_SOURCE_ACCESS_KEY:-}"
SOURCE_SECRET_KEY="${MINIO_SOURCE_SECRET_KEY:-}"
BACKUP_ACCESS_KEY="${BACKUP_MINIO_ROOT_USER:-}"
BACKUP_SECRET_KEY="${BACKUP_MINIO_ROOT_PASSWORD:-}"
BUCKETS_TO_VERIFY="${MINIO_BACKUP_BUCKETS:-haiker-artifacts,haiker-uploads}"
BACKUP_STORAGE_WARN_PERCENT="${MINIO_BACKUP_STORAGE_WARN_PERCENT:-80}"
BACKUP_MAX_AGE_HOURS="${MINIO_BACKUP_MAX_AGE_HOURS:-24}"
LOG_FILE="${MINIO_BACKUP_LOG:-/var/log/minio-backup.log}"
TIMESTAMP="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

# --- Functions ---
log() {
  echo "[${TIMESTAMP}] $*" | tee -a "${LOG_FILE}"
}

error() {
  log "ERROR: $*" >&2
}

configure_aliases() {
  mc alias set "${MINIO_SOURCE_ALIAS}" "${SOURCE_ENDPOINT}" \
    "${SOURCE_ACCESS_KEY}" "${SOURCE_SECRET_KEY}" --api S3v4 >/dev/null 2>&1

  mc alias set "${MINIO_BACKUP_ALIAS}" "${BACKUP_ENDPOINT}" \
    "${BACKUP_ACCESS_KEY}" "${BACKUP_SECRET_KEY}" --api S3v4 >/dev/null 2>&1
}

check_connectivity() {
  local alias="$1"
  local endpoint="$2"

  if mc admin info "${alias}" >/dev/null 2>&1; then
    echo "ok"
  else
    echo "unreachable"
  fi
}

check_object_counts() {
  local bucket="$1"
  local source_count
  local backup_count

  source_count="$(mc ls --recursive "${MINIO_SOURCE_ALIAS}/${bucket}" 2>/dev/null | wc -l)"
  backup_count="$(mc ls --recursive "${MINIO_BACKUP_ALIAS}/${bucket}" 2>/dev/null | wc -l)"

  echo "${source_count}:${backup_count}"
}

check_last_backup_time() {
  local bucket="$1"
  local latest_modified

  # Get the most recently modified object in the backup bucket
  latest_modified="$(mc ls --recursive "${MINIO_BACKUP_ALIAS}/${bucket}" 2>/dev/null \
    | sort -k1,2 | tail -n1 | awk '{print $1 "T" $2}')"

  if [[ -z "${latest_modified}" ]]; then
    echo "never"
    return
  fi

  echo "${latest_modified}"
}

check_storage_capacity() {
  local disk_info
  local used_percent="unknown"

  disk_info="$(mc admin info "${MINIO_BACKUP_ALIAS}" --json 2>/dev/null || echo "{}")"

  if command -v python3 >/dev/null 2>&1; then
    used_percent="$(echo "${disk_info}" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    info = data.get('info', {})
    usage = info.get('usage', {})
    total = usage.get('totalCapacity', 0)
    used = usage.get('usedCapacity', 0)
    if total > 0:
        print(int((used / total) * 100))
    else:
        print('unknown')
except Exception:
    print('unknown')
" 2>/dev/null || echo "unknown")"
  fi

  echo "${used_percent}"
}

# --- Main ---
main() {
  log "=== MinIO Backup Verification Started ==="

  # Validate required variables
  if [[ -z "${SOURCE_ACCESS_KEY}" || -z "${SOURCE_SECRET_KEY}" ]]; then
    error "Source MinIO credentials not set"
    exit 1
  fi

  if [[ -z "${BACKUP_ACCESS_KEY}" || -z "${BACKUP_SECRET_KEY}" ]]; then
    error "Backup MinIO credentials not set"
    exit 1
  fi

  configure_aliases

  # Check connectivity
  local source_status
  local backup_status
  source_status="$(check_connectivity "${MINIO_SOURCE_ALIAS}" "${SOURCE_ENDPOINT}")"
  backup_status="$(check_connectivity "${MINIO_BACKUP_ALIAS}" "${BACKUP_ENDPOINT}")"

  # Check storage capacity
  local storage_used_percent
  storage_used_percent="$(check_storage_capacity)"

  # Verify each bucket
  local overall_status="healthy"
  local bucket_results=""
  IFS=',' read -ra bucket_list <<< "${BUCKETS_TO_VERIFY}"

  for bucket in "${bucket_list[@]}"; do
    bucket="$(echo "${bucket}" | xargs)"
    local counts
    local last_backup
    local bucket_status="healthy"

    counts="$(check_object_counts "${bucket}")"
    local source_count="${counts%%:*}"
    local backup_count="${counts##*:}"

    last_backup="$(check_last_backup_time "${bucket}")"

    # Check object count mismatch
    if [[ "${source_count}" -ne "${backup_count}" ]]; then
      bucket_status="warning"
      log "WARNING: Object count mismatch for ${bucket} (source=${source_count}, backup=${backup_count})"
    fi

    # Check if last backup is too old
    if [[ "${last_backup}" == "never" ]]; then
      bucket_status="critical"
      log "CRITICAL: No backup found for ${bucket}"
      overall_status="critical"
    fi

    # Build JSON entry for this bucket
    if [[ -n "${bucket_results}" ]]; then
      bucket_results="${bucket_results},"
    fi
    bucket_results="${bucket_results}{\"name\":\"${bucket}\",\"source_objects\":${source_count},\"backup_objects\":${backup_count},\"last_backup\":\"${last_backup}\",\"status\":\"${bucket_status}\"}"

    if [[ "${bucket_status}" != "healthy" && "${overall_status}" != "critical" ]]; then
      overall_status="${bucket_status}"
    fi
  done

  # Check storage warning threshold
  local storage_warning="false"
  if [[ "${storage_used_percent}" != "unknown" && "${storage_used_percent}" -ge "${BACKUP_STORAGE_WARN_PERCENT}" ]]; then
    storage_warning="true"
    if [[ "${overall_status}" == "healthy" ]]; then
      overall_status="warning"
    fi
    log "WARNING: Backup storage at ${storage_used_percent}% (threshold: ${BACKUP_STORAGE_WARN_PERCENT}%)"
  fi

  # Check connectivity issues
  if [[ "${source_status}" != "ok" || "${backup_status}" != "ok" ]]; then
    overall_status="critical"
  fi

  log "=== MinIO Backup Verification Completed (status: ${overall_status}) ==="

  # Output structured JSON result
  cat <<EOF
{
  "timestamp": "${TIMESTAMP}",
  "overall_status": "${overall_status}",
  "source": {
    "endpoint": "${SOURCE_ENDPOINT}",
    "connectivity": "${source_status}"
  },
  "backup": {
    "endpoint": "${BACKUP_ENDPOINT}",
    "connectivity": "${backup_status}",
    "storage_used_percent": "${storage_used_percent}",
    "storage_warning": ${storage_warning}
  },
  "buckets": [${bucket_results}],
  "thresholds": {
    "max_age_hours": ${BACKUP_MAX_AGE_HOURS},
    "storage_warn_percent": ${BACKUP_STORAGE_WARN_PERCENT}
  }
}
EOF
}

main "$@"
