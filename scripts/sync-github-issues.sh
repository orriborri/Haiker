#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT_DIR/issues/mvp.yaml"
MODE="dry-run"
ISSUE_LIMIT=1000
LABEL_LIMIT=1000

usage() {
  cat <<'EOF'
Usage: scripts/sync-github-issues.sh [--dry-run | --apply] [--manifest PATH]

Synchronize the managed Haiker GitHub issue backlog from a JSON-compatible YAML
manifest. Dry-run is the default. Use --apply to write to GitHub.
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
  (.milestone.title | type == "string" and length > 0) and
  ((.milestone.description // "") | type == "string") and
  (.labels | type == "array" and length > 0) and
  (.issues | type == "array" and length > 0) and
  ([.issues[].id] | length == (unique | length)) and
  ([.labels[].name | ascii_downcase] | length == (unique | length)) and
  all(.labels[];
    (.name |
      type == "string" and
      length > 0 and
      (contains(",") | not)) and
    (.color | type == "string" and test("^[0-9A-Fa-f]{6}$")) and
    ((.description // "") | type == "string")
  ) and
  (.dispatch | type == "object") and
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
  (. as $manifest |
    all([
      .dispatch.ready_label,
      .dispatch.claim_label,
      .dispatch.dispatched_label
    ][]; . as $name |
      any($manifest.labels[]; (.name | ascii_downcase) == ($name | ascii_downcase)))) and
  (.resource_groups | type == "object" and length > 0) and
  all(.resource_groups | to_entries[];
    (.key | test("^[a-z0-9][a-z0-9-]*$")) and
    (.value | type == "object") and
    (.value.capacity | type == "number" and floor == . and . >= 1) and
    ((.value.description // "") | type == "string") and
    (.value.paths | type == "array" and length > 0 and
      all(.[]; type == "string" and length > 0))
  ) and
  ([.labels[].name] as $label_names |
   (.resource_groups | keys) as $resource_group_names |
   .issues as $issues |
    [$issues[].id] as $ids |
    all($issues[]; . as $issue |
      ($issue.id | type == "string" and test("^[A-Za-z0-9._-]+$")) and
      ($issue.title | type == "string" and length > 0) and
      ($issue.body | type == "string" and length > 0) and
      ($issue.labels | type == "array" and length == (unique | length)) and
      (($issue.blocked_by // []) | type == "array" and length == (unique | length)) and
      (($issue.resource_locks // []) | type == "array" and length == (unique | length)) and
      all($issue.labels[]; . as $label | $label_names | index($label) != null) and
      all(($issue.resource_locks // [])[]; . as $lock |
        $resource_group_names | index($lock) != null) and
      (($issue.parent_id // "") == "" or
        ($issue.parent_id != $issue.id and
          any($issues[]; .id == $issue.parent_id and ((.parent_id // "") == "")))) and
      all(($issue.blocked_by // [])[]; . as $blocker |
        $blocker != $issue.id and ($ids | index($blocker) != null))
    )
  )
' "$MANIFEST" >/dev/null || die "manifest validation failed"

CYCLE_IDS=$(jq -r '
  .issues as $issues
  | reduce range(0; ($issues | length)) as $_ (
      {done: [], remaining: [$issues[].id]};
      . as $state
      | [
          $state.remaining[] as $id
          | ($issues[] | select(.id == $id) | (.blocked_by // [])) as $dependencies
          | select(all($dependencies[]; . as $dependency |
              $state.done | index($dependency) != null))
          | $id
        ] as $ready
      | .done += $ready
      | .remaining -= $ready
    )
  | .remaining
  | join(", ")
' "$MANIFEST")
[[ -z "$CYCLE_IDS" ]] ||
  die "manifest dependency graph has nodes blocked by a cycle: $CYCLE_IDS"

REPO=$(jq -er '.repo' "$MANIFEST")
MILESTONE_TITLE=$(jq -er '.milestone.title' "$MANIFEST")
MILESTONE_DESCRIPTION=$(jq -r '.milestone.description // ""' "$MANIFEST")
ISSUE_COUNT=$(jq -r '.issues | length' "$MANIFEST")
PARENT_COUNT=$(jq -r '[.issues[] | select((.parent_id // "") == "")] | length' "$MANIFEST")
CHILD_COUNT=$((ISSUE_COUNT - PARENT_COUNT))

printf 'Target: %s\n' "$REPO"
printf 'Manifest: %s\n' "$MANIFEST"
printf 'Mode: %s\n' "$MODE"
printf 'Backlog: %s parent issues, %s child issues\n' "$PARENT_COUNT" "$CHILD_COUNT"

gh auth status --hostname github.com >/dev/null
REPO_INFO=$(gh repo view "$REPO" --json nameWithOwner,viewerPermission)
ACTUAL_REPO=$(jq -r '.nameWithOwner' <<<"$REPO_INFO")
PERMISSION=$(jq -r '.viewerPermission' <<<"$REPO_INFO")
[[ "$ACTUAL_REPO" == "$REPO" ]] || die "repository mismatch: expected $REPO, got $ACTUAL_REPO"
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
  --json number,title,body,url,labels,milestone,parent,blockedBy)
REMOTE_LABELS=$(gh label list \
  --repo "$REPO" \
  --limit "$LABEL_LIMIT" \
  --json name,color,description)
REMOTE_MILESTONES=$(gh api "repos/$REPO/milestones?state=all&per_page=100" \
  --paginate \
  --slurp \
  | jq -c 'add')

REMOTE_ISSUE_COUNT=$(jq 'length' <<<"$REMOTE_ISSUES")
REMOTE_LABEL_COUNT=$(jq 'length' <<<"$REMOTE_LABELS")
((REMOTE_ISSUE_COUNT < ISSUE_LIMIT)) ||
  die "issue query reached its $ISSUE_LIMIT issue safety limit"
((REMOTE_LABEL_COUNT < LABEL_LIMIT)) ||
  die "label query reached its $LABEL_LIMIT label safety limit"

MANAGED_IDS=$(jq -c '[.issues[].id]' "$MANIFEST")
INVALID_DEPENDENCIES=$(jq -r --argjson managed_ids "$MANAGED_IDS" '
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
' <<<"$REMOTE_ISSUES")
[[ -z "$INVALID_DEPENDENCIES" ]] ||
  die "invalid or incomplete blockedBy data for $INVALID_DEPENDENCIES"

DUPLICATE_IDS=$(jq -r '
  [
    .[]
    | (.body // "") as $body
    | try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch empty
  ]
  | group_by(.)
  | map(select(length > 1) | .[0])
  | join(", ")
' <<<"$REMOTE_ISSUES")
[[ -z "$DUPLICATE_IDS" ]] || die "duplicate managed issue IDs found: $DUPLICATE_IDS"

declare -A NUMBER_BY_ID=()
declare -A URL_BY_ID=()

while IFS=$'\t' read -r id number url; do
  [[ -n "$id" ]] || continue
  NUMBER_BY_ID["$id"]="$number"
  URL_BY_ID["$id"]="$url"
done < <(jq -r '
  .[]
  | (.body // "") as $body
  | (try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch "") as $id
  | select($id != "")
  | [$id, (.number | tostring), .url]
  | @tsv
' <<<"$REMOTE_ISSUES")

created=0
updated=0
unchanged=0
dependencies_added=0
dependencies_removed=0
labels_created=0
labels_updated=0
labels_unchanged=0

while IFS= read -r label; do
  name=$(jq -r '.name' <<<"$label")
  color=$(jq -r '.color' <<<"$label")
  description=$(jq -r '.description // ""' <<<"$label")
  remote_label=$(jq -c --arg name "$name" '
    first(.[] | select((.name | ascii_downcase) == ($name | ascii_downcase))) // empty
  ' <<<"$REMOTE_LABELS")

  if [[ -z "$remote_label" ]]; then
    if [[ "$MODE" == "apply" ]]; then
      gh label create "$name" \
        --repo "$REPO" \
        --color "$color" \
        --description "$description" >/dev/null
    fi
    ((labels_created += 1))
    printf 'CREATE label: %s\n' "$name"
    continue
  fi

  actual_name=$(jq -r '.name' <<<"$remote_label")
  actual_color=$(jq -r '.color' <<<"$remote_label")
  actual_description=$(jq -r '.description // ""' <<<"$remote_label")
  if [[ "${actual_name,,}" == "${name,,}" &&
        "${actual_color,,}" == "${color,,}" &&
        "$actual_description" == "$description" ]]; then
    ((labels_unchanged += 1))
    printf 'KEEP   label: %s\n' "$name"
    continue
  fi

  if [[ "$MODE" == "apply" ]]; then
    gh label edit "$actual_name" \
      --repo "$REPO" \
      --name "$name" \
      --color "$color" \
      --description "$description" >/dev/null
  fi
  ((labels_updated += 1))
  printf 'UPDATE label: %s\n' "$name"
done < <(jq -c '.labels[]' "$MANIFEST")

milestone=$(jq -c --arg title "$MILESTONE_TITLE" '
  first(.[] | select(.title == $title)) // empty
' <<<"$REMOTE_MILESTONES")

if [[ -z "$milestone" ]]; then
  if [[ "$MODE" == "apply" ]]; then
    milestone_number=$(gh api \
      --method POST \
      "repos/$REPO/milestones" \
      -f title="$MILESTONE_TITLE" \
      -f description="$MILESTONE_DESCRIPTION" \
      --jq '.number')
    printf 'CREATE milestone #%s: %s\n' "$milestone_number" "$MILESTONE_TITLE"
  else
    printf 'CREATE milestone: %s\n' "$MILESTONE_TITLE"
  fi
else
  milestone_number=$(jq -r '.number' <<<"$milestone")
  actual_milestone_description=$(jq -r '.description // ""' <<<"$milestone")
  if [[ "$actual_milestone_description" == "$MILESTONE_DESCRIPTION" ]]; then
    printf 'KEEP   milestone #%s: %s\n' "$milestone_number" "$MILESTONE_TITLE"
  else
    if [[ "$MODE" == "apply" ]]; then
      gh api \
        --method PATCH \
        "repos/$REPO/milestones/$milestone_number" \
        -f title="$MILESTONE_TITLE" \
        -f description="$MILESTONE_DESCRIPTION" \
        --silent
    fi
    printf 'UPDATE milestone #%s: %s\n' "$milestone_number" "$MILESTONE_TITLE"
  fi
fi

managed_body() {
  local id=$1
  local body=$2
  local marker="<!-- haiker-issue-id: $id -->"

  if [[ "$body" == *"$marker"* ]]; then
    printf '%s' "$body"
  else
    printf '%s\n\n%s' "$body" "$marker"
  fi
}

remote_issue_for_id() {
  local id=$1
  jq -c --arg marker "<!-- haiker-issue-id: $id -->" \
    '.[] | select((.body // "") | contains($marker))' <<<"$REMOTE_ISSUES"
}

sync_issue() {
  local issue=$1
  local id title body labels_csv parent_id parent_number desired_body remote body_file
  local number url missing_labels current_parent current_milestone needs_edit
  local -a create_args edit_args

  id=$(jq -r '.id' <<<"$issue")
  title=$(jq -r '.title' <<<"$issue")
  body=$(jq -r '.body' <<<"$issue")
  labels_csv=$(jq -r '.labels | join(",")' <<<"$issue")
  parent_id=$(jq -r '.parent_id // ""' <<<"$issue")
  desired_body=$(managed_body "$id" "$body")
  parent_number=""

  if [[ -n "$parent_id" ]]; then
    parent_number=${NUMBER_BY_ID[$parent_id]:-}
    if [[ "$MODE" == "apply" && -z "$parent_number" ]]; then
      die "parent $parent_id for $id has not been created"
    fi
  fi

  if [[ -z ${NUMBER_BY_ID[$id]:-} ]]; then
    if [[ "$MODE" == "dry-run" ]]; then
      printf 'CREATE %-10s %s\n' "$id" "$title"
      return
    fi

    body_file="$TMP_DIR/$id.md"
    printf '%s' "$desired_body" >"$body_file"
    create_args=(issue create --repo "$REPO" --title "$title" --body-file "$body_file" --milestone "$MILESTONE_TITLE")
    [[ -z "$labels_csv" ]] || create_args+=(--label "$labels_csv")
    [[ -z "$parent_number" ]] || create_args+=(--parent "$parent_number")

    url=$(gh "${create_args[@]}")
    number=${url##*/}
    NUMBER_BY_ID["$id"]="$number"
    URL_BY_ID["$id"]="$url"
    ((created += 1))
    printf 'CREATE #%-4s %-10s %s\n' "$number" "$id" "$title"
    return
  fi

  number=${NUMBER_BY_ID[$id]}
  remote=$(remote_issue_for_id "$id")
  current_parent=$(jq -r '.parent.number // ""' <<<"$remote")
  current_milestone=$(jq -r '.milestone.title // ""' <<<"$remote")
  missing_labels=$(jq -r --argjson wanted "$(jq -c '.labels' <<<"$issue")" '
    [.labels[].name] as $current
    | [$wanted[] as $label | select($current | index($label) == null) | $label]
    | join(",")
  ' <<<"$remote")

  needs_edit=false
  [[ $(jq -r '.title' <<<"$remote") == "$title" ]] || needs_edit=true
  [[ $(jq -r '.body' <<<"$remote") == "$desired_body" ]] || needs_edit=true
  [[ "$current_milestone" == "$MILESTONE_TITLE" ]] || needs_edit=true
  [[ -z "$missing_labels" ]] || needs_edit=true
  [[ "$current_parent" == "$parent_number" ]] || needs_edit=true

  if [[ "$needs_edit" == false ]]; then
    ((unchanged += 1))
    printf 'KEEP   #%-4s %-10s %s\n' "$number" "$id" "$title"
    return
  fi

  if [[ "$MODE" == "dry-run" ]]; then
    printf 'UPDATE #%-4s %-10s %s\n' "$number" "$id" "$title"
    return
  fi

  body_file="$TMP_DIR/$id.md"
  printf '%s' "$desired_body" >"$body_file"
  edit_args=(issue edit "$number" --repo "$REPO" --title "$title" --body-file "$body_file" --milestone "$MILESTONE_TITLE")
  [[ -z "$missing_labels" ]] || edit_args+=(--add-label "$missing_labels")
  if [[ "$current_parent" != "$parent_number" ]]; then
    if [[ -n "$parent_number" ]]; then
      edit_args+=(--parent "$parent_number")
    else
      edit_args+=(--remove-parent)
    fi
  fi

  gh "${edit_args[@]}" >/dev/null
  ((updated += 1))
  printf 'UPDATE #%-4s %-10s %s\n' "$number" "$id" "$title"
}

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

while IFS= read -r issue; do
  sync_issue "$issue"
done < <(jq -c '.issues[] | select((.parent_id // "") == "")' "$MANIFEST")

while IFS= read -r issue; do
  sync_issue "$issue"
done < <(jq -c '.issues[] | select((.parent_id // "") != "")' "$MANIFEST")

array_contains() {
  local needle=$1
  shift
  local value
  for value in "$@"; do
    [[ "$value" == "$needle" ]] && return 0
  done
  return 1
}

declare -A ID_BY_NUMBER=()
for managed_id in "${!NUMBER_BY_ID[@]}"; do
  ID_BY_NUMBER["${NUMBER_BY_ID[$managed_id]}"]="$managed_id"
done

while IFS= read -r issue; do
  id=$(jq -r '.id' <<<"$issue")
  issue_number=${NUMBER_BY_ID[$id]:-}
  desired_numbers=()
  unresolved_ids=()
  add_numbers=()
  remove_numbers=()

  while IFS= read -r blocker_id; do
    [[ -n "$blocker_id" ]] || continue
    blocker_number=${NUMBER_BY_ID[$blocker_id]:-}
    if [[ -n "$blocker_number" ]]; then
      desired_numbers+=("$blocker_number")
    else
      unresolved_ids+=("$blocker_id")
    fi
  done < <(jq -r '.blocked_by // [] | .[]' <<<"$issue")

  if [[ -z "$issue_number" ]]; then
    if ((${#unresolved_ids[@]} > 0)); then
      blockers=$(IFS=,; printf '%s' "${unresolved_ids[*]}")
      printf 'PLAN   %-10s blocked by %s\n' "$id" "$blockers"
    fi
    continue
  fi

  current_numbers=()
  if [[ "$MODE" == "apply" ]]; then
    remote=$(gh issue view "$issue_number" \
      --repo "$REPO" \
      --json number,body,blockedBy)
    current_id=$(jq -r '
      (.body // "") as $body
      | try ($body | capture("<!-- haiker-issue-id: (?<id>[A-Za-z0-9._-]+) -->").id) catch ""
    ' <<<"$remote")
    [[ "$current_id" == "$id" ]] ||
      die "managed marker changed for issue #$issue_number; expected $id, got ${current_id:-none}"

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
    ' <<<"$remote" >/dev/null ||
      die "invalid or incomplete blockedBy data for #$issue_number during apply"
  else
    remote=$(remote_issue_for_id "$id")
  fi
  if [[ -n "$remote" ]]; then
    mapfile -t current_numbers < <(jq -r '(.blockedBy.nodes // [])[].number' <<<"$remote")
  fi

  for blocker_number in "${desired_numbers[@]}"; do
    if ! array_contains "$blocker_number" "${current_numbers[@]}"; then
      add_numbers+=("$blocker_number")
    fi
  done

  for blocker_number in "${current_numbers[@]}"; do
    [[ -n ${ID_BY_NUMBER[$blocker_number]:-} ]] || continue
    if ! array_contains "$blocker_number" "${desired_numbers[@]}"; then
      remove_numbers+=("$blocker_number")
    fi
  done

  if ((${#unresolved_ids[@]} > 0)); then
    [[ "$MODE" == "dry-run" ]] || die "unresolved blockers for $id: ${unresolved_ids[*]}"
    blockers=$(IFS=,; printf '%s' "${unresolved_ids[*]}")
    printf 'PLAN   #%-4s blocked by future issues %s\n' "$issue_number" "$blockers"
  fi

  if ((${#add_numbers[@]} > 0)); then
    blockers_csv=$(IFS=,; printf '%s' "${add_numbers[*]}")
    if [[ "$MODE" == "apply" ]]; then
      gh issue edit "$issue_number" --repo "$REPO" --add-blocked-by "$blockers_csv" >/dev/null
      printf 'LINK   #%-4s blocked by %s\n' "$issue_number" "$blockers_csv"
    else
      printf 'PLAN   link #%s blocked by %s\n' "$issue_number" "$blockers_csv"
    fi
    ((dependencies_added += ${#add_numbers[@]}))
  fi

  if ((${#remove_numbers[@]} > 0)); then
    blockers_csv=$(IFS=,; printf '%s' "${remove_numbers[*]}")
    if [[ "$MODE" == "apply" ]]; then
      gh issue edit "$issue_number" --repo "$REPO" --remove-blocked-by "$blockers_csv" >/dev/null
      printf 'UNLINK #%-4s no longer blocked by %s\n' "$issue_number" "$blockers_csv"
    else
      printf 'PLAN   unlink #%s from blockers %s\n' "$issue_number" "$blockers_csv"
    fi
    ((dependencies_removed += ${#remove_numbers[@]}))
  fi
done < <(jq -c '.issues[]' "$MANIFEST")

printf '\nSummary: issues_created=%s issues_updated=%s issues_unchanged=%s\n' \
  "$created" "$updated" "$unchanged"
printf '         dependencies_added=%s dependencies_removed=%s\n' \
  "$dependencies_added" "$dependencies_removed"
printf '         labels_created=%s labels_updated=%s labels_unchanged=%s\n' \
  "$labels_created" "$labels_updated" "$labels_unchanged"
