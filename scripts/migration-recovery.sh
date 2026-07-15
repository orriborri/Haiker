#!/usr/bin/env bash
set -euo pipefail

# --- Bash version check (declare -A requires bash 4.0+) ---
if [[ "${BASH_VERSINFO[0]}" -lt 4 ]]; then
    echo "ERROR: This script requires bash 4.0+ (for associative arrays). Current: ${BASH_VERSION}" >&2
    echo "On macOS, install a newer bash via Homebrew: brew install bash" >&2
    exit 1
fi

# =============================================================================
# Migration Recovery Script
# Provides subcommands for diagnosing, verifying, rolling back, and forward-fixing
# SQLx database migrations.
#
# SUBCOMMANDS:
#   status      Show all applied migrations from _sqlx_migrations
#   verify      Compare applied migrations against migration files on disk
#   rollback    Roll back the last N migrations (uses .down.sql if available)
#   forward-fix Re-run pending/failed migrations with pre-flight checks
#
# USAGE:
#   scripts/migration-recovery.sh status
#   scripts/migration-recovery.sh verify
#   scripts/migration-recovery.sh rollback [--count N] [--dry-run] [--yes]
#   scripts/migration-recovery.sh forward-fix [--yes]
#
# See docs/runbooks/migration-recovery.md for detailed procedures.
# =============================================================================

# --- Configuration (from environment) ---
PGHOST="${PGHOST:-localhost}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-haiker}"
PGPASSWORD="${PGPASSWORD:-haiker}"
PGDATABASE="${PGDATABASE:-haiker}"
MIGRATIONS_DIR="${MIGRATIONS_DIR:-migrations/}"

# Build DATABASE_URL if not already set
if [[ -z "${DATABASE_URL:-}" ]]; then
    export DATABASE_URL="postgres://${PGUSER}:${PGPASSWORD}@${PGHOST}:${PGPORT}/${PGDATABASE}"
fi

# Export for psql
export PGHOST PGPORT PGUSER PGPASSWORD PGDATABASE

# --- Logging helpers ---
log_info() { echo "{\"level\":\"info\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}" >&2; }
log_error() { echo "{\"level\":\"error\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}" >&2; }
log_warn() { echo "{\"level\":\"warn\",\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"message\":\"$1\"}" >&2; }

# --- Usage ---
usage() {
    cat <<EOF
Usage: $0 <subcommand> [options]

Subcommands:
  status        Show all applied migrations from _sqlx_migrations (JSON output)
  verify        Compare applied migrations against files in migrations/ directory
  rollback      Roll back the last N migrations
  forward-fix   Re-run pending migrations with pre-flight checks

Options for 'rollback':
  --count N     Number of migrations to roll back (default: 1)
  --dry-run     Show what would be done without executing
  --yes         Skip confirmation prompt

Options for 'forward-fix':
  --yes         Skip confirmation prompt

Environment variables:
  PGHOST        PostgreSQL host (default: localhost)
  PGPORT        PostgreSQL port (default: 5432)
  PGUSER        PostgreSQL user (default: haiker)
  PGPASSWORD    PostgreSQL password (default: haiker)
  PGDATABASE    PostgreSQL database (default: haiker)
  DATABASE_URL  Full connection string (overrides individual PG* vars)
  MIGRATIONS_DIR  Path to migrations directory (default: migrations/)
EOF
    exit 1
}

# --- Pre-flight: Check database connectivity ---
check_connectivity() {
    log_info "Checking database connectivity to ${PGHOST}:${PGPORT}/${PGDATABASE}"
    if ! psql -c "SELECT 1;" > /dev/null 2>&1; then
        log_error "Cannot connect to PostgreSQL at ${PGHOST}:${PGPORT}/${PGDATABASE}"
        echo "{\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"status\":\"error\",\"error\":\"Cannot connect to database\"}"
        exit 1
    fi
    log_info "Database connection successful"
}

# --- Pre-flight: Check _sqlx_migrations table exists ---
check_migrations_table() {
    local exists
    exists=$(psql -tAc "SELECT EXISTS(SELECT 1 FROM information_schema.tables WHERE table_name = '_sqlx_migrations');" 2>/dev/null)
    if [[ "${exists}" != "t" ]]; then
        log_error "_sqlx_migrations table does not exist. Has SQLx ever been run against this database?"
        echo "{\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"status\":\"error\",\"error\":\"_sqlx_migrations table not found\"}"
        exit 1
    fi
}

# =============================================================================
# SUBCOMMAND: status
# Query _sqlx_migrations and output JSON with all migration records.
# =============================================================================
cmd_status() {
    check_connectivity
    check_migrations_table

    log_info "Querying _sqlx_migrations table"

    local result
    result=$(psql -tAc "
        SELECT json_agg(
            json_build_object(
                'version', version,
                'description', description,
                'installed_on', installed_on,
                'success', success,
                'checksum', encode(checksum, 'hex'),
                'execution_time', execution_time
            ) ORDER BY version
        )
        FROM _sqlx_migrations;
    " 2>/dev/null)

    # Handle null (no migrations)
    if [[ -z "${result}" ]] || [[ "${result}" == "null" ]] || [[ "${result}" == "" ]]; then
        result="[]"
    fi

    local total_count
    total_count=$(psql -tAc "SELECT count(*) FROM _sqlx_migrations;" 2>/dev/null)

    local failed_count
    failed_count=$(psql -tAc "SELECT count(*) FROM _sqlx_migrations WHERE success = false;" 2>/dev/null)

    cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "ok",
  "total_migrations": ${total_count:-0},
  "failed_migrations": ${failed_count:-0},
  "migrations": ${result}
}
EOF
}

# =============================================================================
# SUBCOMMAND: verify
# Compare applied migrations against files in the migrations/ directory.
# Report: applied, pending, failed, checksum mismatches.
# =============================================================================
cmd_verify() {
    check_connectivity
    check_migrations_table

    log_info "Verifying migrations against files in ${MIGRATIONS_DIR}"

    if [[ ! -d "${MIGRATIONS_DIR}" ]]; then
        log_error "Migrations directory not found: ${MIGRATIONS_DIR}"
        echo "{\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"status\":\"error\",\"error\":\"Migrations directory not found: ${MIGRATIONS_DIR}\"}"
        exit 1
    fi

    # Get applied migrations from database
    local db_migrations
    db_migrations=$(psql -tAc "
        SELECT version || '|' || description || '|' || success || '|' || encode(checksum, 'hex')
        FROM _sqlx_migrations
        ORDER BY version;
    " 2>/dev/null)

    # Get migration files from disk
    local applied=()
    local pending=()
    local failed=()
    local checksum_mismatches=()
    local overall_status="ok"

    # Build associative-like lookup from database records
    declare -A db_versions=()
    declare -A db_success=()
    declare -A db_checksums=()

    while IFS='|' read -r version description success checksum; do
        [[ -z "${version}" ]] && continue
        db_versions["${version}"]="${description}"
        db_success["${version}"]="${success}"
        db_checksums["${version}"]="${checksum}"
    done <<< "${db_migrations}"

    # Iterate over migration files on disk
    for file in "${MIGRATIONS_DIR}"/*.sql; do
        [[ -f "${file}" ]] || continue
        local basename
        basename=$(basename "${file}")

        # Extract version number from filename (YYYYMMDDHHMMSS prefix)
        local version_str
        version_str=$(echo "${basename}" | grep -oE '^[0-9]+' || true)
        [[ -z "${version_str}" ]] && continue

        # Remove leading zeros for numeric comparison (SQLx stores as BIGINT)
        local version
        version=$(echo "${version_str}" | sed 's/^0*//' || echo "0")

        # Extract description from filename (after underscore, before .sql)
        local description
        description=$(echo "${basename}" | sed 's/^[0-9]*_//; s/\.sql$//')

        if [[ -n "${db_versions[${version}]:-}" ]]; then
            # Migration is in the database
            if [[ "${db_success[${version}]}" == "f" ]]; then
                failed+=("{\"version\":${version},\"description\":\"${description}\",\"file\":\"${basename}\"}")
                overall_status="failed"
            else
                applied+=("{\"version\":${version},\"description\":\"${description}\",\"file\":\"${basename}\"}")
            fi

            # Check checksum (compute sha384 of file content for SQLx)
            local file_checksum
            file_checksum=$(sha384sum "${file}" 2>/dev/null | awk '{print $1}' || shasum -a 384 "${file}" 2>/dev/null | awk '{print $1}' || echo "")

            if [[ -n "${file_checksum}" ]] && [[ -n "${db_checksums[${version}]:-}" ]]; then
                if [[ "${file_checksum}" != "${db_checksums[${version}]}" ]]; then
                    checksum_mismatches+=("{\"version\":${version},\"description\":\"${description}\",\"file\":\"${basename}\",\"expected\":\"${db_checksums[${version}]}\",\"actual\":\"${file_checksum}\"}")
                    overall_status="checksum_mismatch"
                fi
            fi
        else
            # Migration file exists but is not in the database (pending)
            pending+=("{\"version\":${version},\"description\":\"${description}\",\"file\":\"${basename}\"}")
        fi
    done

    # Check for extra migrations in database that have no file on disk
    local orphaned=()
    for version in "${!db_versions[@]}"; do
        local found=false
        for file in "${MIGRATIONS_DIR}"/*.sql; do
            [[ -f "${file}" ]] || continue
            local file_version
            file_version=$(basename "${file}" | grep -oE '^[0-9]+' | sed 's/^0*//' || true)
            if [[ "${file_version}" == "${version}" ]]; then
                found=true
                break
            fi
        done
        if [[ "${found}" == "false" ]]; then
            orphaned+=("{\"version\":${version},\"description\":\"${db_versions[${version}]}\"}")
            if [[ "${overall_status}" == "ok" ]]; then
                overall_status="orphaned"
            fi
        fi
    done

    # Build JSON arrays
    local applied_json pending_json failed_json mismatch_json orphaned_json
    applied_json=$(printf '%s,' "${applied[@]}" 2>/dev/null | sed 's/,$//' || echo "")
    pending_json=$(printf '%s,' "${pending[@]}" 2>/dev/null | sed 's/,$//' || echo "")
    failed_json=$(printf '%s,' "${failed[@]}" 2>/dev/null | sed 's/,$//' || echo "")
    mismatch_json=$(printf '%s,' "${checksum_mismatches[@]}" 2>/dev/null | sed 's/,$//' || echo "")
    orphaned_json=$(printf '%s,' "${orphaned[@]}" 2>/dev/null | sed 's/,$//' || echo "")

    cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "${overall_status}",
  "summary": {
    "applied": ${#applied[@]},
    "pending": ${#pending[@]},
    "failed": ${#failed[@]},
    "checksum_mismatches": ${#checksum_mismatches[@]},
    "orphaned": ${#orphaned[@]}
  },
  "applied": [${applied_json}],
  "pending": [${pending_json}],
  "failed": [${failed_json}],
  "checksum_mismatches": [${mismatch_json}],
  "orphaned": [${orphaned_json}]
}
EOF

    if [[ "${overall_status}" != "ok" ]]; then
        exit 1
    fi
}

# =============================================================================
# SUBCOMMAND: rollback
# Roll back the last N migrations. Uses .down.sql files if available.
# If no .down.sql exists, outputs manual SQL guidance.
# =============================================================================
cmd_rollback() {
    local count=1
    local dry_run=false
    local skip_confirm=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --count)
                count="$2"
                shift 2
                ;;
            --dry-run)
                dry_run=true
                shift
                ;;
            --yes)
                skip_confirm=true
                shift
                ;;
            *)
                log_error "Unknown rollback option: $1"
                usage
                ;;
        esac
    done

    check_connectivity
    check_migrations_table

    log_info "Rolling back last ${count} migration(s)"

    # Get the last N applied migrations (ordered by version descending)
    local migrations
    migrations=$(psql -tAc "
        SELECT version || '|' || description
        FROM _sqlx_migrations
        WHERE success = true
        ORDER BY version DESC
        LIMIT ${count};
    " 2>/dev/null)

    if [[ -z "${migrations}" ]]; then
        log_warn "No successful migrations found to roll back"
        echo "{\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"status\":\"warning\",\"message\":\"No migrations to roll back\"}"
        exit 0
    fi

    # Collect rollback plan
    local plan=()
    local has_down_sql=()
    local manual_guidance=()

    while IFS='|' read -r version description; do
        [[ -z "${version}" ]] && continue

        # Find the corresponding migration file
        local migration_file=""
        local down_file=""
        for file in "${MIGRATIONS_DIR}"/*.sql; do
            [[ -f "${file}" ]] || continue
            local file_version
            file_version=$(basename "${file}" | grep -oE '^[0-9]+' | sed 's/^0*//' || true)
            if [[ "${file_version}" == "${version}" ]]; then
                migration_file="${file}"
                # Check for a .down.sql companion
                down_file="${file%.sql}.down.sql"
                break
            fi
        done

        if [[ -n "${down_file}" ]] && [[ -f "${down_file}" ]]; then
            plan+=("${version}|${description}|down_sql|${down_file}")
            has_down_sql+=("true")
        else
            # Generate manual guidance based on migration content
            local guidance=""
            if [[ -n "${migration_file}" ]] && [[ -f "${migration_file}" ]]; then
                guidance=$(generate_rollback_guidance "${migration_file}")
            else
                guidance="-- Migration file not found on disk. Check _sqlx_migrations for version ${version}."
            fi
            plan+=("${version}|${description}|manual|${guidance}")
            has_down_sql+=("false")
        fi
    done <<< "${migrations}"

    # Display plan
    if [[ "${dry_run}" == "true" ]]; then
        log_info "DRY RUN - showing rollback plan without executing"
    fi

    local plan_entries=()
    local idx=0
    for entry in "${plan[@]}"; do
        IFS='|' read -r version description method detail <<< "${entry}"
        if [[ "${method}" == "down_sql" ]]; then
            plan_entries+=("{\"version\":${version},\"description\":\"${description}\",\"method\":\"down_sql\",\"file\":\"${detail}\"}")
        else
            plan_entries+=("{\"version\":${version},\"description\":\"${description}\",\"method\":\"manual\",\"guidance\":\"${detail}\"}")
        fi
        idx=$((idx + 1))
    done

    local plan_json
    plan_json=$(printf '%s,' "${plan_entries[@]}" | sed 's/,$//')

    if [[ "${dry_run}" == "true" ]]; then
        cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "dry_run",
  "message": "Rollback plan (not executed)",
  "rollback_count": ${#plan[@]},
  "plan": [${plan_json}]
}
EOF
        exit 0
    fi

    # Confirmation
    if [[ "${skip_confirm}" != "true" ]]; then
        echo ""
        echo "========================================"
        echo " Migration Rollback"
        echo "========================================"
        echo ""
        echo "  Migrations to roll back: ${#plan[@]}"
        echo ""
        for entry in "${plan[@]}"; do
            IFS='|' read -r version description method detail <<< "${entry}"
            echo "  - [${version}] ${description} (${method})"
        done
        echo ""
        echo "  WARNING: This will modify the database schema!"
        echo ""
        read -r -p "  Type 'ROLLBACK' to confirm: " CONFIRMATION
        if [[ "${CONFIRMATION}" != "ROLLBACK" ]]; then
            log_info "Rollback cancelled by user"
            exit 0
        fi
    fi

    # Execute rollback
    local results=()
    for entry in "${plan[@]}"; do
        IFS='|' read -r version description method detail <<< "${entry}"

        if [[ "${method}" == "down_sql" ]]; then
            log_info "Executing down migration for version ${version}: ${detail}"
            if psql -f "${detail}" 2>&1; then
                # Remove from _sqlx_migrations
                psql -c "DELETE FROM _sqlx_migrations WHERE version = ${version};" > /dev/null 2>&1
                results+=("{\"version\":${version},\"description\":\"${description}\",\"status\":\"rolled_back\"}")
                log_info "Successfully rolled back migration ${version}"
            else
                results+=("{\"version\":${version},\"description\":\"${description}\",\"status\":\"failed\"}")
                log_error "Failed to roll back migration ${version}"
            fi
        else
            log_warn "No .down.sql file for migration ${version} (${description})"
            log_warn "Manual rollback SQL guidance:"
            echo "${detail}" >&2
            echo "" >&2
            log_warn "After manually executing the rollback SQL, remove the migration record:"
            log_warn "  DELETE FROM _sqlx_migrations WHERE version = ${version};"
            results+=("{\"version\":${version},\"description\":\"${description}\",\"status\":\"manual_required\",\"guidance\":\"${detail}\"}")
        fi
    done

    local results_json
    results_json=$(printf '%s,' "${results[@]}" | sed 's/,$//')

    cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "completed",
  "results": [${results_json}]
}
EOF
}

# --- Helper: Generate rollback guidance from a migration file ---
generate_rollback_guidance() {
    local file="$1"
    local content
    content=$(cat "${file}")

    local guidance=""

    # Detect CREATE TABLE statements (process all occurrences)
    if echo "${content}" | grep -qi "CREATE TABLE"; then
        local tables
        tables=$(echo "${content}" | grep -i "CREATE TABLE" | sed -E 's/.*CREATE TABLE[[:space:]]+(IF NOT EXISTS[[:space:]]+)?([^[:space:](]+).*/\2/i')
        while IFS= read -r tbl; do
            [[ -z "${tbl}" ]] && continue
            guidance="${guidance}DROP TABLE IF EXISTS ${tbl} CASCADE; "
        done <<< "${tables}"
    fi

    # Detect ALTER TABLE ADD COLUMN statements (process all occurrences)
    if echo "${content}" | grep -qi "ALTER TABLE.*ADD COLUMN"; then
        while IFS= read -r line; do
            [[ -z "${line}" ]] && continue
            local table col
            table=$(echo "${line}" | sed -E 's/.*ALTER TABLE[[:space:]]+([^[:space:]]+).*/\1/i')
            col=$(echo "${line}" | sed -E 's/.*ADD COLUMN[[:space:]]+([^[:space:]]+).*/\1/i')
            guidance="${guidance}ALTER TABLE ${table} DROP COLUMN IF EXISTS ${col}; "
        done <<< "$(echo "${content}" | grep -i "ALTER TABLE.*ADD COLUMN")"
    fi

    # Detect CREATE INDEX statements (process all occurrences)
    if echo "${content}" | grep -qi "CREATE INDEX"; then
        local indexes
        indexes=$(echo "${content}" | grep -i "CREATE INDEX" | sed -E 's/.*CREATE INDEX[[:space:]]+(IF NOT EXISTS[[:space:]]+)?([^[:space:]]+).*/\2/i')
        while IFS= read -r idx; do
            [[ -z "${idx}" ]] && continue
            guidance="${guidance}DROP INDEX IF EXISTS ${idx}; "
        done <<< "${indexes}"
    fi

    # Detect CREATE SCHEMA statements (process all occurrences)
    if echo "${content}" | grep -qi "CREATE SCHEMA"; then
        local schemas
        schemas=$(echo "${content}" | grep -i "CREATE SCHEMA" | sed -E 's/.*CREATE SCHEMA[[:space:]]+(IF NOT EXISTS[[:space:]]+)?([^[:space:];]+).*/\2/i')
        while IFS= read -r schema; do
            [[ -z "${schema}" ]] && continue
            guidance="${guidance}DROP SCHEMA IF EXISTS ${schema} CASCADE; "
        done <<< "${schemas}"
    fi

    # Fallback if no patterns matched
    if [[ -z "${guidance}" ]]; then
        guidance="-- Review migration file manually: ${file}"
    fi

    # Escape quotes for JSON embedding
    echo "${guidance}" | tr '\n' ' ' | sed 's/"/\\"/g'
}

# =============================================================================
# SUBCOMMAND: forward-fix
# Re-run pending migrations using cargo sqlx migrate run.
# Includes pre-flight checks for connectivity and current state.
# =============================================================================
cmd_forward_fix() {
    local skip_confirm=false

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --yes)
                skip_confirm=true
                shift
                ;;
            *)
                log_error "Unknown forward-fix option: $1"
                usage
                ;;
        esac
    done

    # Pre-flight check 1: Database connectivity
    check_connectivity
    check_migrations_table

    # Pre-flight check 2: Current state verification
    log_info "Running pre-flight state verification"

    local failed_count
    failed_count=$(psql -tAc "SELECT count(*) FROM _sqlx_migrations WHERE success = false;" 2>/dev/null)

    if [[ "${failed_count}" -gt 0 ]]; then
        log_warn "Found ${failed_count} failed migration(s) in _sqlx_migrations"
        log_warn "Failed migrations must be resolved before running forward-fix"

        local failed_versions
        failed_versions=$(psql -tAc "
            SELECT version || ' (' || description || ')'
            FROM _sqlx_migrations
            WHERE success = false
            ORDER BY version;
        " 2>/dev/null)

        echo "{\"timestamp\":\"$(date -u +%Y-%m-%dT%H:%M:%SZ)\",\"status\":\"blocked\",\"message\":\"Failed migrations must be resolved first\",\"failed_migrations\":\"${failed_versions}\"}"
        echo "" >&2
        log_warn "To resolve failed migrations, either:"
        log_warn "  1. Fix the migration SQL and delete the failed record:"
        log_warn "     DELETE FROM _sqlx_migrations WHERE version = <version> AND success = false;"
        log_warn "  2. Mark it as manually resolved:"
        log_warn "     UPDATE _sqlx_migrations SET success = true WHERE version = <version>;"
        exit 1
    fi

    # Pre-flight check 3: Identify pending migrations
    if [[ ! -d "${MIGRATIONS_DIR}" ]]; then
        log_error "Migrations directory not found: ${MIGRATIONS_DIR}"
        exit 1
    fi

    local pending_count=0
    local pending_files=()

    for file in "${MIGRATIONS_DIR}"/*.sql; do
        [[ -f "${file}" ]] || continue
        local file_version
        file_version=$(basename "${file}" | grep -oE '^[0-9]+' | sed 's/^0*//' || true)
        [[ -z "${file_version}" ]] && continue

        local in_db
        in_db=$(psql -tAc "SELECT count(*) FROM _sqlx_migrations WHERE version = ${file_version};" 2>/dev/null)
        if [[ "${in_db}" == "0" ]]; then
            pending_count=$((pending_count + 1))
            pending_files+=("$(basename "${file}")")
        fi
    done

    if [[ "${pending_count}" -eq 0 ]]; then
        log_info "No pending migrations found. Database is up to date."
        cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "ok",
  "message": "No pending migrations. Database is up to date.",
  "pending_count": 0
}
EOF
        exit 0
    fi

    log_info "Found ${pending_count} pending migration(s)"

    # Confirmation
    if [[ "${skip_confirm}" != "true" ]]; then
        echo ""
        echo "========================================"
        echo " Migration Forward-Fix"
        echo "========================================"
        echo ""
        echo "  Pending migrations: ${pending_count}"
        echo ""
        for pf in "${pending_files[@]}"; do
            echo "  - ${pf}"
        done
        echo ""
        echo "  This will apply the pending migrations to the database."
        echo ""
        read -r -p "  Type 'MIGRATE' to confirm: " CONFIRMATION
        if [[ "${CONFIRMATION}" != "MIGRATE" ]]; then
            log_info "Forward-fix cancelled by user"
            exit 0
        fi
    fi

    # Execute migrations
    log_info "Running pending migrations via cargo sqlx migrate run"

    local migrate_output
    if migrate_output=$(cargo sqlx migrate run --source "${MIGRATIONS_DIR}" 2>&1); then
        log_info "Migrations completed successfully"
        cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "ok",
  "message": "Forward-fix completed successfully",
  "pending_applied": ${pending_count},
  "output": "$(echo "${migrate_output}" | tr '\n' ' ' | sed 's/"/\\"/g')"
}
EOF
    else
        log_error "Migration execution failed"
        cat <<EOF
{
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "status": "error",
  "message": "Forward-fix failed",
  "output": "$(echo "${migrate_output}" | tr '\n' ' ' | sed 's/"/\\"/g')"
}
EOF
        exit 1
    fi
}

# =============================================================================
# MAIN: Parse subcommand and dispatch
# =============================================================================
if [[ $# -lt 1 ]]; then
    usage
fi

SUBCOMMAND="$1"
shift

case "${SUBCOMMAND}" in
    status)
        cmd_status
        ;;
    verify)
        cmd_verify
        ;;
    rollback)
        cmd_rollback "$@"
        ;;
    forward-fix)
        cmd_forward_fix "$@"
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        log_error "Unknown subcommand: ${SUBCOMMAND}"
        usage
        ;;
esac
