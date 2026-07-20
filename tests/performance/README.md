# Haiker Performance Test Suite

Load testing infrastructure for the Haiker API using [k6](https://k6.io/).

## Prerequisites

- **k6** installed locally (`brew install k6` / `apt install k6`) or use Docker
- **Docker** and **Docker Compose** for the full test environment
- **bash**, **awk**, and **bc** for fixture generation

## Directory Structure

```
tests/performance/
  docker-compose.perf.yml   # Docker Compose for the perf test environment
  Makefile                  # Test execution targets
  k6/                       # k6 test scripts
    helpers.js              # Shared utilities and constants
    metadata-operations.js  # Metadata endpoint tests
    preview-rendering.js    # Preview/geometry rendering tests
    mutations.js            # Route editing mutation tests
    import-large-file.js    # Large file import tests
    import-large-points.js  # Large point count import tests
    export.js               # Export flow tests
    queue-pressure.js       # Queue pressure/backpressure tests
    editor-interaction.js   # Editor real-time interaction tests
    autosave.js             # Autosave flow tests
    map-failure-recovery.js # Map failure recovery tests
    worker-outage.js        # Worker outage resilience tests
  fixtures/                 # Generated GPX test fixtures
    generate-fixtures.sh    # Fixture generation script
  results/                  # Test output (JSON reports)
```

## Quick Start

### 1. Generate Test Fixtures

Generate the large GPX files needed for import/export tests:

```bash
make -f tests/performance/Makefile generate-fixtures
```

This creates:
- `large-50mb.gpx` - A ~50 MB GPX file for size-based import testing
- `large-500k-points.gpx` - A 500,000-point GPX file for point-count testing
- `representative-route.gpx` - A 10,000-point multi-segment route

### 2. Start the Test Environment

Start the performance test infrastructure (InfluxDB + Grafana for results):

```bash
docker compose -f tests/performance/docker-compose.perf.yml up -d influxdb grafana
```

Start the Haiker API (in another terminal or as a background service):

```bash
cargo run --release --bin haiker-api
```

### 3. Run Tests

Run all test scenarios:

```bash
make -f tests/performance/Makefile run-all
```

Run a specific scenario:

```bash
make -f tests/performance/Makefile run-metadata
make -f tests/performance/Makefile run-preview
make -f tests/performance/Makefile run-import
```

### 4. View Results in Grafana

Open [http://localhost:3030](http://localhost:3030) in your browser.

Configure an InfluxDB data source:
- URL: `http://influxdb:8086`
- Database: `k6`

Import k6 dashboards or create custom ones to visualize test results.

## Running Tests with Docker

If you prefer not to install k6 locally, run tests via Docker:

```bash
docker compose -f tests/performance/docker-compose.perf.yml run --rm k6 \
  run /scripts/metadata-operations.js
```

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `API_BASE_URL` | Target API base URL | `http://localhost:3000` |
| `AUTH_TOKEN` | Bearer token for authenticated requests | (empty) |
| `K6_ARGS` | Additional k6 CLI arguments | (empty) |

### Examples

```bash
# Custom VU count and duration
make -f tests/performance/Makefile run-metadata K6_ARGS="--vus 50 --duration 60s"

# Target a different API host
API_BASE_URL=http://staging.example.com:3000 make -f tests/performance/Makefile run-all

# With authentication
AUTH_TOKEN=eyJ... make -f tests/performance/Makefile run-metadata
```

## Adding New Test Scenarios

1. Create a new `.js` file in `tests/performance/k6/`
2. Import helpers from `./helpers.js`:

```javascript
import { BASE_URL, authenticatedHeaders, standardThresholds } from './helpers.js';
```

3. Define your test options with thresholds:

```javascript
export const options = {
  stages: [
    { duration: '30s', target: 10 },
    { duration: '1m', target: 10 },
    { duration: '10s', target: 0 },
  ],
  thresholds: standardThresholds,
};
```

4. Implement the default export function:

```javascript
export default function () {
  const headers = authenticatedHeaders();
  // ... your test logic
}
```

5. Add a target in the `Makefile` following the existing pattern
6. Update this README

## Performance Targets

From the Haiker requirements (Section 23):

| Operation | p95 Target | p99 Target |
|-----------|-----------|-----------|
| Metadata operations | < 300ms | < 500ms |
| Preview rendering | < 500ms | < 1000ms |
| Route mutations | < 500ms | < 1000ms |
| Editor feedback | < 16ms | - |
| Autosave | < 1s | - |
| General HTTP | < 1000ms | < 2000ms |

## Cleaning Up

Remove test results:

```bash
make -f tests/performance/Makefile clean-results
```

Stop the test environment:

```bash
docker compose -f tests/performance/docker-compose.perf.yml down -v
```
