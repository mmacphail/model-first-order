use actix_web::{HttpResponse, ResponseError};
use std::fmt;

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    BadRequest(String),
    Conflict(String),
    Internal(String),
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ApiError::NotFound => write!(f, "Not found"),
            ApiError::BadRequest(msg) => write!(f, "Bad request: {msg}"),
            ApiError::Conflict(msg) => write!(f, "Conflict: {msg}"),
            ApiError::Internal(msg) => write!(f, "Internal error: {msg}"),
        }
    }
}

impl ResponseError for ApiError {
    fn error_response(&self) -> HttpResponse {
        let body = serde_json::json!({ "error": self.to_string() });
        match self {
            ApiError::NotFound => HttpResponse::NotFound().json(body),
            ApiError::BadRequest(_) => HttpResponse::BadRequest().json(body),
            ApiError::Conflict(_) => HttpResponse::Conflict().json(body),
            ApiError::Internal(_) => HttpResponse::InternalServerError().json(body),
        }
    }
}

impl From<diesel::result::Error> for ApiError {
    fn from(err: diesel::result::Error) -> Self {
        match err {
            diesel::result::Error::NotFound => ApiError::NotFound,
            _ => ApiError::Internal(err.to_string()),
        }
    }
}

impl From<diesel::r2d2::PoolError> for ApiError {
    fn from(err: diesel::r2d2::PoolError) -> Self {
        ApiError::Internal(err.to_string())
    }
}

impl From<actix_web::error::BlockingError> for ApiError {
    fn from(err: actix_web::error::BlockingError) -> Self {
        ApiError::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Display ────────────────────────────────────────────────────────────────

    #[test]
    fn test_display_not_found() {
        assert_eq!(ApiError::NotFound.to_string(), "Not found");
    }

    #[test]
    fn test_display_bad_request() {
        let err = ApiError::BadRequest("missing field".into());
        assert_eq!(err.to_string(), "Bad request: missing field");
    }

    #[test]
    fn test_display_conflict() {
        let err = ApiError::Conflict("duplicate key".into());
        assert_eq!(err.to_string(), "Conflict: duplicate key");
    }

    #[test]
    fn test_display_internal() {
        let err = ApiError::Internal("db connection lost".into());
        assert_eq!(err.to_string(), "Internal error: db connection lost");
    }

    // ── From<diesel::result::Error> ────────────────────────────────────────────

    #[test]
    fn test_from_diesel_not_found_maps_to_api_not_found() {
        let diesel_err = diesel::result::Error::NotFound;
        let api_err = ApiError::from(diesel_err);
        assert!(matches!(api_err, ApiError::NotFound));
    }

    #[test]
    fn test_from_diesel_other_error_maps_to_api_internal() {
        let diesel_err = diesel::result::Error::RollbackTransaction;
        let api_err = ApiError::from(diesel_err);
        assert!(matches!(api_err, ApiError::Internal(_)));
    }

    // ── From<BlockingError> ─────────────────────────────────────────────────────

    #[actix_web::test]
    async fn test_from_blocking_error_maps_to_api_internal() {
        let result = actix_web::web::block(|| {
            panic!("intentional panic to produce BlockingError");
        })
        .await;
        let blocking_err = result.unwrap_err();
        let api_err = ApiError::from(blocking_err);
        assert!(matches!(api_err, ApiError::Internal(_)));
    }

    // ── ResponseError — HTTP status codes ─────────────────────────────────────

    #[test]
    fn test_error_response_not_found_is_404() {
        use actix_web::ResponseError;
        let resp = ApiError::NotFound.error_response();
        assert_eq!(resp.status(), actix_web::http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_error_response_bad_request_is_400() {
        use actix_web::ResponseError;
        let resp = ApiError::BadRequest("bad".into()).error_response();
        assert_eq!(resp.status(), actix_web::http::StatusCode::BAD_REQUEST);
    }

    #[test]
    fn test_error_response_conflict_is_409() {
        use actix_web::ResponseError;
        let resp = ApiError::Conflict("conflict".into()).error_response();
        assert_eq!(resp.status(), actix_web::http::StatusCode::CONFLICT);
    }

    #[test]
    fn test_error_response_internal_is_500() {
        use actix_web::ResponseError;
        let resp = ApiError::Internal("oops".into()).error_response();
        assert_eq!(
            resp.status(),
            actix_web::http::StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
