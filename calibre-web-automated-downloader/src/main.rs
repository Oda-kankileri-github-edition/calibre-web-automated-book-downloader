use std::sync::Arc;

mod config;
mod app;
mod handler;

use config::CONFIG;
use axum::{routing::get, Router};
use handler::handler_home;

#[tokio::main]
async fn main() {
    // Access configuration settings using the global CONFIG instance
    println!("Base Directory: {:?}", CONFIG.base_dir);

    // Initialize the App State
    let app_state = Arc::new(app::AppState::new());
    // build our application with a route
    let app = Router::new().route("/", get(handler_home)).with_state(app_state);

    // run it
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}


