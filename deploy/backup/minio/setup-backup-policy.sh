#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# MinIO Backup Policy Setup Script
# Configures bucket versioning, lifecycle rules, and backup service account
# on the source MinIO instance. Run once during initial setup or when
# reconfiguring backup policies.
# =============================================================================

# --- Configuration (from environment) ---
MINIO_SOURCE_ALIAS="${MINIO_SOURCE_ALIAS:-source}"
SOURCE_ENDPOINT="${MINIO_SOURCE_ENDPOINT:-http://minio:9000}"
SOURCE_ROOT_USER="${MINIO_ROOT_USER:-}"
SOURCE_ROOT_PASSWORD="${MINIO_ROOT_PASSWORD:-}"
BACKUP_SERVICE_ACCOUNT_ACCESS_KEY="${MINIO_BACKUP_SERVICE_ACCESS_KEY:-backup-svc}"
BACKUP_SERVICE_ACCOUNT_SECRET_KEY="${MINIO_BACKUP_SERVICE_SECRET_KEY:-}"
SOURCE_BUCKETS="${MINIO_SOURCE_BUCKETS:-haiker-artifacts,haiker-uploads}"
VERSION_RETENTION_VERSIONS="${MINIO_VERSION_RETENTION_VERSIONS:-5}"
VERSION_EXPIRY_DAYS="${MINIO_VERSION_EXPIRY_DAYS:-90}"
POLICY_FILE="${MINIO_BACKUP_POLICY_FILE:-/etc/minio/policies/bucket-policy.json}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# --- Functions ---
log() {
  echo "[$(date -u +"%Y-%m-%dT%H:%M:%SZ")] $*"
}

error() {
  log "ERROR: $*" >&2
}

configure_admin_alias() {
  log "Configuring admin alias for source MinIO..."

  if [[ -z "${SOURCE_ROOT_USER}" || -z "${SOURCE_ROOT_PASSWORD}" ]]; then
    error "Source MinIO root credentials not set (MINIO_ROOT_USER / MINIO_ROOT_PASSWORD)"
    exit 1
  fi

  mc alias set "${MINIO_SOURCE_ALIAS}" "${SOURCE_ENDPOINT}" \
    "${SOURCE_ROOT_USER}" "${SOURCE_ROOT_PASSWORD}" --api S3v4
}

enable_versioning() {
  local bucket
  IFS=',' read -ra bucket_list <<< "${SOURCE_BUCKETS}"

  for bucket in "${bucket_list[@]}"; do
    bucket="$(echo "${bucket}" | xargs)"
    log "Enabling versioning on bucket: ${bucket}"

    # Create bucket if it does not exist
    if ! mc ls "${MINIO_SOURCE_ALIAS}/${bucket}" >/dev/null 2>&1; then
      log "Creating bucket: ${bucket}"
      mc mb "${MINIO_SOURCE_ALIAS}/${bucket}"
    fi

    mc version enable "${MINIO_SOURCE_ALIAS}/${bucket}"
    log "Versioning enabled for: ${bucket}"
  done
}

configure_lifecycle_rules() {
  local bucket
  IFS=',' read -ra bucket_list <<< "${SOURCE_BUCKETS}"

  for bucket in "${bucket_list[@]}"; do
    bucket="$(echo "${bucket}" | xargs)"
    log "Configuring lifecycle rules for bucket: ${bucket}"

    # Expire noncurrent versions after retention period
    mc ilm rule add "${MINIO_SOURCE_ALIAS}/${bucket}" \
      --noncurrent-expire-days "${VERSION_EXPIRY_DAYS}" \
      --expire-delete-marker || true

    log "Lifecycle rules configured for: ${bucket}"
    log "  - Noncurrent versions expire after ${VERSION_EXPIRY_DAYS} days"
    log "  - Expired delete markers are removed"
  done
}

create_backup_policy() {
  local policy_path="${POLICY_FILE}"

  # Fall back to bundled policy file if the configured path does not exist
  if [[ ! -f "${policy_path}" ]]; then
    policy_path="${SCRIPT_DIR}/bucket-policy.json"
  fi

  if [[ ! -f "${policy_path}" ]]; then
    error "Backup policy file not found at ${policy_path}"
    exit 1
  fi

  log "Creating backup IAM policy from: ${policy_path}"
  mc admin policy create "${MINIO_SOURCE_ALIAS}" backup-readonly "${policy_path}"
  log "IAM policy 'backup-readonly' created"
}

create_backup_service_account() {
  if [[ -z "${BACKUP_SERVICE_ACCOUNT_SECRET_KEY}" ]]; then
    error "Backup service account secret key not set (MINIO_BACKUP_SERVICE_SECRET_KEY)"
    exit 1
  fi

  log "Creating backup service account..."

  # Create a user for the backup service
  mc admin user add "${MINIO_SOURCE_ALIAS}" \
    "${BACKUP_SERVICE_ACCOUNT_ACCESS_KEY}" \
    "${BACKUP_SERVICE_ACCOUNT_SECRET_KEY}" || true

  # Attach the backup-readonly policy
  mc admin policy attach "${MINIO_SOURCE_ALIAS}" backup-readonly \
    --user "${BACKUP_SERVICE_ACCOUNT_ACCESS_KEY}"

  log "Backup service account created and policy attached"
  log "  Access Key: ${BACKUP_SERVICE_ACCOUNT_ACCESS_KEY}"
}

# --- Main ---
main() {
  log "=== MinIO Backup Policy Setup ==="

  configure_admin_alias
  enable_versioning
  configure_lifecycle_rules
  create_backup_policy
  create_backup_service_account

  log "=== Setup Complete ==="
  log ""
  log "Next steps:"
  log "  1. Use the backup service account credentials in your backup environment"
  log "  2. Set MINIO_SOURCE_ACCESS_KEY=${BACKUP_SERVICE_ACCOUNT_ACCESS_KEY}"
  log "  3. Set MINIO_SOURCE_SECRET_KEY=<the secret you configured>"
  log "  4. Start the backup services: docker compose -f deploy/backup/docker-compose.backup.yml up -d"
}

main "$@"
