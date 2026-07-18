#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Test: Alert Rules Validation
#
# Validates that deploy/backup/monitoring/alert-rules.yml:
#   1. Is valid YAML
#   2. Every rule has required fields (name, description, condition, severity, owner, runbook_link)
#   3. Every runbook_link points to an existing file or a known planned file
#   4. Owner values are from the allowed set (platform, backend, on-call)
#   5. Severity values are from the allowed set (critical, warning, info)
#   6. No duplicate rule names exist
# =============================================================================

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
ALERT_RULES_FILE="$ROOT_DIR/deploy/backup/monitoring/alert-rules.yml"

PASS_COUNT=0
FAIL_COUNT=0

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  FAIL_COUNT=$((FAIL_COUNT + 1))
}

pass() {
  printf 'PASS: %s\n' "$*"
  PASS_COUNT=$((PASS_COUNT + 1))
}

# ---------------------------------------------------------------------------
# Check 0: Alert rules file exists
# ---------------------------------------------------------------------------
if [[ ! -f "$ALERT_RULES_FILE" ]]; then
  fail "Alert rules file not found at $ALERT_RULES_FILE"
  printf '\nResults: %d passed, %d failed\n' "$PASS_COUNT" "$FAIL_COUNT"
  exit 1
fi
pass "Alert rules file exists"

# ---------------------------------------------------------------------------
# Check 1: Valid YAML
# ---------------------------------------------------------------------------
if ! python3 -c "
import yaml, sys
with open(sys.argv[1]) as f:
    yaml.safe_load(f)
" "$ALERT_RULES_FILE" 2>/dev/null; then
  fail "Alert rules file is not valid YAML"
  printf '\nResults: %d passed, %d failed\n' "$PASS_COUNT" "$FAIL_COUNT"
  exit 1
fi
pass "Alert rules file is valid YAML"

# ---------------------------------------------------------------------------
# Checks 2-6: Validate rule structure using Python
# ---------------------------------------------------------------------------
validation_output=$(python3 - "$ALERT_RULES_FILE" "$ROOT_DIR" <<'PYTHON'
import yaml
import sys
import os

alert_file = sys.argv[1]
root_dir = sys.argv[2]

with open(alert_file) as f:
    data = yaml.safe_load(f)

rules = data.get("rules", [])
if not rules:
    print("ERROR:No rules found in alert-rules.yml")
    sys.exit(0)

required_fields = ["name", "description", "condition", "severity", "owner", "runbook_link"]
allowed_severities = {"critical", "warning", "info"}
allowed_owners = {"platform", "backend", "on-call"}

# Known planned runbooks (being created in FEAT-002)
planned_runbooks = {
    "docs/runbooks/incident-response.md",
    "docs/runbooks/rollback.md",
    "docs/runbooks/secret-rotation.md",
    "docs/runbooks/data-recovery.md",
    "docs/runbooks/dependency-outage.md",
}

errors = []
names_seen = []
categories_found = set()

# Category detection based on rule name prefixes
category_prefixes = {
    "backup": "backup",
    "api": "api",
    "db": "database",
    "storage": "storage",
    "queue": "queue",
    "worker": "worker",
    "import": "import",
    "export": "export",
    "conflict": "conflict",
    "contract": "contract",
}

for i, rule in enumerate(rules):
    rule_name = rule.get("name", f"<unnamed rule at index {i}>")

    # Check required fields
    for field in required_fields:
        if field not in rule or rule[field] is None or str(rule[field]).strip() == "":
            errors.append(f"MISSING_FIELD:{rule_name}:missing required field '{field}'")

    # Check severity
    severity = rule.get("severity", "")
    if severity not in allowed_severities:
        errors.append(f"BAD_SEVERITY:{rule_name}:invalid severity '{severity}' (allowed: {', '.join(sorted(allowed_severities))})")

    # Check owner
    owner = rule.get("owner", "")
    if owner not in allowed_owners:
        errors.append(f"BAD_OWNER:{rule_name}:invalid owner '{owner}' (allowed: {', '.join(sorted(allowed_owners))})")

    # Check runbook_link
    runbook_link = rule.get("runbook_link", "")
    if runbook_link:
        full_path = os.path.join(root_dir, runbook_link)
        if not os.path.isfile(full_path) and runbook_link not in planned_runbooks:
            errors.append(f"BAD_RUNBOOK:{rule_name}:runbook_link '{runbook_link}' does not exist and is not in the planned set")

    # Track names for duplicate detection
    names_seen.append(rule_name)

    # Track categories
    for prefix, category in category_prefixes.items():
        if rule_name.startswith(prefix):
            categories_found.add(category)
            break

# Check for duplicate names
seen_set = set()
for name in names_seen:
    if name in seen_set:
        errors.append(f"DUPLICATE:{name}:duplicate rule name")
    seen_set.add(name)

# Check all 9 required categories are present
required_categories = {"backup", "api", "database", "storage", "queue", "worker", "import", "export", "conflict", "contract"}
missing_categories = required_categories - categories_found
for cat in sorted(missing_categories):
    errors.append(f"MISSING_CATEGORY:none:missing alert rules for category '{cat}'")

# Output results
print(f"RULE_COUNT:{len(rules)}")
print(f"CATEGORY_COUNT:{len(categories_found)}")

for error in errors:
    print(f"ERROR:{error}")

if not errors:
    print("ALL_CLEAR")
PYTHON
)

# ---------------------------------------------------------------------------
# Parse validation output
# ---------------------------------------------------------------------------
rule_count=$(echo "$validation_output" | grep "^RULE_COUNT:" | cut -d: -f2)
category_count=$(echo "$validation_output" | grep "^CATEGORY_COUNT:" | cut -d: -f2)

if echo "$validation_output" | grep -q "^ALL_CLEAR"; then
  pass "All $rule_count rules have required fields (name, description, condition, severity, owner, runbook_link)"
  pass "All severity values are valid (critical, warning, info)"
  pass "All owner values are valid (platform, backend, on-call)"
  pass "All runbook_link values point to existing or planned runbook files"
  pass "No duplicate rule names found"
  pass "All required subsystem categories covered ($category_count categories)"
else
  # Process individual errors
  has_field_error=false
  has_severity_error=false
  has_owner_error=false
  has_runbook_error=false
  has_duplicate_error=false
  has_category_error=false

  while IFS= read -r line; do
    error_type=$(echo "$line" | cut -d: -f2)
    rule_name=$(echo "$line" | cut -d: -f3)
    message=$(echo "$line" | cut -d: -f4-)

    case "$error_type" in
      MISSING_FIELD)
        if [[ "$has_field_error" == "false" ]]; then
          has_field_error=true
        fi
        fail "Rule '$rule_name': $message"
        ;;
      BAD_SEVERITY)
        if [[ "$has_severity_error" == "false" ]]; then
          has_severity_error=true
        fi
        fail "Rule '$rule_name': $message"
        ;;
      BAD_OWNER)
        if [[ "$has_owner_error" == "false" ]]; then
          has_owner_error=true
        fi
        fail "Rule '$rule_name': $message"
        ;;
      BAD_RUNBOOK)
        if [[ "$has_runbook_error" == "false" ]]; then
          has_runbook_error=true
        fi
        fail "Rule '$rule_name': $message"
        ;;
      DUPLICATE)
        if [[ "$has_duplicate_error" == "false" ]]; then
          has_duplicate_error=true
        fi
        fail "Rule '$rule_name': $message"
        ;;
      MISSING_CATEGORY)
        if [[ "$has_category_error" == "false" ]]; then
          has_category_error=true
        fi
        fail "$message"
        ;;
    esac
  done < <(echo "$validation_output" | grep "^ERROR:")

  [[ "$has_field_error" == "false" ]] && pass "All rules have required fields"
  [[ "$has_severity_error" == "false" ]] && pass "All severity values are valid"
  [[ "$has_owner_error" == "false" ]] && pass "All owner values are valid"
  [[ "$has_runbook_error" == "false" ]] && pass "All runbook_link values are valid"
  [[ "$has_duplicate_error" == "false" ]] && pass "No duplicate rule names"
  [[ "$has_category_error" == "false" ]] && pass "All required categories covered"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
printf '\n============================================================\n'
printf 'Alert Rules Validation: %d passed, %d failed (%s rules across %s categories)\n' \
  "$PASS_COUNT" "$FAIL_COUNT" "$rule_count" "$category_count"
printf '============================================================\n'

if [[ "$FAIL_COUNT" -gt 0 ]]; then
  exit 1
fi

printf '\nPASS: alert rules validation complete\n'
