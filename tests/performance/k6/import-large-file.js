/**
 * Large File Import Load Test (50 MB)
 *
 * Tests the system's ability to accept and process a 50 MB GPX file import.
 * This validates the performance target that a 50 MB file can be accepted
 * and processed to completion.
 *
 * Scenario: A single virtual user uploads the 50 MB fixture file, then polls
 * the import status until completion. Custom Trend metrics track each phase:
 * upload time, processing time (polling), and total import time.
 *
 * The test has a generous timeout (10 minutes) since processing a large file
 * involves worker-side parsing, validation, and storage.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { Trend } from 'k6/metrics';
import {
  BASE_URL,
  authenticatedHeaders,
  generateIdempotencyKey,
  pollForStatus,
} from './helpers.js';

// ---------------------------------------------------------------------------
// Custom Metrics
// ---------------------------------------------------------------------------

const uploadDuration = new Trend('import_upload_duration_ms');
const processingDuration = new Trend('import_processing_duration_ms');
const totalDuration = new Trend('import_total_duration_ms');

// ---------------------------------------------------------------------------
// Options
// ---------------------------------------------------------------------------

export const options = {
  // Single VU for large file import - we want to measure the system's
  // capacity for one large upload at a time
  stages: [
    { duration: '10m', target: 1 },
  ],
  thresholds: {
    // The import should complete within 10 minutes
    import_total_duration_ms: ['p(95)<600000'],
    // Upload phase should be fast (network transfer)
    import_upload_duration_ms: ['p(95)<60000'],
  },
  // Only run one iteration (the test logic handles timing internally)
  iterations: 1,
};

// ---------------------------------------------------------------------------
// Load the 50 MB fixture file using k6's open() built-in.
// open() reads the file at init time, before the test starts.
// ---------------------------------------------------------------------------

const largeGpxData = open('../fixtures/large-50mb.gpx');

// ---------------------------------------------------------------------------
// Test Logic
// ---------------------------------------------------------------------------

export default function () {
  const headers = authenticatedHeaders();
  const totalStart = Date.now();

  // Phase 1: Upload the large GPX file
  const uploadStart = Date.now();
  const importUrl = `${BASE_URL}/v1/imports`;

  const importRes = http.post(importUrl, largeGpxData, {
    headers: {
      ...headers,
      'Content-Type': 'application/gpx+xml',
      'Idempotency-Key': generateIdempotencyKey(),
    },
    timeout: '120s', // Allow up to 2 minutes for the upload itself
  });

  const uploadTime = Date.now() - uploadStart;
  uploadDuration.add(uploadTime);

  const uploadSuccess = check(importRes, {
    'import upload: status is 202 or 201': (r) =>
      r.status === 202 || r.status === 201,
    'import upload: response contains import ID': (r) => {
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

  // Phase 2: Poll for import completion
  const processingStart = Date.now();
  const statusUrl = `${BASE_URL}/v1/imports/${importId}`;

  // Poll for up to 9 minutes (leaving margin within 10 min total timeout)
  const result = pollForStatus(statusUrl, headers, 'completed', 540000);
  const processingTime = Date.now() - processingStart;
  processingDuration.add(processingTime);

  check(result, {
    'import processing: reached completed status': (r) => r !== null,
    'import processing: status is completed': (r) =>
      r !== null && r.status === 'completed',
  });

  // Total import time
  const totalTime = Date.now() - totalStart;
  totalDuration.add(totalTime);

  console.log(`Import completed: upload=${uploadTime}ms, processing=${processingTime}ms, total=${totalTime}ms`);
}
