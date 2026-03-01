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
    docker compose -f infra/docker-compose.yml up -d postgres

# Stop Postgres
db-stop:
    docker compose -f infra/docker-compose.yml down

# Stop Postgres and remove volumes
down:
    docker compose -f infra/docker-compose.yml down -v --remove-orphans

# Run Diesel migrations
migrate:
    diesel migration run

# Revert last migration
migrate-revert:
    diesel migration revert

# Reset database (revert all + re-run)
db-reset:
    diesel database reset

# ── Infrastructure ────────────────────────────────────────

# Start all infrastructure services (Postgres, Kafka, Schema Registry, Debezium, AKHQ)
infra-up:
    docker compose -f infra/docker-compose.yml up -d

# Show logs for a service (e.g. just logs kafka)
logs service="":
    docker compose -f infra/docker-compose.yml logs -f {{ service }}

# ── Debezium ─────────────────────────────────────────────

# Register the Debezium outbox connector (credentials from env / .env)
register-connector:
    #!/usr/bin/env bash
    set -euo pipefail
    jq --arg user "${POSTGRES_USER:-order_api}" \
       --arg pass "${POSTGRES_PASSWORD:-order_api}" \
       --arg db   "${POSTGRES_DB:-order_api}" \
       '.config["database.user"] = $user | .config["database.password"] = $pass | .config["database.dbname"] = $db' \
       infra/debezium/register-connector.json | \
    curl -X POST http://localhost:8083/connectors \
        -H "Content-Type: application/json" \
        -d @-

# Re-register the connector (delete + create)
reload-connector:
    #!/usr/bin/env bash
    set -euo pipefail
    curl -s -X DELETE http://localhost:8083/connectors/order-outbox-connector || true
    sleep 2
    jq --arg user "${POSTGRES_USER:-order_api}" \
       --arg pass "${POSTGRES_PASSWORD:-order_api}" \
       --arg db   "${POSTGRES_DB:-order_api}" \
       '.config["database.user"] = $user | .config["database.password"] = $pass | .config["database.dbname"] = $db' \
       infra/debezium/register-connector.json | \
    curl -X POST http://localhost:8083/connectors \
        -H "Content-Type: application/json" \
        -d @-

# Check the connector status
connector-status:
    curl -s http://localhost:8083/connectors/order-outbox-connector/status | jq .

# ── Schema Registry ─────────────────────────────────────

# Register Avro schemas in Schema Registry with BACKWARD compatibility
register-schemas:
    ./scripts/register-schemas.sh

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

# ── E2E tests ────────────────────────────────────────────

# Run end-to-end tests (starts and stops Docker Compose infrastructure)
test-e2e:
    ./scripts/run_e2e_tests.sh

# Run end-to-end tests and leave infrastructure running afterwards
test-e2e-no-teardown:
    ./scripts/run_e2e_tests.sh --no-teardown

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

# Generate HTML code coverage report (opens in target/llvm-cov/html/index.html)
coverage:
    cargo llvm-cov --html

# Serve the HTML coverage report on http://localhost:8000 (Ctrl-C to stop)
see-coverage:
    @echo "Serving coverage report at http://localhost:8000 — press Ctrl-C to stop"
    python3 -m http.server 8000 --directory target/llvm-cov/html

# Run clippy
lint:
    cargo clippy -- -D warnings

# Shared checks: fmt + clippy + test
_checks:
    cargo fmt -- --check
    cargo clippy -- -D warnings
    cargo test

# Full quality gate: gen + fmt + clippy + test
quality:
    just gen
    just _checks
    @echo "All checks passed"

# Pre-commit checks: gen + diff guard + fmt + clippy + test
pre-commit:
    just gen
    git diff --exit-code openapi.json || (echo "openapi.json is stale — stage it and retry" && exit 1)
    just _checks
    @echo "All checks passed"
