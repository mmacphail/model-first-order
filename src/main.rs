use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use actix_web_prom::PrometheusMetricsBuilder;
use tracing::info;
use tracing_actix_web::TracingLogger;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use order_api::db;
use order_api::openapi::ApiDoc;
use order_api::routes;

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let host = std::env::var("HOST").unwrap_or_else(|_| "127.0.0.1".into());
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse()
        .expect("PORT must be a number");

    let pool = db::init_pool(&database_url);
    db::run_migrations(&pool);

    info!("Starting server at http://{host}:{port}");
    info!("Swagger UI at http://{host}:{port}/swagger-ui/");

    let cors_permissive = std::env::var("CORS_PERMISSIVE")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    HttpServer::new(move || {
        let cors = if cors_permissive {
            Cors::permissive()
        } else {
            Cors::default()
        };

        let prometheus = PrometheusMetricsBuilder::new("api")
            .endpoint("/metrics")
            .exclude("/metrics")
            .exclude("/health")
            .mask_unmatched_patterns("UNKNOWN")
            .build()
            .expect("Failed to initialize Prometheus metrics");

        App::new()
            .wrap(prometheus)
            .wrap(cors)
            .wrap(TracingLogger::default())
            .app_data(web::Data::new(pool.clone()))
            .configure(routes::configure)
            .service(
                SwaggerUi::new("/swagger-ui/{_:.*}")
                    .url("/api-docs/openapi.json", ApiDoc::openapi()),
            )
    })
    .bind((host.as_str(), port))?
    .run()
    .await
}
