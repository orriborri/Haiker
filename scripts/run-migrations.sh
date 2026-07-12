#!/usr/bin/env bash
set -euo pipefail
echo "Running database migrations..."
# Uses DATABASE_URL from environment
cargo sqlx migrate run --source migrations/
echo "Migrations complete."
