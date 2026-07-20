#!/usr/bin/env bash
# =============================================================================
# Haiker E2E Production Test
# =============================================================================
# Exercises the full GPX-to-export user journey against a running Haiker stack.
#
# Environment variables:
#   API_URL         - Base URL of the Haiker API (default: http://localhost:3000)
#   TEST_AUTH_TOKEN - Pre-authenticated bearer token for testing
#   GPX_FIXTURE     - Path to a valid GPX file (default: auto-detected)
#   POLL_TIMEOUT    - Max seconds to poll async jobs (default: 60)
#   POLL_INTERVAL   - Seconds between poll attempts (default: 2)
#
# Usage:
#   TEST_AUTH_TOKEN="tok_xxx" ./tests/e2e-production.sh
#   API_URL=https://staging.haiker.app TEST_AUTH_TOKEN="tok_xxx" ./tests/e2e-production.sh
# =============================================================================

# Note: -e is intentionally omitted. Each test case handles errors individually via
# assertion helpers and early returns, rather than aborting the entire suite on the
# first non-zero exit code. This allows per-test pass/fail reporting.
set -uo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
API_URL="${API_URL:-http://localhost:3000}"
AUTH_TOKEN="${TEST_AUTH_TOKEN:-}"
POLL_TIMEOUT="${POLL_TIMEOUT:-60}"
POLL_INTERVAL="${POLL_INTERVAL:-2}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
GPX_FIXTURE="${GPX_FIXTURE:-$ROOT_DIR/crates/test_support/fixtures/valid_simple.gpx}"
MALFORMED_FIXTURE="$ROOT_DIR/crates/test_support/fixtures/malformed_xml.gpx"

# ---------------------------------------------------------------------------
# Test tracking
# ---------------------------------------------------------------------------
TESTS_PASSED=0
TESTS_FAILED=0
TESTS_RUN=0
CURRENT_TEST=""

# ---------------------------------------------------------------------------
# Helper functions
# ---------------------------------------------------------------------------

log() {
  printf '[%s] %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null || date +%H:%M:%S)" "$*"
}

generate_uuid() {
  if command -v uuidgen &>/dev/null; then
    uuidgen | tr '[:upper:]' '[:lower:]'
  elif [[ -f /proc/sys/kernel/random/uuid ]]; then
    cat /proc/sys/kernel/random/uuid
  else
    python3 -c "import uuid; print(uuid.uuid4())"
  fi
}

json_get() {
  # Extract a field from JSON using python3 or jq
  local json="$1"
  local field="$2"
  if command -v jq &>/dev/null; then
    printf '%s' "$json" | jq -r "$field"
  else
    printf '%s' "$json" | python3 -c "
import sys, json
data = json.load(sys.stdin)
keys = sys.argv[1].lstrip('.').split('.')
val = data
for k in keys:
    if k == '':
        continue
    val = val[k]
print(val if val is not None else '')
" "$field"
  fi
}

# Perform an HTTP request using curl.
# Usage: http_request METHOD PATH [BODY] [EXTRA_HEADERS...]
# Sets global variables: HTTP_STATUS, HTTP_BODY
http_request() {
  local method="$1"
  local path="$2"
  local body="${3:-}"
  shift 3 || shift $#

  local url="${API_URL}${path}"
  local -a curl_args=(
    -s
    -w '\n%{http_code}'
    -X "$method"
  )

  # Add auth header if token is available
  if [[ -n "$AUTH_TOKEN" ]]; then
    curl_args+=(-H "Authorization: Bearer $AUTH_TOKEN")
  fi

  # Add extra headers
  while [[ $# -gt 0 ]]; do
    curl_args+=(-H "$1")
    shift
  done

  # Add body if present
  if [[ -n "$body" ]]; then
    curl_args+=(-H "Content-Type: application/json" -d "$body")
  fi

  local response
  response=$(curl "${curl_args[@]}" "$url")
  HTTP_STATUS=$(printf '%s' "$response" | tail -n1)
  HTTP_BODY=$(printf '%s' "$response" | sed '$d')
}

# Upload a file via multipart form or raw body
# Usage: http_upload METHOD PATH FILE_PATH [EXTRA_HEADERS...]
# Sets global variables: HTTP_STATUS, HTTP_BODY
http_upload() {
  local method="$1"
  local path="$2"
  local file_path="$3"
  shift 3 || shift $#

  local url="${API_URL}${path}"
  local -a curl_args=(
    -s
    -w '\n%{http_code}'
    -X "$method"
  )

  if [[ -n "$AUTH_TOKEN" ]]; then
    curl_args+=(-H "Authorization: Bearer $AUTH_TOKEN")
  fi

  while [[ $# -gt 0 ]]; do
    curl_args+=(-H "$1")
    shift
  done

  curl_args+=(-H "Content-Type: application/gpx+xml" --data-binary "@$file_path")

  local response
  response=$(curl "${curl_args[@]}" "$url")
  HTTP_STATUS=$(printf '%s' "$response" | tail -n1)
  HTTP_BODY=$(printf '%s' "$response" | sed '$d')
}

# ---------------------------------------------------------------------------
# Assertion helpers
# ---------------------------------------------------------------------------

assert_status() {
  local expected="$1"
  local context="${2:-}"
  if [[ "$HTTP_STATUS" != "$expected" ]]; then
    fail "Expected HTTP $expected but got $HTTP_STATUS${context:+ ($context)}. Body: $HTTP_BODY"
    return 1
  fi
  return 0
}

assert_status_one_of() {
  local context="${1:-}"
  shift
  for expected in "$@"; do
    if [[ "$HTTP_STATUS" == "$expected" ]]; then
      return 0
    fi
  done
  fail "Expected HTTP one of [$*] but got $HTTP_STATUS${context:+ ($context)}. Body: $HTTP_BODY"
  return 1
}

assert_body_contains() {
  local needle="$1"
  local context="${2:-}"
  if [[ "$HTTP_BODY" != *"$needle"* ]]; then
    fail "Response body missing '$needle'${context:+ ($context)}. Body: $HTTP_BODY"
    return 1
  fi
  return 0
}

assert_not_empty() {
  local value="$1"
  local name="$2"
  if [[ -z "$value" || "$value" == "null" ]]; then
    fail "$name is empty or null"
    return 1
  fi
  return 0
}

# ---------------------------------------------------------------------------
# Test lifecycle
# ---------------------------------------------------------------------------

begin_test() {
  CURRENT_TEST="$1"
  TESTS_RUN=$((TESTS_RUN + 1))
  log "--- TEST: $CURRENT_TEST ---"
}

pass() {
  TESTS_PASSED=$((TESTS_PASSED + 1))
  printf 'PASS: %s\n' "$CURRENT_TEST"
}

fail() {
  TESTS_FAILED=$((TESTS_FAILED + 1))
  printf 'FAIL: %s - %s\n' "$CURRENT_TEST" "$1" >&2
}

# Poll an endpoint until a condition is met or timeout
# Usage: poll_until PATH JQ_CONDITION DESCRIPTION
# Sets HTTP_BODY/HTTP_STATUS to the last response
poll_until() {
  local path="$1"
  local condition="$2"
  local description="$3"
  local elapsed=0

  while [[ $elapsed -lt $POLL_TIMEOUT ]]; do
    http_request GET "$path"
    if [[ "$HTTP_STATUS" == "200" ]]; then
      local match
      if command -v jq &>/dev/null; then
        match=$(printf '%s' "$HTTP_BODY" | jq -r "$condition" 2>/dev/null)
      else
        match=$(printf '%s' "$HTTP_BODY" | python3 -c "
import sys, json
data = json.load(sys.stdin)
condition = sys.argv[1]
# Simple evaluator for status checks
if '==' in condition:
    field, value = condition.split('==')
    field = field.strip().lstrip('.')
    value = value.strip().strip('\"')
    keys = field.split('.')
    val = data
    for k in keys:
        val = val.get(k, '')
    print('true' if str(val) == value else 'false')
else:
    print('false')
" "$condition" 2>/dev/null)
      fi
      if [[ "$match" == "true" ]]; then
        return 0
      fi
    fi
    sleep "$POLL_INTERVAL"
    elapsed=$((elapsed + POLL_INTERVAL))
  done

  fail "Timed out waiting for $description (${POLL_TIMEOUT}s)"
  return 1
}

# ---------------------------------------------------------------------------
# Test cases
# ---------------------------------------------------------------------------

test_SIGN_IN() {
  begin_test "SIGN_IN"

  if [[ -z "$AUTH_TOKEN" ]]; then
    fail "TEST_AUTH_TOKEN not set - cannot authenticate. Set TEST_AUTH_TOKEN env var."
    return
  fi

  # Verify token works by calling /v1/me
  http_request GET "/v1/me"
  if ! assert_status 200 "GET /v1/me with valid token"; then
    return
  fi

  # Verify response contains user information
  if ! assert_body_contains "id" "user info should contain id"; then
    return
  fi

  pass
}

test_UPLOAD() {
  begin_test "UPLOAD"

  if [[ ! -f "$GPX_FIXTURE" ]]; then
    fail "GPX fixture not found at $GPX_FIXTURE"
    return
  fi

  local file_size
  file_size=$(wc -c < "$GPX_FIXTURE" | tr -d ' ')

  # Generate idempotency key for this upload
  UPLOAD_IDEMPOTENCY_KEY=$(generate_uuid)

  # Start the import
  local body
  body=$(printf '{"filename":"valid_simple.gpx","contentType":"application/gpx+xml","fileSizeBytes":%d}' "$file_size")

  http_request POST "/v1/imports" "$body" \
    "Idempotency-Key: $UPLOAD_IDEMPOTENCY_KEY"

  if ! assert_status 202 "POST /v1/imports"; then
    return
  fi

  # Extract import ID and upload URL
  IMPORT_ID=$(json_get "$HTTP_BODY" ".importId")
  UPLOAD_URL=$(json_get "$HTTP_BODY" ".uploadUrl")

  if ! assert_not_empty "$IMPORT_ID" "importId"; then
    return
  fi
  if ! assert_not_empty "$UPLOAD_URL" "uploadUrl"; then
    return
  fi

  # Upload the GPX file to the presigned URL
  local upload_status
  upload_status=$(curl -s -o /dev/null -w '%{http_code}' -X PUT \
    -H "Content-Type: application/gpx+xml" \
    --data-binary "@$GPX_FIXTURE" \
    "$UPLOAD_URL")

  if [[ "$upload_status" != "200" && "$upload_status" != "201" ]]; then
    fail "File upload to presigned URL returned $upload_status (expected 200 or 201)"
    return
  fi

  # Complete the upload with checksum
  local checksum
  if command -v sha256sum &>/dev/null; then
    checksum=$(sha256sum "$GPX_FIXTURE" | cut -d' ' -f1)
  elif command -v shasum &>/dev/null; then
    checksum=$(shasum -a 256 "$GPX_FIXTURE" | cut -d' ' -f1)
  else
    checksum=$(python3 -c "
import hashlib, sys
with open(sys.argv[1], 'rb') as f:
    print(hashlib.sha256(f.read()).hexdigest())
" "$GPX_FIXTURE")
  fi

  http_request POST "/v1/imports/${IMPORT_ID}/completion" \
    "{\"checksum\":\"$checksum\"}"

  if ! assert_status 200 "POST /v1/imports/{importId}/completion"; then
    return
  fi

  pass
}

test_IMPORT() {
  begin_test "IMPORT"

  if [[ -z "${IMPORT_ID:-}" ]]; then
    fail "No IMPORT_ID from previous UPLOAD test"
    return
  fi

  # Poll until import is completed
  if ! poll_until "/v1/imports/$IMPORT_ID" '.status == "completed"' "import completion"; then
    return
  fi

  # Extract the activity ID from the completed import
  ACTIVITY_ID=$(json_get "$HTTP_BODY" ".activityId")
  if ! assert_not_empty "$ACTIVITY_ID" "activityId from completed import"; then
    return
  fi

  pass
}

test_VIEW() {
  begin_test "VIEW"

  if [[ -z "${ACTIVITY_ID:-}" ]]; then
    fail "No ACTIVITY_ID from previous IMPORT test"
    return
  fi

  # List activities and verify the imported one appears
  http_request GET "/v1/activities"
  if ! assert_status 200 "GET /v1/activities"; then
    return
  fi

  # Verify our activity ID appears in the response
  if ! assert_body_contains "$ACTIVITY_ID" "activity list should contain imported activity"; then
    return
  fi

  pass
}

test_EDIT() {
  begin_test "EDIT"

  if [[ -z "${ACTIVITY_ID:-}" ]]; then
    fail "No ACTIVITY_ID from previous tests"
    return
  fi

  # Create a route draft
  local draft_idempotency_key
  draft_idempotency_key=$(generate_uuid)

  http_request POST "/v1/activities/${ACTIVITY_ID}/route-drafts" \
    '{}' \
    "Idempotency-Key: $draft_idempotency_key"

  if ! assert_status_one_of "create route draft" 201 200; then
    return
  fi

  DRAFT_ID=$(json_get "$HTTP_BODY" ".id")
  if ! assert_not_empty "$DRAFT_ID" "draft id"; then
    return
  fi

  # Get the current revision
  DRAFT_REVISION=$(json_get "$HTTP_BODY" ".revision")
  if [[ -z "$DRAFT_REVISION" || "$DRAFT_REVISION" == "null" ]]; then
    DRAFT_REVISION="1"
  fi

  # Apply a MovePoint operation
  local op_idempotency_key
  op_idempotency_key=$(generate_uuid)

  local op_body
  op_body=$(printf '{
    "expectedRevision": %s,
    "operation": {
      "type": "MovePoint",
      "pointIndex": 0,
      "latitude": 47.2700,
      "longitude": 11.3940
    }
  }' "$DRAFT_REVISION")

  http_request POST "/v1/route-drafts/${DRAFT_ID}/operations" \
    "$op_body" \
    "Idempotency-Key: $op_idempotency_key"

  if ! assert_status 200 "apply MovePoint operation"; then
    return
  fi

  # Update revision after operation
  local new_revision
  new_revision=$(json_get "$HTTP_BODY" ".revision")
  if [[ -n "$new_revision" && "$new_revision" != "null" ]]; then
    DRAFT_REVISION="$new_revision"
  else
    DRAFT_REVISION=$((DRAFT_REVISION + 1))
  fi

  pass
}

test_PUBLISH() {
  begin_test "PUBLISH"

  if [[ -z "${DRAFT_ID:-}" ]]; then
    fail "No DRAFT_ID from previous EDIT test"
    return
  fi

  local pub_idempotency_key
  pub_idempotency_key=$(generate_uuid)
  PUBLISH_IDEMPOTENCY_KEY="$pub_idempotency_key"

  local pub_body
  pub_body=$(printf '{"expectedRevision": %s}' "$DRAFT_REVISION")

  http_request POST "/v1/route-drafts/${DRAFT_ID}/publication" \
    "$pub_body" \
    "Idempotency-Key: $pub_idempotency_key"

  if ! assert_status_one_of "publish route draft" 201 200; then
    return
  fi

  # Extract the route version ID for export
  ROUTE_VERSION_ID=$(json_get "$HTTP_BODY" ".routeVersionId")

  pass
}

test_EXPORT() {
  begin_test "EXPORT"

  if [[ -z "${ACTIVITY_ID:-}" ]]; then
    fail "No ACTIVITY_ID from previous tests"
    return
  fi

  local export_idempotency_key
  export_idempotency_key=$(generate_uuid)

  local export_body
  if [[ -n "${ROUTE_VERSION_ID:-}" && "$ROUTE_VERSION_ID" != "null" ]]; then
    export_body="{\"routeVersionId\":\"$ROUTE_VERSION_ID\",\"format\":\"gpx\"}"
  else
    export_body='{"format":"gpx"}'
  fi

  http_request POST "/v1/activities/${ACTIVITY_ID}/exports" \
    "$export_body" \
    "Idempotency-Key: $export_idempotency_key"

  if ! assert_status 202 "POST /v1/activities/{activityId}/exports"; then
    return
  fi

  EXPORT_ID=$(json_get "$HTTP_BODY" ".exportId")
  if [[ -z "$EXPORT_ID" || "$EXPORT_ID" == "null" ]]; then
    EXPORT_ID=$(json_get "$HTTP_BODY" ".id")
  fi
  if ! assert_not_empty "$EXPORT_ID" "exportId"; then
    return
  fi

  # Poll until export is ready
  if ! poll_until "/v1/exports/$EXPORT_ID" '.status == "ready"' "export ready"; then
    return
  fi

  pass
}

test_DOWNLOAD() {
  begin_test "DOWNLOAD"

  if [[ -z "${EXPORT_ID:-}" ]]; then
    fail "No EXPORT_ID from previous EXPORT test"
    return
  fi

  http_request GET "/v1/exports/${EXPORT_ID}/download"
  if ! assert_status 200 "GET /v1/exports/{exportId}/download"; then
    return
  fi

  # The response should contain either a download URL or GPX content
  # Check for GPX markers or a downloadUrl field
  if [[ "$HTTP_BODY" == *"gpx"* || "$HTTP_BODY" == *"downloadUrl"* || "$HTTP_BODY" == *"url"* ]]; then
    : # Valid response
  else
    fail "Download response does not contain GPX content or download URL. Body: $HTTP_BODY"
    return
  fi

  pass
}

test_DUPLICATE() {
  begin_test "DUPLICATE"

  if [[ -z "${UPLOAD_IDEMPOTENCY_KEY:-}" ]]; then
    fail "No UPLOAD_IDEMPOTENCY_KEY from previous UPLOAD test"
    return
  fi

  if [[ ! -f "$GPX_FIXTURE" ]]; then
    fail "GPX fixture not found at $GPX_FIXTURE"
    return
  fi

  local file_size
  file_size=$(wc -c < "$GPX_FIXTURE" | tr -d ' ')

  # Re-upload with the same idempotency key - should get idempotent response
  local body
  body=$(printf '{"filename":"valid_simple.gpx","contentType":"application/gpx+xml","fileSizeBytes":%d}' "$file_size")

  http_request POST "/v1/imports" "$body" \
    "Idempotency-Key: $UPLOAD_IDEMPOTENCY_KEY"

  # Should return 202 with the same import ID (idempotent replay)
  if ! assert_status 202 "duplicate upload with same Idempotency-Key"; then
    return
  fi

  # Verify it returns the same import ID (not a new one)
  local duplicate_import_id
  duplicate_import_id=$(json_get "$HTTP_BODY" ".importId")
  if [[ "$duplicate_import_id" != "$IMPORT_ID" ]]; then
    fail "Duplicate upload created a new import ($duplicate_import_id) instead of returning existing ($IMPORT_ID)"
    return
  fi

  pass
}

test_CONFLICT() {
  begin_test "CONFLICT"

  if [[ -z "${DRAFT_ID:-}" ]]; then
    # Need to create a new draft to test conflict
    if [[ -z "${ACTIVITY_ID:-}" ]]; then
      fail "No ACTIVITY_ID available for conflict test"
      return
    fi

    local draft_key
    draft_key=$(generate_uuid)
    http_request POST "/v1/activities/${ACTIVITY_ID}/route-drafts" \
      '{}' \
      "Idempotency-Key: $draft_key"

    if [[ "$HTTP_STATUS" != "201" && "$HTTP_STATUS" != "200" ]]; then
      fail "Could not create a draft for conflict test (HTTP $HTTP_STATUS)"
      return
    fi

    DRAFT_ID=$(json_get "$HTTP_BODY" ".id")
  fi

  # Attempt to publish with a stale (wrong) expectedRevision
  local conflict_key
  conflict_key=$(generate_uuid)

  # Use revision 0 which should always be stale after any operation
  local stale_body='{"expectedRevision": 99999}'

  http_request POST "/v1/route-drafts/${DRAFT_ID}/publication" \
    "$stale_body" \
    "Idempotency-Key: $conflict_key"

  if ! assert_status 409 "publish with stale expectedRevision"; then
    return
  fi

  # Verify error follows Problem Details format
  if ! assert_body_contains "status" "error response should contain status field"; then
    return
  fi

  pass
}

test_FAILURE() {
  begin_test "FAILURE"

  # Upload malformed data (not valid GPX/XML)
  local failure_key
  failure_key=$(generate_uuid)

  local malformed_body='{"filename":"bad_file.gpx","contentType":"application/gpx+xml","fileSizeBytes":42}'

  http_request POST "/v1/imports" "$malformed_body" \
    "Idempotency-Key: $failure_key"

  if [[ "$HTTP_STATUS" != "202" ]]; then
    # If the server rejects it outright, that is also acceptable failure handling
    if [[ "$HTTP_STATUS" == "400" || "$HTTP_STATUS" == "422" ]]; then
      # Server rejected malformed input upfront - graceful failure
      if ! assert_body_contains "status" "error response should be Problem Details"; then
        return
      fi
      pass
      return
    fi
    fail "Unexpected status $HTTP_STATUS for malformed import start"
    return
  fi

  # If accepted (202), the import should eventually fail during processing
  local fail_import_id
  fail_import_id=$(json_get "$HTTP_BODY" ".importId")
  local fail_upload_url
  fail_upload_url=$(json_get "$HTTP_BODY" ".uploadUrl")

  if [[ -n "$fail_upload_url" && "$fail_upload_url" != "null" ]]; then
    # Upload malformed content to the presigned URL
    local malformed_content="THIS IS NOT VALID GPX OR XML CONTENT <broken>"
    curl -s -o /dev/null -X PUT \
      -H "Content-Type: application/gpx+xml" \
      -d "$malformed_content" \
      "$fail_upload_url"

    # Complete with a fake checksum
    local fake_checksum
    fake_checksum=$(printf '%s' "$malformed_content" | sha256sum 2>/dev/null | cut -d' ' -f1 || \
      printf '%s' "$malformed_content" | shasum -a 256 2>/dev/null | cut -d' ' -f1 || \
      python3 -c "import hashlib; print(hashlib.sha256(b'THIS IS NOT VALID GPX OR XML CONTENT <broken>').hexdigest())")

    http_request POST "/v1/imports/${fail_import_id}/completion" \
      "{\"checksum\":\"$fake_checksum\"}"
  fi

  # Poll until import fails (or times out)
  if poll_until "/v1/imports/$fail_import_id" '.status == "failed"' "import failure"; then
    # Verify failure reason is present
    local failure_reason
    failure_reason=$(json_get "$HTTP_BODY" ".failureReason")
    if [[ -n "$failure_reason" && "$failure_reason" != "null" ]]; then
      pass
      return
    fi
    # Even without a reason, a failed status is acceptable
    pass
    return
  fi

  # Check if it completed (meaning the server accepted bad data - still a valid response path)
  http_request GET "/v1/imports/$fail_import_id"
  local current_status
  current_status=$(json_get "$HTTP_BODY" ".status")
  if [[ "$current_status" == "failed" ]]; then
    pass
    return
  fi

  fail "Malformed import did not reach 'failed' status (current: $current_status)"
}

test_UNAUTHORIZED() {
  begin_test "UNAUTHORIZED"

  # Save and clear the auth token
  local saved_token="$AUTH_TOKEN"
  AUTH_TOKEN=""

  local all_passed=true

  # Test key endpoints without authentication
  # 1. GET /v1/me
  http_request GET "/v1/me"
  if [[ "$HTTP_STATUS" != "401" ]]; then
    fail "GET /v1/me without auth returned $HTTP_STATUS, expected 401"
    all_passed=false
  fi

  # 2. POST /v1/imports
  http_request POST "/v1/imports" \
    '{"filename":"test.gpx","contentType":"application/gpx+xml","fileSizeBytes":100}' \
    "Idempotency-Key: $(generate_uuid)"
  if [[ "$HTTP_STATUS" != "401" ]]; then
    fail "POST /v1/imports without auth returned $HTTP_STATUS, expected 401"
    all_passed=false
  fi

  # 3. GET /v1/activities
  http_request GET "/v1/activities"
  if [[ "$HTTP_STATUS" != "401" ]]; then
    fail "GET /v1/activities without auth returned $HTTP_STATUS, expected 401"
    all_passed=false
  fi

  # 4. POST /v1/activities/{id}/route-drafts (use a fake UUID)
  http_request POST "/v1/activities/00000000-0000-0000-0000-000000000000/route-drafts" \
    '{}' \
    "Idempotency-Key: $(generate_uuid)"
  if [[ "$HTTP_STATUS" != "401" ]]; then
    fail "POST route-drafts without auth returned $HTTP_STATUS, expected 401"
    all_passed=false
  fi

  # 5. POST /v1/activities/{id}/exports
  http_request POST "/v1/activities/00000000-0000-0000-0000-000000000000/exports" \
    '{"format":"gpx"}' \
    "Idempotency-Key: $(generate_uuid)"
  if [[ "$HTTP_STATUS" != "401" ]]; then
    fail "POST exports without auth returned $HTTP_STATUS, expected 401"
    all_passed=false
  fi

  # 6. GET /v1/exports/{id}/download
  http_request GET "/v1/exports/00000000-0000-0000-0000-000000000000/download"
  if [[ "$HTTP_STATUS" != "401" ]]; then
    fail "GET export download without auth returned $HTTP_STATUS, expected 401"
    all_passed=false
  fi

  # Restore token
  AUTH_TOKEN="$saved_token"

  if [[ "$all_passed" == "true" ]]; then
    pass
  fi
}

# ---------------------------------------------------------------------------
# Main execution
# ---------------------------------------------------------------------------

main() {
  log "Starting Haiker E2E production tests"
  log "API_URL: $API_URL"
  log "Auth: ${AUTH_TOKEN:+configured}${AUTH_TOKEN:-NOT SET}"
  log ""

  # Verify API is reachable
  local health_status
  health_status=$(curl -s -o /dev/null -w '%{http_code}' "$API_URL/health" 2>/dev/null || echo "000")
  if [[ "$health_status" != "200" ]]; then
    log "WARNING: API health check returned $health_status (expected 200)"
    log "The API at $API_URL may not be running. Tests will likely fail."
    log ""
  fi

  # Run all test cases in sequence
  test_SIGN_IN
  test_UPLOAD
  test_IMPORT
  test_VIEW
  test_EDIT
  test_PUBLISH
  test_EXPORT
  test_DOWNLOAD
  test_DUPLICATE
  test_CONFLICT
  test_FAILURE
  test_UNAUTHORIZED

  # Summary
  log ""
  log "==========================================="
  log "E2E Test Results"
  log "==========================================="
  log "Total:  $TESTS_RUN"
  log "Passed: $TESTS_PASSED"
  log "Failed: $TESTS_FAILED"
  log "==========================================="

  if [[ $TESTS_FAILED -gt 0 ]]; then
    exit 1
  fi
  exit 0
}

main "$@"
