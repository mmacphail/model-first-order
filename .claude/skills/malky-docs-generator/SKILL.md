---
name: malky-docs-generator
description: Generates full project documentation following the Diataxis framework (Tutorials, How-to Guides, Reference, Explanation). Reads the actual codebase and produces up to 19 markdown files in docs/ — every fact derived from real code, never generic placeholders. Use when asked to "generate docs", "create documentation", "write project docs", or "run the docs skill".
argument-hint: (no arguments needed — reads the current project)
disable-model-invocation: true
allowed-tools: Read, Write, Glob, Grep, Bash
---

# Diataxis Documentation Generator

Generate a complete `docs/` directory following the **Diataxis framework**. Every fact comes from reading actual project files — never invent, never use placeholders.

## Hard Rules

- **Read before writing.** Complete Phase 1 fully before generating any doc.
- **No invention.** Every endpoint, type, field, env var, port, and command must come from a file you read. If you cannot find a fact, omit it.
- **No duplication.** Do not copy README.md or CLAUDE.md content. Cross-reference them with relative links.
- **Relative links only.** All internal links use relative paths (e.g., `../reference/domain-model.md`).
- **Consistent formatting.** ATX headers, fenced code blocks with language tags, tables for structured data, `> **Note:**` callouts.
- **ASCII diagrams.** No Mermaid — use ASCII art for state machines, pipeline flows, infra topology.
- **Real examples.** Code examples use real types, endpoints, and payloads from the codebase.
- **Adapt to what exists.** Skip docs for features the project doesn't have (see Adaptability Rules).

## Adaptability Rules

Detect what the project has before generating:

| Feature | Detection | If absent |
|---------|-----------|-----------|
| Entities | Glob `src/models/*.rs` | Adjust domain-model.md to whatever models exist |
| State machine | Grep `can_transition_to` | Skip `add-a-status-transition.md`, omit state machine section |
| Outbox / CDC | Grep `outbox` in `migrations/*/up.sql` | Skip `outbox-pattern.md` and `add-outbox-events.md` |
| Infrastructure | Check `infra/docker-compose.yml` | Skip `infrastructure.md`, simplify `run-locally.md` |
| CI | Check `.github/workflows/` | Note "no CI configured" in `deploy.md` |
| OpenAPI | Check `openapi.json` | Build API ref from handler utoipa annotations |
| E2E tests | Check `tests/e2e_test.rs` | Omit E2E section from `run-tests.md` |
| Scripts | Check `scripts/` | Omit scripts references |

---

## Phase 1 — Discovery

Read all key project files. Adapt based on what exists — skip missing categories.

**Root:** `README.md`, `CLAUDE.md`, `Cargo.toml`, `justfile`, `.env.example`

**Source code:** All `src/models/*.rs`, all `src/handlers/*.rs`, `src/routes.rs`, `src/errors.rs`, `src/serializers.rs`, `src/db.rs`, `src/schema.rs`, `src/openapi.rs`, `src/main.rs`, `src/lib.rs`

**OpenAPI:** `openapi.json`

**Migrations:** All `migrations/*/up.sql`

**Infrastructure:** `infra/docker-compose.yml`, `infra/debezium/*.json`, `infra/debezium/Dockerfile`

**Tests:** All files in `tests/`

**CI:** `.github/workflows/*.yml`

**Scripts:** `scripts/*`

After reading you should know: every entity and its fields, the state machine transitions, all API endpoints with request/response types, all DB tables/columns/indexes, all env vars, all justfile recipes, Docker services, test patterns, and CI steps.

---

## Phase 2 — Generate docs/

Generate in 4 dependency-ordered rounds. Earlier rounds produce files that later rounds link to.

### Round 1 — Reference

#### 1. `docs/reference/api.md`

**Sources:** `openapi.json`, `src/handlers/*.rs`, `src/routes.rs`

**Sections:**
- `## Overview` — Base URL, content type, auth notes.
- `## Endpoints` — H3 per group (Orders, Line Items, Health). Per endpoint: method + path, description from utoipa, request body table (field, type, required, description), response (status codes, JSON example with real fields), example curl.
- `## Schemas` — H3 per schema from openapi.json. Table: field, type, description.
- `## Error Responses` — Standard error shape, possible error codes from `src/errors.rs`.

**Cross-links:** `domain-model.md` for business rules, `configuration.md` for env vars.

#### 2. `docs/reference/domain-model.md`

**Sources:** `src/models/*.rs`, `src/handlers/orders.rs`, `migrations/*/up.sql`

**Sections:**
- `## Entities` — H3 per entity. Table: field name, Rust type, DB type, description.
- `## Relationships` — ASCII diagram of entity relationships.
- `## State Machine` — (skip if no `can_transition_to`) ASCII state diagram, transition table (from → to → guard → side effects).
- `## Business Rules` — Numbered invariants from handler logic.
- `## Computed Fields` — DB-generated columns and their expressions.

**Cross-links:** `database-schema.md` for tables, `api.md` for endpoints per entity.

#### 3. `docs/reference/database-schema.md`

**Sources:** All `migrations/*/up.sql`, `src/schema.rs`

**Sections:**
- `## Tables` — H3 per table. Column table (name, type, nullable, default, description), PK, FKs with ON DELETE, indexes, triggers, constraints.
- `## Migration History` — Table: version, name, what it does.
- `## Generated Columns` — Computed columns and expressions.
- `## ER Diagram` — ASCII ER diagram.

**Cross-links:** `domain-model.md` for business meaning, `api.md` for table-to-response mapping.

#### 4. `docs/reference/configuration.md`

**Sources:** `.env.example`, `src/main.rs`, `src/db.rs`, `infra/docker-compose.yml`

**Sections:**
- `## Environment Variables` — Table: variable, required?, default, description.
- `## Server Configuration` — Host, port, CORS.
- `## Database Configuration` — Connection string format, pool settings.
- `## Logging` — `RUST_LOG` format and recommended values.

**Cross-links:** `run-locally.md` for setup, `infrastructure.md` for Docker config.

#### 5. `docs/reference/just-commands.md`

**Sources:** `justfile`

**Sections:**
- `## Quick Reference` — Table of all recipes (command, description).
- Category H2s (Development, Database, Infrastructure, Testing, Code Generation, Quality) with recipe details.
- `## Common Workflows` — Chained recipe sequences (e.g., first-time setup).

**Cross-links:** `run-locally.md`, `run-tests.md`, `deploy.md`.

#### 6. `docs/reference/infrastructure.md`

**Sources:** `infra/docker-compose.yml`, `infra/debezium/*.json`, `infra/debezium/Dockerfile`

Skip if `infra/docker-compose.yml` does not exist.

**Sections:**
- `## Services Overview` — Table: service, image, ports, purpose.
- `## Topology` — ASCII diagram of service connections.
- `## Service Details` — H3 per service (image, ports, env, volumes, deps, health checks).
- `## Debezium Connector` — (skip if no config) Config table, outbox routing, topic naming.
- `## Ports Summary` — All exposed ports.

**Cross-links:** `configuration.md` for env vars, `outbox-pattern.md` for CDC.

### Round 2 — Explanation

#### 7. `docs/explanation/model-first-pipeline.md`

**Sources:** `CLAUDE.md`, `justfile` (`gen` recipe), `src/openapi.rs`

**Sections:**
- `## The Core Idea` — One-directional flow and why it matters.
- `## The Pipeline` — ASCII diagram with generation boundary.
- `## The Generation Boundary` — Hand-written (above) vs. derived (below).
- `## Pipeline Violations` — What not to do and why.
- `## How just gen Works` — Step-by-step.
- `## Benefits` — Single source of truth, compile-time safety, always-fresh docs.

**Cross-links:** `just-commands.md` for `just gen`, `api.md` for OpenAPI output, `add-a-field.md` for pipeline walkthrough.

#### 8. `docs/explanation/outbox-pattern.md`

**Sources:** `src/models/outbox.rs`, outbox migration SQL, `infra/debezium/*.json`, `infra/docker-compose.yml`

Skip if no outbox migration found.

**Sections:**
- `## The Dual-Write Problem` — Why DB + broker separately is unsafe.
- `## The Solution: Transactional Outbox` — Write event in same transaction, CDC picks it up.
- `## How It Works Here` — Project-specific step-by-step flow (handler → transaction → outbox insert → commit → Debezium → Kafka).
- `## The Outbox Table` — Schema with column descriptions.
- `## Debezium Configuration` — Key settings from connector JSON.
- `## Event Payload` — Example JSON from `OrderWithItems` struct shape.

**Cross-links:** `infrastructure.md` for Debezium/Kafka, `database-schema.md` for outbox table, `add-outbox-events.md`.

#### 9. `docs/explanation/precision-preservation.md`

**Sources:** `src/serializers.rs`, `src/models/order.rs`, `src/models/order_line_item.rs`, `migrations/*/up.sql`

**Sections:**
- `## The Problem` — Floating-point can't represent money; JSON numbers lose precision.
- `## The Strategy` — NUMERIC(19,4) → BigDecimal → String.
- `## Database Layer` — Which columns use NUMERIC.
- `## Rust Layer` — BigDecimal in model structs, Diesel mapping.
- `## Serialization Layer` — Walk through `serializers.rs` code.
- `## Why Strings in JSON` — Client parses with their own decimal library.

**Cross-links:** `database-schema.md` for NUMERIC columns, `domain-model.md` for BigDecimal fields, `api.md` for monetary values in responses.

#### 10. `docs/explanation/error-handling.md`

**Sources:** `src/errors.rs`, `src/handlers/orders.rs`

**Sections:**
- `## ApiError Enum` — Each variant, trigger, HTTP status.
- `## Auto-Conversion` — Diesel error → ApiError (`From` impl).
- `## HTTP Response Mapping` — Table: variant → status → body shape.
- `## Error Usage in Handlers` — `?` operator patterns, explicit returns.
- `## Client-Side Handling` — Error shape, status codes, distinguishing error types.

**Cross-links:** `api.md` for error responses, `domain-model.md` for business rule violations.

### Round 3 — How-to Guides

#### 11. `docs/how-to/run-locally.md`

**Sources:** `README.md`, `.env.example`, `justfile`, `Cargo.toml`, `infra/docker-compose.yml`

**Sections:**
- `## Prerequisites` — Tools with versions: Rust, Docker, just, diesel_cli, cargo-watch.
- `## Quick Start` — Numbered steps using actual `just` commands.
- `## Verify It Works` — Curl health endpoint, expected response.
- `## Full Infrastructure` — (if infra/ exists) Start full stack for CDC development.
- `## Troubleshooting` — Common issues: Docker not running, port conflicts, migration failures.

**Cross-links:** `configuration.md`, `just-commands.md`, `infrastructure.md`.

#### 12. `docs/how-to/run-tests.md`

**Sources:** `tests/*.rs`, `justfile`, `.github/workflows/*.yml`

**Sections:**
- `## Prerequisites` — Docker required for testcontainers.
- `## Integration Tests` — `cargo test` / `just test`. Testcontainers, disposable Postgres. Single test syntax.
- `## E2E Tests` — (if exist) `just test-e2e`. What they test, infra requirements.
- `## Code Coverage` — (if recipe exists) `just coverage`, report location.
- `## Quality Gate` — `just quality` and what it checks.
- `## CI Pipeline` — (if CI exists) What runs on push/PR.

**Cross-links:** `just-commands.md`, `deploy.md`.

#### 13. `docs/how-to/add-a-field.md`

**Sources:** `CLAUDE.md`, any migration, any model, any handler

**Sections:**
- `## Overview` — Adding a field follows the model-first pipeline in order.
- `## Step 1: Write the Migration` — Concrete example: adding `notes` TEXT to orders. Show `up.sql`/`down.sql`.
- `## Step 2: Run the Migration` — `just migrate`, updates `src/schema.rs`.
- `## Step 3: Update the Rust Model` — Add to Queryable/Insertable/response structs. Show code.
- `## Step 4: Update the Handler` — Accept field in create/update. Show code.
- `## Step 5: Update utoipa Annotations` — Ensure field appears in OpenAPI.
- `## Step 6: Regenerate OpenAPI` — `just gen`.
- `## Step 7: Run Quality Gate` — `just quality`.

**Cross-links:** `model-first-pipeline.md`, `domain-model.md`, `database-schema.md`.

#### 14. `docs/how-to/add-a-status-transition.md`

**Sources:** `src/models/order_status.rs`, `src/handlers/orders.rs`

Skip if no state machine found.

**Sections:**
- `## Overview` — State machine is in code, not DB constraints.
- `## Step 1: Add the Status Variant` — New enum variant if needed.
- `## Step 2: Update Transition Rules` — Modify `can_transition_to()`, before/after.
- `## Step 3: Add Migration` — If DB enum change needed.
- `## Step 4: Add Handler` — Endpoint code following existing patterns.
- `## Step 5: Register Route` — Add to `src/routes.rs`.
- `## Step 6: Add Test` — Test pattern from existing tests.
- `## Step 7: Run Quality Gate` — `just quality`.

**Cross-links:** `domain-model.md` for state machine, `api.md` for transition endpoints, `model-first-pipeline.md`.

#### 15. `docs/how-to/add-outbox-events.md`

**Sources:** `src/models/outbox.rs`, `src/handlers/orders.rs`, `infra/debezium/*.json`

Skip if no outbox found.

**Sections:**
- `## Overview` — Every mutation should write a transactional outbox event.
- `## Step 1: Define Event Type` — Add constant in relevant module.
- `## Step 2: Call insert_outbox_event() Inside the Transaction` — Show the pattern. Emphasize: same transaction.
- `## Step 3: Verify the Aggregate Snapshot` — Ensure full aggregate is loaded.
- `## Step 4: Test` — Check outbox row assertions.
- `## Step 5: Debezium` — No config change needed (outbox connector routes all events). Explain why.

**Cross-links:** `outbox-pattern.md`, `infrastructure.md`, `database-schema.md`.

#### 16. `docs/how-to/deploy.md`

**Sources:** `.github/workflows/*.yml`, `justfile`, `Cargo.toml`

**Sections:**
- `## CI Pipeline` — (if exists) Each workflow: trigger, steps, checks. If no CI: "No CI configured yet."
- `## Pre-commit Workflow` — `just pre-commit` / `just quality`.
- `## Build for Release` — `cargo build --release`.
- `## Database Migrations in Production` — How to run against production.
- `## Environment Configuration` — Production env vars (link to `configuration.md`).

**Cross-links:** `configuration.md`, `run-tests.md`, `infrastructure.md`.

### Round 4 — Tutorials + Index

#### 17. `docs/tutorials/your-first-order.md`

**Sources:** `src/handlers/orders.rs`, `openapi.json`, `justfile` (smoke recipe)

**Sections:**
- `## What You'll Learn` — Create an order, add items, confirm, verify.
- `## Prerequisites` — Server running (link to `run-locally.md`).
- `## Step 1: Create an Order` — Curl POST, request body, full response, field explanations.
- `## Step 2: Add a Line Item` — Curl POST, explain line_total computation.
- `## Step 3: Add Another Line Item` — Show total_amount updating.
- `## Step 4: View the Order` — Curl GET, full response with items.
- `## Step 5: Confirm the Order` — Curl status transition, confirmed_at set.
- `## Step 6: Try an Invalid Transition` — Curl back to Draft, show error.
- `## Step 7: List All Orders` — Curl GET list.
- `## Next Steps` — Link to how-to guides and explanations.

Use real endpoints, fields, JSON shapes. Use the PORT from `.env.example`.

#### 18. `docs/tutorials/adding-a-new-aggregate.md`

**Sources:** All `src/models/`, `src/handlers/`, `src/routes.rs`, `migrations/`, `CLAUDE.md`

**Sections:**
- `## What You'll Learn` — Add a hypothetical `Product` aggregate end-to-end.
- `## Step 1: Design` — Define Product fields/rules simply.
- `## Step 2: Migration` — `up.sql` following project conventions (UUID PK, timestamps, NUMERIC prices).
- `## Step 3: Diesel Model` — `src/models/product.rs` with Queryable/Insertable.
- `## Step 4: Handler` — `src/handlers/products.rs` with CRUD endpoints.
- `## Step 5: Routes` — Register in `src/routes.rs`.
- `## Step 6: OpenAPI` — utoipa annotations, `just gen`.
- `## Step 7: Tests` — Integration test following testcontainers pattern.
- `## Step 8: Quality Gate` — `just quality`.
- `## Recap` — Pipeline order, link to `model-first-pipeline.md`.

Hypothetical entity but real project patterns, macros, imports, conventions.

#### 19. `docs/README.md`

**Sources:** All generated docs

**Sections:**
- `## <Project Name> Documentation` — Title from `Cargo.toml`.
- Brief description (1-2 sentences from README or Cargo.toml).
- `## Documentation Map` — 2x2 Diataxis grid:

```
|                  | Learning-oriented           | Information-oriented     |
|------------------|-----------------------------|--------------------------|
| **Practical**    | [Tutorials](tutorials/)     | [How-to Guides](how-to/) |
| **Theoretical**  | [Explanation](explanation/) | [Reference](reference/)  |
```

- `## Tutorials` — Bullet list with one-line descriptions.
- `## How-to Guides` — Bullet list.
- `## Reference` — Bullet list.
- `## Explanation` — Bullet list.
- `## Quick Links` — README.md, CLAUDE.md, most-needed docs.

Only include docs that were actually generated.

---

## Phase 3 — Cross-link Verification

1. Read every generated file in `docs/`.
2. Extract all relative markdown links (`[text](../path/to/file.md)`).
3. Verify each target exists in `docs/`.
4. Fix broken links (update path or remove if target was skipped).

---

## Phase 4 — Summary

Print:
1. **File tree** — Generated `docs/` directory.
2. **File count** — Generated vs. skipped (with reasons).
3. **Cross-link status** — X links verified, Y broken fixed.
4. **Next steps** — Review docs, `git add docs/`, link from top-level README.
