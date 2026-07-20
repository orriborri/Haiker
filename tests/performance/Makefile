# Haiker Performance Test Suite
#
# Usage:
#   make -f tests/performance/Makefile generate-fixtures
#   make -f tests/performance/Makefile run-all
#   make -f tests/performance/Makefile run-metadata K6_ARGS="--vus 50 --duration 60s"
#
# Environment variables:
#   API_BASE_URL  - Target API URL (default: http://localhost:3000)
#   AUTH_TOKEN    - Authentication token for API requests
#   K6_ARGS       - Extra arguments passed to k6 (e.g. --vus 10 --duration 30s)

PERF_DIR := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
K6_DIR := $(PERF_DIR)k6
RESULTS_DIR := $(PERF_DIR)results
FIXTURES_DIR := $(PERF_DIR)fixtures

K6 := k6
K6_ARGS ?=
K6_ENV := -e API_BASE_URL=$(or $(API_BASE_URL),http://localhost:3000) -e AUTH_TOKEN=$(or $(AUTH_TOKEN),)

K6_RUN = $(K6) run --out json=$(RESULTS_DIR)/$1.json $(K6_ENV) $(K6_ARGS) $(K6_DIR)/$1.js

.PHONY: generate-fixtures run-all run-metadata run-preview run-mutations \
        run-import run-export run-queue-pressure run-editor run-autosave \
        run-map-failure run-worker-outage clean-results

# ---------------------------------------------------------------------------
# Fixture Generation
# ---------------------------------------------------------------------------

generate-fixtures:
	@echo "Generating performance test fixtures..."
	bash $(FIXTURES_DIR)/generate-fixtures.sh

# ---------------------------------------------------------------------------
# Test Execution
# ---------------------------------------------------------------------------

run-all: run-metadata run-preview run-mutations run-import run-export \
         run-queue-pressure run-editor run-autosave run-map-failure run-worker-outage

run-metadata:
	$(call K6_RUN,metadata-operations)

run-preview:
	$(call K6_RUN,preview-rendering)

run-mutations:
	$(call K6_RUN,mutations)

run-import:
	$(call K6_RUN,import-large-file)
	$(call K6_RUN,import-large-points)

run-export:
	$(call K6_RUN,export)

run-queue-pressure:
	$(call K6_RUN,queue-pressure)

run-editor:
	$(call K6_RUN,editor-interaction)

run-autosave:
	$(call K6_RUN,autosave)

run-map-failure:
	$(call K6_RUN,map-failure-recovery)

run-worker-outage:
	$(call K6_RUN,worker-outage)

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------

clean-results:
	@echo "Cleaning results directory..."
	rm -f $(RESULTS_DIR)/*.json
	@echo "Done."
