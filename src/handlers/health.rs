use actix_web::{web, HttpResponse};
use diesel::prelude::*;
use serde::Serialize;
use tracing::warn;
use utoipa::ToSchema;

use crate::db::DbPool;

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    status: String,
    database: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Service healthy", body = HealthResponse,
            example = json!({"status": "ok", "database": "ok"})),
        (status = 503, description = "Service degraded", body = HealthResponse,
            example = json!({"status": "degraded", "database": "error", "detail": "database unreachable"})),
    ),
    tag = "Health"
)]
pub async fn health(pool: web::Data<DbPool>) -> HttpResponse {
    let result = web::block(move || {
        let mut conn = pool.get().map_err(|e| e.to_string())?;
        diesel::sql_query("SELECT 1")
            .execute(&mut conn)
            .map_err(|e| e.to_string())?;
        Ok::<(), String>(())
    })
    .await;

    match result {
        Ok(Ok(())) => HttpResponse::Ok().json(HealthResponse {
            status: "ok".into(),
            database: "ok".into(),
            detail: None,
        }),
        Ok(Err(db_err)) => {
            warn!(error = %db_err, "Health check: database probe failed");
            HttpResponse::ServiceUnavailable().json(HealthResponse {
                status: "degraded".into(),
                database: "error".into(),
                detail: Some("database unreachable".into()),
            })
        }
        Err(blocking_err) => {
            warn!(error = %blocking_err, "Health check: blocking task failed");
            HttpResponse::ServiceUnavailable().json(HealthResponse {
                status: "degraded".into(),
                database: "error".into(),
                detail: Some("database unreachable".into()),
            })
        }
    }
}
