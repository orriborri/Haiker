#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# MinIO Backup Script
# Mirrors critical buckets from the source MinIO instance to the backup MinIO
# instance using mc (MinIO Client). Encrypts objects at rest via server-side
# encryption on the backup target and verifies replication integrity.
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
BUCKETS_TO_BACKUP="${MINIO_BACKUP_BUCKETS:-haiker-artifacts,haiker-uploads}"
BACKUP_ENCRYPTION_KEY="${MINIO_BACKUP_ENCRYPTION_KEY:-}"
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
  log "Configuring mc aliases..."

  mc alias set "${MINIO_SOURCE_ALIAS}" "${SOURCE_ENDPOINT}" \
    "${SOURCE_ACCESS_KEY}" "${SOURCE_SECRET_KEY}" --api S3v4

  mc alias set "${MINIO_BACKUP_ALIAS}" "${BACKUP_ENDPOINT}" \
    "${BACKUP_ACCESS_KEY}" "${BACKUP_SECRET_KEY}" --api S3v4
}

ensure_backup_buckets() {
  local bucket
  IFS=',' read -ra bucket_list <<< "${BUCKETS_TO_BACKUP}"
  for bucket in "${bucket_list[@]}"; do
    bucket="$(echo "${bucket}" | xargs)"
    if ! mc ls "${MINIO_BACKUP_ALIAS}/${bucket}" >/dev/null 2>&1; then
      log "Creating backup bucket: ${bucket}"
      mc mb "${MINIO_BACKUP_ALIAS}/${bucket}"
    fi

    # Enable versioning on the backup bucket
    mc version enable "${MINIO_BACKUP_ALIAS}/${bucket}" || true
  done
}

mirror_buckets() {
  local bucket
  local exit_code=0
  IFS=',' read -ra bucket_list <<< "${BUCKETS_TO_BACKUP}"

  for bucket in "${bucket_list[@]}"; do
    bucket="$(echo "${bucket}" | xargs)"
    log "Mirroring bucket: ${bucket}"

    local mirror_args=("--preserve" "--overwrite")

    # Use server-side encryption on the backup target if encryption key is provided
    if [[ -n "${BACKUP_ENCRYPTION_KEY}" ]]; then
      export MC_ENCRYPT_KEY="${MINIO_BACKUP_ALIAS}/${bucket}=${BACKUP_ENCRYPTION_KEY}"
      mirror_args+=("--encrypt-key" "${MC_ENCRYPT_KEY}")
    fi

    if mc mirror "${mirror_args[@]}" \
      "${MINIO_SOURCE_ALIAS}/${bucket}" \
      "${MINIO_BACKUP_ALIAS}/${bucket}" 2>&1 | tee -a "${LOG_FILE}"; then
      log "Successfully mirrored bucket: ${bucket}"
    else
      error "Failed to mirror bucket: ${bucket}"
      exit_code=1
    fi

    unset MC_ENCRYPT_KEY
  done

  return ${exit_code}
}

verify_replication() {
  local bucket
  local source_count
  local backup_count
  local status="success"
  IFS=',' read -ra bucket_list <<< "${BUCKETS_TO_BACKUP}"

  for bucket in "${bucket_list[@]}"; do
    bucket="$(echo "${bucket}" | xargs)"

    source_count="$(mc ls --recursive "${MINIO_SOURCE_ALIAS}/${bucket}" 2>/dev/null | wc -l)"
    backup_count="$(mc ls --recursive "${MINIO_BACKUP_ALIAS}/${bucket}" 2>/dev/null | wc -l)"

    if [[ "${source_count}" -ne "${backup_count}" ]]; then
      error "Object count mismatch for ${bucket}: source=${source_count}, backup=${backup_count}"
      status="warning"
    else
      log "Verified ${bucket}: ${source_count} objects match"
    fi
  done

  echo "${status}"
}

cleanup_old_backups() {
  local retention_days="${MINIO_BACKUP_RETENTION_DAYS:-30}"
  local bucket
  IFS=',' read -ra bucket_list <<< "${BUCKETS_TO_BACKUP}"

  for bucket in "${bucket_list[@]}"; do
    bucket="$(echo "${bucket}" | xargs)"
    log "Removing versions older than ${retention_days} days in backup bucket: ${bucket}"
    mc rm --recursive --force --older-than "${retention_days}d" \
      "${MINIO_BACKUP_ALIAS}/${bucket}" 2>/dev/null || true
  done
}

# --- Main ---
main() {
  log "=== MinIO Backup Started ==="

  # Validate required variables
  if [[ -z "${SOURCE_ACCESS_KEY}" || -z "${SOURCE_SECRET_KEY}" ]]; then
    error "Source MinIO credentials not set (MINIO_SOURCE_ACCESS_KEY / MINIO_SOURCE_SECRET_KEY)"
    exit 1
  fi

  if [[ -z "${BACKUP_ACCESS_KEY}" || -z "${BACKUP_SECRET_KEY}" ]]; then
    error "Backup MinIO credentials not set (BACKUP_MINIO_ROOT_USER / BACKUP_MINIO_ROOT_PASSWORD)"
    exit 1
  fi

  configure_aliases
  ensure_backup_buckets
  mirror_buckets
  local verify_status
  verify_status="$(verify_replication)"
  cleanup_old_backups

  log "=== MinIO Backup Completed (status: ${verify_status}) ==="

  # Output structured result for monitoring
  cat <<EOF
{
  "timestamp": "${TIMESTAMP}",
  "status": "${verify_status}",
  "buckets": "${BUCKETS_TO_BACKUP}",
  "source_endpoint": "${SOURCE_ENDPOINT}",
  "backup_endpoint": "${BACKUP_ENDPOINT}"
}
EOF
}

main "$@"
