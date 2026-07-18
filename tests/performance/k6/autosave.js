/**
 * Autosave Acknowledgement Load Test
 *
 * Tests the autosave acknowledgement timing to validate that periodic saves
 * from multiple concurrent editor sessions complete within the 1-second target.
 *
 * Performance target: p95 < 1000ms (1 second)
 *
 * Scenario: 10 virtual users simulate periodic autosave behavior where
 * pending operations are submitted every 5 seconds. Each VU creates a draft
 * and then periodically sends operations, measuring the time from submission
 * to server acknowledgement. The response must contain an updated revision
 * to confirm the save was persisted.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend } from 'k6/metrics';
import {
  BASE_URL,
  authenticatedHeaders,
  generateIdempotencyKey,
  randomCoordinate,
  assertResponseTime,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const autosaveAck = new Trend('autosave_ack_duration_ms');

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '10s', target: 10 }, // Ramp up to 10 VUs over 10 seconds
    { duration: '2m', target: 10 },  // Sustain 10 VUs for 2 minutes
    { duration: '10s', target: 0 },  // Ramp down over 10 seconds
  ],
  thresholds: {
    // Autosave acknowledgement p95 < 1 second
    autosave_ack_duration_ms: ['p(95)<1000'],
    http_req_failed: ['rate<0.05'],
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

  // Each VU creates its own draft
  const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];
  const draftUrl = `${BASE_URL}/v1/activities/${activityId}/drafts`;

  const draftRes = http.post(draftUrl, null, {
    headers: {
      ...headers,
      'Idempotency-Key': generateIdempotencyKey(),
    },
  });

  if (draftRes.status !== 201 && draftRes.status !== 200) {
    console.warn(`VU ${__VU}: Failed to create draft: status=${draftRes.status}`);
    sleep(5);
    return;
  }

  let draftId;
  let revision;
  try {
    const body = JSON.parse(draftRes.body);
    draftId = body.id;
    revision = body.revision || 1;
  } catch (e) {
    console.warn(`VU ${__VU}: Failed to parse draft response`);
    sleep(5);
    return;
  }

  // Simulate periodic autosave: submit operations every 5 seconds
  // In a 2-minute sustained phase, each VU will do ~24 autosaves
  const numSaves = 24;

  for (let i = 0; i < numSaves; i++) {
    // Simulate accumulated edits as a single operation submission
    const operation = {
      type: 'MovePoint',
      pointIndex: Math.floor(Math.random() * 50),
      newPosition: randomCoordinate(),
    };

    const operationUrl = `${BASE_URL}/v1/activities/${activityId}/drafts/${draftId}/operations`;
    const payload = JSON.stringify({
      operation,
      expectedRevision: revision,
    });

    // Measure time from submission to acknowledgement
    const ackStart = Date.now();
    const res = http.post(operationUrl, payload, {
      headers: {
        ...headers,
        'Idempotency-Key': generateIdempotencyKey(),
      },
    });
    const ackDuration = Date.now() - ackStart;

    autosaveAck.add(ackDuration);

    const success = check(res, {
      'autosave: status is 200': (r) => r.status === 200,
      'autosave: response contains revision': (r) => {
        try {
          const body = JSON.parse(r.body);
          return body.revision !== undefined;
        } catch (e) {
          return false;
        }
      },
      'autosave: acknowledged within 1 second': (r) => ackDuration < 1000,
    });

    // Update revision for next save
    if (success && res.status === 200) {
      try {
        const body = JSON.parse(res.body);
        revision = body.revision || revision + 1;
      } catch (e) {
        revision++;
      }
    } else {
      // If save fails, still try next iteration with incremented revision
      revision++;
    }

    // Wait 5 seconds before next autosave (simulating periodic save interval)
    sleep(5);
  }
}
