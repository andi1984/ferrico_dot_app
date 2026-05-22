#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod db;
mod error;

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use db::{
    Bookmark, CreateBookmarkInput, Folder, Tag,
    db_add_bookmark, db_add_folder, db_add_tag,
    db_delete_bookmark, db_delete_folder, db_delete_tag,
    db_export_opml,
    db_get_bookmark_count, db_get_bookmarks, db_get_folders, db_get_tags,
    now, open_db,
};
use error::AppError;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

// ─── App State ────────────────────────────────────────────────────────────────

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub api_token: String,
}

// ─── Notifier ─────────────────────────────────────────────────────────────────

pub(crate) trait BookmarkNotifier: Send + Sync {
    fn notify(&self);
}

struct TauriNotifier(AppHandle);

impl BookmarkNotifier for TauriNotifier {
    fn notify(&self) {
        self.0.emit("bookmark-added", ()).ok();
    }
}

#[derive(Clone)]
struct HttpState {
    db: Arc<Mutex<Connection>>,
    token: String,
    notifier: Arc<dyn BookmarkNotifier>,
}

// ─── Lock Helper ──────────────────────────────────────────────────────────────

macro_rules! lock_db {
    ($state:expr) => {
        $state
            .db
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?
    };
}

// ─── Tauri Commands ──────────────────────────────────────────────────────────

#[tauri::command]
fn get_bookmarks(
    folder_id: Option<String>,
    tag_id: Option<String>,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<Bookmark>, AppError> {
    let db = lock_db!(state);
    db_get_bookmarks(&db, folder_id.as_deref(), tag_id.as_deref(), search.as_deref())
}

#[tauri::command]
fn get_bookmark_count(state: State<'_, AppState>) -> Result<i64, AppError> {
    let db = lock_db!(state);
    db_get_bookmark_count(&db)
}

#[tauri::command]
fn add_bookmark(input: CreateBookmarkInput, state: State<'_, AppState>) -> Result<Bookmark, AppError> {
    let db = lock_db!(state);
    db_add_bookmark(&db, input)
}

#[tauri::command]
fn delete_bookmark(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_delete_bookmark(&db, &id)
}

#[tauri::command]
fn get_folders(state: State<'_, AppState>) -> Result<Vec<Folder>, AppError> {
    let db = lock_db!(state);
    db_get_folders(&db)
}

#[tauri::command]
fn add_folder(
    name: String,
    parent_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Folder, AppError> {
    let db = lock_db!(state);
    db_add_folder(&db, name, parent_id)
}

#[tauri::command]
fn delete_folder(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_delete_folder(&db, &id)
}

#[tauri::command]
fn get_tags(state: State<'_, AppState>) -> Result<Vec<Tag>, AppError> {
    let db = lock_db!(state);
    db_get_tags(&db)
}

#[tauri::command]
fn add_tag(name: String, color: String, state: State<'_, AppState>) -> Result<Tag, AppError> {
    let db = lock_db!(state);
    db_add_tag(&db, name, color)
}

#[tauri::command]
fn delete_tag(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_delete_tag(&db, &id)
}

#[tauri::command]
fn get_api_token(state: State<'_, AppState>) -> String {
    state.api_token.clone()
}

#[tauri::command]
fn export_opml(state: State<'_, AppState>) -> Result<String, AppError> {
    let db = lock_db!(state);
    db_export_opml(&db)
}

#[tauri::command]
fn open_url(url: String) -> Result<(), AppError> {
    open::that(url).map_err(|e| AppError::Validation { message: e.to_string() })
}

// ─── HTTP Server (browser extension endpoint) ─────────────────────────────────

#[derive(Deserialize)]
struct ExtPayload {
    url: String,
    title: String,
    description: Option<String>,
    favicon_url: Option<String>,
    feed_url: Option<String>,
    folder_id: Option<String>,
}

async fn http_add_bookmark(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
    Json(body): Json<ExtPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let auth = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    if auth != Some(state.token.as_str()) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if body.url == "__ping__" && body.title == "__ping__" {
        let ts = now();
        return Ok(Json(serde_json::json!({ "id": "ping", "created_at": ts })));
    }

    let db = state
        .db
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let id = Uuid::new_v4().to_string();
    let ts = now();

    db.execute(
        "INSERT INTO bookmarks \
         (id, url, title, description, favicon_url, feed_url, folder_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            id, body.url, body.title, body.description,
            body.favicon_url, body.feed_url, body.folder_id, ts, ts
        ],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    drop(db);
    state.notifier.notify();

    Ok(Json(serde_json::json!({ "id": id, "created_at": ts })))
}

async fn start_http_server(db: Arc<Mutex<Connection>>, token: String, app_handle: AppHandle) {
    let notifier = Arc::new(TauriNotifier(app_handle)) as Arc<dyn BookmarkNotifier>;
    let state = HttpState { db, token, notifier };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/bookmarks", post(http_add_bookmark))
        .with_state(state)
        .layer(cors);

    match tokio::net::TcpListener::bind("127.0.0.1:59432").await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("HTTP server error: {e}");
            }
        }
        Err(e) => eprintln!("HTTP server failed to bind 127.0.0.1:59432: {e}"),
    }
}

// ─── Settings ─────────────────────────────────────────────────────────────────

fn load_or_create_token(data_dir: &PathBuf) -> String {
    let path = data_dir.join("settings.json");
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(t) = v["api_token"].as_str() {
                return t.to_string();
            }
        }
    }
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let token: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({ "api_token": token })).unwrap(),
    )
    .ok();
    token
}

// ─── Main ─────────────────────────────────────────────────────────────────────

fn main() {
    let data_dir = dirs::data_dir()
        .map(|d| d.join("ferrico"))
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&data_dir).ok();

    let conn = open_db(&data_dir).expect("init db");
    let api_token = load_or_create_token(&data_dir);
    let db = Arc::new(Mutex::new(conn));

    tauri::Builder::default()
        .manage(AppState {
            db: db.clone(),
            api_token: api_token.clone(),
        })
        .setup(move |app| {
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(start_http_server(db.clone(), api_token.clone(), handle));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bookmarks,
            get_bookmark_count,
            add_bookmark,
            delete_bookmark,
            get_folders,
            add_folder,
            delete_folder,
            get_tags,
            add_tag,
            delete_tag,
            get_api_token,
            export_opml,
            open_url,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod http_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::post;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tower::ServiceExt;

    struct MockNotifier {
        called: Arc<AtomicBool>,
    }

    impl BookmarkNotifier for MockNotifier {
        fn notify(&self) {
            self.called.store(true, Ordering::Relaxed);
        }
    }

    fn make_state(token: &str) -> (HttpState, Arc<AtomicBool>) {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        db::init_schema(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let called = Arc::new(AtomicBool::new(false));
        let notifier = Arc::new(MockNotifier { called: called.clone() }) as Arc<dyn BookmarkNotifier>;
        (HttpState { db, token: token.to_string(), notifier }, called)
    }

    #[tokio::test]
    async fn notifies_frontend_after_successful_bookmark_add() {
        let (state, called) = make_state("secret");
        let app = axum::Router::new()
            .route("/bookmarks", post(http_add_bookmark))
            .with_state(state);

        let req = Request::builder()
            .method("POST")
            .uri("/bookmarks")
            .header("content-type", "application/json")
            .header("authorization", "Bearer secret")
            .body(Body::from(r#"{"url":"https://example.com","title":"Test"}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(called.load(Ordering::Relaxed), "notifier must be called on success");
    }

    #[tokio::test]
    async fn does_not_notify_on_auth_failure() {
        let (state, called) = make_state("secret");
        let app = axum::Router::new()
            .route("/bookmarks", post(http_add_bookmark))
            .with_state(state);

        let req = Request::builder()
            .method("POST")
            .uri("/bookmarks")
            .header("content-type", "application/json")
            .header("authorization", "Bearer wrong-token")
            .body(Body::from(r#"{"url":"https://example.com","title":"Test"}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(!called.load(Ordering::Relaxed), "notifier must not be called on auth failure");
    }
}
