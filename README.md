# Order API

A Rust REST API for managing orders, built on the **Model-First Development** pipeline where
code — not specs — is the authoritative source of truth.

**Stack:** Rust · Actix-web · Diesel (Postgres) · utoipa (OpenAPI) · Debezium · Kafka · just

---

## Contents

- [Tutorial: Run your first order through the API](#tutorial-run-your-first-order-through-the-api)
- [How-To Guides](#how-to-guides)
  - [How to add a new field to an order](#how-to-add-a-new-field-to-an-order)
  - [How to run the full CDC pipeline locally](#how-to-run-the-full-cdc-pipeline-locally)
  - [How to run integration tests](#how-to-run-integration-tests)
  - [How to run end-to-end CDC tests](#how-to-run-end-to-end-cdc-tests)
  - [How to reset the database](#how-to-reset-the-database)
  - [How to generate the OpenAPI spec](#how-to-generate-the-openapi-spec)
- [Explanation](#explanation)
  - [The Model-First pipeline](#the-model-first-pipeline)
  - [The order state machine](#the-order-state-machine)
  - [The transactional outbox pattern](#the-transactional-outbox-pattern)
  - [Decimal precision end-to-end](#decimal-precision-end-to-end)
  - [Why synchronous Diesel inside web::block](#why-synchronous-diesel-inside-webblock)
- [Reference](#reference)
  - [API endpoints](#api-endpoints)
  - [Domain types](#domain-types)
  - [Database schema](#database-schema)
  - [Environment variables](#environment-variables)
  - [Just commands](#just-commands)
  - [Project structure](#project-structure)
  - [Error responses](#error-responses)
  - [Pipeline violation rules](#pipeline-violation-rules)

---

## Tutorial: Run your first order through the API

This tutorial walks you through setting up the project from scratch and creating, modifying, and
confirming an order. By the end you will have a running API and a confirmed order with a line item.

### Prerequisites

Install the following before starting:

- [Rust](https://rustup.rs/) stable toolchain
- [just](https://github.com/casey/just) — `cargo install just`
- [Diesel CLI](https://diesel.rs/guides/getting-started) — `cargo install diesel_cli --no-default-features --features postgres`
- [cargo-watch](https://github.com/watchexec/cargo-watch) — `cargo install cargo-watch`
- Docker (for Postgres)

### Step 1: Clone the repository and create your environment file

```bash
git clone <repo-url>
cd model-first-order
cp .env.example .env
```

The `.env` file contains `DATABASE_URL` and other settings. The defaults work out of the box
for local development.

### Step 2: Start Postgres

```bash
just db
```

This starts a Postgres 16 container using Docker Compose. The container is pre-configured with
logical replication enabled (`wal_level=logical`), which is required for the CDC pipeline.

Wait a few seconds for Postgres to finish initializing.

### Step 3: Run the database migrations

```bash
just migrate
```

This runs Diesel migrations, creating the `orders`, `order_line_items`, and
`commerce_order_outbox` tables along with the `order_status` enum type.

### Step 4: Start the API server

```bash
just dev
```

The server starts at `http://localhost:8080` and reloads automatically on file changes.
Swagger UI is available at `http://localhost:8080/swagger-ui/`.

### Step 5: Create a draft order

```bash
curl -s -X POST http://localhost:8080/api/orders \
  -H 'Content-Type: application/json' \
  -d '{"currency": "EUR"}' | jq .
```

You will see a response like:

```json
{
  "id": "a1b2c3d4-...",
  "status": "Draft",
  "currency": "EUR",
  "total_amount": "0.0000",
  "confirmed_at": null,
  "created_at": "2026-03-01T12:00:00Z",
  "updated_at": "2026-03-01T12:00:00Z"
}
```

Save the `id` value — you will use it in the next steps.

### Step 6: Add a line item

Replace `ORDER_ID` with the `id` from the previous step.

```bash
curl -s -X POST http://localhost:8080/api/orders/ORDER_ID/items \
  -H 'Content-Type: application/json' \
  -d '{"product_sku": "WIDGET-001", "quantity": 3, "unit_price": "49.9900"}' | jq .
```

The response includes the new line item with a computed `line_total` (`"149.9700"`).

### Step 7: Confirm the order

```bash
curl -s -X PATCH http://localhost:8080/api/orders/ORDER_ID/status \
  -H 'Content-Type: application/json' \
  -d '{"status": "Confirmed"}' | jq .
```

The response shows the order with `"status": "Confirmed"` and a non-null `confirmed_at`
timestamp. You cannot add or remove line items from the order after this point.

### Step 8: Verify the full order

```bash
curl -s http://localhost:8080/api/orders/ORDER_ID | jq .
```

The response includes the order and its line items together as an `OrderWithItems` aggregate.

You have completed the tutorial. See the [Explanation](#explanation) section for the reasoning
behind the design, and the [Reference](#reference) section for complete API details.

---

## How-To Guides

### How to add a new field to an order

Follow the pipeline in strict order. Adding a field in the wrong order will result in a
compile error.

**1. Write the migration**

```bash
diesel migration generate add_notes_to_orders
```

Edit the generated `up.sql`:

```sql
ALTER TABLE orders ADD COLUMN notes TEXT;
```

Edit `down.sql`:

```sql
ALTER TABLE orders DROP COLUMN notes;
```

Run the migration:

```bash
just migrate
```

Diesel regenerates `src/schema.rs` automatically. Do not edit `schema.rs` by hand.

**2. Add the field to the Rust model**

In `src/models/order.rs`, add the field to `Order`:

```rust
pub notes: Option<String>,
```

If the field should be settable on creation, add it to `NewOrder` as well. If it is only
settable via a separate endpoint, create a new changeset struct.

**3. Update the handler**

In `src/handlers/orders.rs`, update any handler that needs to read or write the new field.
Add utoipa schema annotations if you want the field documented in the OpenAPI spec.

**4. Regenerate the OpenAPI spec**

```bash
just gen
```

This overwrites `openapi.json`. Stage the updated file before committing.

**5. Run the quality gate**

```bash
just quality
```

This runs format check, Clippy, and all tests. Fix any failures before proceeding.

---

### How to run the full CDC pipeline locally

This starts the complete infrastructure: Postgres, Kafka (KRaft mode), Confluent Schema
Registry, Debezium Connect, and AKHQ (Kafka UI).

**1. Start all services**

```bash
just infra-up
```

Wait for all containers to become healthy. You can monitor progress with:

```bash
just logs          # stream all logs
just logs debezium # stream only Debezium logs
```

**2. Run migrations and start the API**

```bash
just migrate
just dev
```

**3. Register the Debezium connector**

```bash
just register-connector
```

This reads credentials from your `.env` file and posts the connector configuration to
Debezium Connect at `http://localhost:8083`. The connector monitors the
`commerce_order_outbox` table and routes events to the Kafka topic
`public.commerce.order.c2.v1`.

**4. Verify the connector is running**

```bash
just connector-status
```

Look for `"state": "RUNNING"` in both the `connector` and `tasks` sections.

**5. Create an order and watch the Kafka topic**

Open AKHQ at `http://localhost:8090` and navigate to the topic `public.commerce.order.c2.v1`.
Then create an order:

```bash
just smoke
```

Refresh the topic view in AKHQ. You should see Avro-encoded messages for `ORDER_CREATED`,
`ORDER_UPDATED`, and `ORDER_CONFIRMED`.

---

### How to run integration tests

Integration tests spin up a disposable Postgres container via testcontainers. Docker must be
running.

```bash
just test
```

To run a single named test:

```bash
cargo test test_order_lifecycle
```

The tests use Actix's `test::init_service` with the real router. There is no mocking — every
test exercises the full handler-to-database path.

---

### How to run end-to-end CDC tests

End-to-end tests require the full Docker Compose infrastructure. The simplest approach is:

```bash
just test-e2e
```

This script:

1. Starts Postgres, Kafka, Schema Registry, and Debezium Connect
2. Waits for each service to become healthy
3. Runs `cargo test --test e2e_test -- --include-ignored`
4. Tears down the stack on exit

To keep the stack running after the test (useful for debugging):

```bash
just test-e2e-no-teardown
```

---

### How to reset the database

To revert all migrations and re-run them from scratch:

```bash
just db-reset
```

This calls `diesel database reset`, which drops and recreates the database, then runs all
migrations. All data is lost.

To revert only the most recent migration:

```bash
just migrate-revert
```

---

### How to generate the OpenAPI spec

```bash
just gen
```

This compiles and runs `src/bin/export_openapi.rs`, which calls `ApiDoc::openapi()` and writes
the result to `openapi.json`. Run this command after any change to a model, handler, or utoipa
annotation.

---

## Explanation

### The Model-First pipeline

The central design principle of this project is that **code, not documents, is the source of
truth**. All authoritative decisions are made in the following fixed order:

```
SQL Migration -> Rust Model -> Handler + utoipa -> Route Registration
                                                       |
                                              GENERATION BOUNDARY
                                                       |
                                               openapi.json -> TS client
```

Each step can only be written after the previous step exists, because each step depends on the
output of the one before it. This is not a convention — it is enforced by the compiler.

- **SQL migrations** define the schema. Postgres column types, constraints, and generated
  columns are decided here. These decisions propagate to every layer above.

- **Rust models** translate the SQL schema into Rust types. The `Order` struct in
  `src/models/order.rs` is the canonical definition of what an order contains. When the struct
  changes, the compiler flags every handler, test, and serialization site that needs updating.

- **Handlers** (`src/handlers/orders.rs`) implement business logic and annotate endpoints
  with `#[utoipa::path(...)]`. These annotations are the only place where API documentation is
  written — and they are checked by the compiler against the actual function signatures.

- **Route registration** (`src/routes.rs`) wires handlers to HTTP paths.

- **`just gen`** crosses the generation boundary. It compiles the binary in
  `src/bin/export_openapi.rs` and writes `openapi.json`. Everything below this line (the spec
  file, any generated TypeScript client) is derived and should never be edited by hand.

The benefit of this direction is that drift is impossible: you cannot have a documented API that
does not match the running code, because the documentation is generated from the running code.

---

### The order state machine

An order moves through a defined set of statuses. The allowed transitions are:

```
Draft --> Confirmed --> Shipped --> Delivered
  |            |
  +------------+--> Cancelled
```

The transitions are enforced in `src/models/order_status.rs`:

```rust
pub fn can_transition_to(&self, target: OrderStatus) -> bool {
    use OrderStatus::*;
    matches!(
        (self, target),
        (Draft, Confirmed)
            | (Draft, Cancelled)
            | (Confirmed, Shipped)
            | (Confirmed, Cancelled)
            | (Shipped, Delivered)
    )
}
```

A request to make any other transition — including backwards transitions like `Confirmed` to
`Draft` — returns `409 Conflict`.

Several behaviors are tied to specific transitions:

- **Draft to Confirmed**: the `confirmed_at` timestamp is set at the moment of confirmation.
  The handler also verifies that `total_amount` equals the sum of line totals before
  allowing the transition.

- **Draft status**: only orders in `Draft` status accept new line items or allow existing
  items to be removed. Any mutation attempt on a non-Draft order returns `409 Conflict`.

- **Cancellation**: valid only from `Draft` or `Confirmed`. A `Shipped` or `Delivered` order
  cannot be cancelled via the API.

Every status transition is executed inside a database transaction that also writes an outbox
event. The event type is derived from the target status via `OrderStatus::as_event_type()`.

---

### The transactional outbox pattern

Every mutating operation — creating an order, adding or removing a line item, transitioning
status — writes a row to the `commerce_order_outbox` table inside the same database
transaction as the business operation.

This solves the dual-write problem: there is no window where the business data has been
committed but the event has not (or vice versa). Either both succeed or both fail.

The outbox row contains:

| Field | Description |
|---|---|
| `event_id` | UUID, primary key of the outbox row |
| `aggregate_type` | Always `"order"` in this service |
| `aggregate_id` | The order's UUID |
| `event_type` | One of `ORDER_CREATED`, `ORDER_UPDATED`, `ORDER_CONFIRMED`, `ORDER_SHIPPED`, `ORDER_DELIVERED`, `ORDER_CANCELLED` |
| `event_date` | Timestamp of insertion |
| `event_data` | Full `OrderWithItems` aggregate serialized as JSONB |
| `sequence_number` | Auto-incrementing `BIGSERIAL`, used to establish insertion order |

The `insert_outbox_event()` function in `src/models/outbox.rs` loads the current
`OrderWithItems` aggregate, serializes it, and inserts the row. It must be called inside the
caller's transaction.

**Debezium** monitors the `commerce_order_outbox` table using PostgreSQL logical replication
(`wal_level=logical`). The Debezium Outbox Event Router SMT maps each row to a Kafka message:

- The Kafka key is `aggregate_id`
- The Kafka topic is `public.commerce.order.c2.v1`
- The message value is an Avro record containing envelope fields
  (`event_id`, `event_type`, `event_date`, `sequence_number`) and the `event_data` payload
- Schemas are registered with Confluent Schema Registry

This means downstream consumers receive strongly typed, schema-versioned events with full order
state at the time of each mutation.

---

### Decimal precision end-to-end

Monetary amounts in this API are stored as `NUMERIC(19,4)` in Postgres. This type preserves
exact decimal arithmetic — there are no floating-point rounding errors.

The precision must be preserved across four layers:

| Layer | Type | Representation |
|---|---|---|
| Postgres | `NUMERIC(19,4)` | Exact decimal |
| Rust | `BigDecimal` (bigdecimal crate) | Exact decimal |
| JSON | `String` | `"149.9700"` |
| TypeScript (generated) | `string` | `"149.9700"` |

If `BigDecimal` were serialized as a JSON number, JavaScript would parse it as an IEEE 754
double, silently losing precision for values like `"12345.6789"`. Serializing as a string
avoids this entirely.

The serializers in `src/serializers.rs` handle the `BigDecimal` to `String` conversion:

- `serialize_bigdecimal_as_string` is used on all `BigDecimal` fields in Queryable structs
- `deserialize_bigdecimal_from_string` is used on all `BigDecimal` fields in Insertable and
  request structs

All `unit_price` amounts must be sent as JSON strings: `"49.9900"`, not `49.99`.

---

### Why synchronous Diesel inside web::block

Diesel's query API is synchronous — it does not support async/await. Actix-web is an async
runtime. Blocking a Tokio thread with synchronous I/O stalls the thread pool and degrades
throughput under load.

The solution used throughout this codebase is `web::block(|| { ... })`, which moves the
synchronous work to a dedicated blocking thread pool managed by Actix. The async handler
awaits the result without blocking the async executor.

```rust
let order = web::block(move || {
    let mut conn = pool.get()?;
    conn.transaction::<_, ApiError, _>(|conn| {
        // All synchronous Diesel work happens here
        Ok(...)
    })
})
.await??;
```

The `??` unwraps both the `Result` from `web::block` (which can return a `BlockingError`) and
the inner `Result<_, ApiError>`.

---

## Reference

### API endpoints

Base URL: `http://localhost:8080`

Interactive documentation (Swagger UI): `http://localhost:8080/swagger-ui/`

| Method | Path | Description | Success |
|---|---|---|---|
| `GET` | `/health` | Health check | `200` |
| `POST` | `/api/orders` | Create a draft order | `201` |
| `GET` | `/api/orders` | List orders (paginated) | `200` |
| `GET` | `/api/orders/{id}` | Get order with line items | `200` |
| `PATCH` | `/api/orders/{id}/status` | Transition order status | `200` |
| `POST` | `/api/orders/{id}/items` | Add a line item | `201` |
| `DELETE` | `/api/orders/{order_id}/items/{item_id}` | Remove a line item | `204` |

#### POST /api/orders

Creates a new order in `Draft` status. Also writes an `ORDER_CREATED` outbox event.

Request body:

```json
{ "currency": "EUR" }
```

`currency` must be a 3-character ISO 4217 code in uppercase ASCII (e.g., `"EUR"`, `"USD"`).
Invalid values return `400 Bad Request`.

Response body: `Order`

#### GET /api/orders

Returns orders sorted by `created_at` descending.

Query parameters:

| Parameter | Type | Default | Constraints |
|---|---|---|---|
| `limit` | integer | `50` | Clamped to `[1, 100]` |
| `offset` | integer | `0` | Negative values clamped to `0` |

Response body: `Order[]`

#### GET /api/orders/{id}

Returns the order and all its line items. Returns `404` if the order does not exist.

Response body: `OrderWithItems`

#### PATCH /api/orders/{id}/status

Transitions the order to a new status. The transition must be valid according to the state
machine. See [The order state machine](#the-order-state-machine).

When transitioning to `Confirmed`, the handler verifies that `total_amount` equals the sum of
all line totals. A mismatch returns `409 Conflict`.

Request body:

```json
{ "status": "Confirmed" }
```

Valid status values: `"Draft"`, `"Confirmed"`, `"Shipped"`, `"Delivered"`, `"Cancelled"`.

Response body: `Order`

Error codes: `404` (not found), `409` (invalid transition or total mismatch)

#### POST /api/orders/{id}/items

Adds a line item to the order. The order must be in `Draft` status. Also recomputes
`total_amount` and writes an `ORDER_UPDATED` outbox event.

Request body:

```json
{
  "product_sku": "WIDGET-001",
  "quantity": 3,
  "unit_price": "49.9900"
}
```

`unit_price` is a string representing a decimal number. `quantity` must be greater than zero.
`unit_price` must be non-negative.

Response body: `OrderLineItem`

Error codes: `400` (invalid input), `404` (order not found), `409` (order not in Draft)

#### DELETE /api/orders/{order_id}/items/{item_id}

Removes a line item from the order. The order must be in `Draft` status. Also recomputes
`total_amount` and writes an `ORDER_UPDATED` outbox event. Removing the last item resets
`total_amount` to `0.0000`.

Response: `204 No Content`

Error codes: `404` (order or item not found), `409` (order not in Draft)

---

### Domain types

#### Order

```
{
  id:           UUID (string)
  status:       "Draft" | "Confirmed" | "Shipped" | "Delivered" | "Cancelled"
  currency:     string  (3-char ISO 4217, e.g. "EUR")
  total_amount: string  (NUMERIC(19,4) serialized as string, e.g. "1299.9900")
  confirmed_at: string | null  (RFC 3339 timestamp, set on Draft->Confirmed)
  created_at:   string  (RFC 3339 timestamp)
  updated_at:   string  (RFC 3339 timestamp)
}
```

#### OrderLineItem

```
{
  id:          UUID (string)
  order_id:    UUID (string)
  product_sku: string
  quantity:    integer (> 0)
  unit_price:  string  (NUMERIC(19,4) as string)
  line_total:  string  (NUMERIC(19,4) as string, DB-generated: quantity x unit_price)
  created_at:  string  (RFC 3339 timestamp)
}
```

#### OrderWithItems

`OrderWithItems` is `Order` with an additional `items` array field containing all
`OrderLineItem` records belonging to that order. The order fields are flattened at the top level.

```
{
  id, status, currency, total_amount, confirmed_at, created_at, updated_at,
  items: OrderLineItem[]
}
```

#### OutboxEvent (internal)

The `commerce_order_outbox` table. Not exposed via the API.

```
{
  event_id:        UUID
  aggregate_type:  string  (always "order")
  aggregate_id:    UUID    (the order's id)
  event_type:      string  (ORDER_CREATED | ORDER_UPDATED | ORDER_CONFIRMED |
                            ORDER_SHIPPED | ORDER_DELIVERED | ORDER_CANCELLED)
  event_date:      timestamp
  event_data:      JSONB   (full OrderWithItems aggregate at time of event)
  sequence_number: bigint  (monotonically increasing, insertion order)
}
```

---

### Database schema

#### orders

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID` | Primary key, generated by `gen_random_uuid()` |
| `status` | `order_status` enum | Default `'draft'` |
| `currency` | `VARCHAR(3)` | ISO 4217 code |
| `total_amount` | `NUMERIC(19,4)` | Default `0`, recomputed on item add/delete |
| `confirmed_at` | `TIMESTAMPTZ` | Nullable; set on Draft to Confirmed transition |
| `created_at` | `TIMESTAMPTZ` | Set on insert |
| `updated_at` | `TIMESTAMPTZ` | Auto-updated by Diesel trigger |

Index: `idx_orders_status` on `status`.

#### order_line_items

| Column | Type | Notes |
|---|---|---|
| `id` | `UUID` | Primary key |
| `order_id` | `UUID` | Foreign key to `orders.id` ON DELETE CASCADE |
| `product_sku` | `VARCHAR(64)` | |
| `quantity` | `INTEGER` | CHECK `> 0` |
| `unit_price` | `NUMERIC(19,4)` | CHECK `>= 0` |
| `line_total` | `NUMERIC(19,4)` | DB-generated: `quantity * unit_price` STORED |
| `created_at` | `TIMESTAMPTZ` | Set on insert |

Index: `idx_line_items_order_id` on `order_id`.

#### commerce_order_outbox

| Column | Type | Notes |
|---|---|---|
| `event_id` | `UUID` | Primary key, generated |
| `aggregate_type` | `VARCHAR(100)` | |
| `aggregate_id` | `UUID` | The order's UUID |
| `event_type` | `VARCHAR(100)` | |
| `event_date` | `TIMESTAMPTZ` | Default `now()` |
| `event_data` | `JSONB` | Full OrderWithItems aggregate |
| `sequence_number` | `BIGSERIAL` | Monotonically increasing |

Indexes on `aggregate_id`, `event_date`, and `sequence_number`.

---

### Environment variables

| Variable | Required | Default | Description |
|---|---|---|---|
| `DATABASE_URL` | Yes | — | Postgres connection URL, e.g. `postgres://order_api:order_api@localhost:5432/order_api` |
| `HOST` | No | `127.0.0.1` | Address the HTTP server binds to |
| `PORT` | No | `8080` | Port the HTTP server listens on |
| `CORS_PERMISSIVE` | No | `false` | Set to `true` or `1` to enable permissive CORS (for local frontend development) |
| `POSTGRES_USER` | No | `order_api` | Used by Docker Compose and `just register-connector` |
| `POSTGRES_PASSWORD` | No | `order_api` | Used by Docker Compose and `just register-connector` |
| `POSTGRES_DB` | No | `order_api` | Used by Docker Compose and `just register-connector` |
| `RUST_LOG` | No | — | Tracing filter, e.g. `order_api=debug,actix_web=info` |

---

### Just commands

#### Development

| Command | Description |
|---|---|
| `just dev` | Start the API server with auto-reload on file changes (`cargo-watch`) |
| `just fmt` | Format all Rust source files |
| `just smoke` | Create a draft order, add a line item, and confirm it against the running server |

#### Database

| Command | Description |
|---|---|
| `just db` | Start Postgres via Docker Compose |
| `just db-stop` | Stop all Docker Compose services |
| `just down` | Stop all services and remove volumes |
| `just migrate` | Run pending Diesel migrations |
| `just migrate-revert` | Revert the most recent migration |
| `just db-reset` | Revert all migrations and re-run them (destroys all data) |

#### Infrastructure

| Command | Description |
|---|---|
| `just infra-up` | Start all services: Postgres, Kafka, Schema Registry, Debezium, AKHQ |
| `just logs [service]` | Stream Docker Compose logs; optionally filter to one service |
| `just register-connector` | Register the Debezium outbox connector |
| `just reload-connector` | Delete and re-register the Debezium connector |
| `just connector-status` | Print the current connector and task status |

#### Quality and testing

| Command | Description |
|---|---|
| `just test` | Run all unit and integration tests |
| `just test-e2e` | Start infrastructure, run E2E CDC tests, tear down |
| `just test-e2e-no-teardown` | Same, but leave infrastructure running |
| `just lint` | Run Clippy with `-D warnings` |
| `just check` | Run Clippy without tests |
| `just coverage` | Generate HTML coverage report (`cargo-llvm-cov`) |
| `just see-coverage` | Serve the coverage report at `http://localhost:8000` |
| `just quality` | Full gate: gen + fmt check + Clippy + test |
| `just pre-commit` | Same as quality, plus checks that `openapi.json` is staged |

#### Generation

| Command | Description |
|---|---|
| `just gen` | Compile and run `export-openapi`, writing `openapi.json` |

---

### Project structure

```
├── migrations/                          # Step 1: SQL is the source of truth
│   ├── 20250201000001_create_orders_and_line_items/
│   │   ├── up.sql                       # orders, order_line_items, order_status enum
│   │   └── down.sql
│   ├── 20260301000001_create_commerce_order_outbox/
│   │   ├── up.sql                       # outbox table for CDC
│   │   └── down.sql
│   └── 20260301000002_add_outbox_sequence_number/
│       ├── up.sql                       # BIGSERIAL sequence_number column
│       └── down.sql
├── src/
│   ├── schema.rs                        # Auto-generated by Diesel CLI — never hand-edit
│   ├── models/                          # Step 2: Rust structs = the authoritative spec
│   │   ├── order_status.rs              # OrderStatus enum + can_transition_to + event constants
│   │   ├── order.rs                     # Order, NewOrder, OrderStatusUpdate, OrderWithItems
│   │   ├── order_line_item.rs           # OrderLineItem, NewLineItem
│   │   └── outbox.rs                    # OutboxEvent, NewOutboxEvent, insert_outbox_event()
│   ├── handlers/                        # Step 3: Business logic + utoipa annotations
│   │   ├── orders.rs                    # All order and line-item endpoints
│   │   └── health.rs                    # GET /health
│   ├── routes.rs                        # Step 4: Route registration
│   ├── openapi.rs                       # ApiDoc struct with utoipa macro
│   ├── serializers.rs                   # BigDecimal <-> String serde helpers
│   ├── db.rs                            # Connection pool init + embedded migrations
│   ├── errors.rs                        # ApiError enum -> HTTP responses
│   ├── lib.rs
│   ├── main.rs                          # Server startup, CORS, tracing, Swagger UI
│   └── bin/
│       └── export_openapi.rs            # Compiled by just gen; writes openapi.json
├── tests/
│   ├── order_lifecycle.rs               # Integration tests (testcontainers Postgres)
│   ├── e2e_test.rs                      # E2E CDC test (full Docker Compose stack)
│   └── openapi_freshness.rs             # Verifies openapi.json matches current code
├── infra/
│   ├── docker-compose.yml               # All infrastructure services
│   └── debezium/
│       ├── Dockerfile                   # Debezium image with Avro converter
│       └── register-connector.json      # Outbox connector configuration
├── scripts/
│   └── run_e2e_tests.sh                 # Orchestrates E2E test run
├── justfile                             # All developer commands
├── Cargo.toml
├── diesel.toml
└── openapi.json                         # Generated — do not edit by hand
```

---

### Error responses

All error responses use the following JSON body:

```json
{ "error": "<message>" }
```

| HTTP Status | `ApiError` variant | When |
|---|---|---|
| `400 Bad Request` | `BadRequest(msg)` | Invalid input (e.g., malformed currency code, non-positive quantity) |
| `404 Not Found` | `NotFound` | Order or line item does not exist |
| `409 Conflict` | `Conflict(msg)` | Invalid state transition, order not in Draft, total mismatch on confirm |
| `500 Internal Server Error` | `Internal(msg)` | Unexpected database error, serialization failure, pool exhaustion |

Diesel's `NotFound` error is automatically mapped to `ApiError::NotFound`. Other Diesel errors
map to `ApiError::Internal`. Connection pool errors map to `ApiError::Internal`.

---

### Pipeline violation rules

These actions break the single-direction flow and must never be done:

| Action | Why it breaks the pipeline |
|---|---|
| Edit `src/schema.rs` by hand | Overwritten by `diesel migration run`; manual edits are silently discarded |
| Edit `openapi.json` by hand | Overwritten by `just gen`; manual edits are silently discarded |
| Write a handler before the migration and model exist | Handler references types that the compiler cannot find |
| Skip `just gen` after changing a model or handler | `openapi.json` becomes stale; the `pre-commit` check fails |
| Skip `just quality` before pushing | Unformatted, unlinted, or untested code reaches the branch |
