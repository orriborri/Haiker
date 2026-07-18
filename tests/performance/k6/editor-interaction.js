/**
 * Editor Interaction Load Test
 *
 * Simulates rapid editor operations at pointer-speed to validate that the
 * API can handle fast mutation sequences without degradation.
 *
 * Performance target: mutation acknowledgement p95 < 500ms
 *
 * Scenario: 5 virtual users each simulate a fast editing session where
 * MovePoint operations are fired rapidly (every 100ms) to simulate the
 * 16ms-target interaction pattern at the API layer. Each VU creates a draft
 * and then fires a continuous stream of small coordinate adjustments,
 * tracking revision progression for consistency validation.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend, Counter } from 'k6/metrics';
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

const mutationAck = new Trend('editor_mutation_ack_ms');
const revisionProgression = new Counter('editor_revision_progression');
const revisionMismatch = new Counter('editor_revision_mismatch');

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '10s', target: 5 },  // Ramp up to 5 VUs over 10 seconds
    { duration: '2m', target: 5 },   // Sustain 5 VUs for 2 minutes
    { duration: '10s', target: 0 },  // Ramp down over 10 seconds
  ],
  thresholds: {
    // Mutation acknowledgement p95 < 500ms
    editor_mutation_ack_ms: ['p(95)<500'],
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

  // Each VU creates its own draft to avoid revision conflicts
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
    sleep(2);
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
    sleep(2);
    return;
  }

  // Fire rapid MovePoint operations simulating pointer-speed editing
  // 100ms between operations simulates the API-layer validation of the
  // 16ms editor feedback target (network + server processing time)
  const numOperations = 30; // ~3 seconds of rapid editing per iteration
  let lastRevision = revision;

  for (let i = 0; i < numOperations; i++) {
    // Generate a small coordinate delta (simulating a point drag)
    const baseLat = 46.5 + (i * 0.0001);
    const baseLon = 8.0 + (i * 0.00005);

    const operation = {
      type: 'MovePoint',
      pointIndex: i % 20, // Cycle through first 20 points
      newPosition: {
        lat: (baseLat + (Math.random() * 0.0002 - 0.0001)).toFixed(6),
        lon: (baseLon + (Math.random() * 0.0002 - 0.0001)).toFixed(6),
      },
    };

    const operationUrl = `${BASE_URL}/v1/activities/${activityId}/drafts/${draftId}/operations`;
    const payload = JSON.stringify({
      operation,
      expectedRevision: lastRevision,
    });

    const opStart = Date.now();
    const res = http.post(operationUrl, payload, {
      headers: {
        ...headers,
        'Idempotency-Key': generateIdempotencyKey(),
      },
    });
    const opDuration = Date.now() - opStart;

    mutationAck.add(opDuration);

    const success = check(res, {
      'editor op: status is 200': (r) => r.status === 200,
    });

    if (success && res.status === 200) {
      try {
        const body = JSON.parse(res.body);
        const newRevision = body.revision;

        // Verify revision progresses monotonically
        if (newRevision > lastRevision) {
          revisionProgression.add(1);
          lastRevision = newRevision;
        } else {
          revisionMismatch.add(1);
        }
      } catch (e) {
        lastRevision++;
      }
    } else {
      // On failure, break to avoid cascading revision errors
      break;
    }

    // 100ms pause between operations to simulate rapid editing
    sleep(0.1);
  }

  // Pause between editing sessions
  sleep(2);
}
