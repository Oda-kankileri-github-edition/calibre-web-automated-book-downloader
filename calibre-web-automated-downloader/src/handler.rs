use crate::app::AppError;
use axum::Json;

// How to take a query parameter for search term
pub async fn handler_search() -> Result<Json<String>, AppError> {
    Ok(Json("{}".to_string()))
}

// How to take a query parameter for MD5 id of the book
pub async fn handler_info() -> Result<Json<String>, AppError> {
    Ok(Json("{}".to_string()))
}

pub async fn handler_download() -> Result<Json<String>, AppError> {
    Ok(Json("{}".to_string()))
}

pub async fn handler_status() -> Result<Json<String>, AppError> {
    log::info!("Status request received");
    Ok(Json("{}".to_string()))
}

pub async fn handler_localdownload() -> Result<Json<String>, AppError> {
    Ok(Json("{}".to_string()))
}
