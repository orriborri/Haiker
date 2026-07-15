#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT_DIR/issues/mvp.yaml"
MODE="dry-run"
ISSUE_SELECTOR=""
MAX_CONCURRENCY_OVERRIDE=""
ALLOW_LOCAL_APPLY="false"
ISSUE_LIMIT=1000
PR_LIMIT=1000
LABEL_LIMIT=1000

usage() {
  cat <<'EOF'
Usage: scripts/dispatch-kiro-issue.sh [--dry-run | --apply]
                                      [--manifest PATH]
                                      [--issue ID_OR_NUMBER]
                                      [--max-concurrency COUNT]
                                      [--allow-local-apply]

Select at most one agent-ready issue and dispatch it to Kiro safely.

The dispatcher accounts for dependency state, assignees, Kiro/claim labels,
open pull requests, previous Kiro timeline activity, and manifest resource
locks. Apply mode creates a non-triggering claim, revalidates all guards, then
adds the exact kiro label once as the final trigger and waits for Kiro's bot
acknowledgement. It never removes kiro.

Dry-run is the default. Prefer the serialized GitHub Actions workflow for
apply mode. Local apply is rejected unless --allow-local-apply is supplied,
because a local flock serializes processes only on the current machine.
EOF
}

die() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

while (($# > 0)); do
  case "$1" in
    --apply)
      MODE="apply"
      ;;
    --dry-run)
      MODE="dry-run"
      ;;
    --manifest)
      shift
      (($# > 0)) || die "--manifest requires a path"
      MANIFEST="$1"
      ;;
    --issue)
      shift
      (($# > 0)) || die "--issue requires an issue ID or number"
      ISSUE_SELECTOR="$1"
      ;;
    --max-concurrency)
      shift
      (($# > 0)) || die "--max-concurrency requires a positive integer"
      MAX_CONCURRENCY_OVERRIDE="$1"
      ;;
    --allow-local-apply)
      ALLOW_LOCAL_APPLY="true"
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
  shift
done

command -v gh >/dev/null || die "gh is required"
command -v jq >/dev/null || die "jq is required"
[[ -f "$MANIFEST" ]] || die "manifest not found: $MANIFEST"
[[ -z "$ISSUE_SELECTOR" || "$ISSUE_SELECTOR" =~ ^[A-Za-z0-9._-]+$ ]] ||
  die "invalid issue selector: $ISSUE_SELECTOR"
[[ -z "$MAX_CONCURRENCY_OVERRIDE" || "$MAX_CONCURRENCY_OVERRIDE" =~ ^[1-9][0-9]*$ ]] ||
  die "--max-concurrency requires a positive integer"

jq -e '
  (.repo | type == "string" and test("^[^/]+/[^/]+$")) and
  (.issues | type == "array" and length > 0) and
  ([.issues[].id] | length == (unique | length)) and
  (.dispatch | type == "object") and
  (.dispatch.trigger_label == "kiro") and
  all([
    .dispatch.ready_label,
    .dispatch.claim_label,
    .dispatch.dispatched_label,
    .dispatch.trigger_label
  ][]; type == "string" and length > 0 and (contains(",") | not)) and
  ([
    .dispatch.ready_label,
    .dispatch.claim_label,
    .dispatch.dispatched_label,
    .dispatch.trigger_label
  ] | length == (map(ascii_downcase) | unique | length)) and
  (.dispatch.max_concurrency |
    type == "number" and floor == . and . >= 1 and . <= 10) and
  (.dispatch.ack_timeout_seconds |
    type == "number" and floor == . and . >= 1 and . <= 300) and
  (.resource_groups | type == "object" and length > 0) and
  all(.resource_groups | to_entries[];
    (.key | test("^[a-z0-9][a-z0-9-]*$")) and
    (.value.capacity | type == "number" and floor == . and . >= 1)) and
  ((.resource_groups | keys) as $groups |
    all(.issues[];
      (.id | type == "string" and test("^[A-Za-z0-9._-]+$")) and
      ((.resource_locks // []) as $issue_resource_locks |
        ($issue_resource_locks | type == "array" and length == (unique | length)) and
        all($issue_resource_locks[]; . as $lock | $groups | index($lock) != null))))
' "$MANIFEST" >/dev/null || die "manifest dispatch configuration is invalid"

REPO=$(jq -r '.repo' "$MANIFEST")
READY_LABEL=$(jq -r '.dispatch.ready_label' "$MANIFEST")
CLAIM_LABEL=$(jq -r '.dispatch.claim_label' "$MANIFEST")
DISPATCHED_LABEL=$(jq -r '.dispatch.dispatched_label' "$MANIFEST")
TRIGGER_LABEL=$(jq -r '.dispatch.trigger_label' "$MANIFEST")
MAX_CONCURRENCY=$(jq -r '.dispatch.max_concurrency' "$MANIFEST")
ACK_TIMEOUT_SECONDS=$(jq -r '.dispatch.ack_timeout_seconds' "$MANIFEST")
[[ -z "$MAX_CONCURRENCY_OVERRIDE" ]] || MAX_CONCURRENCY="$MAX_CONCURRENCY_OVERRIDE"
((MAX_CONCURRENCY >= 1 && MAX_CONCURRENCY <= 10)) ||
  die "max concurrency must be between 1 and Kiro's limit of 10"

if [[ "$MODE" == "apply" ]]; then
  if [[ "${GITHUB_ACTIONS:-false}" != "true" && "$ALLOW_LOCAL_APPLY" != "true" ]]; then
    die "local apply is not globally serialized; use the workflow or pass --allow-local-apply explicitly"
  fi
  command -v flock >/dev/null ||
    die "flock is required for local apply mode; use the serialized GitHub Actions workflow"
  LOCK_NAME=${REPO//\//-}
  LOCK_FILE="${TMPDIR:-/tmp}/$LOCK_NAME-kiro-dispatch.lock"
  exec 9>"$LOCK_FILE"
  flock -n 9 || die "another local dispatcher is already running for $REPO"
fi

printf 'Target: %s\n' "$REPO"
printf 'Manifest: %s\n' "$MANIFEST"
printf 'Mode: %s\n' "$MODE"
printf 'Capacity: %s (one new dispatch maximum per invocation)\n' "$MAX_CONCURRENCY"
[[ -z "$ISSUE_SELECTOR" ]] || printf 'Requested issue: %s\n' "$ISSUE_SELECTOR"

gh auth status --hostname github.com >/dev/null
REPO_INFO=$(gh repo view "$REPO" --json nameWithOwner,viewerPermission,defaultBranchRef)
ACTUAL_REPO=$(jq -r '.nameWithOwner' <<<"$REPO_INFO")
PERMISSION=$(jq -r '.viewerPermission' <<<"$REPO_INFO")
DEFAULT_BRANCH=$(jq -r '.defaultBranchRef.name' <<<"$REPO_INFO")
[[ "$ACTUAL_REPO" == "$REPO" ]] ||
  die "repository mismatch: expected $REPO, got $ACTUAL_REPO"
[[ -n "$DEFAULT_BRANCH" && "$DEFAULT_BRANCH" != "null" ]] ||
  die "could not determine the default branch"

if [[ "$MODE" == "apply" ]]; then
  case "$PERMISSION" in
    ADMIN | MAINTAIN | WRITE) ;;
    *) die "write permission required for $REPO; current permission is $PERMISSION" ;;
  esac
else
  case "$PERMISSION" in
    ADMIN | MAINTAIN | WRITE | TRIAGE | READ) ;;
    *) die "read permission required for $REPO; current permission is $PERMISSION" ;;
  esac
fi

DEFAULT_SHA=$(gh api "repos/$REPO/commits/$DEFAULT_BRANCH" --jq '.sha')

fetch_issues() {
  gh issue list \
    --repo "$REPO" \
    --state all \
    --limit "$ISSUE_LIMIT" \
    --json number,title,state,body,url,labels,assignees,parent,blockedBy,closedByPullRequestsReferences
}

fetch_open_prs() {
  gh pr list \
    --repo "$REPO" \
    --state open \
    --limit "$PR_LIMIT" \
    --json number,title,url,headRefName,closingIssuesReferences
}

fetch_branch_names() {
  gh api \
    --paginate \
    --slurp \
    "repos/$REPO/branches?per_page=100" \
    | jq -c '[.[][] | .name]'
}

pr_issue_numbers() {
  jq -c --arg repo "$REPO" '
    def references:
      .closingIssuesReferences as $references
      | if ($references | type) == "array" then $references
        elif ($references | type) == "object" then ($references.nodes // [])
        else [] end;
    def is_local_reference($repo):
      (.repository.nameWithOwner //
       (if ((.repository.owner.login // "") != "" and
            (.repository.name // "") != "") then
          "\(.repository.owner.login)/\(.repository.name)"
        else "" end)) as $reference_repo
      | $reference_repo == $repo;
    [.[] | references[] | select(is_local_reference($repo)) | .number] | unique
  '
}

build_issue_rows() {
  local issues=$1
  local pr_numbers=$2
  jq -c \
    --slurpfile manifests "$MANIFEST" \
    --arg ready "$READY_LABEL" \
    --arg claim "$CLAIM_LABEL" \
    --arg dispatched "$DISPATCHED_LABEL" \
    --arg trigger "$TRIGGER_LABEL" \
    --argjson pr_numbers "$pr_numbers" '
    def marker_id:
      (.body // "") as $body
      | try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch "";
    def has_label($name):
      [(.labels // [])[].name | ascii_downcase] | index($name | ascii_downcase) != null;
    def pull_requests($value):
      if ($value | type) == "array" then $value
      elif ($value | type) == "object" then ($value.nodes // [])
      else [] end;
    . as $issues
    | $manifests[0] as $manifest
    | [
        $manifest.issues[]
        | select((.parent_id // "") != "")
        | . as $definition
        | ($issues[] | select(marker_id == $definition.id)) as $remote
        | ([((($remote.blockedBy.nodes // [])[])) |
              select((.state | ascii_downcase) == "open")] | length) as $open_blockers
        | (($remote.assignees // []) | length) as $assignee_count
        | ($remote | has_label($ready)) as $has_ready
        | ($remote | has_label($claim)) as $has_claim
        | ($remote | has_label($dispatched)) as $has_dispatched
        | ($remote | has_label($trigger)) as $has_trigger
        | ($pr_numbers | index($remote.number) != null) as $has_open_pr
        | (($remote.state | ascii_downcase) == "closed") as $closed
        | ((pull_requests($remote.closedByPullRequestsReferences) | length) > 0) as $closed_by_merged_pr
        | ($closed and $closed_by_merged_pr) as $terminal
        | (((($has_claim or $has_dispatched) and ($terminal | not)) or
            $has_open_pr) or
           ((($remote.state | ascii_downcase) == "open") and
            ($has_trigger or ($assignee_count > 0)))) as $active
        | (((($remote.state | ascii_downcase) == "open") and
            ($open_blockers == 0) and
            $has_ready and
            ($active | not))) as $queued
        | {
            id: $definition.id,
            number: $remote.number,
            title: $remote.title,
            state: $remote.state,
            resource_locks: ($definition.resource_locks // []),
            open_blockers: $open_blockers,
            assignee_count: $assignee_count,
            has_ready: $has_ready,
            has_claim: $has_claim,
            has_dispatched: $has_dispatched,
            has_trigger: $has_trigger,
            has_open_pr: $has_open_pr,
            terminal: $terminal,
            active: $active,
            queued: $queued,
            active_reason: ([
              if $has_claim then "claim" else empty end,
              if $has_dispatched then "dispatched" else empty end,
              if $has_trigger then "kiro" else empty end,
              if $assignee_count > 0 then "assigned" else empty end,
              if $has_open_pr then "open PR" else empty end
            ] | join("+"))
          }
      ]
    | sort_by(.number)
  ' <<<"$issues"
}

validate_issue_snapshot() {
  local issues=$1
  local count duplicate_ids missing_ids incomplete
  local managed_ids remote_ids

  count=$(jq 'length' <<<"$issues")
  ((count < ISSUE_LIMIT)) ||
    die "issue query reached its $ISSUE_LIMIT issue safety limit"
  managed_ids=$(jq -c '[.issues[].id]' "$MANIFEST")
  remote_ids=$(jq -c '
    [
      .[]
      | (.body // "") as $body
      | try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch empty
    ]
  ' <<<"$issues")
  duplicate_ids=$(jq -r '
    group_by(.) | map(select(length > 1) | .[0]) | join(", ")
  ' <<<"$remote_ids")
  [[ -z "$duplicate_ids" ]] || die "duplicate managed issue IDs found: $duplicate_ids"
  missing_ids=$(jq -nr \
    --argjson expected "$managed_ids" \
    --argjson actual "$remote_ids" \
    '$expected - $actual | join(", ")')
  [[ -z "$missing_ids" ]] ||
    die "managed issues missing from GitHub response: $missing_ids"
  incomplete=$(jq -r --argjson managed_ids "$managed_ids" '
    def valid_blocked_by:
      (.blockedBy | type) == "object" and
      (.blockedBy.totalCount | type) == "number" and
      ((.blockedBy.totalCount | floor) == .blockedBy.totalCount) and
      (.blockedBy.totalCount >= 0) and
      (.blockedBy.nodes | type) == "array" and
      (.blockedBy.totalCount == (.blockedBy.nodes | length)) and
      all(.blockedBy.nodes[];
        (.number | type) == "number" and
        (.state | type) == "string");
    [
      .[]
      | (.body // "") as $body
      | (try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch "") as $id
      | select($managed_ids | index($id) != null)
      | select(valid_blocked_by | not)
      | "#\(.number)"
    ]
    | join(", ")
  ' <<<"$issues")
  [[ -z "$incomplete" ]] ||
    die "invalid or incomplete blockedBy data for $incomplete"
}

validate_pr_snapshot() {
  local prs=$1
  local count incomplete
  count=$(jq 'length' <<<"$prs")
  ((count < PR_LIMIT)) || die "pull request query reached its $PR_LIMIT safety limit"
  incomplete=$(jq -r '
    [
      .[]
      | select(
          (((.closingIssuesReferences | type) == "object") and
           (.closingIssuesReferences.pageInfo.hasNextPage // false)) or
          (((.closingIssuesReferences | type) == "array") and
           ((.closingIssuesReferences | length) >= 100))
        )
      | "#\(.number)"
    ]
    | join(", ")
  ' <<<"$prs")
  [[ -z "$incomplete" ]] ||
    die "incomplete closing-issue data for pull requests $incomplete"
}

dispatch_history_reason() {
  local number=$1
  gh api \
    -H "Accept: application/vnd.github+json" \
    --paginate \
    --slurp \
    "repos/$REPO/issues/$number/timeline?per_page=100" \
    | jq -r '
      [
        .[][]
        | if ((.event // "") == "labeled" and
              ((.label.name // "") | ascii_downcase) == "kiro") then
            "previous kiro label"
          elif (((.user.login // .actor.login // "") | ascii_downcase) ==
                "kiro-agent[bot]" and
                ((.body // "") | test("https://app\\.kiro\\.dev/session/"))) then
            "Kiro bot acknowledgement"
          elif (((.user.login // .actor.login // "") | ascii_downcase) ==
                "kiro-agent[bot]") then
            "Kiro bot activity"
          elif ((.body // "") |
                test("(^|[[:space:]])/kiro([[:space:]]|$)"; "i")) then
            "previous /kiro command"
          else empty end
      ]
      | unique
      | join(", ")
    '
}

wait_for_kiro_acknowledgement() {
  local number=$1
  local deadline=$((SECONDS + ACK_TIMEOUT_SECONDS))
  local history

  while true; do
    history=$(dispatch_history_reason "$number")
    if [[ "$history" == *"Kiro bot acknowledgement"* ]]; then
      return 0
    fi
    ((SECONDS >= deadline)) && return 1
    sleep 5
  done
}

matching_issue_branches() {
  local number=$1
  local branches=$2
  jq -r --arg number "$number" '
    [
      .[]
      | select(test("(^|[-_/])issue[-_/]?" + $number + "($|[-_/])"; "i"))
    ]
    | join(", ")
  ' <<<"$branches"
}

enrich_issue_rows_with_external_activity() {
  local rows=$1
  local branches=$2
  local row number history branch_matches reason

  while IFS= read -r row; do
    number=$(jq -r '.number' <<<"$row")
    history=$(dispatch_history_reason "$number")
    branch_matches=$(matching_issue_branches "$number" "$branches")
    reason=""
    if [[ -n "$history" ]]; then
      reason="history:$history"
    fi
    if [[ -n "$branch_matches" ]]; then
      [[ -z "$reason" ]] || reason+="+"
      reason+="branch:$branch_matches"
    fi
    [[ -n "$reason" ]] || continue

    rows=$(jq -c \
      --argjson number "$number" \
      --arg reason "$reason" '
      map(if .number == $number then
        .active = true
        | .queued = false
        | .active_reason = $reason
      else . end)
    ' <<<"$rows")
  done < <(jq -c '
    .[] | select((.state | ascii_downcase) == "open" and (.active | not))
  ' <<<"$rows")

  printf '%s' "$rows"
}

resource_conflicts() {
  local locks=$1
  local usage=$2
  jq -nr \
    --slurpfile manifests "$MANIFEST" \
    --argjson locks "$locks" \
    --argjson usage "$usage" '
    $manifests[0].resource_groups as $groups
    | [
        $locks[] as $lock
        | select(($usage[$lock] // 0) >= $groups[$lock].capacity)
        | $lock
      ]
    | join(", ")
  '
}

remote_label_name() {
  local labels=$1
  local configured=$2
  local count
  count=$(jq --arg label "$configured" '
    [.[] | select((.name | ascii_downcase) == ($label | ascii_downcase))] | length
  ' <<<"$labels")
  ((count <= 1)) || die "multiple case-insensitive label matches found for $configured"
  jq -r --arg label "$configured" '
    first(.[] | select((.name | ascii_downcase) == ($label | ascii_downcase)) | .name) // empty
  ' <<<"$labels"
}

REMOTE_LABELS=$(gh label list \
  --repo "$REPO" \
  --limit "$LABEL_LIMIT" \
  --json name,color,description)
REMOTE_LABEL_COUNT=$(jq 'length' <<<"$REMOTE_LABELS")
((REMOTE_LABEL_COUNT < LABEL_LIMIT)) ||
  die "label query reached its $LABEL_LIMIT label safety limit"

ACTUAL_READY_LABEL=$(remote_label_name "$REMOTE_LABELS" "$READY_LABEL")
ACTUAL_CLAIM_LABEL=$(remote_label_name "$REMOTE_LABELS" "$CLAIM_LABEL")
ACTUAL_DISPATCHED_LABEL=$(remote_label_name "$REMOTE_LABELS" "$DISPATCHED_LABEL")
ACTUAL_TRIGGER_LABEL=$(remote_label_name "$REMOTE_LABELS" "$TRIGGER_LABEL")

if [[ "$MODE" == "apply" ]]; then
  [[ -n "$ACTUAL_READY_LABEL" ]] || die "label $READY_LABEL is missing; run the issue sync first"
  [[ -n "$ACTUAL_CLAIM_LABEL" ]] || die "label $CLAIM_LABEL is missing; run the issue sync first"
  [[ -n "$ACTUAL_DISPATCHED_LABEL" ]] || die "label $DISPATCHED_LABEL is missing; run the issue sync first"
  [[ "$ACTUAL_TRIGGER_LABEL" == "$TRIGGER_LABEL" ]] ||
    die "the exact Kiro trigger label $TRIGGER_LABEL is missing"
else
  [[ -n "$ACTUAL_READY_LABEL" ]] || ACTUAL_READY_LABEL="$READY_LABEL"
  [[ -n "$ACTUAL_CLAIM_LABEL" ]] || ACTUAL_CLAIM_LABEL="$CLAIM_LABEL"
  [[ -n "$ACTUAL_DISPATCHED_LABEL" ]] || ACTUAL_DISPATCHED_LABEL="$DISPATCHED_LABEL"
  [[ -n "$ACTUAL_TRIGGER_LABEL" ]] || ACTUAL_TRIGGER_LABEL="$TRIGGER_LABEL"
fi

# Use the repository's actual casing in all subsequent comparisons and writes.
READY_LABEL="$ACTUAL_READY_LABEL"
CLAIM_LABEL="$ACTUAL_CLAIM_LABEL"
DISPATCHED_LABEL="$ACTUAL_DISPATCHED_LABEL"
TRIGGER_LABEL="$ACTUAL_TRIGGER_LABEL"

REMOTE_ISSUES=$(fetch_issues)
OPEN_PRS=$(fetch_open_prs)
BRANCH_NAMES=$(fetch_branch_names)
validate_issue_snapshot "$REMOTE_ISSUES"
validate_pr_snapshot "$OPEN_PRS"
OPEN_PR_ISSUE_NUMBERS=$(pr_issue_numbers <<<"$OPEN_PRS")
ISSUE_ROWS=$(build_issue_rows "$REMOTE_ISSUES" "$OPEN_PR_ISSUE_NUMBERS")
ISSUE_ROWS=$(enrich_issue_rows_with_external_activity "$ISSUE_ROWS" "$BRANCH_NAMES")

MANAGED_NUMBERS=$(jq -c '[.[].number]' <<<"$ISSUE_ROWS")
UNKNOWN_PR_COUNT=$(jq --arg repo "$REPO" --argjson managed "$MANAGED_NUMBERS" '
  def references:
    .closingIssuesReferences as $references
    | if ($references | type) == "array" then $references
      elif ($references | type) == "object" then ($references.nodes // [])
      else [] end;
  def is_local_reference($repo):
    (.repository.nameWithOwner //
     (if ((.repository.owner.login // "") != "" and
          (.repository.name // "") != "") then
        "\(.repository.owner.login)/\(.repository.name)"
      else "" end)) as $reference_repo
    | $reference_repo == $repo;
  [
    .[]
    | [references[] | select(is_local_reference($repo)) | .number] as $numbers
    | select(([
        $numbers[] as $number
        | select($managed | index($number) != null)
      ] | length) == 0)
  ]
  | length
' <<<"$OPEN_PRS")
ACTIVE_COUNT=$(jq '[.[] | select(.active)] | length' <<<"$ISSUE_ROWS")
UNCLASSIFIED_ACTIVE_COUNT=$(jq '
  [.[] | select(.active and ((.resource_locks | length) == 0))] | length
' <<<"$ISSUE_ROWS")
TOTAL_ACTIVE=$((ACTIVE_COUNT + UNKNOWN_PR_COUNT))
UNKNOWN_ACTIVE=$((UNCLASSIFIED_ACTIVE_COUNT + UNKNOWN_PR_COUNT))
ACTIVE_LOCK_USAGE=$(jq -c '
  [
    .[]
    | select(.active)
    | .resource_locks[]
  ]
  | group_by(.)
  | map({key: .[0], value: length})
  | from_entries
' <<<"$ISSUE_ROWS")

while IFS= read -r active; do
  number=$(jq -r '.number' <<<"$active")
  id=$(jq -r '.id' <<<"$active")
  reason=$(jq -r '.active_reason' <<<"$active")
  locks=$(jq -r '.resource_locks | if length == 0 then "unclassified" else join(",") end' <<<"$active")
  title=$(jq -r '.title' <<<"$active")
  printf 'ACTIVE #%-4s %-10s %-24s locks=%-32s %s\n' \
    "$number" "$id" "$reason" "$locks" "$title"
done < <(jq -c '.[] | select(.active)' <<<"$ISSUE_ROWS")
((UNKNOWN_PR_COUNT == 0)) ||
  printf 'ACTIVE %-16s %s open pull request(s) are not linked to managed issues\n' \
    "unclassified" "$UNKNOWN_PR_COUNT"

if ((TOTAL_ACTIVE >= MAX_CONCURRENCY)); then
  printf '\nNo dispatch: capacity is full (%s/%s active).\n' \
    "$TOTAL_ACTIVE" "$MAX_CONCURRENCY"
  exit 0
fi

if [[ -n "$ISSUE_SELECTOR" ]]; then
  MATCH_COUNT=$(jq --arg selector "$ISSUE_SELECTOR" '
    [.[] | select(.id == $selector or (.number | tostring) == $selector)] | length
  ' <<<"$ISSUE_ROWS")
  ((MATCH_COUNT == 1)) || die "managed child issue not found: $ISSUE_SELECTOR"
  CANDIDATE_ROWS=$(jq -c --arg selector "$ISSUE_SELECTOR" '
    [.[] | select(.id == $selector or (.number | tostring) == $selector)]
  ' <<<"$ISSUE_ROWS")
else
  CANDIDATE_ROWS=$(jq -c '[.[] | select(.queued)]' <<<"$ISSUE_ROWS")
fi

SELECTED=""
while IFS= read -r candidate; do
  id=$(jq -r '.id' <<<"$candidate")
  number=$(jq -r '.number' <<<"$candidate")
  title=$(jq -r '.title' <<<"$candidate")
  queued=$(jq -r '.queued' <<<"$candidate")

  if [[ "$queued" != "true" ]]; then
    reason=$(jq -r '
      if (.state | ascii_downcase) != "open" then "closed"
      elif .active then "already active: " + .active_reason
      elif (.open_blockers > 0) then "has open blockers"
      elif (.has_ready | not) then "missing agent-ready"
      else "not dispatchable" end
    ' <<<"$candidate")
    printf 'SKIP   #%-4s %-10s %s: %s\n' "$number" "$id" "$reason" "$title"
    continue
  fi

  locks=$(jq -c '.resource_locks' <<<"$candidate")
  lock_count=$(jq 'length' <<<"$locks")
  if ((lock_count == 0 && TOTAL_ACTIVE > 0)); then
    printf 'SKIP   #%-4s %-10s unclassified while other work is active: %s\n' \
      "$number" "$id" "$title"
    continue
  fi
  if ((UNKNOWN_ACTIVE > 0)); then
    printf 'SKIP   #%-4s %-10s active work has unknown resource locks: %s\n' \
      "$number" "$id" "$title"
    continue
  fi

  conflicts=$(resource_conflicts "$locks" "$ACTIVE_LOCK_USAGE")
  if [[ -n "$conflicts" ]]; then
    printf 'SKIP   #%-4s %-10s resource conflict (%s): %s\n' \
      "$number" "$id" "$conflicts" "$title"
    continue
  fi

  SELECTED="$candidate"
  break
done < <(jq -c '.[]' <<<"$CANDIDATE_ROWS")

if [[ -z "$SELECTED" ]]; then
  printf '\nNo safe agent-ready issue can be dispatched.\n'
  exit 0
fi

SELECTED_ID=$(jq -r '.id' <<<"$SELECTED")
SELECTED_NUMBER=$(jq -r '.number' <<<"$SELECTED")
SELECTED_TITLE=$(jq -r '.title' <<<"$SELECTED")
SELECTED_LOCKS=$(jq -c '.resource_locks' <<<"$SELECTED")
SELECTED_LOCK_TEXT=$(jq -r 'if length == 0 then "unclassified/exclusive" else join(",") end' <<<"$SELECTED_LOCKS")

if [[ "$MODE" == "dry-run" ]]; then
  printf '\nPLAN   dispatch #%-4s %-10s locks=%s: %s\n' \
    "$SELECTED_NUMBER" "$SELECTED_ID" "$SELECTED_LOCK_TEXT" "$SELECTED_TITLE"
  printf '       apply order: add %s -> revalidate -> add %s once -> add %s -> remove readiness/claim\n' \
    "$CLAIM_LABEL" "$TRIGGER_LABEL" "$DISPATCHED_LABEL"
  exit 0
fi

printf '\nCLAIM  #%-4s %-10s add %s: %s\n' \
  "$SELECTED_NUMBER" "$SELECTED_ID" "$CLAIM_LABEL" "$SELECTED_TITLE"
gh issue edit "$SELECTED_NUMBER" \
  --repo "$REPO" \
  --add-label "$CLAIM_LABEL" >/dev/null

release_claim_and_die() {
  local message=$1
  if gh issue edit "$SELECTED_NUMBER" \
    --repo "$REPO" \
    --remove-label "$CLAIM_LABEL" >/dev/null; then
    die "$message; claim released before triggering Kiro"
  fi
  die "$message; failed to release claim, inspect #$SELECTED_NUMBER manually"
}

CURRENT_ISSUE=$(gh issue view "$SELECTED_NUMBER" \
  --repo "$REPO" \
  --json number,state,body,labels,assignees,parent,blockedBy)
CURRENT_ID=$(jq -r '
  (.body // "") as $body
  | try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch ""
' <<<"$CURRENT_ISSUE")
[[ "$CURRENT_ID" == "$SELECTED_ID" ]] ||
  release_claim_and_die "managed marker changed for #$SELECTED_NUMBER"
jq -e '
  (.blockedBy | type) == "object" and
  (.blockedBy.totalCount | type) == "number" and
  ((.blockedBy.totalCount | floor) == .blockedBy.totalCount) and
  (.blockedBy.totalCount >= 0) and
  (.blockedBy.nodes | type) == "array" and
  (.blockedBy.totalCount == (.blockedBy.nodes | length)) and
  all(.blockedBy.nodes[];
    (.number | type) == "number" and
    (.state | type) == "string")
' <<<"$CURRENT_ISSUE" >/dev/null ||
  release_claim_and_die "invalid or incomplete blockedBy data for #$SELECTED_NUMBER"

CURRENT_VALID=$(jq -r \
  --arg ready "$READY_LABEL" \
  --arg claim "$CLAIM_LABEL" \
  --arg dispatched "$DISPATCHED_LABEL" \
  --arg trigger "$TRIGGER_LABEL" '
  def has_label($name):
    [(.labels // [])[].name | ascii_downcase] | index($name | ascii_downcase) != null;
  ((.state | ascii_downcase) == "open") and
  (([(.blockedBy.nodes // [])[] | select((.state | ascii_downcase) == "open")] | length) == 0) and
  (((.assignees // []) | length) == 0) and
  has_label($ready) and
  has_label($claim) and
  (has_label($dispatched) | not) and
  (has_label($trigger) | not)
' <<<"$CURRENT_ISSUE")
[[ "$CURRENT_VALID" == "true" ]] ||
  release_claim_and_die "issue #$SELECTED_NUMBER changed after it was selected"

CURRENT_ISSUES=$(fetch_issues)
CURRENT_OPEN_PRS=$(fetch_open_prs)
CURRENT_BRANCH_NAMES=$(fetch_branch_names)
validate_issue_snapshot "$CURRENT_ISSUES"
validate_pr_snapshot "$CURRENT_OPEN_PRS"
CURRENT_PR_ISSUE_NUMBERS=$(pr_issue_numbers <<<"$CURRENT_OPEN_PRS")
CURRENT_ROWS=$(build_issue_rows "$CURRENT_ISSUES" "$CURRENT_PR_ISSUE_NUMBERS")
CURRENT_ROWS=$(enrich_issue_rows_with_external_activity "$CURRENT_ROWS" "$CURRENT_BRANCH_NAMES")
OTHER_ROWS=$(jq -c --argjson selected "$SELECTED_NUMBER" '
  [.[] | select(.number != $selected)]
' <<<"$CURRENT_ROWS")
CURRENT_MANAGED_NUMBERS=$(jq -c '[.[].number]' <<<"$CURRENT_ROWS")
CURRENT_UNKNOWN_PR_COUNT=$(jq --arg repo "$REPO" --argjson managed "$CURRENT_MANAGED_NUMBERS" '
  def references:
    .closingIssuesReferences as $references
    | if ($references | type) == "array" then $references
      elif ($references | type) == "object" then ($references.nodes // [])
      else [] end;
  def is_local_reference($repo):
    (.repository.nameWithOwner //
     (if ((.repository.owner.login // "") != "" and
          (.repository.name // "") != "") then
        "\(.repository.owner.login)/\(.repository.name)"
      else "" end)) as $reference_repo
    | $reference_repo == $repo;
  [
    .[]
    | [references[] | select(is_local_reference($repo)) | .number] as $numbers
    | select(([
        $numbers[] as $number
        | select($managed | index($number) != null)
      ] | length) == 0)
  ]
  | length
' <<<"$CURRENT_OPEN_PRS")
OTHER_ACTIVE_COUNT=$(jq '[.[] | select(.active)] | length' <<<"$OTHER_ROWS")
OTHER_UNCLASSIFIED_COUNT=$(jq '
  [.[] | select(.active and ((.resource_locks | length) == 0))] | length
' <<<"$OTHER_ROWS")
OTHER_TOTAL_ACTIVE=$((OTHER_ACTIVE_COUNT + CURRENT_UNKNOWN_PR_COUNT))
OTHER_UNKNOWN_ACTIVE=$((OTHER_UNCLASSIFIED_COUNT + CURRENT_UNKNOWN_PR_COUNT))
((OTHER_TOTAL_ACTIVE < MAX_CONCURRENCY)) ||
  release_claim_and_die "dispatch capacity changed during revalidation"
((OTHER_UNKNOWN_ACTIVE == 0)) ||
  release_claim_and_die "unclassified work appeared during revalidation"

OTHER_LOCK_USAGE=$(jq -c '
  [.[] | select(.active) | .resource_locks[]]
  | group_by(.)
  | map({key: .[0], value: length})
  | from_entries
' <<<"$OTHER_ROWS")
CURRENT_CONFLICTS=$(resource_conflicts "$SELECTED_LOCKS" "$OTHER_LOCK_USAGE")
[[ -z "$CURRENT_CONFLICTS" ]] ||
  release_claim_and_die "resource conflict appeared during revalidation: $CURRENT_CONFLICTS"

if (( $(jq 'length' <<<"$SELECTED_LOCKS") == 0 && OTHER_TOTAL_ACTIVE > 0 )); then
  release_claim_and_die "unclassified issue cannot run beside active work"
fi
[[ $(jq --argjson selected "$SELECTED_NUMBER" 'index($selected) == null' \
  <<<"$CURRENT_PR_ISSUE_NUMBERS") == "true" ]] ||
  release_claim_and_die "an open pull request appeared for #$SELECTED_NUMBER"

CURRENT_HISTORY=$(dispatch_history_reason "$SELECTED_NUMBER")
[[ -z "$CURRENT_HISTORY" ]] ||
  release_claim_and_die "dispatch evidence appeared during revalidation: $CURRENT_HISTORY"
CURRENT_BRANCH_MATCHES=$(matching_issue_branches "$SELECTED_NUMBER" "$CURRENT_BRANCH_NAMES")
[[ -z "$CURRENT_BRANCH_MATCHES" ]] ||
  release_claim_and_die "an issue branch appeared during revalidation: $CURRENT_BRANCH_MATCHES"
CURRENT_DEFAULT_SHA=$(gh api "repos/$REPO/commits/$DEFAULT_BRANCH" --jq '.sha')
[[ "$CURRENT_DEFAULT_SHA" == "$DEFAULT_SHA" ]] ||
  release_claim_and_die "default branch changed during dispatch planning"

printf 'TRIGGER #%-4s %-10s add %s exactly once: %s\n' \
  "$SELECTED_NUMBER" "$SELECTED_ID" "$TRIGGER_LABEL" "$SELECTED_TITLE"
if ! gh issue edit "$SELECTED_NUMBER" \
  --repo "$REPO" \
  --add-label "$TRIGGER_LABEL" >/dev/null; then
  die "Kiro trigger result is uncertain for #$SELECTED_NUMBER; claim left in place, inspect the timeline and never retry blindly"
fi

if ! wait_for_kiro_acknowledgement "$SELECTED_NUMBER"; then
  die "Kiro did not acknowledge #$SELECTED_NUMBER within ${ACK_TIMEOUT_SECONDS}s; claim left in place, inspect the timeline and never re-add kiro"
fi
printf 'ACK    #%-4s %-10s Kiro bot acknowledged the task\n' \
  "$SELECTED_NUMBER" "$SELECTED_ID"

post_trigger_error=0
gh issue edit "$SELECTED_NUMBER" \
  --repo "$REPO" \
  --add-label "$DISPATCHED_LABEL" >/dev/null || post_trigger_error=1
gh issue edit "$SELECTED_NUMBER" \
  --repo "$REPO" \
  --remove-label "$READY_LABEL" >/dev/null || post_trigger_error=1
gh issue edit "$SELECTED_NUMBER" \
  --repo "$REPO" \
  --remove-label "$CLAIM_LABEL" >/dev/null || post_trigger_error=1

if ((post_trigger_error > 0)); then
  die "Kiro was triggered for #$SELECTED_NUMBER, but bookkeeping did not fully converge; do not re-add kiro"
fi

printf 'DONE   #%-4s %-10s dispatched to Kiro with locks=%s\n' \
  "$SELECTED_NUMBER" "$SELECTED_ID" "$SELECTED_LOCK_TEXT"
