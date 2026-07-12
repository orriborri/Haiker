# Development commands for Haiker

# Start development services (PostgreSQL + MinIO)
dev-up:
    docker compose up -d
    @echo "Services started. PostgreSQL on :5432, MinIO on :9000 (console :9001)"

# Stop development services
dev-down:
    docker compose down

# Reset development services (stop, remove volumes, restart)
dev-reset:
    docker compose down -v
    docker compose up -d
    @echo "Services reset and restarted."
