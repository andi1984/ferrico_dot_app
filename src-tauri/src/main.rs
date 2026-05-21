#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::State;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub url: String,
    pub title: String,
    pub description: Option<String>,
    pub favicon_url: Option<String>,
    pub feed_url: Option<String>,
    pub folder_id: Option<String>,
    pub tags: Vec<Tag>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: i64,
}

pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub api_token: String,
}

#[derive(Clone)]
struct HttpState {
    db: Arc<Mutex<Connection>>,
    token: String,
}

// ─── DB Init ─────────────────────────────────────────────────────────────────

fn init_db(data_dir: &PathBuf) -> Connection {
    let conn = Connection::open(data_dir.join("ferrico.db")).expect("open db");
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;

         CREATE TABLE IF NOT EXISTS folders (
           id         TEXT PRIMARY KEY,
           name       TEXT NOT NULL,
           parent_id  TEXT REFERENCES folders(id) ON DELETE CASCADE,
           created_at INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS bookmarks (
           id          TEXT PRIMARY KEY,
           url         TEXT NOT NULL,
           title       TEXT NOT NULL,
           description TEXT,
           favicon_url TEXT,
           feed_url    TEXT,
           folder_id   TEXT REFERENCES folders(id) ON DELETE SET NULL,
           created_at  INTEGER NOT NULL,
           updated_at  INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS tags (
           id         TEXT PRIMARY KEY,
           name       TEXT NOT NULL UNIQUE,
           color      TEXT NOT NULL DEFAULT '#6366f1',
           created_at INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS bookmark_tags (
           bookmark_id TEXT NOT NULL REFERENCES bookmarks(id) ON DELETE CASCADE,
           tag_id      TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
           PRIMARY KEY (bookmark_id, tag_id)
         );",
    )
    .expect("create tables");
    conn
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

struct RawBookmark {
    id: String,
    url: String,
    title: String,
    description: Option<String>,
    favicon_url: Option<String>,
    feed_url: Option<String>,
    folder_id: Option<String>,
    created_at: i64,
    updated_at: i64,
}

fn row_to_raw(row: &rusqlite::Row) -> rusqlite::Result<RawBookmark> {
    Ok(RawBookmark {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        description: row.get(3)?,
        favicon_url: row.get(4)?,
        feed_url: row.get(5)?,
        folder_id: row.get(6)?,
        created_at: row.get(7)?,
        updated_at: row.get(8)?,
    })
}

fn get_tags_for_bookmark(conn: &Connection, bookmark_id: &str) -> Vec<Tag> {
    let mut stmt = conn
        .prepare(
            "SELECT t.id, t.name, t.color, t.created_at \
             FROM tags t JOIN bookmark_tags bt ON bt.tag_id = t.id \
             WHERE bt.bookmark_id = ?1 ORDER BY t.name",
        )
        .unwrap();
    stmt.query_map(params![bookmark_id], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            created_at: row.get(3)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

fn enrich(raw: RawBookmark, conn: &Connection) -> Bookmark {
    let tags = get_tags_for_bookmark(conn, &raw.id);
    Bookmark {
        id: raw.id,
        url: raw.url,
        title: raw.title,
        description: raw.description,
        favicon_url: raw.favicon_url,
        feed_url: raw.feed_url,
        folder_id: raw.folder_id,
        tags,
        created_at: raw.created_at,
        updated_at: raw.updated_at,
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ─── Tauri Commands ──────────────────────────────────────────────────────────

#[tauri::command]
fn get_bookmarks(
    folder_id: Option<String>,
    tag_id: Option<String>,
    search: Option<String>,
    state: State<'_, AppState>,
) -> Vec<Bookmark> {
    let db = state.db.lock().unwrap();

    let raws: Vec<RawBookmark> = match (&folder_id, &tag_id) {
        (_, Some(tid)) => {
            let mut stmt = db
                .prepare(
                    "SELECT b.id, b.url, b.title, b.description, b.favicon_url, b.feed_url, \
                     b.folder_id, b.created_at, b.updated_at \
                     FROM bookmarks b JOIN bookmark_tags bt ON bt.bookmark_id = b.id \
                     WHERE bt.tag_id = ?1 ORDER BY b.created_at DESC",
                )
                .unwrap();
            stmt.query_map(params![tid], row_to_raw)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
        (Some(fid), None) => {
            let mut stmt = db
                .prepare(
                    "SELECT id, url, title, description, favicon_url, feed_url, folder_id, \
                     created_at, updated_at FROM bookmarks WHERE folder_id = ?1 \
                     ORDER BY created_at DESC",
                )
                .unwrap();
            stmt.query_map(params![fid], row_to_raw)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
        (None, None) => {
            let mut stmt = db
                .prepare(
                    "SELECT id, url, title, description, favicon_url, feed_url, folder_id, \
                     created_at, updated_at FROM bookmarks ORDER BY created_at DESC",
                )
                .unwrap();
            stmt.query_map([], row_to_raw)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
    };

    let mut bookmarks: Vec<Bookmark> = raws.into_iter().map(|r| enrich(r, &db)).collect();

    if let Some(q) = search {
        let q = q.to_lowercase();
        bookmarks.retain(|b| {
            b.title.to_lowercase().contains(&q)
                || b.url.to_lowercase().contains(&q)
                || b.description
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
        });
    }

    bookmarks
}

#[derive(Deserialize)]
pub struct CreateBookmarkInput {
    pub url: String,
    pub title: String,
    pub description: Option<String>,
    pub favicon_url: Option<String>,
    pub feed_url: Option<String>,
    pub folder_id: Option<String>,
    pub tag_ids: Option<Vec<String>>,
}

#[tauri::command]
fn add_bookmark(input: CreateBookmarkInput, state: State<'_, AppState>) -> Bookmark {
    let db = state.db.lock().unwrap();
    let id = Uuid::new_v4().to_string();
    let ts = now();

    db.execute(
        "INSERT INTO bookmarks \
         (id, url, title, description, favicon_url, feed_url, folder_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            input.url,
            input.title,
            input.description,
            input.favicon_url,
            input.feed_url,
            input.folder_id,
            ts,
            ts
        ],
    )
    .unwrap();

    if let Some(tag_ids) = &input.tag_ids {
        for tid in tag_ids {
            db.execute(
                "INSERT OR IGNORE INTO bookmark_tags (bookmark_id, tag_id) VALUES (?1, ?2)",
                params![id, tid],
            )
            .ok();
        }
    }

    let tags = get_tags_for_bookmark(&db, &id);
    Bookmark {
        id,
        url: input.url,
        title: input.title,
        description: input.description,
        favicon_url: input.favicon_url,
        feed_url: input.feed_url,
        folder_id: input.folder_id,
        tags,
        created_at: ts,
        updated_at: ts,
    }
}

#[tauri::command]
fn delete_bookmark(id: String, state: State<'_, AppState>) {
    let db = state.db.lock().unwrap();
    db.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])
        .unwrap();
}

#[tauri::command]
fn get_folders(state: State<'_, AppState>) -> Vec<Folder> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .prepare("SELECT id, name, parent_id, created_at FROM folders ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| {
        Ok(Folder {
            id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            created_at: row.get(3)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[tauri::command]
fn add_folder(name: String, parent_id: Option<String>, state: State<'_, AppState>) -> Folder {
    let db = state.db.lock().unwrap();
    let id = Uuid::new_v4().to_string();
    let ts = now();
    db.execute(
        "INSERT INTO folders (id, name, parent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, parent_id, ts],
    )
    .unwrap();
    Folder {
        id,
        name,
        parent_id,
        created_at: ts,
    }
}

#[tauri::command]
fn delete_folder(id: String, state: State<'_, AppState>) {
    let db = state.db.lock().unwrap();
    db.execute("DELETE FROM folders WHERE id = ?1", params![id])
        .unwrap();
}

#[tauri::command]
fn get_tags(state: State<'_, AppState>) -> Vec<Tag> {
    let db = state.db.lock().unwrap();
    let mut stmt = db
        .prepare("SELECT id, name, color, created_at FROM tags ORDER BY name")
        .unwrap();
    stmt.query_map([], |row| {
        Ok(Tag {
            id: row.get(0)?,
            name: row.get(1)?,
            color: row.get(2)?,
            created_at: row.get(3)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

#[tauri::command]
fn add_tag(name: String, color: String, state: State<'_, AppState>) -> Tag {
    let db = state.db.lock().unwrap();
    let id = Uuid::new_v4().to_string();
    let ts = now();
    db.execute(
        "INSERT OR IGNORE INTO tags (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, color, ts],
    )
    .unwrap();
    Tag {
        id,
        name,
        color,
        created_at: ts,
    }
}

#[tauri::command]
fn delete_tag(id: String, state: State<'_, AppState>) {
    let db = state.db.lock().unwrap();
    db.execute("DELETE FROM tags WHERE id = ?1", params![id])
        .unwrap();
}

#[tauri::command]
fn get_api_token(state: State<'_, AppState>) -> String {
    state.api_token.clone()
}

#[tauri::command]
fn export_opml(state: State<'_, AppState>) -> String {
    let db = state.db.lock().unwrap();

    let folders: Vec<Folder> = {
        let mut stmt = db
            .prepare("SELECT id, name, parent_id, created_at FROM folders ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    };

    let bookmarks: Vec<RawBookmark> = {
        let mut stmt = db
            .prepare(
                "SELECT id, url, title, description, favicon_url, feed_url, folder_id, \
                 created_at, updated_at FROM bookmarks ORDER BY created_at",
            )
            .unwrap();
        stmt.query_map([], row_to_raw)
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    };

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <opml version=\"2.0\">\n\
         <head><title>Ferrico Bookmarks</title></head>\n\
         <body>\n",
    );

    for folder in &folders {
        xml.push_str(&format!(
            "  <outline text=\"{}\">\n",
            xml_escape(&folder.name)
        ));
        for b in bookmarks
            .iter()
            .filter(|b| b.folder_id.as_deref() == Some(&folder.id))
        {
            append_outline(&mut xml, b, 4);
        }
        xml.push_str("  </outline>\n");
    }

    for b in bookmarks.iter().filter(|b| b.folder_id.is_none()) {
        append_outline(&mut xml, b, 2);
    }

    xml.push_str("</body>\n</opml>");
    xml
}

fn append_outline(xml: &mut String, b: &RawBookmark, indent: usize) {
    let pad = " ".repeat(indent);
    xml.push_str(&format!(
        "{}<outline type=\"link\" text=\"{}\" url=\"{}\"",
        pad,
        xml_escape(&b.title),
        xml_escape(&b.url)
    ));
    if let Some(d) = &b.description {
        xml.push_str(&format!(" description=\"{}\"", xml_escape(d)));
    }
    if let Some(f) = &b.feed_url {
        xml.push_str(&format!(" xmlUrl=\"{}\"", xml_escape(f)));
    }
    xml.push_str("/>\n");
}

// ─── HTTP Server (extension endpoint) ────────────────────────────────────────

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
        params![
            id,
            body.url,
            body.title,
            body.description,
            body.favicon_url,
            body.feed_url,
            body.folder_id,
            ts,
            ts
        ],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "id": id, "created_at": ts })))
}

async fn start_http_server(db: Arc<Mutex<Connection>>, token: String) {
    let state = HttpState { db, token };
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/bookmarks", post(http_add_bookmark))
        .with_state(state)
        .layer(cors);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:59432")
        .await
        .expect("bind 127.0.0.1:59432");
    axum::serve(listener, app).await.unwrap();
}

// ─── Settings ────────────────────────────────────────────────────────────────

fn load_or_create_token(data_dir: &PathBuf) -> String {
    let path = data_dir.join("settings.json");
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(t) = v["api_token"].as_str() {
                return t.to_string();
            }
        }
    }
    let token: String = (0..32)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    fs::write(
        &path,
        serde_json::to_string_pretty(&serde_json::json!({ "api_token": token })).unwrap(),
    )
    .ok();
    token
}

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let data_dir = dirs::data_dir()
        .map(|d| d.join("ferrico"))
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&data_dir).ok();

    let conn = init_db(&data_dir);
    let api_token = load_or_create_token(&data_dir);
    let db = Arc::new(Mutex::new(conn));

    tauri::Builder::default()
        .manage(AppState {
            db: db.clone(),
            api_token: api_token.clone(),
        })
        .setup(move |_app| {
            tauri::async_runtime::spawn(start_http_server(db.clone(), api_token.clone()));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_bookmarks,
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
