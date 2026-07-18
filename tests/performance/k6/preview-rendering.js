/**
 * Preview Rendering Load Test
 *
 * Tests the route preview geometry endpoint
 * (GET /v1/activities/{activityId}/route/preview) under sustained load.
 *
 * Performance target: p95 < 500ms
 *
 * Scenario: 30 virtual users request route previews for activities of varying
 * route sizes. This simulates users viewing map thumbnails or route overviews
 * in a list view or detail page. The endpoint returns simplified GeoJSON-like
 * geometry suitable for map rendering at overview zoom levels.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import {
  BASE_URL,
  authenticatedHeaders,
  previewThresholds,
  assertResponseTime,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '20s', target: 30 }, // Ramp up to 30 VUs over 20 seconds
    { duration: '2m', target: 30 },  // Sustain 30 VUs for 2 minutes
    { duration: '20s', target: 0 },  // Ramp down over 20 seconds
  ],
  thresholds: {
    ...previewThresholds,
  },
};

// ---------------------------------------------------------------------------
// Pre-configured activity IDs representing different route sizes.
// These should be seeded in the test database before running.
// Categorized by approximate route point count for varying load.
// ---------------------------------------------------------------------------

const ACTIVITY_IDS_SMALL = (
  __ENV.ACTIVITY_IDS_SMALL || 'activity-small-1,activity-small-2'
).split(',');

const ACTIVITY_IDS_MEDIUM = (
  __ENV.ACTIVITY_IDS_MEDIUM || 'activity-medium-1,activity-medium-2'
).split(',');

const ACTIVITY_IDS_LARGE = (
  __ENV.ACTIVITY_IDS_LARGE || 'activity-large-1,activity-large-2'
).split(',');

// Combined pool with weighting: more small/medium requests than large
const ALL_ACTIVITIES = [
  ...ACTIVITY_IDS_SMALL,
  ...ACTIVITY_IDS_SMALL,
  ...ACTIVITY_IDS_MEDIUM,
  ...ACTIVITY_IDS_MEDIUM,
  ...ACTIVITY_IDS_LARGE,
];

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();

  // Pick a random activity from the weighted pool
  const activityId = ALL_ACTIVITIES[Math.floor(Math.random() * ALL_ACTIVITIES.length)];
  const previewUrl = `${BASE_URL}/v1/activities/${activityId}/route/preview`;

  const res = http.get(previewUrl, { headers });

  check(res, {
    'preview: status is 200': (r) => r.status === 200,
    'preview: response has geometry': (r) => {
      try {
        const body = JSON.parse(r.body);
        // Validate GeoJSON-like structure: should have type and coordinates
        return (
          body.type !== undefined ||
          body.geometry !== undefined ||
          body.coordinates !== undefined
        );
      } catch (e) {
        return false;
      }
    },
    'preview: response is not empty': (r) => r.body && r.body.length > 10,
  });

  assertResponseTime(res, 500, 'preview rendering');

  // Brief pause to simulate realistic request pacing
  sleep(1);
}
