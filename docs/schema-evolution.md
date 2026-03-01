# Schema Evolution

This document describes how Avro schemas for outbox event payloads are managed, registered, and evolved.

## Overview

Outbox event payloads (the `event_data` column in `commerce_order_outbox`) follow the `OrderAggregate` Avro schema defined in `schemas/order-aggregate.avsc`. The schema is registered in Confluent Schema Registry under the subject `commerce.order.aggregate-value` with **BACKWARD** compatibility mode.

## Schema Location

```
schemas/
  order-aggregate.avsc   # Avro schema for the OrderWithItems aggregate payload
```

## Registration

Register schemas in Schema Registry (must be running on `localhost:8081` or pass a custom URL):

```bash
just register-schemas

# Or with a custom Schema Registry URL:
./scripts/register-schemas.sh http://schema-registry.example.com:8081
```

This script:
1. Sets the compatibility mode to `BACKWARD` on the subject.
2. Registers the schema from `schemas/order-aggregate.avsc`.
3. Confirms the compatibility setting.

## Compatibility Mode

The subject uses **BACKWARD** compatibility, which means:

- **New schemas can read data written by the previous schema version.**
- Consumers can be upgraded before producers.
- Adding a new field requires a **default value** so that old records (which lack the field) can still be read.
- Removing a field is allowed only if it had a default value.

### Allowed Changes (BACKWARD-compatible)

| Change | Example | Requirement |
|--------|---------|-------------|
| Add a field | Add `shipping_address` | Must have a `default` value |
| Remove a field that has a default | Remove `confirmed_at` (has `default: null`) | — |
| Add a new enum symbol at the end | Add `Returned` to `OrderStatus` | — |
| Widen a union | Change `["null", "string"]` to `["null", "string", "long"]` | — |

### Disallowed Changes (BACKWARD-incompatible)

| Change | Why |
|--------|-----|
| Remove a field without a default | Old records become unreadable |
| Rename a field | Treated as remove + add |
| Change a field's type | Breaks deserialization |
| Remove an enum symbol | Old records with that symbol become unreadable |

## Evolution Workflow

### 1. Edit the Schema

Modify `schemas/order-aggregate.avsc`. For example, to add a new optional field:

```json
{
  "name": "shipping_address",
  "type": ["null", "string"],
  "default": null,
  "doc": "Shipping address, if provided."
}
```

### 2. Validate Compatibility Locally

Run the schema validation tests to ensure the schema still parses and existing payload shapes still conform:

```bash
cargo test --test schema_validation_test
```

### 3. Check Compatibility with Schema Registry

Before registering, verify the new schema is compatible with the previous version:

```bash
SUBJECT="commerce.order.aggregate-value"
SCHEMA_REGISTRY_URL="http://localhost:8081"

jq -n --arg schema "$(cat schemas/order-aggregate.avsc)" \
  '{"schemaType": "AVRO", "schema": $schema}' | \
curl -s -X POST "$SCHEMA_REGISTRY_URL/compatibility/subjects/$SUBJECT/versions/latest" \
  -H "Content-Type: application/vnd.schemaregistry.v1+json" \
  -d @-
```

Expected response for a compatible schema:
```json
{"is_compatible": true}
```

### 4. Register the New Version

```bash
just register-schemas
```

Schema Registry assigns a new version number. Both old and new versions remain available.

### 5. Update the Rust Code

Update the corresponding Rust structs (`Order`, `OrderLineItem`, `OrderWithItems`) and serialization to match the new schema. Add test cases covering the new field.

### 6. Run Quality Checks

```bash
just quality
```

## Viewing Registered Schemas

```bash
# List all subjects
curl -s http://localhost:8081/subjects | jq .

# List versions for the order aggregate subject
curl -s http://localhost:8081/subjects/commerce.order.aggregate-value/versions | jq .

# Get a specific version
curl -s http://localhost:8081/subjects/commerce.order.aggregate-value/versions/1 | jq .

# Get the latest version
curl -s http://localhost:8081/subjects/commerce.order.aggregate-value/versions/latest | jq .

# Check compatibility mode
curl -s http://localhost:8081/config/commerce.order.aggregate-value | jq .
```

## Relationship with Debezium

Debezium's outbox EventRouter auto-registers its own envelope schema (under the topic subject `public.commerce.order.c2.v1-value`) which wraps envelope fields (`event_id`, `event_type`, `event_date`, `sequence_number`) around a `payload` field containing the order aggregate. The schema registered under `commerce.order.aggregate-value` is the **authoritative definition** for the payload portion and serves as the contract between the producer (order-api) and downstream consumers.
