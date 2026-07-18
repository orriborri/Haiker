/**
 * Queue Pressure Stress Test
 *
 * Stress tests the system by flooding it with import and export requests
 * simultaneously to validate backpressure behavior.
 *
 * Scenario: 50 virtual users submit a mix of import requests (70%) and
 * export requests (30%) continuously. The test validates that:
 * 1. Backpressure triggers appropriately (429 Too Many Requests)
 * 2. Read endpoints (GET /v1/activities) remain responsive under queue pressure
 *
 * Custom metrics track accepted vs rejected requests and read latency
 * under pressure conditions.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend, Counter, Rate } from 'k6/metrics';
import {
  BASE_URL,
  authenticatedHeaders,
  generateIdempotencyKey,
  generateGpxPayload,
  assertResponseTime,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const acceptedRequests = new Counter('accepted_requests');
const rejectedRequests = new Counter('rejected_requests');
const readLatencyUnderPressure = new Trend('read_latency_under_pressure_ms');
const backpressureRate = new Rate('backpressure_triggered');

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '10s', target: 50 }, // Ramp up rapidly to 50 VUs
    { duration: '3m', target: 50 },  // Sustain 50 VUs for 3 minutes
    { duration: '10s', target: 0 },  // Ramp down over 10 seconds
  ],
  thresholds: {
    // Read endpoints should remain responsive under pressure
    read_latency_under_pressure_ms: ['p(95)<1000'],
    // We expect some backpressure but not all requests rejected
    http_req_failed: ['rate<0.50'],
  },
};

// ---------------------------------------------------------------------------
// Pre-configured activity IDs
// ---------------------------------------------------------------------------

const ACTIVITY_IDS = (
  __ENV.ACTIVITY_IDS || 'activity-1,activity-2,activity-3'
).split(',');

// Generate a small GPX payload for import requests (not huge - we want volume)
const SMALL_GPX = generateGpxPayload(100);

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();

  // Determine action based on weighted random: 70% import, 30% export
  const roll = Math.random();

  if (roll < 0.7) {
    // Import request
    const importUrl = `${BASE_URL}/v1/imports`;
    const res = http.post(importUrl, SMALL_GPX, {
      headers: {
        ...headers,
        'Content-Type': 'application/gpx+xml',
        'Idempotency-Key': generateIdempotencyKey(),
      },
    });

    if (res.status === 429) {
      rejectedRequests.add(1);
      backpressureRate.add(true);
    } else if (res.status === 201 || res.status === 202) {
      acceptedRequests.add(1);
      backpressureRate.add(false);
    } else {
      backpressureRate.add(false);
    }

    check(res, {
      'import: accepted or rate-limited': (r) =>
        r.status === 201 || r.status === 202 || r.status === 429,
    });
  } else {
    // Export request
    const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];
    const exportUrl = `${BASE_URL}/v1/activities/${activityId}/exports`;
    const payload = JSON.stringify({ format: 'gpx' });

    const res = http.post(exportUrl, payload, {
      headers: {
        ...headers,
        'Idempotency-Key': generateIdempotencyKey(),
      },
    });

    if (res.status === 429) {
      rejectedRequests.add(1);
      backpressureRate.add(true);
    } else if (res.status === 201 || res.status === 202) {
      acceptedRequests.add(1);
      backpressureRate.add(false);
    } else {
      backpressureRate.add(false);
    }

    check(res, {
      'export: accepted or rate-limited': (r) =>
        r.status === 201 || r.status === 202 || r.status === 429,
    });
  }

  // After each write request, also verify read endpoints remain responsive
  const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];
  const readUrl = `${BASE_URL}/v1/activities`;
  const readRes = http.get(readUrl, { headers });

  readLatencyUnderPressure.add(readRes.timings.duration);

  check(readRes, {
    'read under pressure: status is 200': (r) => r.status === 200,
    'read under pressure: responds within 1s': (r) => r.timings.duration < 1000,
  });

  // Brief pause to avoid overwhelming the test runner itself
  sleep(0.1);
}
