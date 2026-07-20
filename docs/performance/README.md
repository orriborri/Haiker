# Performance Documentation

This directory contains Haiker's performance requirements, browser support policy, and links to test infrastructure.

## Contents

- [Browser Compatibility Matrix](browser-matrix.md) - Supported browsers, minimum versions, and required Web APIs
- [Performance Targets](targets.md) - p95 latency targets, capacity limits, availability requirements, and recovery objectives

## Test Infrastructure

Performance tests are located in [`tests/performance/`](../../tests/performance/). This directory contains load test scripts, benchmark configurations, and tooling for validating that the system meets the targets defined above.

## Results

After test execution, results are stored in `docs/performance/results/`. This directory is created by the test harness and contains timestamped reports from each test run. Results are not checked into version control by default but may be committed for baseline comparisons.
