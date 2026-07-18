/**
 * Large Points Import Load Test (500,000 points)
 *
 * Tests the system's ability to accept and process a GPX file containing
 * 500,000 trackpoints. This validates the performance target for high
 * point-count routes.
 *
 * Scenario: A single virtual user uploads the 500,000-point fixture file,
 * then polls the import status until completion. Custom Trend metrics track
 * upload time, worker processing time, and total elapsed time.
 *
 * The test monitors response sizes and timing patterns during polling to
 * identify potential memory pressure signals in the processing pipeline.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend, Counter } from 'k6/metrics';
import {
  BASE_URL,
  authenticatedHeaders,
  generateIdempotencyKey,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const uploadDuration = new Trend('points_upload_duration_ms');
const processingDuration = new Trend('points_processing_duration_ms');
const totalDuration = new Trend('points_total_duration_ms');
const pollResponseSize = new Trend('points_poll_response_size_bytes');
const pollCount = new Counter('points_poll_iterations');

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  // Single VU for large point-count import
  stages: [
    { duration: '10m', target: 1 },
  ],
  thresholds: {
    // The import should complete within 10 minutes
    points_total_duration_ms: ['p(95)<600000'],
    // Upload phase should complete within 2 minutes
    points_upload_duration_ms: ['p(95)<120000'],
  },
  // Only run one iteration
  iterations: 1,
};

// ---------------------------------------------------------------------------
// Load the 500,000-point fixture file using k6's open() built-in.
// open() reads the file at init time, before the test starts.
// ---------------------------------------------------------------------------

const largePointsGpxData = open('../fixtures/large-500k-points.gpx');

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();
  const totalStart = Date.now();

  // Phase 1: Upload the 500k-point GPX file
  const uploadStart = Date.now();
  const importUrl = `${BASE_URL}/v1/imports`;

  const importRes = http.post(importUrl, largePointsGpxData, {
    headers: {
      ...headers,
      'Content-Type': 'application/gpx+xml',
      'Idempotency-Key': generateIdempotencyKey(),
    },
    timeout: '120s',
  });

  const uploadTime = Date.now() - uploadStart;
  uploadDuration.add(uploadTime);

  const uploadSuccess = check(importRes, {
    'points import upload: status is 202 or 201': (r) =>
      r.status === 202 || r.status === 201,
    'points import upload: response contains import ID': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.id !== undefined;
      } catch (e) {
        return false;
      }
    },
  });

  if (!uploadSuccess) {
    console.error(`Upload failed with status ${importRes.status}: ${importRes.body}`);
    return;
  }

  // Extract the import ID for polling
  let importId;
  try {
    const body = JSON.parse(importRes.body);
    importId = body.id;
  } catch (e) {
    console.error('Failed to parse import response');
    return;
  }

  // Phase 2: Poll for import completion with memory pressure tracking
  const processingStart = Date.now();
  const statusUrl = `${BASE_URL}/v1/imports/${importId}`;
  const timeoutMs = 540000; // 9 minutes
  const pollInterval = 2; // seconds between polls

  let completed = false;
  let lastStatus = '';
  let lastResponseTime = 0;
  let previousResponseTime = 0;

  while (Date.now() - processingStart < timeoutMs) {
    const pollStart = Date.now();
    const res = http.get(statusUrl, { headers });
    const pollTime = Date.now() - pollStart;

    pollCount.add(1);

    if (res.body) {
      pollResponseSize.add(res.body.length);
    }

    // Track timing patterns to detect memory pressure
    // (increasing response times may indicate memory pressure on the worker)
    previousResponseTime = lastResponseTime;
    lastResponseTime = pollTime;

    if (res.status === 200) {
      try {
        const body = JSON.parse(res.body);
        lastStatus = body.status;

        if (body.status === 'completed') {
          completed = true;
          break;
        }

        if (body.status === 'failed') {
          console.error(`Import failed: ${JSON.stringify(body)}`);
          break;
        }
      } catch (e) {
        // Continue polling
      }
    }

    sleep(pollInterval);
  }

  const processingTime = Date.now() - processingStart;
  processingDuration.add(processingTime);

  check(null, {
    'points import: reached completed status': () => completed,
    'points import: did not timeout': () => completed || lastStatus === 'failed',
  });

  // Total import time
  const totalTime = Date.now() - totalStart;
  totalDuration.add(totalTime);

  console.log(
    `500k-point import: upload=${uploadTime}ms, processing=${processingTime}ms, ` +
    `total=${totalTime}ms, final_status=${lastStatus}`
  );
}
