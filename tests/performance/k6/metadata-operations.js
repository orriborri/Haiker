/**
 * Metadata Operations Load Test
 *
 * Tests the activity listing (GET /v1/activities) and activity detail
 * (GET /v1/activities/{activityId}) endpoints under sustained load.
 *
 * Performance target: p95 < 300ms
 *
 * Scenario: 50 virtual users alternate between paginated list requests and
 * detail requests for individual activities. This simulates a typical user
 * browsing pattern where they view their activity feed and click into details.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import {
  BASE_URL,
  authenticatedHeaders,
  metadataThresholds,
  assertResponseTime,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '30s', target: 50 }, // Ramp up to 50 VUs over 30 seconds
    { duration: '2m', target: 50 },  // Sustain 50 VUs for 2 minutes
    { duration: '30s', target: 0 },  // Ramp down over 30 seconds
  ],
  thresholds: {
    ...metadataThresholds,
  },
};

// ---------------------------------------------------------------------------
// Pre-configured activity IDs for detail requests.
// In a real run, these would be seeded beforehand or discovered via the list endpoint.
// ---------------------------------------------------------------------------

const ACTIVITY_IDS = (
  __ENV.ACTIVITY_IDS || 'activity-1,activity-2,activity-3'
).split(',');

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();

  // Alternate between list and detail on each iteration
  if (__ITER % 2 === 0) {
    // Paginated list request
    const page = Math.floor(Math.random() * 5) + 1;
    const listUrl = `${BASE_URL}/v1/activities?page=${page}&per_page=20`;
    const listRes = http.get(listUrl, { headers });

    check(listRes, {
      'list: status is 200': (r) => r.status === 200,
      'list: body is not empty': (r) => r.body && r.body.length > 2,
    });

    assertResponseTime(listRes, 300, 'metadata list');
  } else {
    // Detail request for a random activity
    const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];
    const detailUrl = `${BASE_URL}/v1/activities/${activityId}`;
    const detailRes = http.get(detailUrl, { headers });

    check(detailRes, {
      'detail: status is 200': (r) => r.status === 200,
      'detail: body contains id': (r) => {
        try {
          const body = JSON.parse(r.body);
          return body.id !== undefined;
        } catch (e) {
          return false;
        }
      },
    });

    assertResponseTime(detailRes, 300, 'metadata detail');
  }

  // Brief pause between requests to simulate realistic user pacing
  sleep(0.5);
}
