#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT_DIR/issues/mvp.yaml"
MODE="dry-run"
ISSUE_LIMIT=1000
LABEL_LIMIT=1000

usage() {
  cat <<'EOF'
Usage: scripts/label-unblocked-issues.sh [--dry-run | --apply]
                                         [--manifest PATH]

Reconcile the non-triggering agent-ready label on managed child issues.

An issue is ready when it is open, has no open GitHub blockers, has no
assignee, and has not been claimed or dispatched. The script adds agent-ready
to eligible issues and removes it when those conditions stop being true. It
never adds, removes, or otherwise changes the Kiro trigger label.

Run the dependency sync before this reconciler. Dry-run is the default.
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
  (. as $manifest |
    all([
      .dispatch.ready_label,
      .dispatch.claim_label,
      .dispatch.dispatched_label
    ][]; . as $name |
      any($manifest.labels[]; (.name | ascii_downcase) == ($name | ascii_downcase)))) and
  (.issues as $issues |
    all($issues[]; . as $issue |
      ($issue.id | type == "string" and test("^[A-Za-z0-9._-]+$")) and
      (($issue.parent_id // "") | type == "string") and
      (($issue.parent_id // "") == "" or
        ($issue.parent_id != $issue.id and
          any($issues[];
            .id == $issue.parent_id and ((.parent_id // "") == ""))))
    )
  )
' "$MANIFEST" >/dev/null || die "manifest readiness configuration is invalid"

REPO=$(jq -r '.repo' "$MANIFEST")
READY_LABEL=$(jq -r '.dispatch.ready_label' "$MANIFEST")
CLAIM_LABEL=$(jq -r '.dispatch.claim_label' "$MANIFEST")
DISPATCHED_LABEL=$(jq -r '.dispatch.dispatched_label' "$MANIFEST")
TRIGGER_LABEL=$(jq -r '.dispatch.trigger_label' "$MANIFEST")
READY_COLOR=$(jq -r --arg name "$READY_LABEL" '
  first(.labels[] | select((.name | ascii_downcase) == ($name | ascii_downcase)) | .color)
' "$MANIFEST")
READY_DESCRIPTION=$(jq -r --arg name "$READY_LABEL" '
  first(.labels[] | select((.name | ascii_downcase) == ($name | ascii_downcase)) | (.description // ""))
' "$MANIFEST")
MANAGED_IDS=$(jq -c '[.issues[].id]' "$MANIFEST")
MANAGED_CHILD_IDS=$(jq -c '[.issues[] | select((.parent_id // "") != "") | .id]' "$MANIFEST")

printf 'Target: %s\n' "$REPO"
printf 'Manifest: %s\n' "$MANIFEST"
printf 'Mode: %s\n' "$MODE"
printf 'Readiness label: %s (Kiro trigger remains untouched: %s)\n' \
  "$READY_LABEL" "$TRIGGER_LABEL"

gh auth status --hostname github.com >/dev/null
REPO_INFO=$(gh repo view "$REPO" --json nameWithOwner,viewerPermission)
ACTUAL_REPO=$(jq -r '.nameWithOwner' <<<"$REPO_INFO")
PERMISSION=$(jq -r '.viewerPermission' <<<"$REPO_INFO")
[[ "$ACTUAL_REPO" == "$REPO" ]] ||
  die "repository mismatch: expected $REPO, got $ACTUAL_REPO"

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

REMOTE_ISSUES=$(gh issue list \
  --repo "$REPO" \
  --state all \
  --limit "$ISSUE_LIMIT" \
  --json number,title,state,body,url,labels,assignees,parent,blockedBy)
REMOTE_LABELS=$(gh label list \
  --repo "$REPO" \
  --limit "$LABEL_LIMIT" \
  --json name,color,description)

REMOTE_ISSUE_COUNT=$(jq 'length' <<<"$REMOTE_ISSUES")
REMOTE_LABEL_COUNT=$(jq 'length' <<<"$REMOTE_LABELS")
((REMOTE_ISSUE_COUNT < ISSUE_LIMIT)) ||
  die "issue query reached its $ISSUE_LIMIT issue safety limit"
((REMOTE_LABEL_COUNT < LABEL_LIMIT)) ||
  die "label query reached its $LABEL_LIMIT label safety limit"

REMOTE_MANAGED_IDS=$(jq -c '
  [
    .[]
    | (.body // "") as $body
    | try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch empty
  ]
' <<<"$REMOTE_ISSUES")
DUPLICATE_IDS=$(jq -r '
  group_by(.) | map(select(length > 1) | .[0]) | join(", ")
' <<<"$REMOTE_MANAGED_IDS")
[[ -z "$DUPLICATE_IDS" ]] || die "duplicate managed issue IDs found: $DUPLICATE_IDS"

MISSING_IDS=$(jq -nr \
  --argjson expected "$MANAGED_IDS" \
  --argjson actual "$REMOTE_MANAGED_IDS" \
  '$expected - $actual | join(", ")')
[[ -z "$MISSING_IDS" ]] ||
  die "managed issues missing from GitHub response: $MISSING_IDS"

INVALID_DEPENDENCIES=$(jq -r --argjson child_ids "$MANAGED_CHILD_IDS" '
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
    | select($child_ids | index($id) != null)
    | select(valid_blocked_by | not)
    | "#\(.number)"
  ]
  | join(", ")
' <<<"$REMOTE_ISSUES")
[[ -z "$INVALID_DEPENDENCIES" ]] ||
  die "invalid or incomplete blockedBy data for $INVALID_DEPENDENCIES; refusing to reconcile"

LABEL_MATCH_COUNT=$(jq --arg label "$READY_LABEL" '
  [.[] | select((.name | ascii_downcase) == ($label | ascii_downcase))] | length
' <<<"$REMOTE_LABELS")
((LABEL_MATCH_COUNT <= 1)) ||
  die "multiple case-insensitive label matches found for $READY_LABEL"
ACTUAL_READY_LABEL=$(jq -r --arg label "$READY_LABEL" '
  first(.[] | select((.name | ascii_downcase) == ($label | ascii_downcase)) | .name) // empty
' <<<"$REMOTE_LABELS")
[[ -n "$ACTUAL_READY_LABEL" ]] || ACTUAL_READY_LABEL="$READY_LABEL"

ROWS=$(jq -c \
  --arg ready "$ACTUAL_READY_LABEL" \
  --arg claim "$CLAIM_LABEL" \
  --arg dispatched "$DISPATCHED_LABEL" \
  --arg trigger "$TRIGGER_LABEL" \
  --argjson child_ids "$MANAGED_CHILD_IDS" '
  def has_label($name):
    [(.labels // [])[].name | ascii_downcase] | index($name | ascii_downcase) != null;
  [
    .[]
    | (.body // "") as $body
    | (try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch "") as $id
    | select($child_ids | index($id) != null)
    | ([((.blockedBy.nodes // [])[]) | select((.state | ascii_downcase) == "open")] | length) as $open_blockers
    | ((.assignees // []) | length) as $assignee_count
    | (has_label($ready)) as $has_ready
    | (has_label($claim)) as $has_claim
    | (has_label($dispatched)) as $has_dispatched
    | (has_label($trigger)) as $has_trigger
    | (((.state | ascii_downcase) == "open") and
       ($open_blockers == 0) and
       ($assignee_count == 0) and
       ($has_claim | not) and
       ($has_dispatched | not) and
       ($has_trigger | not)) as $eligible
    | {
        id: $id,
        number,
        title,
        state,
        open_blockers: $open_blockers,
        assignee_count: $assignee_count,
        has_ready: $has_ready,
        has_claim: $has_claim,
        has_dispatched: $has_dispatched,
        has_trigger: $has_trigger,
        eligible: $eligible,
        action: (if $eligible and ($has_ready | not) then "add"
                 elif ($eligible | not) and $has_ready then "remove"
                 else "keep" end),
        reason: (if (.state | ascii_downcase) != "open" then "closed"
                 elif $open_blockers > 0 then "open blockers"
                 elif $assignee_count > 0 then "assigned"
                 elif $has_claim then "claimed"
                 elif $has_dispatched then "dispatched"
                 elif $has_trigger then "Kiro already triggered"
                 else "eligible" end)
      }
  ]
  | sort_by(.number)
' <<<"$REMOTE_ISSUES")

ELIGIBLE_COUNT=$(jq '[.[] | select(.eligible)] | length' <<<"$ROWS")
ADD_COUNT=$(jq '[.[] | select(.action == "add")] | length' <<<"$ROWS")
REMOVE_COUNT=$(jq '[.[] | select(.action == "remove")] | length' <<<"$ROWS")

if ((ADD_COUNT > 0 && LABEL_MATCH_COUNT == 0)); then
  if [[ "$MODE" == "apply" ]]; then
    gh label create "$READY_LABEL" \
      --repo "$REPO" \
      --color "$READY_COLOR" \
      --description "$READY_DESCRIPTION" >/dev/null
    printf 'CREATE label: %s\n' "$READY_LABEL"
  else
    printf 'PLAN   create label: %s\n' "$READY_LABEL"
  fi
fi

added=0
removed=0
kept=0
planned_add=0
planned_remove=0
skipped=0

while IFS= read -r issue; do
  id=$(jq -r '.id' <<<"$issue")
  number=$(jq -r '.number' <<<"$issue")
  title=$(jq -r '.title' <<<"$issue")
  action=$(jq -r '.action' <<<"$issue")
  reason=$(jq -r '.reason' <<<"$issue")
  snapshot_has_ready=$(jq -r '.has_ready' <<<"$issue")

  if [[ "$MODE" == "dry-run" ]]; then
    if [[ "$action" == "keep" ]]; then
      ((kept += 1))
      printf 'KEEP   #%-4s %-10s %-22s %s\n' "$number" "$id" "$reason" "$title"
    elif [[ "$action" == "add" ]]; then
      ((planned_add += 1))
      printf 'PLAN   #%-4s %-10s add %-18s %s\n' \
        "$number" "$id" "$ACTUAL_READY_LABEL" "$title"
    else
      ((planned_remove += 1))
      printf 'PLAN   #%-4s %-10s remove %-15s %s (%s)\n' \
        "$number" "$id" "$ACTUAL_READY_LABEL" "$title" "$reason"
    fi
    continue
  fi

  # A ready label that was correct in the list snapshot can become stale before
  # apply. Re-read every labeled issue as well as every planned mutation.
  if [[ "$action" == "keep" && "$snapshot_has_ready" != "true" ]]; then
    ((kept += 1))
    printf 'KEEP   #%-4s %-10s %-22s %s\n' "$number" "$id" "$reason" "$title"
    continue
  fi

  current=$(gh issue view "$number" \
    --repo "$REPO" \
    --json number,state,body,labels,assignees,parent,blockedBy)
  current_id=$(jq -r '
    (.body // "") as $body
    | try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch ""
  ' <<<"$current")
  [[ "$current_id" == "$id" ]] ||
    die "managed marker changed for issue #$number; expected $id, got ${current_id:-none}"

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
  ' <<<"$current" >/dev/null ||
    die "invalid or incomplete blockedBy data for #$number during apply"

  current_status=$(jq -r \
    --arg ready "$ACTUAL_READY_LABEL" \
    --arg claim "$CLAIM_LABEL" \
    --arg dispatched "$DISPATCHED_LABEL" \
    --arg trigger "$TRIGGER_LABEL" '
    def has_label($name):
      [(.labels // [])[].name | ascii_downcase] | index($name | ascii_downcase) != null;
    ([((.blockedBy.nodes // [])[]) | select((.state | ascii_downcase) == "open")] | length) as $open_blockers
    | ((.assignees // []) | length) as $assignees
    | (has_label($ready)) as $has_ready
    | (((.state | ascii_downcase) == "open") and
       ($open_blockers == 0) and
       ($assignees == 0) and
       (has_label($claim) | not) and
       (has_label($dispatched) | not) and
       (has_label($trigger) | not)) as $eligible
    | [$eligible, $has_ready] | @tsv
  ' <<<"$current")
  IFS=$'\t' read -r current_eligible current_has_ready <<<"$current_status"

  if [[ "$current_eligible" == "true" && "$current_has_ready" == "false" ]]; then
    gh issue edit "$number" --repo "$REPO" --add-label "$ACTUAL_READY_LABEL" >/dev/null
    ((added += 1))
    printf 'LABEL  #%-4s %-10s add %-18s %s\n' \
      "$number" "$id" "$ACTUAL_READY_LABEL" "$title"
  elif [[ "$current_eligible" == "false" && "$current_has_ready" == "true" ]]; then
    gh issue edit "$number" --repo "$REPO" --remove-label "$ACTUAL_READY_LABEL" >/dev/null
    ((removed += 1))
    printf 'UNLABEL #%-4s %-10s remove %-15s %s\n' \
      "$number" "$id" "$ACTUAL_READY_LABEL" "$title"
  else
    ((kept += 1))
    printf 'KEEP   #%-4s %-10s current readiness is correct: %s\n' \
      "$number" "$id" "$title"
  fi
done < <(jq -c '.[]' <<<"$ROWS")

printf '\nSummary: eligible=%s add_needed=%s remove_needed=%s\n' \
  "$ELIGIBLE_COUNT" "$ADD_COUNT" "$REMOVE_COUNT"
printf '         added=%s removed=%s kept=%s planned_add=%s planned_remove=%s skipped=%s\n' \
  "$added" "$removed" "$kept" "$planned_add" "$planned_remove" "$skipped"
