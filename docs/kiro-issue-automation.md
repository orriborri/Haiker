# Kiro issue automation

The exact `kiro` GitHub label is an execution trigger. Adding it starts a Kiro
autonomous-agent task; it is not a readiness or queue label. Do not remove and
re-add it as part of routine reconciliation.

The safe flow is:

```text
issues/mvp.yaml blocked_by graph
  -> sync-github-issues.sh
  -> agent-ready reconciliation
  -> serialized resource-lock check and claim
  -> add kiro to one issue
```

## Commands

All scripts default to dry-run.

```bash
# Reconcile issue bodies, labels, parent links, and blocked-by links.
scripts/sync-github-issues.sh --dry-run
scripts/sync-github-issues.sh --apply

# Add or remove the non-triggering agent-ready label.
scripts/label-unblocked-issues.sh --dry-run
scripts/label-unblocked-issues.sh --apply

# Select at most one safe issue. The manifest default is one active task.
scripts/dispatch-kiro-issue.sh --dry-run
scripts/dispatch-kiro-issue.sh --dry-run --max-concurrency 2
scripts/dispatch-kiro-issue.sh --dry-run --issue G-06 --max-concurrency 2
```

Use the **Dispatch one Kiro issue** manual GitHub Actions workflow for apply
mode. Its repository-wide concurrency group serializes dispatches across
runners, and it runs dependency sync, readiness reconciliation, and dispatch
in that order. Local apply mode uses `flock`, which protects only processes on
the same machine, and therefore requires an explicit `--allow-local-apply`
acknowledgement.

Before enabling the workflow, create a protected GitHub Environment named
`kiro-dispatch`, restrict it to the default branch (and preferably require an
approver), and add an environment secret named `KIRO_GH_TOKEN`. Use a
fine-grained token for a GitHub user whose account is connected to Kiro, scoped
only to this repository with Issues read/write and Contents/Pull requests read.
The normal Actions token represents `github-actions[bot]`, not that connected
user, so the workflow intentionally does not use it for dispatch.

## Dependency and conflict models

`blocked_by` represents a real semantic prerequisite and must remain acyclic.
The sync script rejects cyclic manifests and incomplete GitHub dependency
responses before writing.

`resource_locks` represent temporary file or subsystem overlap. A dispatcher
will not exceed a resource group's capacity. An issue without declared locks
is treated as exclusive: it can run only when no other managed work or
unlinked pull request is active.

The `paths` under each resource group document its intended boundary; the
dispatcher schedules from explicit `resource_locks` and does not infer locks
from a diff. Update an issue's locks before dispatch whenever its planned scope
touches another shared resource.

The first explicit lock assignments cover the current ready frontier:

| Issue | Resource locks |
| --- | --- |
| B-02 | `frontend-client` |
| B-03 | `api-core` |
| D-03 | `route-editing` |
| D-05 | `route-editing` |
| D-10 | `api-core`, `frontend-client`, `route-editing` |
| G-06 | `backup-recovery` |

With D-10 active, B-02, B-03, D-03, and D-05 conflict. G-06 is independent,
but it is dispatched alongside D-10 only when the operator deliberately raises
the maximum concurrency to two.

## Failure handling

The dispatcher adds `agent-dispatching`, revalidates the issue and repository,
then adds `kiro` once. It waits for a Kiro bot acknowledgement before replacing
the claim with `agent-dispatched`. If the trigger result is uncertain or the
acknowledgement times out, it leaves the claim in place and stops. Inspect the
issue timeline, task branch, and pull request before clearing a stale claim.
Never retry by removing and re-adding `kiro`.

Claims and `agent-dispatched` remain active when an issue is merely closed,
because closing an issue does not prove that a Kiro task or pull request has
stopped. A closed issue is considered terminal automatically only when GitHub
identifies a merged pull request that closed it. Otherwise, clear
`agent-dispatched` manually only after the task is cancelled or its pull request
is merged and the resource locks can safely be released.

Run the isolated regression suite with:

```bash
scripts/tests/test-issue-automation.sh
```
