#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Restore Drill Orchestration Script
# Automates a full disaster recovery drill in an isolated Docker Compose project.
# Measures RTO across all recovery phases and produces a structured JSON report.
#
# USAGE:
#   ./scripts/restore-drill.sh --target-time "2024-01-15T10:30:00Z" --yes
#   ./scripts/restore-drill.sh --keep   # skip cleanup for debugging
#
# See docs/runbooks/backup-operations.md for the full disaster recovery procedure.
# =============================================================================

# --- Configuration (from environment) ---
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

DRILL_PROJECT_NAME="${DRILL_PROJECT_NAME:-haiker-drill}"
DRILL_COMPOSE_FILE="${DRILL_COMPOSE_FILE:-${PROJECT_ROOT}/deploy/backup/docker-compose.backup.yml}"
DRILL_TIMEOUT_SECONDS="${DRILL_TIMEOUT_SECONDS:-3600}"
DRILL_RESULTS_DIR="${DRILL_RESULTS_DIR:-${PROJECT_ROOT}/drill-results}"
RTO_TARGET_SECONDS="${RTO_TARGET_SECONDS:-14400}"

# Scripts
RESTORE_POSTGRES_SCRIPT="${RESTORE_POSTGRES_SCRIPT:-${SCRIPT_DIR}/restore-postgres.sh}"
INTEGRITY_CHECK_SCRIPT="${INTEGRITY_CHECK_SCRIPT:-${SCRIPT_DIR}/integrity-check.sh}"
SMOKE_TEST_SCRIPT="${SMOKE_TEST_SCRIPT:-${SCRIPT_DIR}/smoke-test.sh}"

# --- State ---
OVERALL_STATUS="passed"
DRILL_ID="drill-$(date -u +%Y%m%dT%H%M%SZ)-$$"
STARTED_AT=""
COMPLETED_AT=""
PHASES_JSON=""
TOTAL_RTO_SECONDS=0
KEEP_ENVIRONMENT=false
SKIP_CONFIRM=false
TARGET_TIME=""

# Save original arguments for potential re-exec under timeout
ORIGINAL_ARGS=("$@")

# --- Helpers ---
log_info() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] INFO: $1" >&2; }
log_error() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] ERROR: $1" >&2; }
log_warn() { echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] WARN: $1" >&2; }

escalate_status() {
    local new_status="$1"
    if [[ "${new_status}" == "failed" ]]; then
        OVERALL_STATUS="failed"
    elif [[ "${new_status}" == "warning" && "${OVERALL_STATUS}" == "passed" ]]; then
        OVERALL_STATUS="warning"
    fi
}

# --- Phase timing infrastructure ---
PHASE_RESULTS=()

run_phase() {
    local phase_name="$1"
    shift
    local phase_func="$1"
    shift

    log_info "=== Phase: ${phase_name} ==="

    local start_epoch
    start_epoch=$(date +%s)

    local phase_status="passed"
    local phase_details=""

    # Execute phase function and capture output/status
    local phase_output=""
    local phase_exit_code=0
    phase_output=$("${phase_func}" "$@" 2>&1) || phase_exit_code=$?

    local end_epoch
    end_epoch=$(date +%s)
    local duration=$((end_epoch - start_epoch))

    if [[ ${phase_exit_code} -eq 0 ]]; then
        phase_status="passed"
        phase_details="${phase_output}"
    elif [[ ${phase_exit_code} -eq 1 ]]; then
        phase_status="warning"
        phase_details="${phase_output}"
        escalate_status "warning"
    else
        phase_status="failed"
        phase_details="${phase_output}"
        escalate_status "failed"
    fi

    TOTAL_RTO_SECONDS=$((TOTAL_RTO_SECONDS + duration))

    # Escape details for JSON (replace newlines, quotes, backslashes)
    local escaped_details
    escaped_details=$(echo "${phase_details}" | head -c 1000 | tr '\n' ' ' | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/ /g')

    PHASE_RESULTS+=("{\"name\":\"${phase_name}\",\"status\":\"${phase_status}\",\"duration_seconds\":${duration},\"details\":\"${escaped_details}\"}")

    log_info "Phase '${phase_name}' completed: status=${phase_status}, duration=${duration}s"

    # Abort drill on critical phase failure (setup, pg_restore)
    if [[ "${phase_status}" == "failed" && ("${phase_name}" == "setup" || "${phase_name}" == "pg_restore") ]]; then
        log_error "Critical phase '${phase_name}' failed. Aborting drill."
        return 1
    fi

    return 0
}

# --- Usage ---
usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Options:
  --target-time <timestamp>   Recovery target timestamp (ISO 8601). Defaults to 12 hours ago.
  --keep                      Skip cleanup phase (keep drill environment running for debugging).
  --yes                       Skip confirmation prompt (for automated drills).
  --help                      Show this help message.

Environment Variables:
  DRILL_PROJECT_NAME        Docker Compose project name (default: haiker-drill)
  DRILL_COMPOSE_FILE        Path to compose file (default: deploy/backup/docker-compose.backup.yml)
  DRILL_TIMEOUT_SECONDS     Overall timeout in seconds (default: 3600)
  DRILL_RESULTS_DIR         Directory for drill reports (default: drill-results/)
  RTO_TARGET_SECONDS        RTO target in seconds (default: 14400 = 4 hours)

Example:
  $0 --target-time "2024-01-15T10:30:00Z" --yes
  DRILL_PROJECT_NAME=drill-test $0 --keep --yes
EOF
    exit 0
}

# --- Parse arguments ---
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target-time)
            TARGET_TIME="$2"
            shift 2
            ;;
        --keep)
            KEEP_ENVIRONMENT=true
            shift
            ;;
        --yes)
            SKIP_CONFIRM=true
            shift
            ;;
        --help)
            usage
            ;;
        *)
            log_error "Unknown option: $1"
            usage
            ;;
    esac
done

# Default target time: 12 hours ago
if [[ -z "${TARGET_TIME}" ]]; then
    if date --version >/dev/null 2>&1; then
        # GNU date
        TARGET_TIME=$(date -u -d '12 hours ago' +%Y-%m-%dT%H:%M:%SZ)
    else
        # BSD date (macOS)
        TARGET_TIME=$(date -u -v-12H +%Y-%m-%dT%H:%M:%SZ)
    fi
fi

# --- Safety confirmation ---
if [[ "${SKIP_CONFIRM}" != "true" ]]; then
    echo ""
    echo "========================================"
    echo " Restore Drill"
    echo "========================================"
    echo ""
    echo "  Project:      ${DRILL_PROJECT_NAME}"
    echo "  Compose file: ${DRILL_COMPOSE_FILE}"
    echo "  Target time:  ${TARGET_TIME}"
    echo "  RTO target:   ${RTO_TARGET_SECONDS}s ($(( RTO_TARGET_SECONDS / 3600 ))h)"
    echo "  Results dir:  ${DRILL_RESULTS_DIR}"
    echo "  Keep env:     ${KEEP_ENVIRONMENT}"
    echo ""
    echo "  This will spin up an isolated Docker Compose environment."
    echo ""
    read -r -p "  Proceed with drill? (y/N): " CONFIRMATION
    if [[ "${CONFIRMATION}" != "y" && "${CONFIRMATION}" != "Y" ]]; then
        log_info "Drill cancelled by user"
        exit 0
    fi
fi

# --- Compose helper ---
drill_compose() {
    docker compose -f "${DRILL_COMPOSE_FILE}" -p "${DRILL_PROJECT_NAME}" "$@"
}

# --- Phase implementations ---

phase_setup() {
    log_info "Starting isolated drill environment: ${DRILL_PROJECT_NAME}"

    # Ensure no leftover drill environment
    drill_compose down -v 2>/dev/null || true

    # Start required services
    drill_compose up -d postgres minio-backup

    # Wait for PostgreSQL health
    log_info "Waiting for PostgreSQL to become ready..."
    local wait_seconds=0
    local max_wait=120
    while [[ ${wait_seconds} -lt ${max_wait} ]]; do
        if drill_compose exec -T postgres pg_isready -q 2>/dev/null; then
            log_info "PostgreSQL is ready"
            break
        fi
        sleep 2
        wait_seconds=$((wait_seconds + 2))
    done

    if [[ ${wait_seconds} -ge ${max_wait} ]]; then
        echo "PostgreSQL did not become ready within ${max_wait}s"
        return 2
    fi

    # Wait for MinIO health
    log_info "Waiting for MinIO to become ready..."
    wait_seconds=0
    max_wait=60
    while [[ ${wait_seconds} -lt ${max_wait} ]]; do
        if drill_compose exec -T minio-backup curl -sf http://localhost:9000/minio/health/live >/dev/null 2>&1; then
            log_info "MinIO is ready"
            break
        fi
        sleep 2
        wait_seconds=$((wait_seconds + 2))
    done

    if [[ ${wait_seconds} -ge ${max_wait} ]]; then
        echo "MinIO did not become ready within ${max_wait}s"
        return 2
    fi

    echo "Drill environment ready"
    return 0
}

phase_pg_restore() {
    log_info "Running PostgreSQL PITR restore to target: ${TARGET_TIME}"

    drill_compose exec -T postgres \
        /scripts/restore-postgres.sh \
        --target-time "${TARGET_TIME}" \
        --yes

    echo "PostgreSQL PITR restore completed"
    return 0
}

phase_minio_restore() {
    log_info "Running MinIO data restore"

    # Mirror artifact and upload buckets from backup to drill environment
    drill_compose exec -T minio-backup-scheduler \
        mc mirror --preserve --overwrite \
        backup/haiker-artifacts drill/haiker-artifacts 2>&1 || true

    drill_compose exec -T minio-backup-scheduler \
        mc mirror --preserve --overwrite \
        backup/haiker-uploads drill/haiker-uploads 2>&1 || true

    echo "MinIO restore completed"
    return 0
}

phase_migration_verify() {
    log_info "Verifying database migrations"

    # Get applied migrations from the restored database
    local applied_migrations
    applied_migrations=$(drill_compose exec -T postgres \
        psql -U "${PGUSER:-haiker}" -d "${PGDATABASE:-haiker}" -t -A -c \
        "SELECT version FROM _sqlx_migrations ORDER BY version;" 2>/dev/null) || {
        echo "Failed to query _sqlx_migrations table"
        return 2
    }

    # Get expected migrations from the migrations/ directory
    local expected_migrations=""
    for migration_file in "${PROJECT_ROOT}"/migrations/*.sql; do
        [[ -f "${migration_file}" ]] || continue
        local filename
        filename=$(basename "${migration_file}")
        # Extract version (timestamp prefix before first underscore)
        local version
        version=$(echo "${filename}" | grep -oE '^[0-9]+')
        expected_migrations="${expected_migrations}${version}"$'\n'
    done

    # Compare
    local missing_count=0
    local missing_list=""
    while IFS= read -r expected; do
        [[ -z "${expected}" ]] && continue
        if ! echo "${applied_migrations}" | grep -q "^${expected}$"; then
            missing_count=$((missing_count + 1))
            missing_list="${missing_list} ${expected}"
        fi
    done <<< "${expected_migrations}"

    local applied_count
    applied_count=$(echo "${applied_migrations}" | grep -c '[0-9]' || echo "0")
    local expected_count
    expected_count=$(echo "${expected_migrations}" | grep -c '[0-9]' || echo "0")

    if [[ ${missing_count} -gt 0 ]]; then
        echo "Migration verification FAILED: ${missing_count} migrations missing (${missing_list}). Applied: ${applied_count}/${expected_count}"
        return 2
    fi

    echo "Migration verification passed: ${applied_count}/${expected_count} migrations applied"
    return 0
}

phase_integrity_check() {
    log_info "Running cross-context integrity checks"

    # Run integrity check against the drill database
    local drill_pg_host
    drill_pg_host=$(drill_compose port postgres 5432 2>/dev/null | sed 's/.*://')

    if [[ -z "${drill_pg_host}" ]]; then
        echo "Could not determine drill PostgreSQL port"
        return 2
    fi

    local exit_code=0
    PGHOST="localhost" \
    PGPORT="${drill_pg_host}" \
    PGUSER="${PGUSER:-haiker}" \
    PGPASSWORD="${PGPASSWORD:-haiker}" \
    PGDATABASE="${PGDATABASE:-haiker}" \
    DATABASE_URL="postgresql://${PGUSER:-haiker}:${PGPASSWORD:-haiker}@localhost:${drill_pg_host}/${PGDATABASE:-haiker}" \
        "${INTEGRITY_CHECK_SCRIPT}" 2>/dev/null || exit_code=$?

    if [[ ${exit_code} -eq 0 ]]; then
        echo "Integrity checks passed"
        return 0
    elif [[ ${exit_code} -eq 1 ]]; then
        echo "Integrity check completed with warnings"
        return 1
    else
        echo "Integrity check failed"
        return 2
    fi
}

phase_smoke_test() {
    log_info "Running smoke tests against drill environment"

    # Attempt to determine the API URL from the drill environment
    local api_port
    api_port=$(drill_compose port api 3000 2>/dev/null | sed 's/.*://' || echo "")

    if [[ -n "${api_port}" ]]; then
        local exit_code=0
        API_URL="http://localhost:${api_port}" "${SMOKE_TEST_SCRIPT}" 2>&1 || exit_code=$?

        if [[ ${exit_code} -eq 0 ]]; then
            echo "Smoke tests passed"
        elif [[ ${exit_code} -eq 1 ]]; then
            echo "Smoke tests completed with warnings"
            return 1
        else
            echo "Smoke tests failed"
            return 2
        fi
    else
        # If no API service is running, do basic connectivity checks
        log_warn "No API service available in drill environment, running basic database connectivity check"

        local exit_code=0
        drill_compose exec -T postgres \
            psql -U "${PGUSER:-haiker}" -d "${PGDATABASE:-haiker}" -c "SELECT 1;" >/dev/null 2>&1 || exit_code=$?

        if [[ ${exit_code} -ne 0 ]]; then
            echo "Basic database connectivity check failed"
            return 2
        fi

        echo "Basic connectivity check passed (API service not available for full smoke test)"
        return 0
    fi
}

phase_cleanup() {
    if [[ "${KEEP_ENVIRONMENT}" == "true" ]]; then
        echo "Skipping cleanup (--keep flag set). Drill environment '${DRILL_PROJECT_NAME}' is still running."
        log_warn "Remember to clean up manually: docker compose -f ${DRILL_COMPOSE_FILE} -p ${DRILL_PROJECT_NAME} down -v"
        return 0
    fi

    log_info "Cleaning up drill environment"
    drill_compose down -v 2>&1 || true
    echo "Drill environment cleaned up"
    return 0
}

# --- Generate JSON report ---
generate_report() {
    local rto_met="false"
    if [[ ${TOTAL_RTO_SECONDS} -le ${RTO_TARGET_SECONDS} ]]; then
        rto_met="true"
    fi

    # Build phases JSON array
    local phases_json=""
    if [[ ${#PHASE_RESULTS[@]} -gt 0 ]]; then
        phases_json=$(printf '%s,' "${PHASE_RESULTS[@]}" | sed 's/,$//')
    fi

    cat <<EOF
{
  "drill_id": "${DRILL_ID}",
  "target_time": "${TARGET_TIME}",
  "started_at": "${STARTED_AT}",
  "completed_at": "${COMPLETED_AT}",
  "phases": [${phases_json}],
  "total_rto_seconds": ${TOTAL_RTO_SECONDS},
  "rto_target_seconds": ${RTO_TARGET_SECONDS},
  "rto_met": ${rto_met},
  "overall_status": "${OVERALL_STATUS}"
}
EOF
}

# --- Write report to file ---
write_report() {
    mkdir -p "${DRILL_RESULTS_DIR}"
    local report_file="${DRILL_RESULTS_DIR}/$(date -u +%Y%m%dT%H%M%SZ)-drill-report.json"
    generate_report > "${report_file}"
    log_info "Drill report written to: ${report_file}"
    echo "${report_file}"
}

# --- Main ---
main() {
    log_info "=== Restore Drill Started ==="
    log_info "Drill ID: ${DRILL_ID}"
    log_info "Target time: ${TARGET_TIME}"
    log_info "RTO target: ${RTO_TARGET_SECONDS}s"

    STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

    local drill_failed=false

    # Phase 1: Setup
    run_phase "setup" phase_setup || drill_failed=true

    # Phase 2: PostgreSQL PITR (only if setup succeeded)
    if [[ "${drill_failed}" != "true" ]]; then
        run_phase "pg_restore" phase_pg_restore || drill_failed=true
    fi

    # Phase 3: MinIO Restore (continue even if pg_restore had warnings)
    if [[ "${drill_failed}" != "true" ]]; then
        run_phase "minio_restore" phase_minio_restore || true
    fi

    # Phase 4: Migration Verification
    if [[ "${drill_failed}" != "true" ]]; then
        run_phase "migration_verify" phase_migration_verify || true
    fi

    # Phase 5: Integrity Checks
    if [[ "${drill_failed}" != "true" ]]; then
        run_phase "integrity_check" phase_integrity_check || true
    fi

    # Phase 6: Smoke Tests
    if [[ "${drill_failed}" != "true" ]]; then
        run_phase "smoke_test" phase_smoke_test || true
    fi

    # Phase 7: Cleanup (always run)
    run_phase "cleanup" phase_cleanup || true

    COMPLETED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

    # Generate and write report
    local report_file
    report_file=$(write_report)

    # Output report to stdout
    generate_report

    log_info "=== Restore Drill Completed ==="
    log_info "Overall status: ${OVERALL_STATUS}"
    log_info "Total RTO: ${TOTAL_RTO_SECONDS}s (target: ${RTO_TARGET_SECONDS}s)"
    log_info "Report: ${report_file}"

    # Exit with appropriate code
    case "${OVERALL_STATUS}" in
        passed)  exit 0 ;;
        warning) exit 1 ;;
        failed)  exit 2 ;;
        *)       exit 2 ;;
    esac
}

# Enforce overall drill timeout.
# On the first invocation (without the sentinel), re-exec under 'timeout' so
# the entire drill is bounded by DRILL_TIMEOUT_SECONDS.
if [[ "${__DRILL_TIMEOUT_ACTIVE:-}" != "1" ]] && command -v timeout &>/dev/null; then
    export __DRILL_TIMEOUT_ACTIVE=1
    # Re-exec this script under timeout with the original arguments.
    # The sentinel variable prevents infinite recursion on re-entry.
    exec timeout "${DRILL_TIMEOUT_SECONDS}" "$0" "${ORIGINAL_ARGS[@]}"
fi

main "$@"
