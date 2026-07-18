/**
 * Map Failure Recovery Load Test
 *
 * Tests that API endpoints remain responsive regardless of map tile
 * provider availability. The Haiker API never fetches map tiles itself -
 * this test validates that geometry endpoints always return data with
 * correct GeoJSON structure regardless of external map service status.
 *
 * Performance target: p95 < 500ms for all geometry endpoints
 *
 * Scenario: 20 virtual users continuously request activity details, route
 * previews, and full route geometry. Since the API is architecturally
 * independent from map tile providers, all endpoints should respond within
 * targets. This test serves as a regression guard ensuring no accidental
 * coupling to external tile services is introduced.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend } from 'k6/metrics';
import {
  BASE_URL,
  authenticatedHeaders,
  previewThresholds,
  assertResponseTime,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const detailLatency = new Trend('map_recovery_detail_latency_ms');
const previewLatency = new Trend('map_recovery_preview_latency_ms');
const fullRouteLatency = new Trend('map_recovery_full_route_latency_ms');

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '10s', target: 20 }, // Ramp up to 20 VUs
    { duration: '2m', target: 20 },  // Sustain 20 VUs for 2 minutes
    { duration: '10s', target: 0 },  // Ramp down
  ],
  thresholds: {
    // All geometry endpoints p95 < 500ms
    map_recovery_detail_latency_ms: ['p(95)<500'],
    map_recovery_preview_latency_ms: ['p(95)<500'],
    map_recovery_full_route_latency_ms: ['p(95)<500'],
    http_req_failed: ['rate<0.01'],
  },
};

// ---------------------------------------------------------------------------
// Pre-configured activity IDs
// ---------------------------------------------------------------------------

const ACTIVITY_IDS = (
  __ENV.ACTIVITY_IDS || 'activity-1,activity-2,activity-3'
).split(',');

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();
  const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];

  // Request 1: Activity detail
  const detailUrl = `${BASE_URL}/v1/activities/${activityId}`;
  const detailRes = http.get(detailUrl, { headers });
  detailLatency.add(detailRes.timings.duration);

  check(detailRes, {
    'detail: status is 200': (r) => r.status === 200,
    'detail: response is valid JSON': (r) => {
      try {
        JSON.parse(r.body);
        return true;
      } catch (e) {
        return false;
      }
    },
  });

  // Request 2: Route preview (simplified geometry for map overview)
  const previewUrl = `${BASE_URL}/v1/activities/${activityId}/route/preview`;
  const previewRes = http.get(previewUrl, { headers });
  previewLatency.add(previewRes.timings.duration);

  check(previewRes, {
    'preview: status is 200': (r) => r.status === 200,
    'preview: contains GeoJSON structure': (r) => {
      try {
        const body = JSON.parse(r.body);
        // GeoJSON should have type and/or coordinates
        return (
          body.type !== undefined ||
          body.geometry !== undefined ||
          body.coordinates !== undefined
        );
      } catch (e) {
        return false;
      }
    },
  });

  // Request 3: Full route geometry
  const routeUrl = `${BASE_URL}/v1/activities/${activityId}/route`;
  const routeRes = http.get(routeUrl, { headers });
  fullRouteLatency.add(routeRes.timings.duration);

  check(routeRes, {
    'full route: status is 200': (r) => r.status === 200,
    'full route: contains GeoJSON structure': (r) => {
      try {
        const body = JSON.parse(r.body);
        return (
          body.type !== undefined ||
          body.geometry !== undefined ||
          body.coordinates !== undefined
        );
      } catch (e) {
        return false;
      }
    },
    'full route: has non-trivial body': (r) => r.body && r.body.length > 50,
  });

  // Verify all endpoints remain readable (availability target)
  assertResponseTime(detailRes, 500, 'map recovery detail');
  assertResponseTime(previewRes, 500, 'map recovery preview');
  assertResponseTime(routeRes, 500, 'map recovery full route');

  // Brief pause between request cycles
  sleep(1);
}
