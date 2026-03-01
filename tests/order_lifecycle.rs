use actix_web::{test, web, App};
use diesel::prelude::*;
use order_api::db::{self, DbPool};
use order_api::models::order::Order;
use order_api::models::order_line_item::OrderLineItem;
use order_api::models::order_status::OrderStatus;
use order_api::models::outbox::OutboxEvent;
use order_api::routes;
use order_api::schema::{commerce_order_outbox, order_line_items, orders};
use uuid::Uuid;

fn setup_test_pool() -> DbPool {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL_TEST")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("DATABASE_URL_TEST or DATABASE_URL must be set for tests");
    let pool = db::init_pool(&database_url);
    db::run_migrations(&pool);
    pool
}

/// Deletes all test data. Tables are deleted in FK-dependency order:
/// order_line_items → orders (line items reference orders via FK).
fn cleanup(pool: &DbPool) {
    let mut conn = pool.get().expect("Failed to get connection for cleanup");
    diesel::delete(commerce_order_outbox::table)
        .execute(&mut conn)
        .expect("Failed to clean outbox");
    diesel::delete(order_line_items::table)
        .execute(&mut conn)
        .expect("Failed to clean line items");
    diesel::delete(orders::table)
        .execute(&mut conn)
        .expect("Failed to clean orders");
}

#[actix_web::test]
async fn test_order_lifecycle() {
    let pool = setup_test_pool();
    cleanup(&pool);

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
    let pool = setup_test_pool();
    cleanup(&pool);

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
    let pool = setup_test_pool();
    cleanup(&pool);

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
    test::TestRequest::post()
        .uri(&format!("/api/orders/{}/items", order.id))
        .set_json(serde_json::json!({
            "product_sku": "WIDGET-001",
            "quantity": 1,
            "unit_price": "10.0000"
        }))
        .send_request(&app)
        .await;

    // Confirm
    test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Confirmed" }))
        .send_request(&app)
        .await;

    // Ship
    test::TestRequest::patch()
        .uri(&format!("/api/orders/{}/status", order.id))
        .set_json(serde_json::json!({ "status": "Shipped" }))
        .send_request(&app)
        .await;

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
    let pool = setup_test_pool();
    cleanup(&pool);

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
    let pool = setup_test_pool();
    cleanup(&pool);

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
    let pool = setup_test_pool();
    cleanup(&pool);

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
