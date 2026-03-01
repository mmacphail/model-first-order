use actix_web::{test, web, App};
use order_api::db::{self, DbPool};
use order_api::models::order::Order;
use order_api::models::order_line_item::OrderLineItem;
use order_api::models::order_status::OrderStatus;
use order_api::routes;

fn setup_test_pool() -> DbPool {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for tests");
    let pool = db::init_pool(&database_url);
    db::run_migrations(&pool);
    pool
}

#[actix_web::test]
async fn test_order_lifecycle() {
    let pool = setup_test_pool();

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
