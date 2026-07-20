/**
 * Export Flow Load Test
 *
 * Tests the full export lifecycle: request export, poll for readiness,
 * and download the generated file.
 *
 * Performance target: p95 < 30s for total export generation time
 *
 * Scenario: 10 virtual users concurrently request exports, poll for
 * completion, and download the result. This validates that the export
 * pipeline can handle concurrent requests without degrading response times.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend } from 'k6/metrics';
import {
  BASE_URL,
  authenticatedHeaders,
  generateIdempotencyKey,
  pollForStatus,
  assertResponseTime,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const exportRequestDuration = new Trend('export_request_duration_ms');
const exportGenerationDuration = new Trend('export_generation_duration_ms');
const exportDownloadDuration = new Trend('export_download_duration_ms');
const exportTotalDuration = new Trend('export_total_duration_ms');

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  stages: [
    { duration: '15s', target: 10 }, // Ramp up to 10 VUs over 15 seconds
    { duration: '2m', target: 10 },  // Sustain 10 VUs for 2 minutes
    { duration: '15s', target: 0 },  // Ramp down over 15 seconds
  ],
  thresholds: {
    // Total export flow (request + generation + download) p95 < 30s
    export_total_duration_ms: ['p(95)<30000'],
    // Export request acceptance should be fast
    export_request_duration_ms: ['p(95)<1000'],
    http_req_failed: ['rate<0.05'],
  },
};

// ---------------------------------------------------------------------------
// Pre-configured activity IDs for export testing
// ---------------------------------------------------------------------------

const ACTIVITY_IDS = (
  __ENV.ACTIVITY_IDS || 'activity-1,activity-2,activity-3'
).split(',');

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();
  const totalStart = Date.now();

  // Pick a random activity to export
  const activityId = ACTIVITY_IDS[Math.floor(Math.random() * ACTIVITY_IDS.length)];

  // Phase 1: Request export
  const exportRequestStart = Date.now();
  const exportUrl = `${BASE_URL}/v1/activities/${activityId}/exports`;
  const exportPayload = JSON.stringify({
    format: 'gpx',
  });

  const exportRes = http.post(exportUrl, exportPayload, {
    headers: {
      ...headers,
      'Idempotency-Key': generateIdempotencyKey(),
    },
  });

  const exportRequestTime = Date.now() - exportRequestStart;
  exportRequestDuration.add(exportRequestTime);

  const requestSuccess = check(exportRes, {
    'export request: status is 202 or 201': (r) =>
      r.status === 202 || r.status === 201,
    'export request: response has export ID': (r) => {
      try {
        const body = JSON.parse(r.body);
        return body.id !== undefined;
      } catch (e) {
        return false;
      }
    },
  });

  if (!requestSuccess) {
    console.warn(`Export request failed: status=${exportRes.status}`);
    sleep(2);
    return;
  }

  // Extract export ID
  let exportId;
  try {
    const body = JSON.parse(exportRes.body);
    exportId = body.id;
  } catch (e) {
    console.error('Failed to parse export response');
    sleep(2);
    return;
  }

  // Phase 2: Poll for export completion
  const generationStart = Date.now();
  const statusUrl = `${BASE_URL}/v1/exports/${exportId}`;

  const result = pollForStatus(statusUrl, headers, 'completed', 60000);
  const generationTime = Date.now() - generationStart;
  exportGenerationDuration.add(generationTime);

  const generationSuccess = check(result, {
    'export generation: completed successfully': (r) => r !== null,
    'export generation: status is completed': (r) =>
      r !== null && r.status === 'completed',
  });

  if (!generationSuccess) {
    console.warn(`Export generation timed out or failed for export ${exportId}`);
    sleep(2);
    return;
  }

  // Phase 3: Download the exported file
  const downloadStart = Date.now();
  const downloadUrl = `${BASE_URL}/v1/exports/${exportId}/download`;
  const downloadRes = http.get(downloadUrl, { headers });

  const downloadTime = Date.now() - downloadStart;
  exportDownloadDuration.add(downloadTime);

  check(downloadRes, {
    'export download: status is 200': (r) => r.status === 200,
    'export download: has content-type': (r) => {
      const contentType = r.headers['Content-Type'] || r.headers['content-type'] || '';
      return contentType.length > 0;
    },
    'export download: body is not empty': (r) => r.body && r.body.length > 0,
  });

  // Total export time
  const totalTime = Date.now() - totalStart;
  exportTotalDuration.add(totalTime);

  // Pause between export cycles
  sleep(2);
}
