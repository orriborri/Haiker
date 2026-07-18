#!/usr/bin/env bash
# =============================================================================
# Validates the structure of tests/e2e-production.sh without needing a running server.
# Checks:
#   1. The script file exists and is executable
#   2. It contains all 12 required test case names
#   3. It uses proper error handling
#   4. It accepts API_URL environment variable
# =============================================================================
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
E2E_SCRIPT="$ROOT_DIR/tests/e2e-production.sh"

PASS_COUNT=0
FAIL_COUNT=0

pass() {
  PASS_COUNT=$((PASS_COUNT + 1))
  printf 'PASS: %s\n' "$1"
}

fail() {
  FAIL_COUNT=$((FAIL_COUNT + 1))
  printf 'FAIL: %s\n' "$1" >&2
}

# ---------------------------------------------------------------------------
# Check 1: File exists and is executable
# ---------------------------------------------------------------------------
if [[ ! -f "$E2E_SCRIPT" ]]; then
  fail "E2E script does not exist at $E2E_SCRIPT"
  printf '\nResult: %d passed, %d failed\n' "$PASS_COUNT" "$FAIL_COUNT"
  exit 1
fi
pass "E2E script exists at tests/e2e-production.sh"

if [[ ! -x "$E2E_SCRIPT" ]]; then
  fail "E2E script is not executable"
else
  pass "E2E script is executable"
fi

# ---------------------------------------------------------------------------
# Check 2: Contains all 12 required test case names
# ---------------------------------------------------------------------------
REQUIRED_TESTS=(
  SIGN_IN
  UPLOAD
  IMPORT
  VIEW
  EDIT
  PUBLISH
  EXPORT
  DOWNLOAD
  DUPLICATE
  CONFLICT
  FAILURE
  UNAUTHORIZED
)

script_content=$(cat "$E2E_SCRIPT")

all_tests_found=true
for test_name in "${REQUIRED_TESTS[@]}"; do
  if grep -q "test_${test_name}\|${test_name}" "$E2E_SCRIPT"; then
    pass "Contains test case: $test_name"
  else
    fail "Missing test case: $test_name"
    all_tests_found=false
  fi
done

# Verify tests are defined as functions
for test_name in "${REQUIRED_TESTS[@]}"; do
  if grep -qE "^test_${test_name}\\(\\)" "$E2E_SCRIPT"; then
    : # function defined
  elif grep -qE "^[[:space:]]*test_${test_name}\\(\\)" "$E2E_SCRIPT"; then
    : # function defined with indentation
  else
    # Check if there is at least a function-like definition
    if ! grep -qE "test_${test_name}" "$E2E_SCRIPT"; then
      fail "Test case $test_name not defined as a function"
    fi
  fi
done

# ---------------------------------------------------------------------------
# Check 3: Proper error handling
# ---------------------------------------------------------------------------
# Check for set -euo pipefail or set -uo pipefail (the script may handle errors per-test)
if grep -qE 'set\s+-(e?u?o?\s*pipefail|uo\s+pipefail|euo\s+pipefail)' "$E2E_SCRIPT"; then
  pass "Uses proper error handling (set options with pipefail)"
elif grep -q 'pipefail' "$E2E_SCRIPT"; then
  pass "Uses pipefail error handling"
else
  fail "Missing error handling (no set -euo pipefail or pipefail)"
fi

# Check for per-test error capture (fail function, assertions)
if grep -qE '(assert_status|fail|FAIL)' "$E2E_SCRIPT"; then
  pass "Has per-test assertion/failure handling"
else
  fail "Missing per-test assertion/failure handling"
fi

# Check for test result tracking
if grep -qE '(TESTS_PASSED|TESTS_FAILED|PASS_COUNT|FAIL_COUNT)' "$E2E_SCRIPT"; then
  pass "Tracks test pass/fail counts"
else
  fail "Missing test pass/fail count tracking"
fi

# Check for exit code based on results
if grep -qE 'exit\s+1' "$E2E_SCRIPT" && grep -qE 'exit\s+0' "$E2E_SCRIPT"; then
  pass "Exits with appropriate code based on results"
else
  fail "Missing appropriate exit codes (should exit 0 on success, 1 on failure)"
fi

# ---------------------------------------------------------------------------
# Check 4: Accepts API_URL environment variable
# ---------------------------------------------------------------------------
if grep -q 'API_URL' "$E2E_SCRIPT"; then
  pass "Accepts API_URL environment variable"
else
  fail "Does not reference API_URL environment variable"
fi

# Check it has a default value for API_URL
if grep -qE 'API_URL.*localhost' "$E2E_SCRIPT" || grep -qE 'API_URL:-' "$E2E_SCRIPT"; then
  pass "API_URL has a default value"
else
  fail "API_URL does not have a default value"
fi

# ---------------------------------------------------------------------------
# Additional structural checks
# ---------------------------------------------------------------------------

# Uses curl for HTTP calls
if grep -q 'curl' "$E2E_SCRIPT"; then
  pass "Uses curl for HTTP calls"
else
  fail "Does not use curl for HTTP calls"
fi

# Has helper functions
if grep -qE '^(http_request|http_upload)\(' "$E2E_SCRIPT" || \
   grep -qE '^[[:space:]]*(http_request|http_upload)\(' "$E2E_SCRIPT"; then
  pass "Has HTTP helper functions"
else
  fail "Missing HTTP helper functions"
fi

# Has JSON parsing capability
if grep -qE '(jq|python3.*json|json_get)' "$E2E_SCRIPT"; then
  pass "Has JSON parsing capability (jq or python3)"
else
  fail "Missing JSON parsing capability"
fi

# Checks HTTP status codes
if grep -qE '(HTTP_STATUS|http_code|assert_status)' "$E2E_SCRIPT"; then
  pass "Checks HTTP status codes"
else
  fail "Does not check HTTP status codes"
fi

# Uses TEST_AUTH_TOKEN
if grep -q 'TEST_AUTH_TOKEN' "$E2E_SCRIPT"; then
  pass "Accepts TEST_AUTH_TOKEN for authentication"
else
  fail "Does not accept TEST_AUTH_TOKEN"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
printf '\n'
printf '===========================================\n'
printf 'E2E Structure Validation Results\n'
printf '===========================================\n'
printf 'Passed: %d\n' "$PASS_COUNT"
printf 'Failed: %d\n' "$FAIL_COUNT"
printf '===========================================\n'

if [[ $FAIL_COUNT -gt 0 ]]; then
  exit 1
fi
exit 0
