set dotenv-load

default:
    @just --list

# ── Development ────────────────────────────────────────────

# Start the API server with auto-reload on file changes
dev:
    cargo watch -x 'run --bin order-api'

# Format code
fmt:
    cargo fmt

# ── Database ───────────────────────────────────────────────

# Start Postgres via Docker Compose
db:
    docker compose -f infra/docker-compose.yml up -d

# Stop Postgres
db-stop:
    docker compose -f infra/docker-compose.yml down

# Stop Postgres and remove volumes
down:
    docker compose -f infra/docker-compose.yml down -v

# Run Diesel migrations
migrate:
    diesel migration run

# Revert last migration
migrate-revert:
    diesel migration revert

# Reset database (revert all + re-run)
db-reset:
    diesel database reset

# ── Smoke test ────────────────────────────────────────────

# Create a draft order with a line item, then confirm it
smoke:
    #!/usr/bin/env bash
    set -euo pipefail
    BASE="http://${HOST:-127.0.0.1}:${PORT:-8080}"

    echo "==> Creating draft order..."
    ORDER=$(curl -s -X POST "$BASE/api/orders" \
      -H 'Content-Type: application/json' \
      -d '{"currency": "EUR"}')
    ORDER_ID=$(echo "$ORDER" | jq -r '.id')
    echo "$ORDER" | jq .

    echo ""
    echo "==> Adding line item..."
    ITEM=$(curl -s -X POST "$BASE/api/orders/$ORDER_ID/items" \
      -H 'Content-Type: application/json' \
      -d '{"product_sku": "WIDGET-001", "quantity": 3, "unit_price": "49.9900"}')
    echo "$ITEM" | jq .

    echo ""
    echo "==> Fetching order with items..."
    curl -s "$BASE/api/orders/$ORDER_ID" | jq .

    echo ""
    echo "==> Confirming order..."
    curl -s -X PATCH "$BASE/api/orders/$ORDER_ID/status" \
      -H 'Content-Type: application/json' \
      -d '{"status": "Confirmed"}' | jq .

# ── Generation boundary ───────────────────────────────────

# Export OpenAPI spec + generate TS client (everything below the line is derived)
gen:
    cargo run --bin export-openapi > openapi.json
    @echo "Generated openapi.json"

# ── Quality ────────────────────────────────────────────────

# Fast compile check + clippy (no tests, no codegen)
check:
    cargo clippy -- -D warnings

# Run tests
test:
    cargo test

# Run clippy
lint:
    cargo clippy -- -D warnings

# Pre-commit checks: fmt + clippy + test
pre-commit:
    cargo fmt -- --check
    cargo clippy -- -D warnings
    cargo test
    @echo "All checks passed"
