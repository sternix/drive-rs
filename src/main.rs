mod config;
mod handlers;
mod middleware;
mod models;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::{
    Router,
    body::Body,
    http::{StatusCode, header},
    response::Response,
    routing::{delete, get, patch, post, put},
};
use deadpool_postgres::{Config, Pool, Runtime};
use include_dir::{Dir, include_dir};
use mime_guess::from_path;
use tokio::sync::{RwLock, broadcast};
use tokio_postgres::NoTls;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use config::AppConfig;

#[derive(Clone)]
pub struct AppState {
    pub db: Pool,
    pub config: AppConfig,
    pub transfer_channels: Arc<RwLock<HashMap<String, Arc<broadcast::Sender<String>>>>>,
}

// Derleme anında ui/build klasörünü binary'e ekle
static UI: Dir = include_dir!("$CARGO_MANIFEST_DIR/ui/build");

#[tokio::main]
async fn main() {
    // Load .env
    dotenvy::dotenv().ok();

    print_version();

    // Init tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "drive_rs=debug,tower_http=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app_config = AppConfig::from_env();

    // Database pool
    let mut pg_config = Config::new();
    pg_config.url = Some(app_config.database_url.clone());
    let db = pg_config
        .create_pool(Some(Runtime::Tokio1), NoTls)
        .expect("Failed to create database pool");

    // Run migrations
    {
        let client = db.get().await.expect("Failed to get DB connection");
        client
            .batch_execute(include_str!("../migrations/001_init.sql"))
            .await
            .expect("Failed to run migrations");
    }

    // Create upload directory
    tokio::fs::create_dir_all(&app_config.upload_dir)
        .await
        .expect("Failed to create upload directory");

    let state = AppState {
        db,
        config: app_config.clone(),
        transfer_channels: Arc::new(RwLock::new(HashMap::new())),
    };

    // CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Routes
    let auth_routes = Router::new()
        .route("/register", post(handlers::auth::register))
        .route("/login", post(handlers::auth::login))
        .route("/me", get(handlers::auth::me));

    let file_routes = Router::new()
        .route("/", get(handlers::files::list))
        .route("/upload", post(handlers::files::upload))
        .route("/{id}", get(handlers::files::download))
        .route("/{id}", delete(handlers::files::delete))
        .route("/{id}/rename", patch(handlers::files::rename))
        .route("/{id}/move", put(handlers::files::move_file))
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024 * 1024)); // 5GB limit

    let folder_routes = Router::new()
        .route("/", get(handlers::folders::list))
        .route("/", post(handlers::folders::create))
        .route("/{id}", get(handlers::folders::get))
        .route("/{id}", delete(handlers::folders::delete))
        .route("/{id}/rename", patch(handlers::folders::rename));

    let share_routes = Router::new()
        .route("/", post(handlers::share::create_link))
        .route("/{token}", get(handlers::share::get_share_info))
        .route("/{token}/download", get(handlers::share::download_shared));

    let transfer_routes = Router::new()
        .route("/", post(handlers::transfer::create_session))
        .route("/{token}", get(handlers::transfer::get_session))
        .route("/ws/{token}", get(handlers::transfer::ws_handler));

    let app = Router::new()
        .nest("/api/auth", auth_routes)
        .nest("/api/files", file_routes)
        .nest("/api/folders", folder_routes)
        .nest("/api/share", share_routes)
        .nest("/api/transfer", transfer_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", app_config.host, app_config.port);
    tracing::info!("Server starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind");

    axum::serve(listener, app).await.expect("Server error");
}

fn print_version() {
    use chrono::{DateTime, Local};

    let commit_branch = env!("VERGEN_GIT_BRANCH");
    let commit_hash = env!("VERGEN_GIT_SHA");
    const GIT_TIMESTAMP: &str = env!("VERGEN_GIT_COMMIT_TIMESTAMP");

    let commit_date = if let Ok(parsed_date) = DateTime::parse_from_rfc3339(GIT_TIMESTAMP) {
        // 2. Yerel saate veya Utc'ye çevir
        let local_date = parsed_date.with_timezone(&Local);

        // 3. İstediğin gibi formatla (Örn: 04 May 2026 11:04)
        local_date.format("%d %b %Y %H:%M").to_string()
    } else {
        // Eğer parse başarısız olursa ham halini göster
        GIT_TIMESTAMP.to_string()
    };

    println!("Commit Branch: {commit_branch}");
    println!("Commit Hash: {commit_hash}");
    println!("Commit Tarihi: {commit_date}");
}

async fn serve_static(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Önce tam eşleşme dene
    let file = UI
        .get_file(path)
        // Bulamazsan SPA fallback: index.html
        .or_else(|| UI.get_file("index.html"));

    match file {
        Some(f) => {
            let mime = from_path(f.path()).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(f.contents()))
                .unwrap()
        }
        None => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("404"))
            .unwrap(),
    }
}
