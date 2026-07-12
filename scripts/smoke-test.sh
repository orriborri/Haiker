#!/usr/bin/env bash
set -euo pipefail
BASE_URL="${API_URL:-http://localhost:3000}"
MAX_RETRIES=30
RETRY_INTERVAL=2

echo "Waiting for API to be ready..."
for i in $(seq 1 $MAX_RETRIES); do
  if curl -sf "$BASE_URL/health" > /dev/null 2>&1; then
    echo "API is healthy!"
    break
  fi
  if [ "$i" -eq "$MAX_RETRIES" ]; then
    echo "ERROR: API did not become healthy after $((MAX_RETRIES * RETRY_INTERVAL))s"
    exit 1
  fi
  sleep $RETRY_INTERVAL
done

echo "Checking /health..."
HEALTH=$(curl -sf "$BASE_URL/health")
echo "$HEALTH" | grep -q '"status"' || { echo "FAIL: /health missing status"; exit 1; }

echo "Checking /ready..."
READY=$(curl -sf "$BASE_URL/ready")
echo "$READY" | grep -q '"status"' || { echo "FAIL: /ready missing status"; exit 1; }

echo "Checking auth enforcement..."
HTTP_CODE=$(curl -s -o /dev/null -w "%{http_code}" "$BASE_URL/me")
[ "$HTTP_CODE" = "401" ] || { echo "FAIL: /me without auth returned $HTTP_CODE, expected 401"; exit 1; }

echo "All smoke tests passed!"
