/**
 * Mutations Load Test
 *
 * Tests route editing operations via
 * POST /v1/activities/{activityId}/drafts/{draftId}/operations
 *
 * Performance target: p95 < 500ms per mutation acknowledgement
 *
 * Scenario: 20 virtual users each create a draft and then rapidly apply
 * route editing operations (MovePoint, AddPoint, DeletePoint). Each operation
 * requires an Idempotency-Key header and an expectedRevision in the body,
 * which is incremented after each successful operation. This simulates
 * concurrent editors working on different routes simultaneously.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import {
  BASE_URL,
  authenticatedHeaders,
  mutationThresholds,
  generateIdempotencyKey,
  randomCoordinate,
  assertResponseTime,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '20s', target: 20 }, // Ramp up to 20 VUs over 20 seconds
    { duration: '2m', target: 20 },  // Sustain 20 VUs for 2 minutes
    { duration: '20s', target: 0 },  // Ramp down over 20 seconds
  ],
  thresholds: {
    ...mutationThresholds,
  },
};

// ---------------------------------------------------------------------------
// Pre-configured activity IDs for mutation testing.
// Each VU will create its own draft to avoid revision conflicts between VUs.
// ---------------------------------------------------------------------------

const ACTIVITY_IDS = (
  __ENV.ACTIVITY_IDS || 'activity-1,activity-2,activity-3'
).split(',');

// ---------------------------------------------------------------------------
// Helper: Create a new draft for the given activity
// ---------------------------------------------------------------------------

function createDraft(activityId, headers) {
  const url = `${BASE_URL}/v1/activities/${activityId}/drafts`;
  const res = http.post(url, null, {
    headers: {
      ...headers,
      'Idempotency-Key': generateIdempotencyKey(),
    },
  });

  if (res.status === 201 || res.status === 200) {
    try {
      const body = JSON.parse(res.body);
      return { draftId: body.id, revision: body.revision || 1 };
    } catch (e) {
      return null;
    }
  }
  return null;
}

// ---------------------------------------------------------------------------
// Helper: Apply an operation to a draft
// ---------------------------------------------------------------------------

function applyOperation(activityId, draftId, operation, expectedRevision, headers) {
  const url = `${BASE_URL}/v1/activities/${activityId}/drafts/${draftId}/operations`;
  const payload = JSON.stringify({
    operation,
    expectedRevision,
  });

  const res = http.post(url, payload, {
    headers: {
      ...headers,
      'Idempotency-Key': generateIdempotencyKey(),
    },
  });

  return res;
}

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();

  // Each VU picks a random activity and creates its own draft
  const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];
  const draft = createDraft(activityId, headers);

  if (!draft) {
    console.warn(`VU ${__VU}: Failed to create draft for activity ${activityId}`);
    sleep(1);
    return;
  }

  const { draftId } = draft;
  let revision = draft.revision;

  // Perform a sequence of rapid operations simulating an editing session
  const operations = [
    // MovePoint operation
    {
      type: 'MovePoint',
      pointIndex: 5,
      newPosition: randomCoordinate(),
    },
    // AddPoint operation
    {
      type: 'AddPoint',
      afterIndex: 10,
      position: randomCoordinate(),
    },
    // MovePoint operation with different target
    {
      type: 'MovePoint',
      pointIndex: 3,
      newPosition: randomCoordinate(),
    },
    // DeletePoint operation
    {
      type: 'DeletePoint',
      pointIndex: 8,
    },
    // Another MovePoint
    {
      type: 'MovePoint',
      pointIndex: 12,
      newPosition: randomCoordinate(),
    },
  ];

  for (const operation of operations) {
    const res = applyOperation(activityId, draftId, operation, revision, headers);

    const success = check(res, {
      'mutation: status is 200': (r) => r.status === 200,
      'mutation: response has revision': (r) => {
        try {
          const body = JSON.parse(r.body);
          return body.revision !== undefined;
        } catch (e) {
          return false;
        }
      },
    });

    assertResponseTime(res, 500, 'mutation operation');

    // Increment revision on success to maintain consistency
    if (success && res.status === 200) {
      try {
        const body = JSON.parse(res.body);
        revision = body.revision || revision + 1;
      } catch (e) {
        revision++;
      }
    } else {
      // If an operation fails, break out to avoid cascading revision errors
      break;
    }

    // Small pause between operations (simulating user think time)
    sleep(0.2);
  }

  // Pause between full editing sessions
  sleep(1);
}
