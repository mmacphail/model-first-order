use actix_web::{test, web, App};
use diesel::prelude::*;
use diesel::r2d2::{ConnectionManager, Pool};
use order_api::db::{self, DbPool};
use order_api::models::order::Order;
use order_api::models::order_line_item::OrderLineItem;
use order_api::models::order_status::OrderStatus;
use order_api::models::outbox::OutboxEvent;
use order_api::routes;
use order_api::schema::commerce_order_outbox;
use std::time::Duration;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};
use uuid::Uuid;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind failed")
        .local_addr()
        .expect("addr failed")
        .port()
}

async fn setup_db() -> (ContainerAsync<GenericImage>, DbPool) {
    let port = free_port();
    let container = GenericImage::new("postgres", "16-alpine")
        .with_wait_for(WaitFor::message_on_stderr(
            "database system is ready to accept connections",
        ))
        .with_mapped_port(port, ContainerPort::Tcp(5432))
        .with_env_var("POSTGRES_USER", "postgres")
        .with_env_var("POSTGRES_PASSWORD", "postgres")
        .with_env_var("POSTGRES_DB", "postgres")
        .start()
        .await
        .expect("Failed to start Postgres container");

    let url = format!("postgres://postgres:postgres@127.0.0.1:{}/postgres", port);
    let pool = db::init_pool(&url);
    db::run_migrations(&pool);
    (container, pool)
}

#[actix_web::test]
async fn test_order_lifecycle() {
    let (_container, pool) = setup_db().await;

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    // Create order
    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({
            "currency": "EUR"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let order: Order = test::read_body_json(resp).await;
    assert_eq!(order.status, OrderStatus::Draft);

    // Add line item
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "WIDGET-001",
            "quantity": 3,
            "unit_price": "49.9900"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    // Confirm order
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let order: Order = test::read_body_json(resp).await;
    assert!(order.confirmed_at.is_some());

    // Cannot add items to confirmed order (EARS: state-driven)
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "GADGET-002",
            "quantity": 1,
            "unit_price": "9.99"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);

    // Cannot transition backwards
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Draft" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_bigdecimal_precision_preserved() {
    let (_container, pool) = setup_db().await;

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    // Create order
    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({
            "currency": "USD"
        }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    // Add line item with precise decimal
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "PRECISE-001",
            "quantity": 1,
            "unit_price": "12345.6789"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let item: OrderLineItem = test::read_body_json(resp).await;
    assert_eq!(item.unit_price.to_string(), "12345.6789");

    // Fetch order and verify total preserves all 4 decimal places
    let resp = test::TestRequest::get()
        .uri(&format!("/api/orders/{}", order.id))
        .send_request(&app)
        .await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total_amount"], "12345.6789");
}

#[actix_web::test]
async fn test_cancellation_rules() {
    let (_container, pool) = setup_db().await;

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    // Create and confirm an order
    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({
            "currency": "EUR"
        }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    // Add item so total matches
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "WIDGET-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    // Confirm
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    // Ship
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Shipped" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    // Cannot cancel shipped order (edge case from spec)
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Cancelled" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_pagination_edge_cases() {
    let (_container, pool) = setup_db().await;

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    // Create an order so there's at least one result
    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    // Negative limit is clamped to 1 (returns at most 1 row)
    let resp = test::TestRequest::get()
        .uri("/api/orders?limit=-1")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Vec<Order> = test::read_body_json(resp).await;
    assert_eq!(body.len(), 1);

    // Negative offset is clamped to 0 (does not error)
    let resp = test::TestRequest::get()
        .uri("/api/orders?offset=-5")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Vec<Order> = test::read_body_json(resp).await;
    assert!(!body.is_empty());

    // Offset beyond result set returns empty
    let resp = test::TestRequest::get()
        .uri("/api/orders?offset=9999")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Vec<Order> = test::read_body_json(resp).await;
    assert!(body.is_empty());

    // Limit exceeding cap is clamped to 100 (succeeds, returns <= 100 rows)
    let resp = test::TestRequest::get()
        .uri("/api/orders?limit=999")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Vec<Order> = test::read_body_json(resp).await;
    assert!(body.len() <= 100);
}

#[actix_web::test]
async fn test_delete_nonexistent_line_item_returns_404() {
    let (_container, pool) = setup_db().await;

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    // Create a draft order
    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let order: Order = test::read_body_json(resp).await;

    // Attempt to delete a non-existent line item
    let fake_item_id = Uuid::new_v4();
    let resp = test::TestRequest::delete()
        .uri(&format!("/api/orders/{}/items/{}", order.id, fake_item_id))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 404);
}

#[actix_web::test]
async fn test_outbox_events_written_with_order_lifecycle() {
    let (_container, pool) = setup_db().await;

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    // Create order → ORDER_CREATED
    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "EUR" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let order: Order = test::read_body_json(resp).await;

    // Add first line item → ORDER_UPDATED
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "WIDGET-001",
            "quantity": 2,
            "unit_price": "25.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    // Add second line item (will be deleted) → ORDER_UPDATED
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "GADGET-002",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let temp_item: OrderLineItem = test::read_body_json(resp).await;

    // Delete second line item → ORDER_UPDATED
    let resp = test::TestRequest::delete()
        .uri(&format!("/api/orders/{}/items/{}", order.id, temp_item.id))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 204);

    // Confirm order → ORDER_CONFIRMED
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    // Query outbox — expect 5 events ordered by sequence_number
    let mut conn = pool.get().expect("Failed to get connection");
    let events: Vec<OutboxEvent> = commerce_order_outbox::table
        .filter(commerce_order_outbox::aggregate_id.eq(order.id))
        .order(commerce_order_outbox::sequence_number.asc())
        .select(OutboxEvent::as_select())
        .load(&mut conn)
        .expect("Failed to load outbox events");

    assert_eq!(events.len(), 5);

    // Verify event types in insertion order
    assert_eq!(events[0].event_type, "ORDER_CREATED");
    assert_eq!(events[1].event_type, "ORDER_UPDATED"); // add item 1
    assert_eq!(events[2].event_type, "ORDER_UPDATED"); // add item 2
    assert_eq!(events[3].event_type, "ORDER_UPDATED"); // delete item 2
    assert_eq!(events[4].event_type, "ORDER_CONFIRMED");

    // All events reference the same aggregate
    for event in &events {
        assert_eq!(event.aggregate_type, "order");
        assert_eq!(event.aggregate_id, order.id);
    }

    // Verify event_data contains the full aggregate payload
    let last_event_data = &events[4].event_data;
    assert_eq!(last_event_data["id"], order.id.to_string());
    assert_eq!(last_event_data["status"], "Confirmed");
    assert!(last_event_data["items"].is_array());
    assert_eq!(last_event_data["items"].as_array().unwrap().len(), 1);
}

// ── health check ───────────────────────────────────────────────────────────

#[actix_web::test]
async fn test_health_check_returns_database_ok() {
    let (_container, pool) = setup_db().await;

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::get()
        .uri("/health")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert_eq!(body["database"], "ok");
    assert!(body.get("detail").is_none());
}

#[actix_web::test]
async fn test_health_check_returns_degraded_when_db_unreachable() {
    // Build a pool pointing to a non-existent host with a very short timeout
    let manager = ConnectionManager::<diesel::PgConnection>::new(
        "postgres://postgres:postgres@127.0.0.1:1/nonexistent",
    );
    let bad_pool: DbPool = Pool::builder()
        .connection_timeout(Duration::from_millis(100))
        .build_unchecked(manager);

    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(bad_pool))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::get()
        .uri("/health")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 503);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["status"], "degraded");
    assert_eq!(body["database"], "error");
    assert_eq!(body["detail"], "database unreachable");
}

// ── create_order — validation ──────────────────────────────────────────────

#[actix_web::test]
async fn test_create_order_with_lowercase_currency_returns_400() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "eur" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 400);
}

#[actix_web::test]
async fn test_create_order_with_too_short_currency_returns_400() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "US" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 400);
}

#[actix_web::test]
async fn test_create_order_with_too_long_currency_returns_400() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "EURO" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 400);
}

#[actix_web::test]
async fn test_create_order_with_digit_in_currency_returns_400() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "U5D" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 400);
}

#[actix_web::test]
async fn test_new_order_has_zero_total_and_null_confirmed_at() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total_amount"], "0");
    assert!(body["confirmed_at"].is_null());
    assert_eq!(body["status"], "Draft");
}

// ── get_order ──────────────────────────────────────────────────────────────

#[actix_web::test]
async fn test_get_order_not_found_returns_404() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let nonexistent_id = Uuid::new_v4();
    let resp = test::TestRequest::get()
        .uri(&format!("/api/orders/{nonexistent_id}"))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 404);
}

#[actix_web::test]
async fn test_get_order_returns_items_array() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "GBP" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::get()
        .uri(&format!("/api/orders/{}", order.id))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["id"], order.id.to_string());
    assert!(body["items"].is_array());
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

// ── list_orders ────────────────────────────────────────────────────────────

#[actix_web::test]
async fn test_list_orders_empty_database_returns_empty_array() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::get()
        .uri("/api/orders")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Vec<serde_json::Value> = test::read_body_json(resp).await;
    assert!(body.is_empty());
}

// ── add_line_item — validation ─────────────────────────────────────────────

#[actix_web::test]
async fn test_add_line_item_zero_quantity_returns_400() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 0,
            "unit_price": "10.00"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 400);
}

#[actix_web::test]
async fn test_add_line_item_negative_quantity_returns_400() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": -5,
            "unit_price": "10.00"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 400);
}

#[actix_web::test]
async fn test_add_line_item_to_nonexistent_order_returns_404() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let nonexistent_id = Uuid::new_v4();
    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{nonexistent_id}/items"))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.00"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 404);
}

#[actix_web::test]
async fn test_add_line_item_to_confirmed_order_returns_409() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-NEW",
            "quantity": 1,
            "unit_price": "5.00"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_add_line_item_to_shipped_order_returns_409() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Shipped" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-NEW",
            "quantity": 1,
            "unit_price": "5.00"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_add_line_item_to_cancelled_order_returns_409() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Cancelled" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.00"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_add_multiple_items_total_amount_is_sum_of_line_totals() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    // Item 1: 2 x 10.00 = 20.00
    test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-A",
            "quantity": 2,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;

    // Item 2: 3 x 5.00 = 15.00
    test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-B",
            "quantity": 3,
            "unit_price": "5.0000"
        }))
        .send_request(&app)
        .await;

    // total_amount should be 20.00 + 15.00 = 35.00
    let resp = test::TestRequest::get()
        .uri(&format!("/api/orders/{}", order.id))
        .send_request(&app)
        .await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total_amount"], "35.0000");
    assert_eq!(body["items"].as_array().unwrap().len(), 2);
}

// ── transition_status ──────────────────────────────────────────────────────

#[actix_web::test]
async fn test_transition_nonexistent_order_returns_404() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let nonexistent_id = Uuid::new_v4();
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{nonexistent_id}/status"))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 404);
}

#[actix_web::test]
async fn test_cancel_from_draft_succeeds() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Cancelled" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Order = test::read_body_json(resp).await;
    assert_eq!(body.status, OrderStatus::Cancelled);
}

#[actix_web::test]
async fn test_cancel_from_confirmed_succeeds() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Cancelled" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Order = test::read_body_json(resp).await;
    assert_eq!(body.status, OrderStatus::Cancelled);
}

#[actix_web::test]
async fn test_ship_to_delivered_succeeds() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Shipped" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Delivered" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    let body: Order = test::read_body_json(resp).await;
    assert_eq!(body.status, OrderStatus::Delivered);
}

#[actix_web::test]
async fn test_transition_from_delivered_returns_409() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 201);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Shipped" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Delivered" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    // Delivered is terminal
    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Cancelled" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_outbox_event_contains_order_cancelled_event_type() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Cancelled" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let mut conn = pool.get().expect("Failed to get connection");
    let events: Vec<OutboxEvent> = commerce_order_outbox::table
        .filter(commerce_order_outbox::aggregate_id.eq(order.id))
        .order(commerce_order_outbox::sequence_number.asc())
        .select(OutboxEvent::as_select())
        .load(&mut conn)
        .expect("Failed to load outbox events");

    assert_eq!(events.len(), 2); // ORDER_CREATED + ORDER_CANCELLED
    assert_eq!(events[0].event_type, "ORDER_CREATED");
    assert_eq!(events[1].event_type, "ORDER_CANCELLED");
    assert_eq!(events[1].event_data["status"], "Cancelled");
}

// ── delete_line_item ───────────────────────────────────────────────────────

#[actix_web::test]
async fn test_delete_last_item_resets_total_to_zero() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "ONLY-SKU",
            "quantity": 2,
            "unit_price": "15.0000"
        }))
        .send_request(&app)
        .await;
    let item: OrderLineItem = test::read_body_json(resp).await;

    // Verify total is non-zero
    let resp = test::TestRequest::get()
        .uri(&format!("/api/orders/{}", order.id))
        .send_request(&app)
        .await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total_amount"], "30.0000");

    // Delete the only item
    let resp = test::TestRequest::delete()
        .uri(&format!("/api/orders/{}/items/{}", order.id, item.id))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 204);

    // Total must reset to zero
    let resp = test::TestRequest::get()
        .uri(&format!("/api/orders/{}", order.id))
        .send_request(&app)
        .await;
    let body: serde_json::Value = test::read_body_json(resp).await;
    assert_eq!(body["total_amount"], "0");
    assert_eq!(body["items"].as_array().unwrap().len(), 0);
}

#[actix_web::test]
async fn test_delete_item_from_confirmed_order_returns_409() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;
    let item: OrderLineItem = test::read_body_json(resp).await;

    let resp = test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);

    let resp = test::TestRequest::delete()
        .uri(&format!("/api/orders/{}/items/{}", order.id, item.id))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 409);
}

#[actix_web::test]
async fn test_delete_item_belonging_to_different_order_returns_404() {
    let (_container, pool) = setup_db().await;
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure),
    )
    .await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "USD" }))
        .send_request(&app)
        .await;
    let order_a: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri("/api/orders")
        .set_json(serde_json::json!({ "currency": "EUR" }))
        .send_request(&app)
        .await;
    let order_b: Order = test::read_body_json(resp).await;

    let resp = test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order_b.id))
        .set_json(serde_json::json!({
            "product_sku": "SKU-B1",
            "quantity": 1,
            "unit_price": "5.0000"
        }))
        .send_request(&app)
        .await;
    let item_b: OrderLineItem = test::read_body_json(resp).await;

    // Try to delete order_b's item using order_a's ID
    let resp = test::TestRequest::delete()
        .uri(&format!("/api/orders/{}/items/{}", order_a.id, item_b.id))
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 404);
}
