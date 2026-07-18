# Performance Test Results Template

Use this template to record findings from each load test run. Copy this file
and fill in the sections below. Store completed reports in `tests/performance/results/`
or link them from your issue tracker.

---

## Test Run Metadata

| Field         | Value                |
|---------------|----------------------|
| Date          | YYYY-MM-DD HH:MM UTC |
| Tester        | (name / CI bot)      |
| Environment   | local / staging / CI |
| Commit SHA    | `abcdef1`            |
| Branch        | `main` / feature branch |
| Test Duration | e.g. 5m, 10m        |
| k6 Version    | e.g. 0.50.0         |

---

## Environment Description

| Component          | Details                       |
|--------------------|-------------------------------|
| CPU                | e.g. 4 vCPU (GitHub Actions runner) |
| Memory             | e.g. 16 GB                    |
| OS                 | e.g. Ubuntu 22.04             |
| Database           | PostGIS 16-3.4, pool size X   |
| Database Size      | e.g. 500 activities, 2M points |
| Object Storage     | MinIO (local) / S3            |
| Fixture Details    | e.g. large_route.gpx (50k pts) |
| Concurrent Services| API + Worker + PostgreSQL + MinIO |
| Network Latency    | localhost / same-region        |

---

## Test Parameters

| Parameter       | Value                         |
|-----------------|-------------------------------|
| Scripts Run     | e.g. metadata-operations.js, preview-rendering.js |
| Virtual Users   | e.g. 10                       |
| Duration        | e.g. 1m                       |
| Ramp-Up Pattern | e.g. constant / ramping-vus   |
| Overrides       | e.g. BASE_URL, custom thresholds |
| Scenario Filter | e.g. all / metadata / preview |

---

## Results Summary

| Endpoint / Scenario     | p50 (ms) | p95 (ms) | p99 (ms) | Max (ms) | Throughput (req/s) | Error Rate (%) |
|-------------------------|--------:|--------:|--------:|--------:|-----------------:|---------------:|
| GET /activities (list)  |         |         |         |         |                  |                |
| GET /activities/:id     |         |         |         |         |                  |                |
| POST /activities/import |         |         |         |         |                  |                |
| GET /preview/:id        |         |         |         |         |                  |                |
| PUT /route/move-point   |         |         |         |         |                  |                |
| POST /route/publish     |         |         |         |         |                  |                |
| POST /export            |         |         |         |         |                  |                |
| Autosave (debounced)    |         |         |         |         |                  |                |

### Target Comparison

| Metric              | Target    | Actual | Status         |
|---------------------|-----------|--------|----------------|
| Metadata p95        | < 300 ms  |        | PASS / FAIL    |
| Preview p95         | < 500 ms  |        | PASS / FAIL    |
| Mutation p95        | < 500 ms  |        | PASS / FAIL    |
| Autosave            | < 1 s     |        | PASS / FAIL    |
| Editor feedback     | < 16 ms   |        | N/A (client)   |

---

## Identified Bottlenecks

For each bottleneck discovered, fill out one block:

### Bottleneck #1

| Field               | Value                              |
|---------------------|-------------------------------------|
| Trace ID            | e.g. `abc123def456`                |
| Description         | Brief summary of the issue         |
| Root Cause Hypothesis | e.g. N+1 query in activity list  |
| Supporting Evidence | Flame graph link, DB EXPLAIN plan, k6 trend chart |
| Impact Severity     | Critical / High / Medium / Low     |
| Resolution Status   | Open / In Progress / Resolved      |
| Resolution Notes    | What was done or planned           |

_(Copy this block for additional bottlenecks.)_

---

## Capacity Assumptions Validated / Revised

| Assumption                      | Expected       | Observed        | Revision Needed? |
|---------------------------------|----------------|-----------------|------------------|
| Max concurrent users            | 50             |                 | Yes / No         |
| Queue depth steady state        | < 10           |                 | Yes / No         |
| Max pending jobs (backpressure) | 1000           |                 | Yes / No         |
| Import processing time          | < 5 min        |                 | Yes / No         |
| Max route size (points)         | 500,000        |                 | Yes / No         |
| Per-activity storage            | (estimated)    |                 | Yes / No         |

---

## Launch Blocker Assessment

| Finding                  | Owner  | Severity | Target Resolution Date | Status |
|--------------------------|--------|----------|------------------------|--------|
| (describe blocking issue)| @name  | Critical |                        | Open   |

If no launch blockers were found, note: **No launch blockers identified in this run.**

---

## Action Items

| Priority | Action                                  | Owner  | Due Date   |
|----------|-----------------------------------------|--------|------------|
| P0       | (critical items)                        |        |            |
| P1       | (important follow-ups)                  |        |            |
| P2       | (nice-to-have improvements)             |        |            |

---

## Raw Data Location

| Artifact              | Location                                          |
|-----------------------|---------------------------------------------------|
| k6 JSON output        | `tests/performance/results/<run-id>/summary.json` |
| k6 HTML report        | `tests/performance/results/<run-id>/report.html`  |
| Grafana dashboard     | (link to dashboard with time range)               |
| CI workflow run        | (link to GitHub Actions run)                      |
| Flame graphs          | (link or path)                                    |
| DB EXPLAIN plans      | (link or path)                                    |
