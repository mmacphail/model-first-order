use actix_web_prom::{PrometheusMetrics, PrometheusMetricsBuilder};

pub mod db;
pub mod errors;
pub mod handlers;
pub mod models;
pub mod openapi;
pub mod routes;
pub mod schema;
pub mod serializers;

pub fn build_prometheus() -> PrometheusMetrics {
    PrometheusMetricsBuilder::new("api")
        .endpoint("/metrics")
        .exclude("/metrics")
        .exclude("/health")
        .mask_unmatched_patterns("UNKNOWN")
        .build()
        .expect("Failed to initialize Prometheus metrics")
}
