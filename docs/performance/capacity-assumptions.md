# Capacity Assumptions

This document captures the initial capacity assumptions for Haiker. These
inform performance test parameters, infrastructure sizing, and launch readiness
decisions. Assumptions are validated through load testing and revised as data
becomes available.

---

## Concurrent Users

| Parameter                   | Value | Rationale                              |
|-----------------------------|-------|----------------------------------------|
| MVP concurrent users        | 50    | Initial launch target for early adopters |
| Peak concurrent users       | 100   | 2x headroom over expected steady state |
| Concurrent uploads          | 10    | Subset of users uploading simultaneously |
| Concurrent editors          | 20    | Subset of users in route editor        |

---

## Route Sizes

| Parameter                   | Value       | Rationale                             |
|-----------------------------|-------------|---------------------------------------|
| Maximum route points        | 500,000     | Hard limit enforced at import         |
| Maximum route segments      | 10,000      | Hard limit enforced at import         |
| Maximum file size           | 50 MB       | Hard limit enforced at upload         |
| Typical route (casual user) | 1,000-5,000 points | Average recreational activity  |
| Typical route (power user)  | 5,000-10,000 points | Multi-hour recording          |
| Large route (stress test)   | 50,000-100,000 points | Ultra/multi-day activities  |
| Edge case (max capacity)    | 500,000 points | System must handle without crash  |

---

## Import Frequency

| Parameter                         | Value    | Rationale                          |
|-----------------------------------|----------|------------------------------------|
| Imports per user per hour         | 2-3      | Typical upload-then-edit workflow   |
| Total system imports per hour     | 50-100   | 50 users x 2 imports average       |
| Peak imports per minute           | 10       | Burst after event/race completion   |
| Import processing timeout         | 5 min    | Worker timeout for a single import  |

---

## Queue Depth

| Parameter                         | Value    | Source                             |
|-----------------------------------|----------|------------------------------------|
| Max pending jobs (backpressure)   | 1,000    | `BACKPRESSURE_MAX_PENDING_JOBS` in platform/src/backpressure.rs |
| Expected steady-state queue depth | < 10     | Most jobs processed within seconds |
| Alert threshold                   | 100      | Investigate if queue exceeds this  |
| Critical threshold                | 500      | Trigger scaling or shed load       |

When the queue reaches the max of 1,000 pending jobs, the system applies
backpressure by rejecting new import requests with HTTP 503 (Service Unavailable).

---

## Worker Processing Budgets

| Operation              | Budget    | Notes                                |
|------------------------|-----------|--------------------------------------|
| GPX import (parsing)   | 5 min     | Hard timeout per job                 |
| Preview generation     | 30 s      | Simplification + tile rendering      |
| Export generation       | 2 min     | Serialize corrected route to GPX     |
| Autosave persistence   | 1 s       | Must complete within autosave window |
| Route correction apply | 500 ms    | Server-side mutation budget          |

---

## Storage Growth

| Component                | Size Estimate    | Notes                             |
|--------------------------|------------------|-----------------------------------|
| GPX source file          | 1-50 MB          | Stored in object storage (MinIO/S3) |
| Parsed geometry (DB)     | 50 KB - 5 MB     | PostGIS geometry column            |
| Preview tiles/image      | 100 KB - 1 MB    | Cached in object storage           |
| Per-activity total       | ~2-55 MB         | Source + geometry + preview        |
| Monthly growth (50 users)| ~5-10 GB         | Assuming 100 activities/day avg    |
| Yearly projection        | ~60-120 GB       | Linear growth at MVP scale         |

Object storage (MinIO/S3) handles the bulk of data. PostgreSQL stores metadata
and geometry but not raw files.

---

## Database Connection Pool

| Parameter              | Value    | Rationale                            |
|------------------------|----------|--------------------------------------|
| Pool size (API)        | 20       | 50 concurrent users, not all DB-bound |
| Pool size (Worker)     | 10       | Workers are I/O-bound on file processing |
| Total connections      | 30       | API pool + worker pool               |
| Max PostgreSQL conns   | 100      | Default; leaves headroom for admin   |
| Connection timeout     | 5 s      | Fail fast if pool exhausted          |
| Idle timeout           | 300 s    | Return unused connections            |

---

## Memory Budget

| Process          | Budget    | Notes                                  |
|------------------|-----------|----------------------------------------|
| API process      | 512 MB    | Request handling, serialization        |
| Worker process   | 1 GB      | Large GPX parsing can be memory-intensive |
| Per-import ceiling| 256 MB   | Single import must not exceed this     |
| Total system     | 2 GB      | Combined for MVP deployment            |

Large imports (500,000 points) require streaming/chunked parsing to stay within
the per-import memory ceiling.

---

## Network

| Parameter                    | Value    | Notes                              |
|------------------------------|----------|------------------------------------|
| Max upload bandwidth (single)| 50 MB    | Full file upload                   |
| Upload timeout               | 5 min    | Matches worker timeout             |
| Concurrent upload capacity   | 10       | Limited by connection pool + backpressure |
| Expected upload duration     | 10-60 s  | Depends on client connection speed |
| API response payload (typical)| < 100 KB | JSON metadata + GeoJSON preview  |

---

## Recovery Targets

| Metric | Target   | Notes                                    |
|--------|----------|------------------------------------------|
| RPO    | 15 min   | Recovery Point Objective (max data loss)  |
| RTO    | 4 hours  | Recovery Time Objective (max downtime)    |

These targets inform backup frequency and disaster recovery procedures.

---

## Revision Log

Track changes to capacity assumptions as load testing reveals actual system behavior.

| Date       | Author  | Change Description                       | Evidence              |
|------------|---------|------------------------------------------|-----------------------|
| 2024-01-01 | Initial | Document created with initial assumptions | Architecture planning |
|            |         |                                          |                       |
|            |         |                                          |                       |
