/**
 * Shared utilities for Haiker k6 performance tests.
 *
 * Provides authentication helpers, payload generators, response assertion
 * utilities, and common threshold definitions used across all test scripts.
 */

import { check, sleep } from 'k6';
import http from 'k6/http';
import { uuidv4 } from 'https://jslib.k6.io/k6-utils/1.4.0/index.js';

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/**
 * Base URL for the API under test.
 * Override via the K6_ENV variable API_BASE_URL.
 */
export const BASE_URL = __ENV.API_BASE_URL || 'http://localhost:3000';

// ---------------------------------------------------------------------------
// Authentication
// ---------------------------------------------------------------------------

/**
 * Returns HTTP headers that include a bearer token for authenticated requests.
 * The token is read from the AUTH_TOKEN environment variable.
 */
export function authenticatedHeaders() {
  const token = __ENV.AUTH_TOKEN || '';
  return {
    'Content-Type': 'application/json',
    Authorization: `Bearer ${token}`,
  };
}

// ---------------------------------------------------------------------------
// Idempotency
// ---------------------------------------------------------------------------

/**
 * Generates a unique idempotency key using a v4 UUID.
 */
export function generateIdempotencyKey() {
  return uuidv4();
}

// ---------------------------------------------------------------------------
// GPX Payload Generation
// ---------------------------------------------------------------------------

/**
 * Generates a random coordinate within the Alps region.
 * Latitude: 46.0 - 47.5 (roughly central Alps)
 * Longitude: 6.5 - 12.5 (western to eastern Alps)
 */
export function randomCoordinate() {
  const lat = 46.0 + Math.random() * 1.5;
  const lon = 6.5 + Math.random() * 6.0;
  return { lat: lat.toFixed(6), lon: lon.toFixed(6) };
}

/**
 * Generates a valid GPX XML string with the specified number of trackpoints.
 * Each point is placed sequentially along a path with small increments to
 * simulate a realistic hiking route.
 *
 * @param {number} numPoints - Number of trackpoints to generate
 * @returns {string} Valid GPX XML document
 */
export function generateGpxPayload(numPoints) {
  const startLat = 46.5;
  const startLon = 8.0;
  const latStep = 0.0001;
  const lonStep = 0.00005;

  let gpx = `<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" creator="haiker-k6-tests"
  xmlns="http://www.topografix.com/GPX/1/1">
  <trk>
    <name>k6 Generated Route</name>
    <trkseg>
`;

  for (let i = 0; i < numPoints; i++) {
    const lat = (startLat + i * latStep).toFixed(6);
    const lon = (startLon + i * lonStep).toFixed(6);
    const ele = (1500 + Math.sin(i * 0.01) * 200).toFixed(1);
    gpx += `      <trkpt lat="${lat}" lon="${lon}"><ele>${ele}</ele></trkpt>\n`;
  }

  gpx += `    </trkseg>
  </trk>
</gpx>`;

  return gpx;
}

// ---------------------------------------------------------------------------
// Response Assertions
// ---------------------------------------------------------------------------

/**
 * Asserts that a response completed within the specified time limit.
 *
 * @param {object} response - k6 HTTP response object
 * @param {number} maxMs - Maximum acceptable response time in milliseconds
 * @param {string} metricName - Descriptive name for the check
 * @returns {boolean} True if the assertion passed
 */
export function assertResponseTime(response, maxMs, metricName) {
  const passed = check(response, {
    [`${metricName} response time < ${maxMs}ms`]: (r) =>
      r.timings.duration < maxMs,
  });
  return passed;
}

// ---------------------------------------------------------------------------
// Standard Thresholds
// ---------------------------------------------------------------------------

/**
 * Default performance thresholds aligned with Haiker non-functional requirements.
 *
 * Metadata operations: p95 < 300ms
 * Preview rendering:   p95 < 500ms
 * Mutations:           p95 < 500ms
 * General HTTP:        p95 < 1000ms, p99 < 2000ms
 */
export const standardThresholds = {
  http_req_duration: ['p(95)<1000', 'p(99)<2000'],
  http_req_failed: ['rate<0.01'],
};

export const metadataThresholds = {
  http_req_duration: ['p(95)<300', 'p(99)<500'],
  http_req_failed: ['rate<0.01'],
};

export const previewThresholds = {
  http_req_duration: ['p(95)<500', 'p(99)<1000'],
  http_req_failed: ['rate<0.01'],
};

export const mutationThresholds = {
  http_req_duration: ['p(95)<500', 'p(99)<1000'],
  http_req_failed: ['rate<0.01'],
};

// ---------------------------------------------------------------------------
// Polling Helper
// ---------------------------------------------------------------------------

/**
 * Polls a URL until the response contains a target status value or the
 * timeout is exceeded.
 *
 * @param {string} url - URL to poll
 * @param {object} headers - HTTP headers to include
 * @param {string} targetStatus - Status value to wait for (e.g. "completed")
 * @param {number} timeoutMs - Maximum time to wait in milliseconds
 * @returns {object|null} The final response body parsed as JSON, or null on timeout
 */
export function pollForStatus(url, headers, targetStatus, timeoutMs) {
  const startTime = Date.now();
  const pollInterval = 1; // seconds between polls

  while (Date.now() - startTime < timeoutMs) {
    const res = http.get(url, { headers });

    if (res.status === 200) {
      try {
        const body = JSON.parse(res.body);
        if (body.status === targetStatus) {
          return body;
        }
      } catch (e) {
        // Response not valid JSON yet, continue polling
      }
    }

    sleep(pollInterval);
  }

  return null;
}
