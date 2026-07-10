mod db;
mod error;
mod gdrive;
mod health_check;
mod io;
mod io_validate;
mod merge;
mod og_image;

use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use db::{
    Bookmark, CreateBookmarkInput, Folder, ImportResult, ImportRowInput,
    InboxSortAssignment, InboxSortResult, SidebarData, Tag,
    db_add_bookmark, db_add_folder, db_add_tag,
    db_apply_inbox_sort, db_clear_all_data,
    db_delete_bookmark, db_delete_bookmarks, db_delete_folder, db_delete_tag,
    db_empty_bin, db_get_bin_bookmarks, db_get_bin_count,
    db_get_broken_bookmarks, db_get_broken_count,
    db_get_inbox_count, db_get_sidebar, db_get_urls_for_health_check, db_import_bookmarks,
    db_move_bookmark, db_move_folder, db_permanently_delete_bookmark,
    db_purge_expired_bin, db_restore_bookmark,
    db_get_bookmark_count, db_get_bookmarks, db_get_folders, db_get_tags,
    db_related_tags, db_lookup_by_url,
    db_find_duplicate_bookmarks, db_merge_bookmark_duplicates,
    db_update_bookmark_health_batch,
    db_get_bookmarks_without_cover, db_update_cover_url,
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
use tauri::{AppHandle, Emitter, Manager, State};
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
fn get_sidebar(state: State<'_, AppState>) -> Result<SidebarData, AppError> {
    let db = lock_db!(state);
    db_get_sidebar(&db)
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
fn move_folder(
    id: String,
    parent_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let db = lock_db!(state);
    db_move_folder(&db, &id, parent_id.as_deref())
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

/// Tags that most often co-occur with the already-selected ones. Powers the
/// context-aware suggestions in the New Bookmark tag combobox (same engine the
/// browser extension uses over HTTP).
#[tauri::command]
fn related_tags(tag_ids: Vec<String>, state: State<'_, AppState>) -> Result<Vec<Tag>, AppError> {
    let db = lock_db!(state);
    db_related_tags(&db, &tag_ids, 8)
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
    run_claude_model(prompt, "").await
}

async fn run_claude_model(prompt: &str, model: &str) -> Result<String, AppError> {
    use tokio::io::AsyncWriteExt;

    let home = std::env::var("HOME").unwrap_or_default();
    let existing_path = std::env::var("PATH").unwrap_or_default();
    let extended_path =
        format!("{home}/.local/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:{existing_path}");

    let mut cmd = tokio::process::Command::new("claude");
    cmd.arg("-p").arg("-");
    if !model.is_empty() {
        cmd.arg("--model").arg(model);
    }
    cmd.env("PATH", &extended_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    // Pass prompt via stdin to avoid E2BIG when prompt exceeds ARG_MAX
    let mut child = cmd.spawn().map_err(|e| AppError::Validation {
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

// ─── AI Chat Search ───────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct AiSearchBookmark {
    id: String,
    title: String,
    url: String,
    description: Option<String>,
    tags: Vec<String>,
    folder_name: Option<String>,
}

#[derive(serde::Serialize)]
struct AiSearchResponse {
    reply: String,
    bookmark_ids: Vec<String>,
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        // Safe truncation at char boundary
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}

fn domain_of(url: &str) -> &str {
    let s = url.trim_start_matches("https://").trim_start_matches("http://");
    s.split('/').next().unwrap_or(s)
}

#[tauri::command]
async fn ai_search(
    query: String,
    bookmarks: Vec<AiSearchBookmark>,
) -> Result<AiSearchResponse, AppError> {
    if bookmarks.is_empty() {
        return Ok(AiSearchResponse {
            reply: "No bookmarks to search through.".to_string(),
            bookmark_ids: vec![],
        });
    }

    // Compact format: index|id|title|domain|tags  — ~60 chars/line avg
    // Cap at 400 bookmarks to stay within context limits
    let bookmark_list: String = bookmarks
        .iter()
        .take(400)
        .enumerate()
        .map(|(i, b)| {
            let title = truncate(b.title.trim(), 60);
            let domain = domain_of(&b.url);
            let domain = truncate(domain, 40);
            let tags = b.tags.join(",");
            let tags = truncate(&tags, 40);
            let folder = b.folder_name.as_deref().unwrap_or("");
            // Include a short description excerpt only when available
            let desc_part = b
                .description
                .as_deref()
                .map(|d| {
                    let d = d.trim();
                    if d.is_empty() { String::new() } else { format!("|{}", truncate(d, 80)) }
                })
                .unwrap_or_default();
            format!("{}|{}|{}|{}|{}|{}{}", i + 1, b.id, title, domain, folder, tags, desc_part)
        })
        .collect::<Vec<_>>()
        .join("\n");

    let prompt = format!(
        "Bookmark search. Query: {query}\n\
         Format: index|id|title|domain|folder|tags|desc\n\n\
         {bookmark_list}\n\n\
         Return JSON only: {{\"reply\":\"1-2 sentences\",\"ids\":[\"id1\",\"id2\"]}}\n\
         Include all relevant matches. Empty ids if none match."
    );

    let raw = run_claude_model(&prompt, "claude-haiku-4-5-20251001").await?;
    let json_str = extract_json(&raw);

    #[derive(serde::Deserialize)]
    struct RawResponse {
        reply: String,
        ids: Vec<String>,
    }

    let parsed: RawResponse =
        serde_json::from_str(json_str).map_err(|e| AppError::Validation {
            message: format!(
                "Failed to parse AI response: {e}. Raw: {}",
                raw.chars().take(300).collect::<String>()
            ),
        })?;

    Ok(AiSearchResponse { reply: parsed.reply, bookmark_ids: parsed.ids })
}

// ─── HTTP Server (browser extension endpoint) ─────────────────────────────────

#[derive(Deserialize)]
struct ExtPayload {
    url: String,
    title: String,
    description: Option<String>,
    favicon_url: Option<String>,
    #[serde(default)]
    cover_url: Option<String>,
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

#[derive(Deserialize)]
struct RelatedTagsQuery {
    /// Comma-separated tag ids the user has already selected.
    ids: Option<String>,
}

/// Suggest tags co-occurring with the already-selected ones (`?ids=a,b,c`).
/// Powers the extension's context-aware tag suggestions.
async fn http_related_tags(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
    Query(q): Query<RelatedTagsQuery>,
) -> Result<Json<Vec<Tag>>, StatusCode> {
    if !auth_ok(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let ids: Vec<String> = q
        .ids
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    db_related_tags(&db, &ids, 8)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Deserialize)]
struct LookupQuery {
    /// The full URL of the page being saved.
    url: Option<String>,
}

/// Report existing bookmarks for `?url=…`, split into an exact-page match and
/// other pages on the same host. Powers the extension's "already saved" panel.
async fn http_lookup_bookmarks(
    AxumState(state): AxumState<HttpState>,
    headers: HeaderMap,
    Query(q): Query<LookupQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    if !auth_ok(&headers, &state.token) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let url = q.url.unwrap_or_default();
    let db = state.db.lock().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let matches = db_lookup_by_url(&db, &url).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    serde_json::to_value(matches)
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
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

    if let Some(ref cover) = body.cover_url {
        db_update_cover_url(&db, &bookmark.id, cover)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

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
        .route("/tags/related", get(http_related_tags))
        .route("/bookmarks/lookup", get(http_lookup_bookmarks))
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

// ─── Background Cover Scanner ─────────────────────────────────────────────────

async fn background_cover_scanner(db: Arc<Mutex<Connection>>, app: AppHandle) {
    // Wait for app to fully start before hammering the network.
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;

    let client = match og_image::build_client() {
        Ok(c) => c,
        Err(_) => return,
    };

    loop {
        let bookmarks = {
            match db.lock() {
                Ok(conn) => db_get_bookmarks_without_cover(&conn).unwrap_or_default(),
                Err(_) => vec![],
            }
        };

        for (id, url) in bookmarks {
            if let Some(cover_url) = og_image::fetch_og_image(&client, &url).await {
                if let Ok(conn) = db.lock() {
                    let _ = db_update_cover_url(&conn, &id, &cover_url);
                }
                app.emit("cover-updated", serde_json::json!({
                    "id": id,
                    "cover_url": cover_url,
                }))
                .ok();
            }
            // Polite rate limit between fetches.
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }

        // Re-check every 10 minutes for newly added bookmarks.
        tokio::time::sleep(std::time::Duration::from_secs(600)).await;
    }
}

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

// ─── Google Drive Backup commands ──────────────────────────────────────────────
//
// Thin wrappers over `gdrive::BackupEngine` (held in managed state). Network-bound
// operations are `async`; pure config edits stay sync.

#[tauri::command]
fn backup_status(engine: State<'_, gdrive::BackupEngine>) -> Result<gdrive::BackupStatus, AppError> {
    engine.status()
}

#[tauri::command]
fn backup_set_credentials(
    client_id: String,
    client_secret: String,
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::BackupStatus, AppError> {
    engine.set_credentials(client_id, client_secret)
}

#[tauri::command]
async fn backup_connect(
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::BackupStatus, AppError> {
    let engine = engine.inner().clone();
    engine.connect().await
}

#[tauri::command]
fn backup_disconnect(
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::BackupStatus, AppError> {
    engine.disconnect()
}

#[tauri::command]
async fn backup_list_folders(
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<Vec<gdrive::DriveFolder>, AppError> {
    let engine = engine.inner().clone();
    engine.list_folders().await
}

#[tauri::command]
async fn backup_create_folder(
    name: String,
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::DriveFolder, AppError> {
    let engine = engine.inner().clone();
    engine.create_folder(name).await
}

#[tauri::command]
fn backup_select_folder(
    folder_id: String,
    folder_name: String,
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::BackupStatus, AppError> {
    engine.select_folder(folder_id, folder_name)
}

#[tauri::command]
fn backup_set_enabled(
    enabled: bool,
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::BackupStatus, AppError> {
    engine.set_enabled(enabled)
}

#[tauri::command]
fn backup_set_interval(
    interval_min: u64,
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::BackupStatus, AppError> {
    engine.set_interval(interval_min)
}

#[tauri::command]
async fn backup_sync_now(
    engine: State<'_, gdrive::BackupEngine>,
) -> Result<gdrive::BackupStatus, AppError> {
    let engine = engine.inner().clone();
    engine.sync_now().await
}

// ─── Entry Point ──────────────────────────────────────────────────────────────

/// Where the SQLite DB and `settings.json` live. Platform-split on purpose:
/// desktop must stay on `dirs::data_dir()/ferrico` — switching to Tauri's
/// `app_data_dir()` (which resolves per bundle identifier) would orphan every
/// existing user database.
#[cfg(desktop)]
fn resolve_data_dir(_app: &AppHandle) -> PathBuf {
    dirs::data_dir()
        .map(|d| d.join("ferrico"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// On mobile `dirs::data_dir()` returns `None`; the app-private data dir has
/// to come from Tauri's path resolver instead.
#[cfg(mobile)]
fn resolve_data_dir(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().expect("app data dir")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();

            let data_dir = resolve_data_dir(&handle);
            fs::create_dir_all(&data_dir).ok();

            let conn = open_db(&data_dir).expect("init db");
            let api_token = load_or_create_token(&data_dir);
            let db = Arc::new(Mutex::new(conn));

            // Commands can't fire before setup() completes, so managing state
            // here is equivalent to managing it before the builder runs.
            app.manage(AppState {
                db: db.clone(),
                api_token: api_token.clone(),
            });

            tauri::async_runtime::spawn(start_http_server(db.clone(), api_token.clone(), handle.clone()));
            tauri::async_runtime::spawn(background_cover_scanner(db.clone(), handle.clone()));

            // ── Google Drive backup engine + lifecycle wiring ──
            let engine = gdrive::BackupEngine::new(db.clone(), data_dir, handle.clone());
            app.manage(engine.clone());

            // Pull-and-replace on open: wait briefly for the UI to mount, then
            // reconcile down from Drive. The engine emits `backup-synced` so the
            // frontend refreshes once the local DB has been replaced.
            {
                let engine = engine.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    engine.pull_if_active().await;
                });
            }

            // Periodic autosave push.
            tauri::async_runtime::spawn(engine.clone().run_autosave());

            // Push-before-close: intercept the main window close, hold it open
            // while the final backup uploads, then close for real. The atomic
            // guard prevents the re-entrant CloseRequested (from `win.close()`)
            // from looping.
            if let Some(window) = app.get_webview_window("main") {
                let engine = engine.clone();
                let win = window.clone();
                let closing = Arc::new(std::sync::atomic::AtomicBool::new(false));
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        use std::sync::atomic::Ordering;
                        if closing.load(Ordering::SeqCst) || !engine.is_active() {
                            return; // second pass, or nothing to back up — let it close
                        }
                        closing.store(true, Ordering::SeqCst);
                        api.prevent_close();
                        let engine = engine.clone();
                        let win = win.clone();
                        tauri::async_runtime::spawn(async move {
                            engine.push_if_active().await;
                            let _ = win.close();
                        });
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bookmarks,
            get_bookmark_count,
            get_sidebar,
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
            move_folder,
            delete_folder,
            get_tags,
            add_tag,
            delete_tag,
            related_tags,
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
            scan_cover_images,
            read_text_file,
            pick_csv_file,
            pick_import_file,
            ai_search,
            backup_status,
            backup_set_credentials,
            backup_connect,
            backup_disconnect,
            backup_list_folders,
            backup_create_folder,
            backup_select_folder,
            backup_set_enabled,
            backup_set_interval,
            backup_sync_now,
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

    let broken = results.iter().filter(|r| r.is_broken).count();
    // One transaction for the whole write-back instead of N auto-commits.
    let updates: Vec<(String, bool, i64)> = results
        .iter()
        .map(|r| (r.id.clone(), r.is_broken, r.last_checked_at))
        .collect();
    {
        let db = lock_db!(state);
        db_update_bookmark_health_batch(&db, &updates)?;
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

// ─── Cover Image Scan ─────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct CoverScanResult {
    total: usize,
    found: usize,
}

#[tauri::command]
async fn scan_cover_images(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CoverScanResult, AppError> {
    let bookmarks = {
        let db = lock_db!(state);
        db_get_bookmarks_without_cover(&db)?
    };

    let total = bookmarks.len();
    if total == 0 {
        return Ok(CoverScanResult { total: 0, found: 0 });
    }

    let client = og_image::build_client()
        .map_err(|e| AppError::Validation { message: e.to_string() })?;

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(5));
    let mut set = tokio::task::JoinSet::new();

    for (id, url) in bookmarks {
        let client = client.clone();
        let sem = sem.clone();
        set.spawn(async move {
            let _permit = sem.acquire_owned().await.expect("semaphore should not close");
            let cover = og_image::fetch_og_image(&client, &url).await;
            (id, cover)
        });
    }

    let mut results: Vec<(String, Option<String>)> = Vec::new();
    let mut completed = 0usize;

    while let Some(task_result) = set.join_next().await {
        if let Ok(pair) = task_result {
            results.push(pair);
        }
        completed += 1;
        app.emit("cover-scan-progress", serde_json::json!({
            "current": completed,
            "total": total,
        }))
        .ok();
    }

    let mut found = 0usize;
    {
        let db = lock_db!(state);
        for (id, cover_url) in &results {
            if let Some(url) = cover_url {
                db_update_cover_url(&db, id, url)?;
                found += 1;
            }
        }
    }

    Ok(CoverScanResult { total: results.len(), found })
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

#[tauri::command]
async fn pick_csv_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .add_filter("CSV", &["csv"])
        .pick_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
}

#[tauri::command]
async fn pick_import_file() -> Option<String> {
    rfd::AsyncFileDialog::new()
        .add_filter("Bookmarks", &["json", "html", "htm", "opml", "xml", "csv"])
        .pick_file()
        .await
        .map(|f| f.path().to_string_lossy().into_owned())
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
            .route("/bookmarks/lookup", axum::routing::get(http_lookup_bookmarks))
            .route("/folders", axum::routing::get(http_get_folders).post(http_add_folder))
            .route("/tags", axum::routing::get(http_get_tags).post(http_add_tag))
            .with_state(state)
            .layer(cors);
        (app, called)
    }

    #[tokio::test]
    async fn lookup_returns_401_without_auth() {
        let (app, _) = make_full_app("tok");
        let req = Request::builder()
            .method("GET")
            .uri("/bookmarks/lookup?url=https://example.com")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn lookup_splits_exact_and_same_domain_over_http() {
        let (app, _) = make_full_app("tok");

        // Seed two pages on the same host: one exact, one elsewhere on the site.
        for url in ["https://example.com/a", "https://example.com/b"] {
            let req = Request::builder()
                .method("POST")
                .uri("/bookmarks")
                .header("content-type", "application/json")
                .header("authorization", "Bearer tok")
                .body(Body::from(format!(r#"{{"url":"{url}","title":"T"}}"#)))
                .unwrap();
            assert_eq!(app.clone().oneshot(req).await.unwrap().status(), StatusCode::OK);
        }

        let req = Request::builder()
            .method("GET")
            .uri("/bookmarks/lookup?url=https%3A%2F%2Fexample.com%2Fa")
            .header("authorization", "Bearer tok")
            .body(Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["domain"], "example.com");
        assert_eq!(json["exact"].as_array().unwrap().len(), 1);
        assert_eq!(json["exact"][0]["url"], "https://example.com/a");
        assert_eq!(json["same_domain"].as_array().unwrap().len(), 1);
        assert_eq!(json["same_domain"][0]["url"], "https://example.com/b");
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
