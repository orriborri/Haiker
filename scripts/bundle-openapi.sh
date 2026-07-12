#!/usr/bin/env bash
set -euo pipefail
mkdir -p dist
npx @redocly/cli@latest bundle openapi/openapi.yaml -o dist/openapi.yaml --config .redocly.yaml
npx @redocly/cli@latest lint openapi/openapi.yaml --config .redocly.yaml
echo "OpenAPI spec bundled and validated successfully"
