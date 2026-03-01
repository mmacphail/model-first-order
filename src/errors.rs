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
