#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Cross-Context Integrity Check Script
# Verifies referential coherence across all bounded contexts after a database
# restore. Checks FK-like relationships that span schema boundaries.
# Outputs structured JSON for monitoring integration.
# =============================================================================

# --- Configuration (from environment) ---
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-haiker}"
PGPASSWORD="${PGPASSWORD:-haiker}"
PGDATABASE="${PGDATABASE:-haiker}"

DATABASE_URL="${DATABASE_URL:-postgresql://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/${PGDATABASE}}"

MINIO_ALIAS="${MINIO_ALIAS:-haiker}"
MINIO_BUCKET="${MINIO_BUCKET:-haiker-artifacts}"
MINIO_SAMPLE_SIZE="${MINIO_SAMPLE_SIZE:-5}"

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

run_sql() {
    psql "${DATABASE_URL}" -t -A -c "$1" 2>/dev/null
}

run_sql_count() {
    local result
    result=$(run_sql "$1")
    echo "${result:-0}"
}

# --- Check 1: Orphan activities (owner_id not in identity.users) ---
check_orphan_activities() {
    log_info "Checking activities with invalid owner references"

    local count
    count=$(run_sql_count "
        SELECT COUNT(*)
        FROM activity_catalog.activities a
        WHERE NOT EXISTS (
            SELECT 1 FROM identity.users u WHERE u.id = a.owner_id
        );
    ")

    if [[ "${count}" -gt 0 ]]; then
        add_check "orphan_activities" "critical" "${count} activities reference non-existent owner_id in identity.users"
    else
        add_check "orphan_activities" "healthy" "All activities reference valid owners"
    fi
}

# --- Check 2: Import references (owner, artifact, activity) ---
check_import_references() {
    log_info "Checking import referential integrity"

    local orphan_owners
    orphan_owners=$(run_sql_count "
        SELECT COUNT(*)
        FROM imports.imports i
        WHERE NOT EXISTS (
            SELECT 1 FROM identity.users u WHERE u.id = i.owner_id
        );
    ")

    local orphan_artifacts
    orphan_artifacts=$(run_sql_count "
        SELECT COUNT(*)
        FROM imports.imports i
        WHERE i.source_artifact_id IS NOT NULL
          AND NOT EXISTS (
            SELECT 1 FROM recorded_activity.source_artifacts sa WHERE sa.id = i.source_artifact_id
        );
    ")

    local orphan_activities
    orphan_activities=$(run_sql_count "
        SELECT COUNT(*)
        FROM imports.imports i
        WHERE i.activity_id IS NOT NULL
          AND NOT EXISTS (
            SELECT 1 FROM activity_catalog.activities a WHERE a.id = i.activity_id
        );
    ")

    local total=$((orphan_owners + orphan_artifacts + orphan_activities))

    if [[ "${total}" -gt 0 ]]; then
        add_check "import_references" "critical" "Import integrity violations: ${orphan_owners} invalid owners, ${orphan_artifacts} invalid source_artifact_ids, ${orphan_activities} invalid activity_ids"
    else
        add_check "import_references" "healthy" "All import references are valid"
    fi
}

# --- Check 3: Source revision references (activity, artifact) ---
check_source_revision_references() {
    log_info "Checking source revision referential integrity"

    local orphan_activities
    orphan_activities=$(run_sql_count "
        SELECT COUNT(*)
        FROM recorded_activity.source_revisions sr
        WHERE NOT EXISTS (
            SELECT 1 FROM activity_catalog.activities a WHERE a.id = sr.activity_id
        );
    ")

    local orphan_artifacts
    orphan_artifacts=$(run_sql_count "
        SELECT COUNT(*)
        FROM recorded_activity.source_revisions sr
        WHERE NOT EXISTS (
            SELECT 1 FROM recorded_activity.source_artifacts sa WHERE sa.id = sr.source_artifact_id
        );
    ")

    local total=$((orphan_activities + orphan_artifacts))

    if [[ "${total}" -gt 0 ]]; then
        add_check "source_revision_references" "critical" "Source revision integrity violations: ${orphan_activities} invalid activity_ids, ${orphan_artifacts} invalid source_artifact_ids"
    else
        add_check "source_revision_references" "healthy" "All source revision references are valid"
    fi
}

# --- Check 4: Recorded track references (source_revision) ---
check_recorded_track_references() {
    log_info "Checking recorded track referential integrity"

    local count
    count=$(run_sql_count "
        SELECT COUNT(*)
        FROM recorded_activity.recorded_tracks rt
        WHERE NOT EXISTS (
            SELECT 1 FROM recorded_activity.source_revisions sr WHERE sr.id = rt.source_revision_id
        );
    ")

    if [[ "${count}" -gt 0 ]]; then
        add_check "recorded_track_references" "critical" "${count} recorded tracks reference non-existent source_revision_id"
    else
        add_check "recorded_track_references" "healthy" "All recorded tracks reference valid source revisions"
    fi
}

# --- Check 5: Route draft references (owner, activity) ---
check_route_draft_references() {
    log_info "Checking route draft referential integrity"

    local orphan_owners
    orphan_owners=$(run_sql_count "
        SELECT COUNT(*)
        FROM route_editing.drafts d
        WHERE NOT EXISTS (
            SELECT 1 FROM identity.users u WHERE u.id = d.owner_id
        );
    ")

    local orphan_activities
    orphan_activities=$(run_sql_count "
        SELECT COUNT(*)
        FROM route_editing.drafts d
        WHERE NOT EXISTS (
            SELECT 1 FROM activity_catalog.activities a WHERE a.id = d.activity_id
        );
    ")

    local total=$((orphan_owners + orphan_activities))

    if [[ "${total}" -gt 0 ]]; then
        add_check "route_draft_references" "critical" "Route draft integrity violations: ${orphan_owners} invalid owner_ids, ${orphan_activities} invalid activity_ids"
    else
        add_check "route_draft_references" "healthy" "All route draft references are valid"
    fi
}

# --- Check 6: current_route_version_id pointers ---
check_current_route_version_pointers() {
    log_info "Checking current_route_version_id pointers in activities"

    # Check if the route_versioning.route_versions table exists
    local table_exists
    table_exists=$(run_sql "
        SELECT COUNT(*)
        FROM information_schema.tables
        WHERE table_schema = 'route_versioning'
          AND table_name = 'route_versions';
    ")

    local activities_with_version
    activities_with_version=$(run_sql_count "
        SELECT COUNT(*)
        FROM activity_catalog.activities
        WHERE current_route_version_id IS NOT NULL;
    ")

    if [[ "${activities_with_version}" -eq 0 ]]; then
        add_check "current_route_version_pointers" "healthy" "No activities have current_route_version_id set"
    elif [[ "${table_exists}" -eq 0 ]]; then
        add_check "current_route_version_pointers" "warning" "${activities_with_version} activities have current_route_version_id set but route_versioning.route_versions table does not exist"
    else
        local orphan_versions
        orphan_versions=$(run_sql_count "
            SELECT COUNT(*)
            FROM activity_catalog.activities a
            WHERE a.current_route_version_id IS NOT NULL
              AND NOT EXISTS (
                SELECT 1 FROM route_versioning.route_versions rv WHERE rv.id = a.current_route_version_id
            );
        ")

        if [[ "${orphan_versions}" -gt 0 ]]; then
            add_check "current_route_version_pointers" "critical" "${orphan_versions} activities reference non-existent route versions"
        else
            add_check "current_route_version_pointers" "healthy" "All current_route_version_id pointers reference valid route versions"
        fi
    fi
}

# --- Check 7: Audit log actor references ---
check_audit_log_actors() {
    log_info "Checking audit log actor references"

    local count
    count=$(run_sql_count "
        SELECT COUNT(*)
        FROM platform.audit_log al
        WHERE NOT EXISTS (
            SELECT 1 FROM identity.users u WHERE u.id = al.actor_id
        );
    ")

    if [[ "${count}" -gt 0 ]]; then
        add_check "audit_log_actors" "warning" "${count} audit log entries reference non-existent actor_id (may include system actors)"
    else
        add_check "audit_log_actors" "healthy" "All audit log actors reference valid users"
    fi
}

# --- Check 8: Artifact storage keys in MinIO (optional) ---
check_artifact_storage_keys() {
    log_info "Checking artifact storage keys in MinIO"

    # Skip if mc (MinIO client) is not available
    if ! command -v mc &>/dev/null; then
        add_check "artifact_storage_keys" "warning" "Skipped: mc (MinIO client) not available"
        return
    fi

    # Check MinIO connectivity
    if ! mc alias list "${MINIO_ALIAS}" &>/dev/null; then
        add_check "artifact_storage_keys" "warning" "Skipped: MinIO alias '${MINIO_ALIAS}' not configured or unreachable"
        return
    fi

    # Get a sample of object_storage_key values
    local keys
    keys=$(run_sql "
        SELECT object_storage_key
        FROM recorded_activity.source_artifacts
        ORDER BY created_at DESC
        LIMIT ${MINIO_SAMPLE_SIZE};
    ")

    if [[ -z "${keys}" ]]; then
        add_check "artifact_storage_keys" "healthy" "No source artifacts to verify"
        return
    fi

    local missing=0
    local checked=0

    while IFS= read -r key; do
        [[ -z "${key}" ]] && continue
        checked=$((checked + 1))
        if ! mc stat "${MINIO_ALIAS}/${MINIO_BUCKET}/${key}" &>/dev/null; then
            missing=$((missing + 1))
        fi
    done <<< "${keys}"

    if [[ "${missing}" -gt 0 ]]; then
        add_check "artifact_storage_keys" "critical" "${missing}/${checked} sampled artifact storage keys not found in MinIO bucket '${MINIO_BUCKET}'"
    else
        add_check "artifact_storage_keys" "healthy" "All ${checked} sampled artifact storage keys exist in MinIO"
    fi
}

# --- Run all checks ---
check_orphan_activities
check_import_references
check_source_revision_references
check_recorded_track_references
check_route_draft_references
check_current_route_version_pointers
check_audit_log_actors
check_artifact_storage_keys

# --- Output structured JSON ---
CHECKS_JSON=$(printf '%s,' "${CHECKS[@]}" | sed 's/,$//')

cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "${OVERALL_STATUS}",
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
