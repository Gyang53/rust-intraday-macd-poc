// src/error.rs
use actix_web::{HttpResponse, ResponseError};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    pub success: bool,
    pub error: String,
    pub code: u16,
}

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Redis error: {0}")]
    Redis(String),

    #[error("HTTP client error: {0}")]
    Reqwest(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Data not found: {0}")]
    DataNotFound(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Internal server error")]
    Internal,
}

impl AppError {
    pub fn status_code(&self) -> u16 {
        match self {
            AppError::Database(_) => 500,
            AppError::Redis(_) => 500,
            AppError::Reqwest(_) => 502,
            AppError::Serialization(_) => 500,
            AppError::Config(_) => 500,
            AppError::DataNotFound(_) => 404,
            AppError::Validation(_) => 400,
            AppError::Internal => 500,
        }
    }
}

impl ResponseError for AppError {
    fn error_response(&self) -> HttpResponse {
        let status_code = self.status_code();
        let error_response = ApiErrorResponse {
            success: false,
            error: self.to_string(),
            code: status_code,
        };

        HttpResponse::build(
            actix_web::http::StatusCode::from_u16(status_code)
                .unwrap_or(actix_web::http::StatusCode::INTERNAL_SERVER_ERROR),
        )
        .json(error_response)
    }
}

impl From<anyhow::Error> for AppError {
    fn from(_err: anyhow::Error) -> Self {
        AppError::Internal
    }
}

impl From<config::ConfigError> for AppError {
    fn from(err: config::ConfigError) -> Self {
        AppError::Config(err.to_string())
    }
}

impl From<chrono::ParseError> for AppError {
    fn from(err: chrono::ParseError) -> Self {
        AppError::Validation(format!("Date parsing error: {}", err))
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<redis::RedisError> for AppError {
    fn from(err: redis::RedisError) -> Self {
        AppError::Redis(err.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        AppError::Reqwest(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Serialization(err.to_string())
    }
}

// Convenience type alias for Result
pub type Result<T> = std::result::Result<T, AppError>;
