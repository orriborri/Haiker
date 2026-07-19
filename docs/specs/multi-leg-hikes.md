# Spec: Multi-Leg Hikes

## Objective

Allow a hike activity to be composed of multiple legs, where each leg represents a separately-recorded GPS track (typically one per day or per section). Users can import multiple GPX files into one activity, view them together on a map, see statistics aggregated by leg, by day, or for the whole hike, and edit each leg's route independently.

## Domain Model

### Current State

```
Activity (1) ──── (1) SourceRevision ──── (1) RecordedTrack
```

An activity has exactly one recorded track.

### Target State

```
Activity (1) ──── (1..*) Leg ──── (1) RecordedTrack
                    │
                    ├── leg_number: u32 (ordering)
                    ├── title: Option<String> (e.g. "Abisko to Alesjaure")
                    ├── date: NaiveDate (the day this leg was hiked)
                    └── source_revision_id: FK
```

- A **Leg** is a recorded section of a hike with its own GPS track.
- An Activity has one or more Legs, ordered by `leg_number`.
- Each Leg has a `date` for day-level aggregation.
- Existing single-track activities become 1-leg activities (data migration).
- A Leg can be renamed, reordered, and edited independently.

### Statistics Aggregation

| Level | Scope |
|-------|-------|
| Leg | Single leg's recorded/corrected stats |
| Day | Sum of all legs sharing the same date |
| Hike | Sum of all legs in the activity |

Aggregated fields: distance, elevation gain, elevation loss, point count, duration.

## User Stories

1. **Import a leg into an existing activity** — User opens an activity and adds a new leg by uploading a GPX file. The leg is appended with the next leg_number.
2. **Import as new activity with one leg** — Current behavior. A new activity is created with a single leg.
3. **Create an empty multi-leg activity first** — User creates a named activity (e.g. "Kungsleden 2026"), then adds legs to it over time.
4. **View all legs on the map** — Activity detail shows all legs on the same map with visual differentiation (different colors or line styles per leg).
5. **View statistics by leg, day, or whole hike** — Toggle or tabs for aggregation level.
6. **Reorder legs** — Drag or move a leg to a different position in the sequence.
7. **Rename a leg** — Give a leg a custom title.
8. **Edit a leg's route** — Open the route editor for a specific leg (not the whole hike).
9. **Remove a leg** — Delete a leg from an activity without deleting the whole activity.

## API Changes

### New Endpoints

```
POST   /v1/activities/{activityId}/legs          — Add a leg (import GPX into existing activity)
GET    /v1/activities/{activityId}/legs          — List legs for an activity
GET    /v1/activities/{activityId}/legs/{legId}  — Get a specific leg's detail
PATCH  /v1/activities/{activityId}/legs/{legId}  — Update leg title, date, order
DELETE /v1/activities/{activityId}/legs/{legId}  — Remove a leg
```

### Modified Endpoints

```
GET /v1/activities/{activityId}                  — Include legs summary, aggregated stats
GET /v1/activities/{activityId}/recorded-route   — Return geometry for all legs (or accept ?leg={legId} filter)
POST /v1/activities/{activityId}/route-drafts    — Accept legId to specify which leg to edit
```

### Response Shape: Activity with Legs

```json
{
  "id": "...",
  "title": "Kungsleden 2026",
  "activityType": "hike",
  "legs": [
    {
      "id": "...",
      "legNumber": 1,
      "title": "Abisko to Alesjaure",
      "date": "2026-07-07",
      "recordedSummary": { "distance_meters": 13411, ... }
    },
    {
      "id": "...",
      "legNumber": 2,
      "title": "Alesjaure to Tjaktja",
      "date": "2026-07-08",
      "recordedSummary": { "distance_meters": 12800, ... }
    }
  ],
  "aggregatedStats": {
    "totalDistance": 26211,
    "totalElevationGain": 940,
    "totalDays": 2,
    "totalLegs": 2
  }
}
```

## Database Changes

### New Table: `recorded_activity.legs`

```sql
CREATE TABLE recorded_activity.legs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    activity_id UUID NOT NULL REFERENCES activity_catalog.activities(id),
    leg_number INTEGER NOT NULL,
    title TEXT,
    date DATE NOT NULL,
    source_revision_id UUID REFERENCES recorded_activity.source_revisions(id),
    recorded_track_id UUID REFERENCES recorded_activity.recorded_tracks(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (activity_id, leg_number)
);
```

### Migration for Existing Data

Existing activities get a single leg created automatically:

```sql
INSERT INTO recorded_activity.legs (activity_id, leg_number, date, source_revision_id, recorded_track_id)
SELECT
    a.id,
    1,
    COALESCE(a.started_at::date, a.created_at::date),
    sr.id,
    rt.id
FROM activity_catalog.activities a
JOIN recorded_activity.source_revisions sr ON sr.activity_id = a.id
JOIN recorded_activity.recorded_tracks rt ON rt.source_revision_id = sr.id;
```

## Frontend Changes

### Activity Detail Page

- Show a leg list/tabs below the map
- Map renders all legs with different colors (one hue per leg)
- Stats section has toggle: "Per leg" | "Per day" | "Total"
- Each leg row: title (or "Leg N"), date, distance, duration
- "Add leg" button → opens import flow scoped to this activity
- Leg context menu: rename, reorder, delete, edit route

### Activity Library

- Show total legs count badge for multi-leg activities
- Show aggregated stats (total distance, total days)

## Boundaries

- **Always:** Legs belong to one activity. Activity ownership still governs access.
- **Ask first:** Merging legs, splitting a leg into two.
- **Never:** Auto-reorder legs by date (user controls order explicitly).

## Success Criteria

1. User can import a second GPX file into an existing activity as Leg 2
2. Activity detail page shows both legs on the map with visual differentiation
3. Statistics show correct aggregation at leg, day, and hike level
4. Each leg can be edited independently in the route editor
5. Existing single-leg activities continue to work unchanged
6. Legs can be reordered and renamed

## Open Questions

1. Should "day" aggregation be automatic (based on date) or user-defined groups?
   → **Decision: Automatic based on leg date.**
2. Max legs per activity? → Suggest no hard limit, soft cap at 30 for UI.
3. Should the import flow ask "Add to existing activity or create new?" → Yes, after upload.
