//! Validates that `OrderWithItems` JSON payloads conform to the Avro schema
//! defined in `schemas/order-aggregate.avsc`.
//!
//! These tests do NOT require Docker or any external infrastructure — they
//! parse the `.avsc` file directly and validate in-memory.

use apache_avro::Schema;
use serde_json::json;

/// Load and parse the order-aggregate Avro schema from disk.
fn load_schema() -> Schema {
    let schema_path = format!(
        "{}/schemas/order-aggregate.avsc",
        env!("CARGO_MANIFEST_DIR")
    );
    let schema_str = std::fs::read_to_string(&schema_path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", schema_path, e));
    Schema::parse_str(&schema_str).unwrap_or_else(|e| panic!("Failed to parse Avro schema: {}", e))
}

/// Convert a `serde_json::Value` into an `apache_avro::types::Value` guided
/// by the given Avro schema, then validate it against the schema by performing
/// a write+read round-trip through Avro binary encoding.
fn validate_payload(schema: &Schema, payload: &serde_json::Value) -> Result<(), String> {
    let avro_value = apache_avro::to_value(payload)
        .map_err(|e| format!("Failed to convert JSON to Avro value: {e}"))?;

    let resolved = avro_value
        .resolve(schema)
        .map_err(|e| format!("Payload does not conform to schema: {e}"))?;

    // Round-trip through binary encoding to confirm the value is fully valid.
    let mut writer = apache_avro::Writer::new(schema, Vec::new());
    writer
        .append(resolved)
        .map_err(|e| format!("Avro write failed: {e}"))?;
    let encoded = writer
        .into_inner()
        .map_err(|e| format!("Avro flush failed: {e}"))?;

    let reader = apache_avro::Reader::new(&encoded[..])
        .map_err(|e| format!("Avro reader creation failed: {e}"))?;
    for result in reader {
        result.map_err(|e| format!("Avro read-back failed: {e}"))?;
    }

    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn test_schema_parses_successfully() {
    let schema = load_schema();
    // Verify the top-level record name.
    if let Schema::Record(record) = &schema {
        assert_eq!(
            record.name.fullname(None),
            "com.commerce.order.OrderAggregate"
        );
    } else {
        panic!("Expected a Record schema, got {:?}", schema);
    }
}

#[test]
fn test_order_with_items_payload_conforms_to_schema() {
    let schema = load_schema();

    let payload = json!({
        "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "status": "Draft",
        "currency": "EUR",
        "total_amount": "149.9700",
        "confirmed_at": null,
        "created_at": "2026-03-01T12:00:00Z",
        "updated_at": "2026-03-01T12:00:00Z",
        "items": [
            {
                "id": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
                "order_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "product_sku": "WIDGET-001",
                "quantity": 3,
                "unit_price": "49.9900",
                "line_total": "149.9700",
                "created_at": "2026-03-01T12:00:00Z"
            }
        ]
    });

    validate_payload(&schema, &payload)
        .unwrap_or_else(|e| panic!("Payload validation failed: {e}"));
}

#[test]
fn test_confirmed_order_payload_conforms_to_schema() {
    let schema = load_schema();

    let payload = json!({
        "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "status": "Confirmed",
        "currency": "USD",
        "total_amount": "299.9400",
        "confirmed_at": "2026-03-01T14:30:00Z",
        "created_at": "2026-03-01T12:00:00Z",
        "updated_at": "2026-03-01T14:30:00Z",
        "items": [
            {
                "id": "b2c3d4e5-f6a7-8901-bcde-f12345678901",
                "order_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "product_sku": "WIDGET-001",
                "quantity": 2,
                "unit_price": "99.9900",
                "line_total": "199.9800",
                "created_at": "2026-03-01T12:00:00Z"
            },
            {
                "id": "c3d4e5f6-a7b8-9012-cdef-123456789012",
                "order_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
                "product_sku": "GADGET-002",
                "quantity": 1,
                "unit_price": "99.9600",
                "line_total": "99.9600",
                "created_at": "2026-03-01T13:00:00Z"
            }
        ]
    });

    validate_payload(&schema, &payload)
        .unwrap_or_else(|e| panic!("Payload validation failed: {e}"));
}

#[test]
fn test_empty_items_array_conforms_to_schema() {
    let schema = load_schema();

    let payload = json!({
        "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "status": "Draft",
        "currency": "EUR",
        "total_amount": "0",
        "confirmed_at": null,
        "created_at": "2026-03-01T12:00:00Z",
        "updated_at": "2026-03-01T12:00:00Z",
        "items": []
    });

    validate_payload(&schema, &payload)
        .unwrap_or_else(|e| panic!("Payload validation failed: {e}"));
}

#[test]
fn test_missing_required_field_fails_validation() {
    let schema = load_schema();

    // Missing "status" field.
    let payload = json!({
        "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "currency": "EUR",
        "total_amount": "0",
        "confirmed_at": null,
        "created_at": "2026-03-01T12:00:00Z",
        "updated_at": "2026-03-01T12:00:00Z",
        "items": []
    });

    assert!(
        validate_payload(&schema, &payload).is_err(),
        "Payload missing a required field should fail validation"
    );
}

#[test]
fn test_invalid_status_enum_fails_validation() {
    let schema = load_schema();

    let payload = json!({
        "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "status": "InvalidStatus",
        "currency": "EUR",
        "total_amount": "0",
        "confirmed_at": null,
        "created_at": "2026-03-01T12:00:00Z",
        "updated_at": "2026-03-01T12:00:00Z",
        "items": []
    });

    assert!(
        validate_payload(&schema, &payload).is_err(),
        "Payload with invalid enum value should fail validation"
    );
}

#[test]
fn test_all_order_statuses_conform_to_schema() {
    let schema = load_schema();

    for status in &["Draft", "Confirmed", "Shipped", "Delivered", "Cancelled"] {
        let payload = json!({
            "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "status": status,
            "currency": "EUR",
            "total_amount": "0",
            "confirmed_at": null,
            "created_at": "2026-03-01T12:00:00Z",
            "updated_at": "2026-03-01T12:00:00Z",
            "items": []
        });

        validate_payload(&schema, &payload)
            .unwrap_or_else(|e| panic!("Payload with status '{status}' failed validation: {e}"));
    }
}

#[test]
fn test_confirmed_at_null_union_branch_conforms_to_schema() {
    let schema = load_schema();

    let payload = json!({
        "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "status": "Draft",
        "currency": "EUR",
        "total_amount": "0",
        "confirmed_at": null,
        "created_at": "2026-03-01T12:00:00Z",
        "updated_at": "2026-03-01T12:00:00Z",
        "items": []
    });
    validate_payload(&schema, &payload).unwrap_or_else(|e| panic!("null confirmed_at failed: {e}"));
}

#[test]
fn test_confirmed_at_string_union_branch_conforms_to_schema() {
    let schema = load_schema();

    // When confirmed_at is present, serde serializes it as a plain JSON string.
    let payload = json!({
        "id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
        "status": "Confirmed",
        "currency": "EUR",
        "total_amount": "0",
        "confirmed_at": "2026-03-01T14:30:00Z",
        "created_at": "2026-03-01T12:00:00Z",
        "updated_at": "2026-03-01T12:00:00Z",
        "items": []
    });
    validate_payload(&schema, &payload)
        .unwrap_or_else(|e| panic!("timestamped confirmed_at failed: {e}"));
}
