#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Unified Backup Monitoring Script
# Calls both PostgreSQL and MinIO verification scripts, evaluates alert
# conditions, sends webhook alerts when thresholds are violated, and outputs
# a unified health status as structured JSON.
# =============================================================================

# --- Configuration (from environment) ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VERIFY_BACKUP_SCRIPT="${VERIFY_BACKUP_SCRIPT:-${SCRIPT_DIR}/verify-backup.sh}"
VERIFY_MINIO_SCRIPT="${VERIFY_MINIO_SCRIPT:-${SCRIPT_DIR}/verify-minio-backup.sh}"

ALERT_WEBHOOK_URL="${ALERT_WEBHOOK_URL:-}"
ALERT_CHANNEL="${ALERT_CHANNEL:-#backup-alerts}"
CAPACITY_WARNING_PERCENT="${CAPACITY_WARNING_PERCENT:-80}"
CAPACITY_CRITICAL_PERCENT="${CAPACITY_CRITICAL_PERCENT:-90}"
BACKUP_RPO_WAL_MINUTES="${BACKUP_RPO_WAL_MINUTES:-15}"
BACKUP_RPO_BASE_HOURS="${BACKUP_RPO_BASE_HOURS:-24}"
BACKUP_RETENTION_DAYS="${BACKUP_RETENTION_DAYS:-7}"
MINIO_BACKUP_RETENTION_DAYS="${MINIO_BACKUP_RETENTION_DAYS:-30}"

# --- State ---
OVERALL_STATUS="healthy"
ALERTS=()
TIMESTAMP="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

# --- Helpers ---
log_info() {
    echo "[${TIMESTAMP}] INFO: $1" >&2
}

log_error() {
    echo "[${TIMESTAMP}] ERROR: $1" >&2
}

escalate_status() {
    local new_status="$1"
    if [[ "${new_status}" == "critical" ]]; then
        OVERALL_STATUS="critical"
    elif [[ "${new_status}" == "warning" && "${OVERALL_STATUS}" == "healthy" ]]; then
        OVERALL_STATUS="warning"
    fi
}

add_alert() {
    local rule="$1"
    local severity="$2"
    local message="$3"
    ALERTS+=("{\"rule\":\"${rule}\",\"severity\":\"${severity}\",\"message\":\"${message}\",\"timestamp\":\"${TIMESTAMP}\"}")
    escalate_status "${severity}"
}

# --- Send webhook alert ---
send_webhook_alert() {
    if [[ -z "${ALERT_WEBHOOK_URL}" ]]; then
        log_info "No ALERT_WEBHOOK_URL configured, skipping alert delivery"
        return 0
    fi

    local alerts_json
    alerts_json=$(printf '%s,' "${ALERTS[@]}" | sed 's/,$//')

    local payload
    payload=$(cat <<EOF
{
  "channel": "${ALERT_CHANNEL}",
  "status": "${OVERALL_STATUS}",
  "timestamp": "${TIMESTAMP}",
  "source": "backup-monitor",
  "alerts": [${alerts_json}]
}
EOF
)

    log_info "Sending alert to webhook: ${ALERT_WEBHOOK_URL}"

    local http_code
    http_code=$(curl -s -o /dev/null -w "%{http_code}" \
        -X POST \
        -H "Content-Type: application/json" \
        -d "${payload}" \
        "${ALERT_WEBHOOK_URL}" 2>/dev/null || echo "000")

    if [[ "${http_code}" -ge 200 && "${http_code}" -lt 300 ]]; then
        log_info "Alert delivered successfully (HTTP ${http_code})"
    else
        log_error "Alert delivery failed (HTTP ${http_code})"
    fi
}

# --- Run PostgreSQL verification ---
run_pg_verification() {
    log_info "Running PostgreSQL backup verification..."

    local pg_result=""
    local pg_exit_code=0

    if [[ -x "${VERIFY_BACKUP_SCRIPT}" ]]; then
        pg_result=$("${VERIFY_BACKUP_SCRIPT}" 2>/dev/null) || pg_exit_code=$?
    else
        log_error "PostgreSQL verify script not found or not executable: ${VERIFY_BACKUP_SCRIPT}"
        pg_exit_code=127
    fi

    # Evaluate alert conditions
    if [[ ${pg_exit_code} -ne 0 ]]; then
        if [[ ${pg_exit_code} -eq 2 ]]; then
            add_alert "backup_failure" "critical" "PostgreSQL backup verification failed with critical status"
        else
            add_alert "backup_failure" "warning" "PostgreSQL backup verification returned non-zero exit code: ${pg_exit_code}"
        fi
    fi

    # Parse JSON output for specific checks if available
    if [[ -n "${pg_result}" ]] && command -v python3 >/dev/null 2>&1; then
        local check_results
        check_results=$(echo "${pg_result}" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    checks = data.get('checks', [])
    for check in checks:
        name = check.get('name', '')
        status = check.get('status', '')
        detail = check.get('detail', '')
        print(f'{name}|{status}|{detail}')
except Exception:
    pass
" 2>/dev/null || echo "")

        while IFS='|' read -r check_name check_status check_detail; do
            [[ -z "${check_name}" ]] && continue

            case "${check_name}" in
                base_backup_age)
                    if [[ "${check_status}" == "critical" ]]; then
                        add_alert "backup_age_warning" "critical" "Base backup age exceeded: ${check_detail}"
                    fi
                    ;;
                wal_archive_age)
                    if [[ "${check_status}" == "critical" ]]; then
                        add_alert "backup_age_critical" "critical" "WAL archive age exceeded: ${check_detail}"
                    fi
                    ;;
                storage_capacity)
                    if [[ "${check_status}" == "critical" ]]; then
                        add_alert "capacity_critical" "critical" "PostgreSQL backup storage critical: ${check_detail}"
                    elif [[ "${check_status}" == "warning" ]]; then
                        add_alert "capacity_warning" "warning" "PostgreSQL backup storage warning: ${check_detail}"
                    fi
                    ;;
            esac
        done <<< "${check_results}"
    fi

    echo "${pg_result}"
}

# --- Run MinIO verification ---
run_minio_verification() {
    log_info "Running MinIO backup verification..."

    local minio_result=""
    local minio_exit_code=0

    if [[ -x "${VERIFY_MINIO_SCRIPT}" ]]; then
        minio_result=$("${VERIFY_MINIO_SCRIPT}" 2>/dev/null) || minio_exit_code=$?
    else
        log_error "MinIO verify script not found or not executable: ${VERIFY_MINIO_SCRIPT}"
        minio_exit_code=127
    fi

    # Evaluate alert conditions
    if [[ ${minio_exit_code} -ne 0 ]]; then
        add_alert "backup_failure" "critical" "MinIO backup verification failed with exit code: ${minio_exit_code}"
    fi

    # Parse JSON output for capacity checks if available
    if [[ -n "${minio_result}" ]] && command -v python3 >/dev/null 2>&1; then
        local storage_percent
        storage_percent=$(echo "${minio_result}" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    backup = data.get('backup', {})
    pct = backup.get('storage_used_percent', 'unknown')
    print(pct)
except Exception:
    print('unknown')
" 2>/dev/null || echo "unknown")

        if [[ "${storage_percent}" != "unknown" ]]; then
            if [[ "${storage_percent}" -ge "${CAPACITY_CRITICAL_PERCENT}" ]]; then
                add_alert "capacity_critical" "critical" "MinIO backup storage at ${storage_percent}% (critical threshold: ${CAPACITY_CRITICAL_PERCENT}%)"
            elif [[ "${storage_percent}" -ge "${CAPACITY_WARNING_PERCENT}" ]]; then
                add_alert "capacity_warning" "warning" "MinIO backup storage at ${storage_percent}% (warning threshold: ${CAPACITY_WARNING_PERCENT}%)"
            fi
        fi
    fi

    echo "${minio_result}"
}

# --- Check retention compliance ---
check_retention_compliance() {
    log_info "Checking retention compliance..."

    local backup_storage="${BACKUP_STORAGE_PATH:-/backups}"
    local retention_seconds=$((BACKUP_RETENTION_DAYS * 86400))
    local now_epoch
    now_epoch=$(date +%s)

    if [[ ! -d "${backup_storage}" ]]; then
        return
    fi

    # Check for backups older than retention policy
    local has_violation=false
    while IFS= read -r backup_file; do
        [[ -z "${backup_file}" ]] && continue

        local file_epoch
        if stat --version >/dev/null 2>&1; then
            file_epoch=$(stat -c %Y "${backup_file}")
        else
            file_epoch=$(stat -f %m "${backup_file}")
        fi

        local age_seconds=$((now_epoch - file_epoch))
        if [[ ${age_seconds} -gt ${retention_seconds} ]]; then
            has_violation=true
            break
        fi
    done < <(find "${backup_storage}" -name "*.tar.gz.gpg" -type f 2>/dev/null)

    if [[ "${has_violation}" == "true" ]]; then
        add_alert "retention_violation" "warning" "Backups older than ${BACKUP_RETENTION_DAYS} days still exist in ${backup_storage}"
    fi

    # Check that at least one backup exists (newest backup must not be missing)
    local backup_count
    backup_count=$(find "${backup_storage}" -name "*.tar.gz.gpg" -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "${backup_count}" -eq 0 ]]; then
        add_alert "retention_violation" "critical" "No backups found - newest backup is missing"
    fi
}

# --- Main ---
main() {
    log_info "=== Backup Monitor Started ==="

    # Run verification scripts
    local pg_output
    pg_output=$(run_pg_verification)

    local minio_output
    minio_output=$(run_minio_verification)

    # Check retention compliance
    check_retention_compliance

    # Send alerts if any conditions were violated
    if [[ ${#ALERTS[@]} -gt 0 ]]; then
        send_webhook_alert
    fi

    # Build unified output JSON
    local alerts_json="[]"
    if [[ ${#ALERTS[@]} -gt 0 ]]; then
        local joined
        joined=$(printf '%s,' "${ALERTS[@]}" | sed 's/,$//')
        alerts_json="[${joined}]"
    fi

    local pg_status="unknown"
    local minio_status="unknown"

    if [[ -n "${pg_output}" ]] && command -v python3 >/dev/null 2>&1; then
        pg_status=$(echo "${pg_output}" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    print(data.get('status', 'unknown'))
except Exception:
    print('unknown')
" 2>/dev/null || echo "unknown")
    fi

    if [[ -n "${minio_output}" ]] && command -v python3 >/dev/null 2>&1; then
        minio_status=$(echo "${minio_output}" | python3 -c "
import sys, json
try:
    data = json.load(sys.stdin)
    print(data.get('overall_status', 'unknown'))
except Exception:
    print('unknown')
" 2>/dev/null || echo "unknown")
    fi

    cat <<EOF
{
  "timestamp": "${TIMESTAMP}",
  "overall_status": "${OVERALL_STATUS}",
  "alert_channel": "${ALERT_CHANNEL}",
  "subsystems": {
    "postgresql": {
      "status": "${pg_status}"
    },
    "minio": {
      "status": "${minio_status}"
    }
  },
  "alerts": ${alerts_json},
  "thresholds": {
    "backup_rpo_wal_minutes": ${BACKUP_RPO_WAL_MINUTES},
    "backup_rpo_base_hours": ${BACKUP_RPO_BASE_HOURS},
    "capacity_warning_percent": ${CAPACITY_WARNING_PERCENT},
    "capacity_critical_percent": ${CAPACITY_CRITICAL_PERCENT},
    "backup_retention_days": ${BACKUP_RETENTION_DAYS},
    "minio_retention_days": ${MINIO_BACKUP_RETENTION_DAYS}
  }
}
EOF

    log_info "=== Backup Monitor Completed (status: ${OVERALL_STATUS}) ==="

    # Exit with appropriate code
    case "${OVERALL_STATUS}" in
        healthy)  exit 0 ;;
        warning)  exit 1 ;;
        critical) exit 2 ;;
        *)        exit 3 ;;
    esac
}

main "$@"
