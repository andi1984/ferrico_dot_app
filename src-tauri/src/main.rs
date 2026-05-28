#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod db;
mod error;
mod health_check;
mod io;
mod io_validate;

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use db::{
    Bookmark, CreateBookmarkInput, Folder, ImportResult, ImportRowInput,
    InboxSortAssignment, InboxSortResult, Tag,
    db_add_bookmark, db_add_folder, db_add_tag,
    db_apply_inbox_sort, db_clear_all_data,
    db_delete_bookmark, db_delete_bookmarks, db_delete_folder, db_delete_tag,
    db_empty_bin, db_get_bin_bookmarks, db_get_bin_count,
    db_get_broken_bookmarks, db_get_broken_count,
    db_get_inbox_count, db_get_urls_for_health_check, db_import_bookmarks,
    db_move_bookmark, db_permanently_delete_bookmark,
    db_purge_expired_bin, db_restore_bookmark,
    db_get_bookmark_count, db_get_bookmarks, db_get_folders, db_get_tags,
    db_find_duplicate_bookmarks, db_merge_bookmark_duplicates,
    db_update_bookmark_health,
    now, open_db,
};
use error::AppError;
use rand::RngCore;
use rand::rngs::OsRng;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, State};
use tower_http::cors::{Any, CorsLayer};

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
    inbox_only: Option<bool>,
    state: State<'_, AppState>,
) -> Result<Vec<Bookmark>, AppError> {
    let db = lock_db!(state);
    db_get_bookmarks(
        &db,
        folder_id.as_deref(),
        tag_id.as_deref(),
        search.as_deref(),
        inbox_only.unwrap_or(false),
    )
}

#[tauri::command]
fn get_inbox_count(state: State<'_, AppState>) -> Result<i64, AppError> {
    let db = lock_db!(state);
    db_get_inbox_count(&db)
}

#[tauri::command]
fn apply_inbox_sort(
    assignments: Vec<InboxSortAssignment>,
    state: State<'_, AppState>,
) -> Result<InboxSortResult, AppError> {
    let db = lock_db!(state);
    db_apply_inbox_sort(&db, assignments)
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
fn get_bin_bookmarks(state: State<'_, AppState>) -> Result<Vec<Bookmark>, AppError> {
    let db = lock_db!(state);
    db_get_bin_bookmarks(&db)
}

#[tauri::command]
fn get_bin_count(state: State<'_, AppState>) -> Result<i64, AppError> {
    let db = lock_db!(state);
    db_get_bin_count(&db)
}

#[tauri::command]
fn restore_bookmark(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_restore_bookmark(&db, &id)
}

#[tauri::command]
fn move_bookmark(id: String, folder_id: Option<String>, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_move_bookmark(&db, &id, folder_id.as_deref())
}

#[tauri::command]
fn permanently_delete_bookmark(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_permanently_delete_bookmark(&db, &id)
}

#[tauri::command]
fn empty_bin(state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_empty_bin(&db)
}

#[tauri::command]
fn purge_expired_bin(days: i64, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_purge_expired_bin(&db, days)
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
    // Delegate to io.rs so all format logic lives in one place.
    io::export_opml(&db)
}

// ─── Import / Export (JSON, Netscape HTML, OPML) ─────────────────────────────

#[tauri::command]
fn export_json(state: State<'_, AppState>) -> Result<String, AppError> {
    let db = lock_db!(state);
    io::export_json(&db)
}

#[tauri::command]
fn import_json(json: String, state: State<'_, AppState>) -> Result<ImportResult, AppError> {
    let db = lock_db!(state);
    io::import_json(&db, &json)
}

#[tauri::command]
fn export_netscape_html(state: State<'_, AppState>) -> Result<String, AppError> {
    let db = lock_db!(state);
    io::export_netscape_html(&db)
}

#[tauri::command]
fn import_netscape_html(html: String, state: State<'_, AppState>) -> Result<ImportResult, AppError> {
    let db = lock_db!(state);
    io::import_netscape_html(&db, &html)
}

#[tauri::command]
fn import_opml(xml: String, state: State<'_, AppState>) -> Result<ImportResult, AppError> {
    let db = lock_db!(state);
    io::import_opml(&db, &xml)
}

#[tauri::command]
fn export_csv(state: State<'_, AppState>) -> Result<String, AppError> {
    let db = lock_db!(state);
    io::export_csv(&db)
}

#[tauri::command]
fn open_url(url: String) -> Result<(), AppError> {
    open::that(url).map_err(|e| AppError::Validation { message: e.to_string() })
}

#[tauri::command]
fn clear_all_data(state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_clear_all_data(&db)
}

// ─── Claude CLI Helper ────────────────────────────────────────────────────────

async fn run_claude(prompt: &str) -> Result<String, AppError> {
    use tokio::io::AsyncWriteExt;

    let home = std::env::var("HOME").unwrap_or_default();
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let extended_path =
        format!("{home}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:{existing_path}");

    // Pass prompt via stdin to avoid E2BIG when prompt exceeds ARG_MAX
    let mut child = tokio::process::Command::new("claude")
        .arg("-p")
        .arg("-")
        .env("PATH", &extended_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| AppError::Validation {
            message: format!("claude CLI not found: {e}"),
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prompt.as_bytes())
            .await
            .map_err(|e| AppError::Validation {
                message: format!("failed to write to claude stdin: {e}"),
            })?;
    }

    let output = child.wait_with_output().await.map_err(|e| AppError::Validation {
        message: format!("claude process error: {e}"),
    })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(AppError::Validation {
            message: format!(
                "claude exited {:?} — stderr: {} stdout: {}",
                output.status.code(),
                stderr.trim(),
                stdout.trim()
            ),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

// ─── Inbox Sort ───────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct InboxBookmarkInput {
    id: String,
    url: String,
    title: String,
    description: Option<String>,
}

#[derive(serde::Serialize)]
struct InboxSortSuggestion {
    bookmark_id: String,
    folder_name: String,
}

#[tauri::command]
async fn suggest_inbox_sort(
    bookmarks: Vec<InboxBookmarkInput>,
    folder_names: Vec<String>,
) -> Result<Vec<InboxSortSuggestion>, AppError> {
    if bookmarks.is_empty() {
        return Ok(vec![]);
    }

    let folders_list = if folder_names.is_empty() {
        "none yet — suggest new folders as needed".to_string()
    } else {
        folder_names.join(", ")
    };

    let bookmarks_text: String = bookmarks
        .iter()
        .enumerate()
        .map(|(i, b)| {
            let desc = b.description.as_deref().unwrap_or("");
            format!("{}. id={} title={:?} url={:?} desc={:?}", i + 1, b.id, b.title, b.url, desc)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "You are helping a user sort their inbox bookmarks into folders.\n\n\
         Existing folders: {folders_list}\n\n\
         Bookmarks to sort:\n{bookmarks_text}\n\n\
         For each bookmark, suggest the best folder. Prefer existing folders when a good match \
         exists. Suggest a new folder name only when no existing folder fits.\n\
         Keep folder names short (1-3 words), title-cased.\n\n\
         Respond ONLY with a JSON array, one object per bookmark:\n\
         [{{\"bookmark_id\": \"<id>\", \"folder_name\": \"<folder>\"}}, ...]\n\
         Include every bookmark exactly once."
    );

    let raw = run_claude(&prompt).await?;
    let trimmed = raw.trim();
    let json_str = if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']')) {
        &trimmed[start..=end]
    } else {
        return Err(AppError::Validation {
            message: "Could not find JSON array in Claude response".into(),
        });
    };

    let value: Vec<serde_json::Value> =
        serde_json::from_str(json_str).map_err(|e| AppError::Validation {
            message: format!("Could not parse Claude response: {e}"),
        })?;

    let suggestions: Vec<InboxSortSuggestion> = value
        .into_iter()
        .filter_map(|obj| {
            let bookmark_id = obj["bookmark_id"].as_str()?.to_string();
            let folder_name = obj["folder_name"].as_str()?.to_string();
            if bookmark_id.is_empty() || folder_name.is_empty() {
                return None;
            }
            Some(InboxSortSuggestion { bookmark_id, folder_name })
        })
        .collect();

    Ok(suggestions)
}

// ─── CSV Import ───────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct CsvFieldMapping {
    url: Option<String>,
    title: Option<String>,
    description: Option<String>,
    favicon_url: Option<String>,
    feed_url: Option<String>,
    folder_name: Option<String>,
    tag_names: Option<String>,
}

fn extract_json(text: &str) -> &str {
    let trimmed = text.trim();
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if end >= start {
            return &trimmed[start..=end];
        }
    }
    trimmed
}

#[tauri::command]
async fn suggest_csv_mapping(
    headers: Vec<String>,
    sample_rows: Vec<Vec<String>>,
) -> Result<CsvFieldMapping, AppError> {
    let headers_list = headers.join(", ");
    let mut sample_text = String::new();
    for (i, row) in sample_rows.iter().take(5).enumerate() {
        let pairs: Vec<String> = headers
            .iter()
            .zip(row.iter())
            .map(|(h, v)| format!("{h}: {v:?}"))
            .collect();
        sample_text.push_str(&format!("Row {}: {{{}}}\n", i + 1, pairs.join(", ")));
    }

    let prompt = format!(
        "Map CSV columns to bookmark fields.\n\n\
         Bookmark fields:\n\
         - url (required): Web URL\n\
         - title (required): Display title\n\
         - description (optional): Notes or summary\n\
         - favicon_url (optional): Favicon/icon URL\n\
         - feed_url (optional): RSS/Atom feed URL\n\
         - folder_name (optional): Folder or category name (single value)\n\
         - tag_names (optional): Tags, possibly comma-separated in one cell\n\n\
         CSV columns: {headers_list}\n\n\
         Sample data:\n{sample_text}\n\
         Respond ONLY with a JSON object mapping each field to the best matching \
         CSV column name, or null if no match:\n\
         {{\"url\": \"col_or_null\", \"title\": \"col_or_null\", \
         \"description\": null, \"favicon_url\": null, \"feed_url\": null, \
         \"folder_name\": null, \"tag_names\": null}}"
    );

    let raw = run_claude(&prompt).await?;
    let json_str = extract_json(&raw);

    let value: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| AppError::Validation {
            message: format!("Could not parse Claude response: {e}"),
        })?;

    // Only accept values that are actual CSV header names; discard anything else
    let pick = |key: &str| -> Option<String> {
        value[key]
            .as_str()
            .filter(|v| headers.iter().any(|h| h == v))
            .map(String::from)
    };

    Ok(CsvFieldMapping {
        url: pick("url"),
        title: pick("title"),
        description: pick("description"),
        favicon_url: pick("favicon_url"),
        feed_url: pick("feed_url"),
        folder_name: pick("folder_name"),
        tag_names: pick("tag_names"),
    })
}

#[tauri::command]
fn import_bookmarks(
    inputs: Vec<ImportRowInput>,
    state: State<'_, AppState>,
) -> Result<ImportResult, AppError> {
    let db = lock_db!(state);
    db_import_bookmarks(&db, inputs)
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
    tag_ids: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct ExtFolderPayload {
    name: String,
    parent_id: Option<String>,
}

#[derive(Deserialize)]
struct ExtTagPayload {
    name: String,
    color: String,
}

fn auth_ok(headers: &HeaderMap, token: &str) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        == Some(token)
}

async fn http_get_folders(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Folder>>, StatusCode> {
    if !auth_ok(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db_get_folders(&db).map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn http_get_tags(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
) -> Result<Json<Vec<Tag>>, StatusCode> {
    if !auth_ok(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db_get_tags(&db).map(Json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn http_add_folder(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
    Json(body): Json<ExtFolderPayload>,
) -> Result<Json<Folder>, StatusCode> {
    if !auth_ok(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db_add_folder(&db, body.name, body.parent_id)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn http_add_tag(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
    Json(body): Json<ExtTagPayload>,
) -> Result<Json<Tag>, StatusCode> {
    if !auth_ok(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db_add_tag(&db, body.name, body.color)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn http_add_bookmark(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
    Json(body): Json<ExtPayload>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth_ok(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    if body.url == "__ping__" && body.title == "__ping__" {
        let ts = now();
        return Ok(Json(serde_json::json!({ "id": "ping", "created_at": ts })));
    }

    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let bookmark = db_add_bookmark(&db, CreateBookmarkInput {
        url: body.url,
        title: body.title,
        description: body.description,
        favicon_url: body.favicon_url,
        feed_url: body.feed_url,
        folder_id: body.folder_id,
        tag_ids: body.tag_ids,
    })
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    drop(db);
    state.notifier.notify();

    Ok(Json(serde_json::json!({ "id": bookmark.id, "created_at": bookmark.created_at })))
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
        .route("/folders", get(http_get_folders).post(http_add_folder))
        .route("/tags", get(http_get_tags).post(http_add_tag))
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

fn load_or_create_token(data_dir: &Path) -> String {
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
            get_inbox_count,
            add_bookmark,
            delete_bookmark,
            get_bin_bookmarks,
            get_bin_count,
            restore_bookmark,
            move_bookmark,
            permanently_delete_bookmark,
            empty_bin,
            purge_expired_bin,
            get_folders,
            add_folder,
            delete_folder,
            get_tags,
            add_tag,
            delete_tag,
            get_api_token,
            export_opml,
            export_json,
            import_json,
            export_netscape_html,
            import_netscape_html,
            import_opml,
            export_csv,
            open_url,
            clear_all_data,
            suggest_csv_mapping,
            import_bookmarks,
            suggest_inbox_sort,
            apply_inbox_sort,
            find_duplicate_bookmarks,
            merge_bookmark_duplicates,
            suggest_duplicate_resolution,
            scan_broken_bookmarks,
            get_broken_bookmarks,
            get_broken_count,
            delete_bookmarks,
            read_text_file,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// ─── Health Check ─────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct ScanResult {
    total: usize,
    broken: usize,
}

#[tauri::command]
async fn scan_broken_bookmarks(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ScanResult, AppError> {
    let urls = {
        let db = lock_db!(state);
        db_get_urls_for_health_check(&db)?
    };

    let total = urls.len();
    if total == 0 {
        return Ok(ScanResult { total: 0, broken: 0 });
    }

    let client = health_check::build_client()
        .map_err(|e| AppError::Validation { message: e.to_string() })?;

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(10));
    let mut set = tokio::task::JoinSet::new();

    for (id, url) in urls {
        let client = client.clone();
        let sem = sem.clone();
        set.spawn(async move {
            // acquire_owned only fails if the semaphore is explicitly closed, which never
            // happens here — using expect so a logic error surfaces rather than silently
            // dropping check results.
            let _permit = sem.acquire_owned().await.expect("semaphore should not close");
            health_check::check_url(&client, id, url).await
        });
    }

    let mut results = Vec::new();
    let mut completed = 0usize;

    while let Some(task_result) = set.join_next().await {
        if let Ok(check) = task_result {
            results.push(check);
        }
        completed += 1;
        app.emit("health-check-progress", serde_json::json!({
            "current": completed,
            "total": total,
        }))
        .ok();
    }

    let mut broken = 0usize;
    {
        let db = lock_db!(state);
        for r in &results {
            db_update_bookmark_health(&db, &r.id, r.is_broken, r.last_checked_at)?;
            if r.is_broken {
                broken += 1;
            }
        }
    }

    // Report how many bookmarks were actually checked (results.len()), not the original
    // URL count — they differ when tasks panic and are caught by JoinSet.
    Ok(ScanResult { total: results.len(), broken })
}

#[tauri::command]
fn get_broken_bookmarks(state: State<'_, AppState>) -> Result<Vec<Bookmark>, AppError> {
    let db = lock_db!(state);
    db_get_broken_bookmarks(&db)
}

#[tauri::command]
fn get_broken_count(state: State<'_, AppState>) -> Result<i64, AppError> {
    let db = lock_db!(state);
    db_get_broken_count(&db)
}

#[tauri::command]
fn delete_bookmarks(ids: Vec<String>, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_delete_bookmarks(&db, &ids)
}

// ─── Deduplication ────────────────────────────────────────────────────────────

#[tauri::command]
fn find_duplicate_bookmarks(state: State<'_, AppState>) -> Result<Vec<Vec<Bookmark>>, AppError> {
    let db = lock_db!(state);
    db_find_duplicate_bookmarks(&db)
}

#[derive(serde::Deserialize)]
struct MergeInput {
    keeper_id: String,
    discard_ids: Vec<String>,
}

#[tauri::command]
fn merge_bookmark_duplicates(input: MergeInput, state: State<'_, AppState>) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_merge_bookmark_duplicates(&db, &input.keeper_id, &input.discard_ids)
}

// Each bookmark in a duplicate group — no url (same for all entries in group)
#[derive(serde::Deserialize)]
struct DupBookmarkInput {
    id: String,
    title: String,
    description: Option<String>,
}

#[derive(serde::Deserialize)]
struct DuplicateGroupInput {
    group_index: usize,
    bookmarks: Vec<DupBookmarkInput>,
}

#[derive(serde::Serialize)]
struct DuplicateResolution {
    group_index: usize,
    keeper_id: String,
}

#[tauri::command]
async fn suggest_duplicate_resolution(
    groups: Vec<DuplicateGroupInput>,
) -> Result<Vec<DuplicateResolution>, AppError> {
    if groups.is_empty() {
        return Ok(vec![]);
    }

    // Compact one-line-per-group format: "0: A=<id> "Title" [desc] B=<id> "Title2""
    let letters = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H'];
    let groups_text: String = groups
        .iter()
        .map(|g| {
            let entries = g
                .bookmarks
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let letter = letters.get(i).copied().unwrap_or('?');
                    let desc = if b.description.as_deref().map(|d| !d.is_empty()).unwrap_or(false) {
                        "[desc]"
                    } else {
                        ""
                    };
                    format!("{}={} {:?} {}", letter, b.id, b.title, desc)
                })
                .collect::<Vec<_>>()
                .join("  ");
            format!("{}: {}", g.group_index, entries)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Pick best bookmark per group. Prefer: specific title > has description > longer title.\n\
         Reply ONLY with JSON array, one object per group: [{{\"g\":0,\"k\":\"<id>\"}}, ...]\n\n\
         {groups_text}"
    );

    let raw = run_claude(&prompt).await?;
    let trimmed = raw.trim();
    let json_str = if let (Some(start), Some(end)) = (trimmed.find('['), trimmed.rfind(']')) {
        &trimmed[start..=end]
    } else {
        return Err(AppError::Validation {
            message: format!("No JSON array in Claude response: {}", &trimmed[..trimmed.len().min(200)]),
        });
    };

    let value: Vec<serde_json::Value> =
        serde_json::from_str(json_str).map_err(|e| AppError::Validation {
            message: format!("Could not parse Claude response: {e}"),
        })?;

    let resolutions: Vec<DuplicateResolution> = value
        .into_iter()
        .filter_map(|obj| {
            let group_index = obj["g"].as_u64()? as usize;
            let keeper_id = obj["k"].as_str()?.to_string();
            if keeper_id.is_empty() { return None; }
            Some(DuplicateResolution { group_index, keeper_id })
        })
        .collect();

    Ok(resolutions)
}

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| e.to_string())
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

    fn make_full_app(token: &str) -> (axum::Router, Arc<AtomicBool>) {
        let (state, called) = make_state(token);
        let cors = tower_http::cors::CorsLayer::new()
            .allow_origin(tower_http::cors::Any)
            .allow_methods(tower_http::cors::Any)
            .allow_headers(tower_http::cors::Any);
        let app = axum::Router::new()
            .route("/bookmarks", post(http_add_bookmark))
            .route("/folders", axum::routing::get(http_get_folders).post(http_add_folder))
            .route("/tags", axum::routing::get(http_get_tags).post(http_add_tag))
            .with_state(state)
            .layer(cors);
        (app, called)
    }

    #[tokio::test]
    async fn get_folders_returns_empty_list_with_auth() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("GET")
            .uri("/folders")
            .header("authorization", "Bearer tok")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_folders_returns_401_without_auth() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("GET")
            .uri("/folders")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_tags_returns_empty_list_with_auth() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("GET")
            .uri("/tags")
            .header("authorization", "Bearer tok")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_tags_returns_401_without_auth() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("GET")
            .uri("/tags")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_folders_creates_folder() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("POST")
            .uri("/folders")
            .header("content-type", "application/json")
            .header("authorization", "Bearer tok")
            .body(Body::from(r#"{"name":"Work"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_folders_returns_401_without_auth() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("POST")
            .uri("/folders")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"name":"Work"}"#))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn post_tags_creates_tag() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("POST")
            .uri("/tags")
            .header("content-type", "application/json")
            .header("authorization", "Bearer tok")
            .body(Body::from(r##"{"name":"rust","color":"#6366f1"}"##))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn post_tags_returns_401_without_auth() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("POST")
            .uri("/tags")
            .header("content-type", "application/json")
            .body(Body::from(r##"{"name":"rust","color":"#6366f1"}"##))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn ping_returns_ok() {
        let (state, called) = make_state("tok");
        let app = axum::Router::new()
            .route("/bookmarks", post(http_add_bookmark))
            .with_state(state);

        let req = Request::builder()
            .method("POST")
            .uri("/bookmarks")
            .header("content-type", "application/json")
            .header("authorization", "Bearer tok")
            .body(Body::from(r#"{"url":"__ping__","title":"__ping__"}"#))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!called.load(Ordering::Relaxed), "ping must not trigger a notification");
    }
}
