# Project Overview

## What is Haiker?

Haiker is a hiking-route management application that lets users import historical hiking activities, view them on a map, correct inaccurate GPS tracks, preserve the original recording, publish corrected route versions, and export corrected routes.

## Core User Journey (MVP)

```
Upload GPX → Import activity → View recorded route → Create editing draft
→ Correct route → Publish corrected version → Export corrected GPX
```

## Product Principles

1. Preserve the original recording — never modify what the device reported.
2. Make all corrections reversible and transparent.
3. Distinguish recorded truth from corrected route geometry.
4. Keep routes private by default.
5. Make the server authoritative — the database is the source of truth.
6. Prefer explicit domain actions over generic CRUD.
7. Deliver complete user outcomes through vertical slices.
8. Avoid premature microservices.
9. Treat uploaded files as hostile input.
10. Keep the public API stable and contract-driven.

## System of Record

The Haiker server-side PostgreSQL database is the authoritative source of truth for all activities, routes, versions, drafts, imports, and exports. External providers (Polar Flow) and uploaded files (GPX, FIT, TCX) are ingestion sources only. The browser cache is never authoritative.

## Key Domain Rules

- Recorded tracks are immutable after import.
- Published route versions are immutable.
- Corrected routes are versioned — each publication creates a new immutable version.
- New external data (e.g., Polar sync) creates a new source revision but never overwrites user corrections.
- Every route edit uses optimistic concurrency (expectedRevision).
- Every retryable command is idempotent.
- Clients are non-authoritative.

## MVP Scope

The MVP supports: user authentication, GPX 1.0/1.1 upload, async GPX import, activity library, activity detail, recorded route display, route draft creation, move/add/delete point, delete/replace section, split/join segment, undo/redo, reset draft, publish immutable corrected route version, recorded vs. corrected distance display, GPX export, exact duplicate detection, audit events, backup/restore.

## Explicitly Excluded from MVP

- Live hike recording
- Social network features
- Public route discovery
- Editing heart-rate or sensor values
- Automatic trail snapping
- Full event sourcing
- Microservice decomposition
- GraphQL, tRPC, or browser-facing gRPC

## Reference

The complete product and engineering plan is in `#[[file:haiker-complete-plan.md]]`.
