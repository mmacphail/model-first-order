use actix_web::{test, web, App};

#[actix_web::test]
async fn test_metrics_endpoint_returns_prometheus_data() {
    let prometheus = order_api::build_prometheus();

    let app = test::init_service(
        App::new()
            .wrap(prometheus)
            .route("/ping", web::get().to(|| async { "pong" })),
    )
    .await;

    // Make a request so the middleware records at least one metric
    let _ = test::TestRequest::get()
        .uri("/ping")
        .send_request(&app)
        .await;

    let resp = test::TestRequest::get()
        .uri("/metrics")
        .send_request(&app)
        .await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "text/plain; version=0.0.4; charset=utf-8"
    );

    let body = test::read_body(resp).await;
    let body_str = std::str::from_utf8(&body).expect("body is not valid UTF-8");
    assert!(
        body_str.contains("# HELP"),
        "expected Prometheus metrics body to contain '# HELP'"
    );
}
