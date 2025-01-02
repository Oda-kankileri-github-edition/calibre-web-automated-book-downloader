mod app;
mod book_manager;
mod config;
mod handler;
mod models;
mod network;

use axum::{routing::get, Router};
use config::CONFIG;
use tower_http::services::{ServeDir, ServeFile};

#[tokio::main]
async fn main() {
    // Access configuration settings using the global CONFIG instance
    println!("Base Directory: {:?}", CONFIG.base_dir);

    // Build our application with routes and static files
    let root_app = Router::new()
        .route("/info", get(handler::handler_info))
        .route("/search", get(handler::handler_search))
        .route("/download", get(handler::handler_download))
        .route("/status", get(handler::handler_status))
        .route("/localdownload", get(handler::handler_localdownload));
    let app = Router::new()
        // How to make this router to handler mapping better?
        .route_service("/", ServeFile::new("../static/index.html"))
        .nest("/api", root_app.clone())
        .nest("/request/api", root_app)
        .nest_service("/static", ServeDir::new("../static"));

    // run it
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    println!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}
