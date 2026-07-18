# Incident Response Runbook

> **Owner:** Platform / Infrastructure team  
> **Last updated:** 2025-01-15  
> **Review cadence:** Quarterly (aligned with disaster recovery drill)

---

## Table of Contents

1. [Overview](#overview)
2. [Incident Classification](#incident-classification)
3. [On-Call Response Procedure](#on-call-response-procedure)
4. [Escalation Matrix](#escalation-matrix)
5. [War Room Setup](#war-room-setup)
6. [Communication Templates](#communication-templates)
7. [Incident Timeline Template](#incident-timeline-template)
8. [Post-Incident Review](#post-incident-review)
9. [Contact and Ownership](#contact-and-ownership)

---

## Overview

This runbook defines the Haiker incident response process from detection through resolution and post-incident review. It applies to all production incidents affecting the Haiker platform (API, worker, PostgreSQL, MinIO, OIDC authentication).

### Key References

| Document | Purpose |
|----------|---------|
| [Backup Operations](backup-operations.md) | Data recovery and backup alert response |
| [Migration Recovery](migration-recovery.md) | Database migration failures |
| [Rollback](rollback.md) | Application and database rollback procedures |
| [Data Recovery](data-recovery.md) | Single-record and bulk data recovery |
| [Dependency Outage](dependency-outage.md) | External dependency failure handling |

---

## Incident Classification

### Severity Levels

| Severity | Definition | Examples (Haiker-specific) | Response Time | Update Cadence |
|----------|-----------|---------------------------|---------------|----------------|
| **P1 - Critical** | Complete service outage or data loss affecting all users | PostgreSQL down; API unreachable; data corruption; credential compromise; backup system failed with no recent backups | 15 minutes | Every 30 minutes |
| **P2 - High** | Major feature degradation affecting many users | Worker queue stalled (imports/exports failing); OIDC provider unreachable (no new logins); MinIO unavailable (no file uploads/downloads) | 30 minutes | Every 1 hour |
| **P3 - Medium** | Partial degradation affecting some users or non-critical functionality | Slow API responses (>2s p95); individual import failures; single bounded context errors (e.g., route_editing unavailable) | 2 hours | Every 4 hours |
| **P4 - Low** | Minor issue with minimal user impact | Cosmetic errors; non-critical background job delays; staging environment issues; monitoring false positives | Next business day | As resolved |

### Severity Decision Tree

```
Is the service completely unavailable to all users?
  YES -> P1
  NO  -> Is there confirmed or suspected data loss?
           YES -> P1
           NO  -> Are >50% of users affected or a core feature broken?
                    YES -> P2
                    NO  -> Are some users affected or a secondary feature broken?
                             YES -> P3
                             NO  -> P4
```

### Classification Examples

**P1 Scenarios:**
- `haiker-api` returns 5xx on all endpoints (health check failing)
- PostgreSQL connection pool exhausted; no queries completing
- Backup encryption key compromised (potential unauthorized access to backup data)
- Database corruption detected across multiple schemas

**P2 Scenarios:**
- `haiker-worker` is running but not processing jobs (queue backlog growing)
- OIDC provider timeout; existing sessions work but new logins fail
- MinIO returning errors; file uploads failing but API otherwise functional
- Polar integration endpoint failing (sync data stale)

**P3 Scenarios:**
- API latency elevated (p95 > 2s) but requests completing
- Single user's import failing repeatedly due to edge case
- Route versioning returning intermittent errors
- Export generation timing out for large datasets

**P4 Scenarios:**
- Staging environment deployment failed
- Non-critical monitoring alert (e.g., backup storage at 75%)
- Intermittent test failures in CI
- Documentation out of date

---

## On-Call Response Procedure

### Step 1: Acknowledge the Alert (within 5 minutes)

```bash
# Check system health immediately
scripts/smoke-test.sh

# Check API health endpoint
curl -sf http://localhost:3000/health | jq .

# Check worker status
docker compose ps worker

# Check database connectivity
docker compose exec postgres pg_isready -U haiker
```

### Step 2: Classify the Incident

Use the severity decision tree above. When in doubt, classify higher and downgrade later.

### Step 3: Create Incident Record

Record the following in the incident channel:

- **Incident ID:** INC-YYYYMMDD-NNN
- **Severity:** P1/P2/P3/P4
- **Detection time:** UTC timestamp
- **Reporter:** Alert system or person who noticed
- **Affected component:** API / Worker / PostgreSQL / MinIO / OIDC
- **Initial symptoms:** Brief description

### Step 4: Begin Investigation

```bash
# Check application logs
docker compose logs --tail=200 api | grep -i error
docker compose logs --tail=200 worker | grep -i error

# Check PostgreSQL logs
docker compose logs --tail=100 postgres | grep -i "error\|fatal\|panic"

# Check system resources
docker stats --no-stream

# Check disk space
df -h

# Check backup health
scripts/backup-monitor.sh
```

### Step 5: Determine Response Path

| Scenario | Action | Reference |
|----------|--------|-----------|
| Recent deployment caused the issue | Rollback deployment | [rollback.md](rollback.md) |
| Database migration failed | Run migration recovery | [migration-recovery.md](migration-recovery.md) |
| Data loss or corruption | Initiate data recovery | [data-recovery.md](data-recovery.md) |
| External dependency down | Follow dependency playbook | [dependency-outage.md](dependency-outage.md) |
| Credential compromised | Rotate secrets immediately | [secret-rotation.md](secret-rotation.md) |
| Backup alert triggered | Follow backup alert response | [backup-operations.md](backup-operations.md#alert-response) |

### Step 6: Resolve and Verify

After applying the fix:

```bash
# Run smoke tests
scripts/smoke-test.sh

# Verify health endpoint
curl -sf http://localhost:3000/health | jq .

# Check for error logs (should be clean)
docker compose logs --tail=50 api | grep -i error

# Verify backup monitoring is green
scripts/backup-monitor.sh
```

### Step 7: Close the Incident

1. Send resolution communication (see templates below)
2. Update the incident record with resolution details
3. Schedule post-incident review (within 48 hours for P1/P2)

---

## Escalation Matrix

### Escalation Levels

| Level | Role | Responsibilities | Contact Method |
|-------|------|-----------------|----------------|
| **L1 - On-Call** | On-call engineer | Initial triage, run documented procedures, communicate status | PagerDuty / Alert webhook |
| **L2 - Platform** | Platform team lead | Investigate complex issues, manual recovery, architecture decisions | Slack #platform-team |
| **L3 - Database** | Database specialist | PITR decisions, data integrity, complex migrations | Slack #database-team + phone |
| **L3 - Security** | Security lead | Credential compromise, access violations, forensics | Slack #security + phone |

### When to Escalate

| Condition | Escalate To |
|-----------|-------------|
| Documented procedures do not resolve P1/P2 within 30 minutes | L2 Platform |
| PITR or data recovery decision required | L3 Database |
| Credential compromise suspected | L3 Security (immediately) |
| Multiple services affected simultaneously | L2 Platform |
| Issue requires infrastructure changes (disk, network, DNS) | L2 Platform |
| Unsure whether rollback SQL is safe | L3 Database |

### Escalation Timing

| Severity | Time to L2 Escalation | Time to L3 Escalation |
|----------|----------------------|----------------------|
| P1 | 15 minutes if unresolved | 30 minutes if unresolved |
| P2 | 30 minutes if unresolved | 1 hour if unresolved |
| P3 | 4 hours if unresolved | Next business day |
| P4 | As needed | As needed |

---

## War Room Setup

### When to Open a War Room

- All P1 incidents
- P2 incidents lasting longer than 30 minutes
- Any incident requiring coordination between multiple teams

### War Room Structure

| Role | Responsibility |
|------|---------------|
| **Incident Commander (IC)** | Coordinates response, makes decisions, manages communication |
| **Technical Lead** | Directs technical investigation and remediation |
| **Communications Lead** | Sends status updates, manages stakeholder expectations |
| **Scribe** | Records timeline, actions taken, decisions made |

### War Room Checklist

- [ ] Create dedicated Slack channel: `#inc-YYYYMMDD-brief-description`
- [ ] Assign Incident Commander
- [ ] Pin incident summary to channel
- [ ] Set up video call if needed (for complex multi-team coordination)
- [ ] Begin incident timeline (see template below)
- [ ] Notify stakeholders with initial communication
- [ ] Set timer for next status update

### War Room Rules

1. All communication goes through the incident channel
2. IC makes final decisions when consensus is not reached
3. Non-essential discussions happen in a separate thread
4. Actions are recorded before being taken (scribe responsibility)
5. Status updates go out on schedule regardless of progress

---

## Communication Templates

### Initial Notification (P1/P2)

```
INCIDENT NOTIFICATION

Incident: INC-YYYYMMDD-NNN
Severity: P1/P2
Status: Investigating

Summary: [Brief description of the issue]

Impact: [What users are experiencing]
- Affected services: [API / Worker / Uploads / Authentication]
- Affected users: [All / Subset / Internal only]

Timeline:
- [HH:MM UTC] Issue detected via [monitoring / user report / deployment]
- [HH:MM UTC] On-call acknowledged, investigation started

Next update: [HH:MM UTC]

Incident Commander: [Name]
```

### Status Update

```
INCIDENT UPDATE

Incident: INC-YYYYMMDD-NNN
Severity: P1/P2
Status: [Investigating / Identified / Mitigating / Resolved]

Update: [What we know now, what has changed since last update]

Current actions:
- [Action being taken by whom]
- [Next step planned]

Impact change: [Same / Improved / Worsened]

Next update: [HH:MM UTC]
```

### Resolution Notification

```
INCIDENT RESOLVED

Incident: INC-YYYYMMDD-NNN
Severity: P1/P2
Status: Resolved

Summary: [What happened]

Resolution: [What was done to fix it]

Duration: [Total time from detection to resolution]
Impact window: [Time users were affected]

Root cause: [Brief description - full analysis in post-incident review]

Follow-up:
- Post-incident review scheduled for [date/time]
- [Any remaining action items]
```

### Stakeholder Notification (External / User-facing)

```
Service Disruption Notice

We experienced a [brief / extended] disruption to [affected feature].

- Start: [HH:MM UTC, date]
- Resolved: [HH:MM UTC, date]
- Duration: [X hours Y minutes]

What happened: [User-friendly, non-technical description]

What we are doing: [Preventive measures being taken]

If you were affected: [Any actions users need to take, or confirmation no action needed]

We apologize for the inconvenience.
```

---

## Incident Timeline Template

Use this template to record events during an incident:

```markdown
## Incident Timeline: INC-YYYYMMDD-NNN

### Summary
- **Severity:** P1/P2/P3
- **Duration:** [detection to resolution]
- **Impact:** [brief impact description]
- **Root cause:** [brief root cause]

### Timeline (all times UTC)

| Time | Event | Actor |
|------|-------|-------|
| HH:MM | Alert fired: [alert name] | Monitoring |
| HH:MM | On-call acknowledged | [Name] |
| HH:MM | Initial investigation started | [Name] |
| HH:MM | Root cause identified: [description] | [Name] |
| HH:MM | Remediation started: [action] | [Name] |
| HH:MM | Fix deployed / rollback completed | [Name] |
| HH:MM | Smoke tests passing | [Name] |
| HH:MM | Incident declared resolved | IC |

### Actions Taken
1. [Action and result]
2. [Action and result]

### Follow-up Items
- [ ] [Action item with owner and due date]
- [ ] [Action item with owner and due date]
```

---

## Post-Incident Review

### Scheduling

| Severity | Review Timing | Participants |
|----------|--------------|--------------|
| P1 | Within 48 hours | All involved + engineering leadership |
| P2 | Within 1 week | All involved + team leads |
| P3 | Within 2 weeks | Primary responders |
| P4 | Optional | As needed |

### Review Agenda

1. **Timeline review** (10 min) - Walk through the incident timeline
2. **Root cause analysis** (20 min) - What caused the incident
3. **Detection** (10 min) - How was it detected? Could we detect it earlier?
4. **Response** (10 min) - What went well? What could be improved?
5. **Action items** (10 min) - Preventive measures, documentation updates, tooling improvements

### Blameless Post-Mortem Principles

- Focus on systems and processes, not individuals
- Assume everyone acted with the best information available at the time
- Ask "how can we make this harder to happen?" not "who caused this?"
- Document findings to improve future responses

### Post-Incident Review Template

```markdown
## Post-Incident Review: INC-YYYYMMDD-NNN

### Incident Summary
- **Date:** YYYY-MM-DD
- **Duration:** X hours Y minutes
- **Severity:** P1/P2/P3
- **Impact:** [Description of user impact]

### Root Cause
[Detailed technical explanation]

### Contributing Factors
1. [Factor 1]
2. [Factor 2]

### Detection
- **How detected:** [Monitoring alert / User report / Manual check]
- **Time to detect:** [Duration from start of issue to detection]
- **Could we detect earlier?** [Yes/No - how]

### Response Assessment
- **What went well:**
  - [Item]
- **What could be improved:**
  - [Item]

### Action Items
| # | Action | Owner | Due Date | Status |
|---|--------|-------|----------|--------|
| 1 | [Action] | [Name] | YYYY-MM-DD | Open |
| 2 | [Action] | [Name] | YYYY-MM-DD | Open |

### Lessons Learned
- [Lesson]
```

---

## Contact and Ownership

| Role | Responsibility |
|------|---------------|
| Platform team | Incident tooling, runbooks, monitoring |
| On-call engineer | First response, triage, documented procedures |
| Engineering leadership | P1 escalation, resource allocation |

### Communication Channels

| Channel | Purpose |
|---------|---------|
| PagerDuty / Alert webhook | Automated alerting (configured via `ALERT_WEBHOOK_URL`) |
| `#incidents` | Active incident coordination |
| `#platform-team` | Platform team escalation |
| `#database-team` | Database specialist escalation |
| `#security` | Security incident escalation |
