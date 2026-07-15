#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
TMP_ROOT=$(mktemp -d)
trap 'rm -rf "$TMP_ROOT"' EXIT

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local haystack=$1
  local needle=$2
  [[ "$haystack" == *"$needle"* ]] || fail "expected output to contain: $needle"
}

mkdir -p "$TMP_ROOT/bin" "$TMP_ROOT/state" "$TMP_ROOT/tmp"

cat >"$TMP_ROOT/bin/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail

: "${FAKE_GH_STATE:?FAKE_GH_STATE is required}"
printf '%q ' "$@" >>"$FAKE_GH_STATE/commands.log"
printf '\n' >>"$FAKE_GH_STATE/commands.log"

replace_issues() {
  local filter=$1
  shift
  local tmp
  tmp=$(mktemp "$FAKE_GH_STATE/issues.XXXXXX")
  jq "$@" "$filter" "$FAKE_GH_STATE/issues.json" >"$tmp"
  mv "$tmp" "$FAKE_GH_STATE/issues.json"
}

case "${1:-} ${2:-}" in
  "auth status")
    exit 0
    ;;
  "repo view")
    printf '%s\n' '{"nameWithOwner":"example/haiker","viewerPermission":"WRITE","defaultBranchRef":{"name":"main"}}'
    ;;
  "issue list")
    cat "${FAKE_GH_ISSUE_LIST_FILE:-$FAKE_GH_STATE/issues.json}"
    ;;
  "issue view")
    number=$3
    jq --argjson number "$number" 'first(.[] | select(.number == $number))' \
      "$FAKE_GH_STATE/issues.json"
    ;;
  "issue edit")
    number=$3
    shift 3
    add_label=""
    remove_label=""
    add_blocked_by=""
    remove_blocked_by=""
    while (($# > 0)); do
      case "$1" in
        --add-label)
          add_label=$2
          shift 2
          ;;
        --remove-label)
          remove_label=$2
          shift 2
          ;;
        --add-blocked-by)
          add_blocked_by=$2
          shift 2
          ;;
        --remove-blocked-by)
          remove_blocked_by=$2
          shift 2
          ;;
        *)
          shift
          ;;
      esac
    done
    if [[ -n "$add_label" ]]; then
      replace_issues '
        map(if .number == $number then
          .labels = (((.labels // []) + [{name: $label}]) | unique_by(.name | ascii_downcase))
        else . end)
      ' --argjson number "$number" --arg label "$add_label"
    fi
    if [[ -n "$remove_label" ]]; then
      replace_issues '
        map(if .number == $number then
          .labels = [(.labels // [])[] | select((.name | ascii_downcase) != ($label | ascii_downcase))]
        else . end)
      ' --argjson number "$number" --arg label "$remove_label"
    fi
    if [[ -n "$add_blocked_by" ]]; then
      replace_issues '
        . as $issues
        | ($csv | split(",") | map(tonumber)) as $blocker_numbers
        | map(if .number == $number then
            .blockedBy.nodes = (((.blockedBy.nodes // []) + [
              $blocker_numbers[] as $blocker_number
              | $issues[]
              | select(.number == $blocker_number)
              | {number, state}
            ]) | unique_by(.number))
            | .blockedBy.totalCount = (.blockedBy.nodes | length)
          else . end)
      ' --argjson number "$number" --arg csv "$add_blocked_by"
    fi
    if [[ -n "$remove_blocked_by" ]]; then
      replace_issues '
        ($csv | split(",") | map(tonumber)) as $blocker_numbers
        | map(if .number == $number then
            .blockedBy.nodes = [
              (.blockedBy.nodes // [])[] as $node
              | select($blocker_numbers | index($node.number) == null)
              | $node
            ]
            | .blockedBy.totalCount = (.blockedBy.nodes | length)
          else . end)
      ' --argjson number "$number" --arg csv "$remove_blocked_by"
    fi
    ;;
  "label list")
    cat "$FAKE_GH_STATE/labels.json"
    ;;
  "label create")
    name=$3
    tmp=$(mktemp "$FAKE_GH_STATE/labels.XXXXXX")
    jq --arg name "$name" '. + [{name: $name, color: "aaaaaa", description: ""}]' \
      "$FAKE_GH_STATE/labels.json" >"$tmp"
    mv "$tmp" "$FAKE_GH_STATE/labels.json"
    ;;
  "pr list")
    cat "$FAKE_GH_STATE/prs.json"
    ;;
  api\ *)
    endpoint=""
    for argument in "$@"; do
      if [[ "$argument" == repos/* ]]; then
        endpoint=$argument
        break
      fi
    done
    case "$endpoint" in
      repos/*/commits/*)
        printf '%s\n' 'main-sha'
        ;;
      repos/*/branches*)
        printf '%s\n' '[[]]'
        ;;
      repos/*/issues/*/timeline*)
        number=${endpoint#*/issues/}
        number=${number%%/*}
        if jq -e --argjson number "$number" '
          first(.[] | select(.number == $number))
          | [.labels[].name | ascii_downcase]
          | index("kiro") != null
        ' "$FAKE_GH_STATE/issues.json" >/dev/null; then
          printf '%s\n' '[[{"event":"commented","user":{"login":"kiro-agent[bot]"},"body":"Started: https://app.kiro.dev/session/test. Comment /kiro any time."}]]'
        else
          printf '%s\n' '[[]]'
        fi
        ;;
      repos/*/milestones*)
        printf '%s\n' '[[]]'
        ;;
      *)
        printf '%s\n' '{}'
        ;;
    esac
    ;;
  *)
    printf 'unsupported fake gh command: %q ' "$@" >&2
    printf '\n' >&2
    exit 2
    ;;
esac
FAKE_GH
chmod +x "$TMP_ROOT/bin/gh"

cat >"$TMP_ROOT/manifest.json" <<'EOF'
{
  "repo": "example/haiker",
  "milestone": {"title": "MVP", "description": "test"},
  "labels": [
    {"name": "mvp", "color": "0E8A16", "description": "test"},
    {"name": "agent-ready", "color": "2DA44E", "description": "ready"},
    {"name": "agent-dispatching", "color": "BF8700", "description": "claim"},
    {"name": "agent-dispatched", "color": "8250DF", "description": "dispatched"}
  ],
  "dispatch": {
    "ready_label": "agent-ready",
    "claim_label": "agent-dispatching",
    "dispatched_label": "agent-dispatched",
    "trigger_label": "kiro",
    "max_concurrency": 1,
    "ack_timeout_seconds": 5
  },
  "resource_groups": {
    "api-core": {"capacity": 1, "description": "api", "paths": ["api"]},
    "frontend-client": {"capacity": 1, "description": "web", "paths": ["web"]},
    "route-editing": {"capacity": 1, "description": "route", "paths": ["route"]},
    "backup-recovery": {"capacity": 1, "description": "backup", "paths": ["backup"]}
  },
  "issues": [
    {"id": "EPIC", "title": "Epic", "labels": ["mvp"], "body": "<!-- haiker-issue-id: EPIC -->"},
    {"id": "WEB", "title": "Web task", "labels": ["mvp"], "parent_id": "EPIC", "resource_locks": ["frontend-client"], "body": "<!-- haiker-issue-id: WEB -->"},
    {"id": "API", "title": "API task", "labels": ["mvp"], "parent_id": "EPIC", "resource_locks": ["api-core"], "body": "<!-- haiker-issue-id: API -->"},
    {"id": "ROUTE", "title": "Route task", "labels": ["mvp"], "parent_id": "EPIC", "resource_locks": ["route-editing"], "body": "<!-- haiker-issue-id: ROUTE -->"},
    {"id": "BACKUP", "title": "Backup task", "labels": ["mvp"], "parent_id": "EPIC", "resource_locks": ["backup-recovery"], "body": "<!-- haiker-issue-id: BACKUP -->"},
    {"id": "ACTIVE", "title": "Active task", "labels": ["mvp"], "parent_id": "EPIC", "resource_locks": ["api-core", "frontend-client", "route-editing"], "body": "<!-- haiker-issue-id: ACTIVE -->"},
    {"id": "BLOCKED", "title": "Blocked task", "labels": ["mvp"], "parent_id": "EPIC", "blocked_by": ["WEB"], "body": "<!-- haiker-issue-id: BLOCKED -->"}
  ]
}
EOF

cat >"$TMP_ROOT/state/labels.json" <<'EOF'
[
  {"name":"mvp","color":"0E8A16","description":"test"},
  {"name":"agent-ready","color":"2DA44E","description":"ready"},
  {"name":"agent-dispatching","color":"BF8700","description":"claim"},
  {"name":"agent-dispatched","color":"8250DF","description":"dispatched"},
  {"name":"kiro","color":"AAAAAA","description":""}
]
EOF

cat >"$TMP_ROOT/state/issues.json" <<'EOF'
[
  {"number":1,"title":"Epic","state":"OPEN","body":"<!-- haiker-issue-id: EPIC -->","url":"https://example/1","labels":[{"name":"mvp"}],"assignees":[],"parent":null,"blockedBy":{"totalCount":0,"nodes":[]}},
  {"number":2,"title":"Web task","state":"OPEN","body":"<!-- haiker-issue-id: WEB -->","url":"https://example/2","labels":[{"name":"mvp"}],"assignees":[],"parent":{"number":1},"blockedBy":{"totalCount":0,"nodes":[]}},
  {"number":3,"title":"API task","state":"OPEN","body":"<!-- haiker-issue-id: API -->","url":"https://example/3","labels":[{"name":"mvp"}],"assignees":[],"parent":{"number":1},"blockedBy":{"totalCount":0,"nodes":[]}},
  {"number":4,"title":"Route task","state":"OPEN","body":"<!-- haiker-issue-id: ROUTE -->","url":"https://example/4","labels":[{"name":"mvp"}],"assignees":[],"parent":{"number":1},"blockedBy":{"totalCount":0,"nodes":[]}},
  {"number":5,"title":"Backup task","state":"OPEN","body":"<!-- haiker-issue-id: BACKUP -->","url":"https://example/5","labels":[{"name":"mvp"}],"assignees":[],"parent":{"number":1},"blockedBy":{"totalCount":0,"nodes":[]}},
  {"number":6,"title":"Active task","state":"OPEN","body":"<!-- haiker-issue-id: ACTIVE -->","url":"https://example/6","labels":[{"name":"mvp"},{"name":"kiro"}],"assignees":[],"parent":{"number":1},"blockedBy":{"totalCount":0,"nodes":[]}},
  {"number":7,"title":"Blocked task","state":"OPEN","body":"<!-- haiker-issue-id: BLOCKED -->","url":"https://example/7","labels":[{"name":"mvp"},{"name":"agent-ready"}],"assignees":[],"parent":{"number":1},"blockedBy":{"totalCount":1,"nodes":[{"number":2,"state":"OPEN"}]}}
]
EOF
cat >"$TMP_ROOT/state/prs.json" <<'EOF'
[
  {"number":75,"title":"Active work","url":"https://example/pr/75","headRefName":"feat/issue-6","closingIssuesReferences":[{"number":6,"repository":{"name":"haiker","owner":{"login":"example"}}}]}
]
EOF
: >"$TMP_ROOT/state/commands.log"

export PATH="$TMP_ROOT/bin:$PATH"
export FAKE_GH_STATE="$TMP_ROOT/state"
export TMPDIR="$TMP_ROOT/tmp"

if "$ROOT_DIR/scripts/dispatch-kiro-issue.sh" \
  --dry-run \
  --manifest "$TMP_ROOT/manifest.json" \
  --max-concurrency 11 >"$TMP_ROOT/max.out" 2>&1; then
  fail "dispatcher accepted concurrency above Kiro's limit"
fi
grep -F "max concurrency must be between 1 and Kiro's limit of 10" \
  "$TMP_ROOT/max.out" >/dev/null || fail "invalid concurrency error was unclear"

cp "$TMP_ROOT/state/issues.json" "$TMP_ROOT/state/issues.valid.json"
jq 'map(if .number == 2 then del(.blockedBy) else . end)' \
  "$TMP_ROOT/state/issues.valid.json" >"$TMP_ROOT/state/issues.json"
if "$ROOT_DIR/scripts/label-unblocked-issues.sh" \
  --dry-run \
  --manifest "$TMP_ROOT/manifest.json" >"$TMP_ROOT/invalid-blockers.out" 2>&1; then
  fail "readiness accepted missing blockedBy data"
fi
grep -F 'invalid or incomplete blockedBy data' "$TMP_ROOT/invalid-blockers.out" >/dev/null ||
  fail "missing blockedBy failure was unclear"
mv "$TMP_ROOT/state/issues.valid.json" "$TMP_ROOT/state/issues.json"

readiness_dry=$(
  "$ROOT_DIR/scripts/label-unblocked-issues.sh" \
    --dry-run \
    --manifest "$TMP_ROOT/manifest.json"
)
assert_contains "$readiness_dry" "add agent-ready"
assert_contains "$readiness_dry" "remove agent-ready"

"$ROOT_DIR/scripts/label-unblocked-issues.sh" \
  --apply \
  --manifest "$TMP_ROOT/manifest.json" >/dev/null

jq -e '
  (first(.[] | select(.number == 2)) | [.labels[].name] | index("agent-ready") != null) and
  (first(.[] | select(.number == 3)) | [.labels[].name] | index("agent-ready") != null) and
  (first(.[] | select(.number == 4)) | [.labels[].name] | index("agent-ready") != null) and
  (first(.[] | select(.number == 5)) | [.labels[].name] | index("agent-ready") != null) and
  (first(.[] | select(.number == 7)) | [.labels[].name] | index("agent-ready") == null)
' "$TMP_ROOT/state/issues.json" >/dev/null || fail "readiness reconciliation did not converge"
if grep -E -- '--(add|remove)-label kiro([[:space:]]|$)' "$TMP_ROOT/state/commands.log" >/dev/null; then
  fail "readiness reconciliation touched the Kiro trigger"
fi

: >"$TMP_ROOT/state/commands.log"
"$ROOT_DIR/scripts/label-unblocked-issues.sh" \
  --apply \
  --manifest "$TMP_ROOT/manifest.json" >/dev/null
if grep -E -- '^issue edit ' "$TMP_ROOT/state/commands.log" >/dev/null; then
  fail "a converged readiness rerun still mutated issues"
fi

cp "$TMP_ROOT/state/issues.json" "$TMP_ROOT/state/issues.list-snapshot.json"
jq 'map(if .number == 2 then
  .blockedBy = {totalCount: 1, nodes: [{number: 3, state: "OPEN"}]}
else . end)' "$TMP_ROOT/state/issues.json" >"$TMP_ROOT/state/issues.changed.json"
mv "$TMP_ROOT/state/issues.changed.json" "$TMP_ROOT/state/issues.json"
export FAKE_GH_ISSUE_LIST_FILE="$TMP_ROOT/state/issues.list-snapshot.json"
"$ROOT_DIR/scripts/label-unblocked-issues.sh" \
  --apply \
  --manifest "$TMP_ROOT/manifest.json" >/dev/null
unset FAKE_GH_ISSUE_LIST_FILE
jq -e '
  first(.[] | select(.number == 2))
  | [.labels[].name]
  | index("agent-ready") == null
' "$TMP_ROOT/state/issues.json" >/dev/null ||
  fail "readiness did not remove a label made stale after the list snapshot"
mv "$TMP_ROOT/state/issues.list-snapshot.json" "$TMP_ROOT/state/issues.json"

cp "$TMP_ROOT/state/prs.json" "$TMP_ROOT/state/prs.linked.json"
jq '. + [{
  number: 76,
  title: "Unmanaged work",
  url: "https://example/pr/76",
  headRefName: "other-work",
  closingIssuesReferences: [{
    number: 999,
    repository: {name: "haiker", owner: {login: "example"}}
  }]
}]' "$TMP_ROOT/state/prs.linked.json" >"$TMP_ROOT/state/prs.json"
unknown_pr_dry=$(
  "$ROOT_DIR/scripts/dispatch-kiro-issue.sh" \
    --dry-run \
    --manifest "$TMP_ROOT/manifest.json" \
    --max-concurrency 3
)
assert_contains "$unknown_pr_dry" "not linked to managed issues"
assert_contains "$unknown_pr_dry" "No safe agent-ready issue"
mv "$TMP_ROOT/state/prs.linked.json" "$TMP_ROOT/state/prs.json"

dispatch_dry=$(
  "$ROOT_DIR/scripts/dispatch-kiro-issue.sh" \
    --dry-run \
    --manifest "$TMP_ROOT/manifest.json" \
    --max-concurrency 2
)
assert_contains "$dispatch_dry" "resource conflict (frontend-client)"
assert_contains "$dispatch_dry" "resource conflict (api-core)"
assert_contains "$dispatch_dry" "resource conflict (route-editing)"
assert_contains "$dispatch_dry" "dispatch #5"

: >"$TMP_ROOT/state/commands.log"
"$ROOT_DIR/scripts/dispatch-kiro-issue.sh" \
  --apply \
  --allow-local-apply \
  --manifest "$TMP_ROOT/manifest.json" \
  --max-concurrency 2 >/dev/null

jq -e '
  first(.[] | select(.number == 5))
  | ([.labels[].name] | index("kiro") != null) and
    ([.labels[].name] | index("agent-dispatched") != null) and
    ([.labels[].name] | index("agent-ready") == null) and
    ([.labels[].name] | index("agent-dispatching") == null)
' "$TMP_ROOT/state/issues.json" >/dev/null || fail "dispatcher bookkeeping did not converge"

trigger_count=$(grep -Ec -- 'issue edit 5 .*--add-label kiro([[:space:]]|$)' \
  "$TMP_ROOT/state/commands.log" || true)
[[ "$trigger_count" == "1" ]] || fail "expected exactly one Kiro trigger, got $trigger_count"
if grep -E -- '--remove-label kiro([[:space:]]|$)' "$TMP_ROOT/state/commands.log" >/dev/null; then
  fail "dispatcher removed the Kiro trigger"
fi

cp "$TMP_ROOT/state/issues.json" "$TMP_ROOT/state/issues.after-dispatch.json"
jq 'map(if .number == 5 then
  .state = "CLOSED" | .closedByPullRequestsReferences = []
else . end)' "$TMP_ROOT/state/issues.after-dispatch.json" >"$TMP_ROOT/state/issues.json"
closed_unverified=$(
  "$ROOT_DIR/scripts/dispatch-kiro-issue.sh" \
    --dry-run \
    --manifest "$TMP_ROOT/manifest.json" \
    --max-concurrency 3
)
assert_contains "$closed_unverified" "ACTIVE #5"

jq 'map(if .number == 5 then
  .closedByPullRequestsReferences = [{state: "MERGED", mergedAt: "2026-01-01T00:00:00Z"}]
else . end)' "$TMP_ROOT/state/issues.json" >"$TMP_ROOT/state/issues.terminal.json"
mv "$TMP_ROOT/state/issues.terminal.json" "$TMP_ROOT/state/issues.json"
closed_terminal=$(
  "$ROOT_DIR/scripts/dispatch-kiro-issue.sh" \
    --dry-run \
    --manifest "$TMP_ROOT/manifest.json" \
    --max-concurrency 3
)
if [[ "$closed_terminal" == *"ACTIVE #5"* ]]; then
  fail "a closed issue with a merged closing PR still held resource locks"
fi
mv "$TMP_ROOT/state/issues.after-dispatch.json" "$TMP_ROOT/state/issues.json"

sync_dry=$(
  "$ROOT_DIR/scripts/sync-github-issues.sh" \
    --dry-run \
    --manifest "$TMP_ROOT/manifest.json"
)
assert_contains "$sync_dry" "dependencies_added=0 dependencies_removed=0"

jq 'map(if .number == 7 then
  .blockedBy = {
    totalCount: 2,
    nodes: [{number: 3, state: "OPEN"}, {number: 999, state: "OPEN"}]
  }
else . end)' "$TMP_ROOT/state/issues.json" >"$TMP_ROOT/state/issues.drifted.json"
mv "$TMP_ROOT/state/issues.drifted.json" "$TMP_ROOT/state/issues.json"
: >"$TMP_ROOT/state/commands.log"
"$ROOT_DIR/scripts/sync-github-issues.sh" \
  --apply \
  --manifest "$TMP_ROOT/manifest.json" >/dev/null
jq -e '
  first(.[] | select(.number == 7))
  | ([.blockedBy.nodes[].number] | sort) == [2, 999]
' "$TMP_ROOT/state/issues.json" >/dev/null ||
  fail "dependency sync did not add desired, remove stale, and preserve unmanaged blockers"
grep -E -- 'issue edit 7 .*--add-blocked-by 2([[:space:]]|$)' \
  "$TMP_ROOT/state/commands.log" >/dev/null || fail "dependency add was not issued"
grep -E -- 'issue edit 7 .*--remove-blocked-by 3([[:space:]]|$)' \
  "$TMP_ROOT/state/commands.log" >/dev/null || fail "dependency removal was not issued"

: >"$TMP_ROOT/state/commands.log"
"$ROOT_DIR/scripts/sync-github-issues.sh" \
  --apply \
  --manifest "$TMP_ROOT/manifest.json" >/dev/null
if grep -E -- '--(add|remove)-blocked-by' "$TMP_ROOT/state/commands.log" >/dev/null; then
  fail "a converged dependency rerun still mutated blocker links"
fi

jq '
  .issues |= map(
    if .id == "WEB" then .blocked_by = ["BLOCKED"] else . end
  )
' "$TMP_ROOT/manifest.json" >"$TMP_ROOT/cycle.json"
if "$ROOT_DIR/scripts/sync-github-issues.sh" \
  --dry-run \
  --manifest "$TMP_ROOT/cycle.json" >"$TMP_ROOT/cycle.out" 2>&1; then
  fail "cyclic dependency manifest unexpectedly passed"
fi
grep -F 'dependency graph has nodes blocked by a cycle' "$TMP_ROOT/cycle.out" >/dev/null ||
  fail "cycle failure did not identify the dependency cycle"

printf 'PASS: issue automation safety tests\n'
