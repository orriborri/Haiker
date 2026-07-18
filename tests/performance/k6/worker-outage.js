/**
 * Worker Outage Load Test
 *
 * Tests system behavior when background workers are unavailable. Validates
 * that read operations remain fast and writes are accepted (queued) even
 * when workers cannot process them.
 *
 * Performance target: read operations p95 < 500ms even during worker outage
 *
 * Scenario: 20 virtual users run in two phases:
 * - Phase 1 (first minute): Normal read operations to establish a baseline
 * - Phase 2 (second minute): Submits imports/exports that will queue (workers
 *   assumed stopped externally) while continuing reads to verify they stay fast
 *
 * This test validates the availability target: existing activities remain
 * readable when workers are unavailable. The assumption is that workers have
 * been stopped externally before or during Phase 2.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend, Counter } from 'k6/metrics';
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

const readLatency = new Trend('worker_outage_read_latency_ms');
const writeAccepted = new Counter('worker_outage_write_accepted');
const queueGrowth = new Counter('worker_outage_queue_growth');

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
    // Read operations must stay fast even during worker outage
    worker_outage_read_latency_ms: ['p(95)<500'],
    http_req_failed: ['rate<0.10'],
  },
};

// ---------------------------------------------------------------------------
// Pre-configured activity IDs
// ---------------------------------------------------------------------------

const ACTIVITY_IDS = (
  __ENV.ACTIVITY_IDS || 'activity-1,activity-2,activity-3'
).split(',');

// Small GPX payload for import requests during Phase 2
const SMALL_GPX = generateGpxPayload(50);

// Test start time (set during first iteration)
let testStartTime = 0;

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();
  const now = Date.now();

  // Initialize test start time on first iteration
  if (testStartTime === 0) {
    testStartTime = now;
  }

  // Calculate elapsed time to determine phase
  // Phase 1: first ~60 seconds (baseline reads)
  // Phase 2: after ~60 seconds (reads + writes with workers assumed down)
  const elapsedSeconds = (now - testStartTime) / 1000;
  const isPhase2 = elapsedSeconds > 60;

  // Always perform read operations (the core availability test)
  const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];

  // Read: Activity list
  const listUrl = `${BASE_URL}/v1/activities`;
  const listRes = http.get(listUrl, { headers });
  readLatency.add(listRes.timings.duration);

  check(listRes, {
    'read: list status is 200': (r) => r.status === 200,
    'read: list responds within target': (r) => r.timings.duration < 500,
  });

  // Read: Activity detail
  const detailUrl = `${BASE_URL}/v1/activities/${activityId}`;
  const detailRes = http.get(detailUrl, { headers });
  readLatency.add(detailRes.timings.duration);

  check(detailRes, {
    'read: detail status is 200': (r) => r.status === 200,
    'read: detail responds within target': (r) => r.timings.duration < 500,
  });

  // Phase 2: Also submit writes that will queue (workers assumed unavailable)
  if (isPhase2) {
    // Submit import request (should be accepted into queue)
    const importUrl = `${BASE_URL}/v1/imports`;
    const importRes = http.post(importUrl, SMALL_GPX, {
      headers: {
        ...headers,
        'Content-Type': 'application/gpx+xml',
        'Idempotency-Key': generateIdempotencyKey(),
      },
    });

    if (importRes.status === 201 || importRes.status === 202) {
      writeAccepted.add(1);
      queueGrowth.add(1);
    }

    check(importRes, {
      'write: import accepted into queue': (r) =>
        r.status === 201 || r.status === 202 || r.status === 429,
    });

    // Also submit export request occasionally
    if (Math.random() < 0.3) {
      const exportUrl = `${BASE_URL}/v1/activities/${activityId}/exports`;
      const exportPayload = JSON.stringify({ format: 'gpx' });
      const exportRes = http.post(exportUrl, exportPayload, {
        headers: {
          ...headers,
          'Idempotency-Key': generateIdempotencyKey(),
        },
      });

      if (exportRes.status === 201 || exportRes.status === 202) {
        writeAccepted.add(1);
        queueGrowth.add(1);
      }

      check(exportRes, {
        'write: export accepted into queue': (r) =>
          r.status === 201 || r.status === 202 || r.status === 429,
      });
    }
  }

  // Verify read latency remains within targets regardless of phase
  assertResponseTime(listRes, 500, 'worker outage read list');
  assertResponseTime(detailRes, 500, 'worker outage read detail');

  // Pause between iterations
  sleep(1);
}
