pub mod static_files;
pub mod templates;

use axum::{
    extract::State,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use std::sync::Arc;
use sqlx::PgPool;

pub fn create_frontend_router() -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/explorer", get(serve_index))
}

async fn serve_index() -> impl IntoResponse {
    let html_content = include_str!("templates/index.html");
    Html(html_content)
}
