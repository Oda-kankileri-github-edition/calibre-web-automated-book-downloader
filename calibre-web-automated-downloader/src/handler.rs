use axum::extract::State;
use axum::response::Html;
use minijinja::context;
use std::sync::Arc;
use crate::app::{AppError, AppState};

pub async fn handler_home(State(state): State<Arc<AppState>>) -> Result<Html<String>, AppError> {
    let template = state.templating_env.get_template("index").unwrap();

    let rendered = template
        .render(context! {})
        .unwrap();

    Ok(Html(rendered))
}