# Performance Targets

This document defines the performance targets for Haiker. All targets represent commitments that the system must meet under normal operating conditions. Targets are organized by category and include measurement criteria.

## API Targets

| Metric                         | Target          | Notes                                    |
|--------------------------------|-----------------|------------------------------------------|
| Metadata request latency       | p95 < 300ms     | Lightweight reads (activity metadata)    |
| Preview geometry latency       | p95 < 500ms     | Returning simplified geometry for map    |
| Mutation acknowledgement       | p95 < 500ms     | Excludes async background work           |

**Measurement scope:** Targets are measured at the API boundary (HTTP response time). They exclude network transit time between client and server. Async work (thumbnail generation, Polar sync) is explicitly excluded from mutation acknowledgement timing.

## Editor Targets

| Metric                         | Target          | Notes                                    |
|--------------------------------|-----------------|------------------------------------------|
| Pointer feedback               | 16ms            | Where practical (one frame at 60 FPS)    |
| Autosave acknowledgement       | < 1 second      | Under normal conditions                  |

**Measurement scope:** Pointer feedback is measured as the time from pointer event to visual feedback in the browser. The 16ms target represents a single frame at 60 FPS and is aspirational where map rendering constraints allow. Autosave acknowledgement is the time from triggering autosave to confirming the draft is persisted (to IndexedDB or server).

## Import Targets

| Metric                         | Target          | Notes                                          |
|--------------------------------|-----------------|------------------------------------------------|
| Maximum GPX file size accepted | 50 MB           | Files up to this size must be accepted         |
| Maximum point count accepted   | 500,000 points  | Subject to benchmark validation                |
| Synchronous parsing            | Never           | API process must never parse large files synchronously |

**Measurement scope:** The import pipeline must accept files up to 50 MB and routes with up to 500,000 points. The point count target is subject to ongoing benchmark validation as real-world usage patterns emerge. The API process must never block on parsing large files; all heavy parsing is delegated to the worker process.

## Availability Targets

| Requirement                              | Target                                         |
|------------------------------------------|------------------------------------------------|
| Worker unavailability                    | Existing activities remain readable            |
| Map provider failure                     | Does not block route data access               |
| Polar integration failure                | Does not affect stored activity data           |

**Design intent:** The system degrades gracefully. A failure in any single external dependency (map tiles, Polar API, background workers) must not prevent users from accessing their existing data. New imports and syncs may be delayed, but reads remain available.

## Recovery Targets

| Metric                         | Target          | Notes                                    |
|--------------------------------|-----------------|------------------------------------------|
| Recovery Point Objective (RPO) | 15 minutes      | Maximum acceptable data loss window      |
| Recovery Time Objective (RTO)  | 4 hours         | Maximum acceptable downtime              |

**Measurement scope:** RPO is measured as the maximum amount of data loss acceptable in a disaster scenario (database backups run at least every 15 minutes). RTO is the maximum time to restore full service from a complete failure. These are initial targets and will be refined as operational experience grows.

## Formal Revision Process

Performance targets are commitments, not aspirations. Revising a target requires evidence and team consensus. The process is:

1. **Load test evidence** - The proposer must provide load test results demonstrating that the current target is unrealistic under expected production conditions. Anecdotal evidence or single data points are insufficient.

2. **Proposed revised target** - A specific new target value must be proposed, with justification explaining why this value is achievable and still provides acceptable user experience.

3. **Team approval** - The revised target must be reviewed and approved by the team. Performance regressions affect all users and require collective agreement.

4. **ADR documenting the change** - An Architecture Decision Record must be created in `docs/adr/` documenting:
   - The original target and why it was set
   - The evidence showing it is unrealistic
   - The new target and its justification
   - Any compensating measures (caching, UX changes) to mitigate the impact

No target may be silently weakened. All revisions are tracked in version control and linked from the relevant ADR.
