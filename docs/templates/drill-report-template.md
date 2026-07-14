# Restore Drill Report

## Drill Metadata

| Field | Value |
|-------|-------|
| Date | _YYYY-MM-DD_ |
| Operator | _Name_ |
| Environment | _e.g., isolated Docker Compose (haiker-drill)_ |
| Target Recovery Time | _ISO 8601 timestamp used as --target-time_ |
| Drill Script Version | _Git commit SHA or tag of scripts/restore-drill.sh_ |
| Drill ID | _Auto-generated or manual identifier_ |

---

## Per-Phase Results

| Phase | Status | Duration | Notes |
|-------|--------|----------|-------|
| Infrastructure Provisioning (Setup) | pass / fail / warning | _MM:SS_ | _Any issues during environment startup_ |
| PostgreSQL PITR Restore | pass / fail / warning | _MM:SS_ | _WAL replay details, backup file used_ |
| MinIO Object Restore | pass / fail / warning | _MM:SS_ | _Object counts, any mirror errors_ |
| Migration Verification | pass / fail / warning | _MM:SS_ | _Applied/expected count, any missing_ |
| Integrity Checks | pass / fail / warning | _MM:SS_ | _Summary of check results_ |
| Application Startup | pass / fail / warning | _MM:SS_ | _Service readiness, any config issues_ |
| Smoke Tests | pass / fail / warning | _MM:SS_ | _Endpoints tested, any failures_ |

---

## RTO Summary

| Metric | Value |
|--------|-------|
| Total Recovery Time | _HH:MM:SS (total seconds)_ |
| RTO Target | 4 hours (14,400 seconds) |
| RTO Met? | Yes / No |
| Margin | _Seconds under/over target_ |

---

## RPO Summary

| Metric | Value |
|--------|-------|
| Restore Target Timestamp | _ISO 8601_ |
| Last WAL Timestamp (PostgreSQL) | _From pg_last_xact_replay_timestamp()_ |
| PostgreSQL Data Loss Window | _Difference (should be < 15 min)_ |
| Last MinIO Sync Timestamp | _Most recent backup object timestamp_ |
| MinIO Data Loss Window | _Difference (should be < 24h)_ |
| RPO Targets Met? | Yes / No |

---

## Integrity Check Results

| Check | Status | Count | Notes |
|-------|--------|-------|-------|
| Orphan Activities | pass / fail | _0_ | _Activities with invalid owner_id_ |
| Dangling Imports (owners) | pass / fail | _0_ | _Imports with invalid owner_id_ |
| Dangling Imports (artifacts) | pass / fail | _0_ | _Imports with invalid source_artifact_id_ |
| Dangling Imports (activities) | pass / fail | _0_ | _Imports with invalid activity_id_ |
| Orphan Tracks | pass / fail | _0_ | _Tracks with invalid source_revision_id_ |
| Invalid Draft References (owners) | pass / fail | _0_ | _Drafts with invalid owner_id_ |
| Invalid Draft References (activities) | pass / fail | _0_ | _Drafts with invalid activity_id_ |
| Broken Version Pointers | pass / fail | _0_ | _Activities with invalid current_route_version_id_ |
| Unknown Audit Actors | pass / fail / acceptable | _0_ | _Audit entries with unresolved actor_id_ |
| MinIO Artifact Existence | pass / fail / skipped | _N/M sampled_ | _Keys checked against bucket_ |

---

## Gaps Discovered

| Gap | Severity | Owner | Remediation | Target Date | Status |
|-----|----------|-------|-------------|-------------|--------|
| _Description_ | critical / high / medium / low | _Team/person_ | _Fix description_ | _YYYY-MM-DD_ | open / in-progress / resolved |
| | | | | | |
| | | | | | |

---

## Remediation Actions

| Action | Priority | Owner | Due Date | Status | Notes |
|--------|----------|-------|----------|--------|-------|
| _Specific action item_ | _P1/P2/P3_ | _Name_ | _YYYY-MM-DD_ | open / in-progress / done | _Additional context_ |
| | | | | | |
| | | | | | |

---

## Sign-Off

| Role | Name | Date | Approved |
|------|------|------|----------|
| Platform Team Lead | _Name_ | _YYYY-MM-DD_ | Yes / No |
| On-Call Lead | _Name_ | _YYYY-MM-DD_ | Yes / No |

### Sign-Off Conditions

- [ ] Drill passed or all failures are documented with remediation
- [ ] Total RTO within 4-hour target
- [ ] All critical/high gaps resolved or have approved remediation plan
- [ ] Runbook updated with any corrections discovered during drill

### Comments

_Additional notes from approvers:_

---

> **Template version:** 1.0
> **Associated runbook:** [docs/runbooks/restore-drill.md](../runbooks/restore-drill.md)
> **Automated drill script:** [scripts/restore-drill.sh](../../scripts/restore-drill.sh)
