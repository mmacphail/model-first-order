//! End-to-end test: create order → Debezium CDC → Avro → Kafka topic.
//!
//! Requires the full infrastructure stack to be running:
//!
//!   just infra-up
//!
//! The easiest way to run this test is via the helper script:
//!
//!   ./scripts/run_e2e_tests.sh
//!
//! Or start infrastructure manually and run with:
//!
//!   DATABASE_URL=postgres://order_api:order_api@localhost:5432/order_api \
//!     cargo test --test e2e_test -- --include-ignored --nocapture

use apache_avro::types::Value as AvroValue;
use futures::StreamExt;
use order_api::db;
use order_api::routes;
use rdkafka::consumer::{Consumer, StreamConsumer};
use rdkafka::message::{BorrowedMessage, Message};
use rdkafka::ClientConfig;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use uuid::Uuid;

const DEBEZIUM_URL: &str = "http://localhost:8083";
const SCHEMA_REGISTRY_URL: &str = "http://localhost:8081";
const KAFKA_BROKERS: &str = "localhost:9092";
const KAFKA_TOPIC: &str = "public.commerce.order.c2.v1";
const APP_PORT: u16 = 18080;
const KAFKA_WAIT_SECS: u64 = 60;

/// Wait until `url` returns an HTTP response, retrying every `interval` for up
/// to `timeout` total. Panics if the service never becomes reachable.
async fn wait_for_http(label: &str, url: &str, timeout: Duration, interval: Duration) {
    let client = Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .unwrap();
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if tokio::time::Instant::now() > deadline {
            panic!("{} did not become ready within {:?}", label, timeout);
        }
        if client.get(url).send().await.is_ok() {
            return;
        }
        tokio::time::sleep(interval).await;
    }
}

/// Register (or replace) the Debezium outbox connector.
///
/// Column mapping matches this project's `commerce_order_outbox` table:
/// `event_id`, `event_data`, `event_date`, `sequence_number`.
async fn register_debezium_connector(http: &Client) {
    // Remove any stale connector so registration is idempotent.
    let _ = http
        .delete(format!(
            "{}/connectors/order-outbox-connector",
            DEBEZIUM_URL
        ))
        .send()
        .await;
    tokio::time::sleep(Duration::from_secs(2)).await;

    let connector_config = json!({
        "name": "order-outbox-connector",
        "config": {
            "connector.class": "io.debezium.connector.postgresql.PostgresConnector",
            "database.hostname": "postgres",
            "database.port": "5432",
            "database.user": "order_api",
            "database.password": "order_api",
            "database.dbname": "order_api",
            "topic.prefix": "order_api_e2e",
            "plugin.name": "pgoutput",
            "slot.name": "e2e_slot",
            "publication.name": "e2e_pub",
            "table.include.list": "public.commerce_order_outbox",
            "tombstones.on.delete": "false",
            "transforms": "outbox",
            "transforms.outbox.type": "io.debezium.transforms.outbox.EventRouter",
            "transforms.outbox.table.field.event.id": "event_id",
            "transforms.outbox.table.field.event.key": "aggregate_id",
            "transforms.outbox.table.field.event.type": "event_type",
            "transforms.outbox.table.field.event.payload": "event_data",
            "transforms.outbox.route.by.field": "aggregate_type",
            "transforms.outbox.route.topic.replacement": KAFKA_TOPIC,
            "transforms.outbox.table.fields.additional.placement":
                "event_id:envelope,event_type:envelope,event_date:envelope,sequence_number:envelope",
            "transforms.outbox.table.expand.json.payload": "true",
            "key.converter": "org.apache.kafka.connect.storage.StringConverter",
            "value.converter": "io.confluent.connect.avro.AvroConverter",
            "value.converter.schema.registry.url": "http://schema-registry:8081"
        }
    });

    let resp = http
        .post(format!("{}/connectors", DEBEZIUM_URL))
        .json(&connector_config)
        .send()
        .await
        .expect("Failed to POST connector to Debezium");

    assert!(
        resp.status().is_success(),
        "Debezium connector registration failed ({}): {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );
}

/// Poll the Debezium connector status until both the connector and its task
/// report RUNNING.
async fn wait_for_connector_running(http: &Client) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        if tokio::time::Instant::now() > deadline {
            panic!("Debezium connector did not reach RUNNING state within 60 s");
        }
        let resp = http
            .get(format!(
                "{}/connectors/order-outbox-connector/status",
                DEBEZIUM_URL
            ))
            .send()
            .await;

        if let Ok(r) = resp {
            if let Ok(v) = r.json::<Value>().await {
                let connector_running = v["connector"]["state"].as_str() == Some("RUNNING");
                let task_running = v["tasks"]
                    .as_array()
                    .and_then(|tasks| tasks.first())
                    .and_then(|t| t["state"].as_str())
                    == Some("RUNNING");

                if connector_running && task_running {
                    return;
                }

                let task_failed = v["tasks"]
                    .as_array()
                    .and_then(|tasks| tasks.first())
                    .and_then(|t| t["state"].as_str())
                    == Some("FAILED");
                if task_failed {
                    let trace = v["tasks"][0]["trace"]
                        .as_str()
                        .unwrap_or("<no trace>")
                        .lines()
                        .take(5)
                        .collect::<Vec<_>>()
                        .join("\n");
                    panic!("Debezium connector task entered FAILED state:\n{}", trace);
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

// ── Test ──────────────────────────────────────────────────────────────────────

/// Full end-to-end flow:
///  1. Start the order-api service (actix-web) in a background task.
///  2. Register the Debezium outbox connector (Avro + Schema Registry).
///  3. Create an order, add a line item, and confirm it via the REST API.
///  4. Consume the Kafka topic until the `ORDER_CONFIRMED` event matching
///     the order ID is received (up to 60 s).
///
/// Messages are Avro records with envelope fields (`event_id`, `event_type`,
/// `event_date`, `sequence_number`) plus a nested `payload`.
#[tokio::test]
#[ignore = "requires docker-compose infrastructure – run via scripts/run_e2e_tests.sh"]
async fn test_order_event_reaches_kafka() {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://order_api:order_api@localhost:5432/order_api".into());

    // ── 1. Start the order-api service ───────────────────────────────────────
    let pool = db::init_pool(&database_url);
    db::run_migrations(&pool);

    let server = actix_web::HttpServer::new(move || {
        actix_web::App::new()
            .app_data(actix_web::web::Data::new(pool.clone()))
            .configure(routes::configure)
    })
    .bind(("127.0.0.1", APP_PORT))
    .expect("Failed to bind the order-api service")
    .run();

    tokio::spawn(server);

    let app_url = format!("http://127.0.0.1:{}", APP_PORT);

    wait_for_http(
        "order-api",
        &format!("{}/api/orders?limit=1", app_url),
        Duration::from_secs(10),
        Duration::from_millis(300),
    )
    .await;

    let http = Client::new();

    // ── 2. Register the Debezium connector ──────────────────────────────────
    wait_for_http(
        "Schema Registry",
        &format!("{}/subjects", SCHEMA_REGISTRY_URL),
        Duration::from_secs(60),
        Duration::from_secs(2),
    )
    .await;

    wait_for_http(
        "Debezium Connect",
        &format!("{}/connectors", DEBEZIUM_URL),
        Duration::from_secs(60),
        Duration::from_secs(2),
    )
    .await;

    register_debezium_connector(&http).await;
    wait_for_connector_running(&http).await;

    // ── 3. Create a Kafka consumer ──────────────────────────────────────────
    let consumer: StreamConsumer = ClientConfig::new()
        .set("bootstrap.servers", KAFKA_BROKERS)
        .set("group.id", format!("e2e-{}", Uuid::new_v4()))
        .set("auto.offset.reset", "earliest")
        .set("enable.auto.commit", "false")
        .create()
        .expect("Failed to create Kafka consumer");

    consumer
        .subscribe(&[KAFKA_TOPIC])
        .expect("Failed to subscribe to Kafka topic");

    // ── 4. Create an order, add a line item, confirm it ─────────────────────
    let create_resp = http
        .post(format!("{}/api/orders", app_url))
        .json(&json!({ "currency": "EUR" }))
        .send()
        .await
        .expect("Failed to POST /api/orders");

    assert_eq!(
        create_resp.status(),
        201,
        "Expected 201 Created from POST /api/orders"
    );

    let body: Value = create_resp
        .json()
        .await
        .expect("Failed to parse order body");
    let order_id = body["id"]
        .as_str()
        .expect("Response body missing 'id' field")
        .to_string();

    println!("Created order id={}", order_id);

    // Add a line item
    let item_resp = http
        .post(format!("{}/api/orders/{}/items", app_url, order_id))
        .json(&json!({
            "product_sku": "WIDGET-E2E",
            "quantity": 3,
            "unit_price": "29.9900"
        }))
        .send()
        .await
        .expect("Failed to POST line item");

    assert_eq!(item_resp.status(), 201, "Expected 201 from POST line item");

    // Confirm the order
    let confirm_resp = http
        .patch(format!("{}/api/orders/{}/status", app_url, order_id))
        .json(&json!({ "status": "Confirmed" }))
        .send()
        .await
        .expect("Failed to PATCH order status");

    assert_eq!(
        confirm_resp.status(),
        200,
        "Expected 200 from PATCH order status"
    );

    println!("Order confirmed, waiting for Kafka event...");

    // ── 5. Poll Kafka until the matching ORDER_CONFIRMED event appears ──────
    let deadline = tokio::time::Instant::now() + Duration::from_secs(KAFKA_WAIT_SECS);
    let mut kafka_stream = consumer.stream();
    let mut found = false;

    loop {
        if tokio::time::Instant::now() > deadline {
            break;
        }

        let msg: BorrowedMessage<'_> =
            match tokio::time::timeout(Duration::from_secs(5), kafka_stream.next()).await {
                Ok(Some(Ok(m))) => m,
                Ok(Some(Err(e))) => {
                    eprintln!("Kafka error: {}", e);
                    continue;
                }
                _ => continue,
            };

        let raw_bytes = match msg.payload() {
            Some(b) => b,
            None => continue,
        };

        let record = match decode_avro_record(raw_bytes, &http).await {
            Some(r) => r,
            None => {
                eprintln!("Failed to decode Avro record ({} bytes)", raw_bytes.len());
                continue;
            }
        };

        // Extract the `payload` field.
        let payload_avro = match record.get("payload") {
            Some(v) => v,
            None => {
                eprintln!("Avro record missing 'payload' field");
                continue;
            }
        };

        let event: Value = match apache_avro::from_value(payload_avro) {
            Ok(Value::String(s)) => match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Failed to parse payload string as JSON: {}", e);
                    continue;
                }
            },
            Ok(v) => v,
            Err(e) => {
                eprintln!("Failed to convert Avro payload to JSON Value: {}", e);
                continue;
            }
        };

        // Check envelope event_type — we're looking for ORDER_CONFIRMED specifically.
        let event_type = match record.get("event_type") {
            Some(AvroValue::String(s)) => s.clone(),
            _ => continue,
        };

        // Filter to this order's events.
        let payload_order_id = event["id"].as_str().unwrap_or_default();
        if payload_order_id != order_id {
            continue;
        }

        println!(
            "Received Kafka event: type={}, order_id={}",
            event_type, payload_order_id
        );

        if event_type != "ORDER_CONFIRMED" {
            // Keep consuming — we'll see ORDER_CREATED and ORDER_UPDATED first.
            continue;
        }

        // ── Payload assertions ──────────────────────────────────────────────
        assert_eq!(
            event["status"].as_str(),
            Some("Confirmed"),
            "ORDER_CONFIRMED event should have status Confirmed"
        );
        assert_eq!(
            event["currency"].as_str(),
            Some("EUR"),
            "ORDER_CONFIRMED event currency mismatch"
        );

        let items = event["items"]
            .as_array()
            .expect("ORDER_CONFIRMED event 'items' should be an array");
        assert_eq!(items.len(), 1, "Expected exactly 1 order line in event");
        assert_eq!(
            items[0]["product_sku"].as_str(),
            Some("WIDGET-E2E"),
            "Order line product_sku mismatch"
        );
        assert_eq!(
            items[0]["quantity"].as_i64(),
            Some(3),
            "Order line quantity mismatch"
        );

        // ── Envelope field assertions ───────────────────────────────────────
        let event_id = match record.get("event_id") {
            Some(AvroValue::String(s)) => s.clone(),
            _ => panic!("Avro envelope missing 'event_id' string field"),
        };
        assert!(
            !event_id.is_empty(),
            "Avro envelope event_id should be non-empty"
        );

        let event_date = match record.get("event_date") {
            Some(AvroValue::String(s)) => s.clone(),
            _ => panic!("Avro envelope missing 'event_date' string field"),
        };
        assert!(
            !event_date.is_empty(),
            "Avro envelope event_date should be non-empty"
        );

        found = true;
        break;
    }

    assert!(
        found,
        "ORDER_CONFIRMED event for order '{}' was not received on Kafka topic '{}' within {} s",
        order_id, KAFKA_TOPIC, KAFKA_WAIT_SECS
    );
}

// ── Avro wire format helpers ────────────────────────────────────────────────

/// Decode an Avro-encoded record from the Confluent wire format.
///
/// Wire format: magic byte (0x00) + 4-byte big-endian schema ID + Avro binary.
async fn decode_avro_record(
    bytes: &[u8],
    http: &Client,
) -> Option<std::collections::HashMap<String, AvroValue>> {
    if bytes.len() < 5 || bytes[0] != 0x00 {
        return None;
    }

    let schema_id = u32::from_be_bytes(bytes[1..5].try_into().ok()?);
    let avro_bytes = &bytes[5..];

    let schema_url = format!("{}/schemas/ids/{}", SCHEMA_REGISTRY_URL, schema_id);
    let schema_resp: Value = http.get(&schema_url).send().await.ok()?.json().await.ok()?;
    let schema_str = schema_resp["schema"].as_str()?;

    let schema = apache_avro::Schema::parse_str(schema_str).ok()?;

    let value =
        apache_avro::from_avro_datum(&schema, &mut avro_bytes.to_vec().as_slice(), None).ok()?;

    if let AvroValue::Record(fields) = value {
        Some(fields.into_iter().collect())
    } else {
        None
    }
}
