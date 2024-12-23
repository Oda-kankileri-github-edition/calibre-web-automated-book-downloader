use axum::{http::StatusCode, response::{IntoResponse, Response}};
use minijinja::Environment;

// Create a AppState
pub struct AppState {
    pub templating_env: Environment<'static>,
}

impl AppState {
    pub fn new() -> Self {
        // Create a new templating environment
        let mut templating_env = Environment::new();
        templating_env.add_template("index", include_str!("../../templates/index.html"))
        .unwrap();
        Self { templating_env }
    }
}

// Make our own error that wraps `anyhow::Error`.
pub struct AppError(anyhow::Error);

// Tell axum how to convert `AppError` into a response.
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Something went wrong: {}", self.0),
        )
            .into_response()
    }
}

// This enables using `?` on functions that return `Result<_, anyhow::Error>` to turn them into
// `Result<_, AppError>`. That way you don't need to do that manually.
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}