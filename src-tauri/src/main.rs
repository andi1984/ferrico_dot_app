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
use rand::RngCore;
use rand::rngs::OsRng;
use rusqlite::{params, params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

fn init_db(data_dir: &PathBuf) -> Result<Connection, rusqlite::Error> {
    let conn = Connection::open(data_dir.join("ferrico.db"))?;
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
    )?;
    Ok(conn)
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

fn get_tags_batch(
    conn: &Connection,
    bookmark_ids: &[String],
) -> rusqlite::Result<HashMap<String, Vec<Tag>>> {
    let mut map: HashMap<String, Vec<Tag>> = HashMap::new();
    if bookmark_ids.is_empty() {
        return Ok(map);
    }
    let placeholders = (1..=bookmark_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT bt.bookmark_id, t.id, t.name, t.color, t.created_at \
         FROM tags t JOIN bookmark_tags bt ON bt.tag_id = t.id \
         WHERE bt.bookmark_id IN ({placeholders}) ORDER BY t.name"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(bookmark_ids.iter()), |row| {
        Ok((
            row.get::<_, String>(0)?,
            Tag {
                id: row.get(1)?,
                name: row.get(2)?,
                color: row.get(3)?,
                created_at: row.get(4)?,
            },
        ))
    })?;
    for row in rows.flatten() {
        map.entry(row.0).or_default().push(row.1);
    }
    Ok(map)
}

fn enrich_batch(raws: Vec<RawBookmark>, conn: &Connection) -> rusqlite::Result<Vec<Bookmark>> {
    let ids: Vec<String> = raws.iter().map(|r| r.id.clone()).collect();
    let mut tags_map = get_tags_batch(conn, &ids)?;
    Ok(raws
        .into_iter()
        .map(|r| {
            let tags = tags_map.remove(&r.id).unwrap_or_default();
            Bookmark {
                id: r.id,
                url: r.url,
                title: r.title,
                description: r.description,
                favicon_url: r.favicon_url,
                feed_url: r.feed_url,
                folder_id: r.folder_id,
                tags,
                created_at: r.created_at,
                updated_at: r.updated_at,
            }
        })
        .collect())
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
) -> Result<Vec<Bookmark>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    let raws: Vec<RawBookmark> = match (&folder_id, &tag_id) {
        (_, Some(tid)) => {
            let mut stmt = db
                .prepare(
                    "SELECT b.id, b.url, b.title, b.description, b.favicon_url, b.feed_url, \
                     b.folder_id, b.created_at, b.updated_at \
                     FROM bookmarks b JOIN bookmark_tags bt ON bt.bookmark_id = b.id \
                     WHERE bt.tag_id = ?1 ORDER BY b.created_at DESC",
                )
                .map_err(|e| e.to_string())?;
            stmt.query_map(params![tid], row_to_raw)
                .map_err(|e| e.to_string())?
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
                .map_err(|e| e.to_string())?;
            stmt.query_map(params![fid], row_to_raw)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect()
        }
        (None, None) => {
            let mut stmt = db
                .prepare(
                    "SELECT id, url, title, description, favicon_url, feed_url, folder_id, \
                     created_at, updated_at FROM bookmarks ORDER BY created_at DESC",
                )
                .map_err(|e| e.to_string())?;
            stmt.query_map([], row_to_raw)
                .map_err(|e| e.to_string())?
                .filter_map(|r| r.ok())
                .collect()
        }
    };

    let mut bookmarks = enrich_batch(raws, &db).map_err(|e| e.to_string())?;

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

    Ok(bookmarks)
}

#[tauri::command]
fn get_bookmark_count(state: State<'_, AppState>) -> Result<i64, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.query_row("SELECT COUNT(*) FROM bookmarks", [], |r| r.get(0))
        .map_err(|e| e.to_string())
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
fn add_bookmark(input: CreateBookmarkInput, state: State<'_, AppState>) -> Result<Bookmark, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
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
    .map_err(|e| e.to_string())?;

    if let Some(tag_ids) = &input.tag_ids {
        for tid in tag_ids {
            db.execute(
                "INSERT OR IGNORE INTO bookmark_tags (bookmark_id, tag_id) VALUES (?1, ?2)",
                params![id, tid],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    let tags = get_tags_batch(&db, &[id.clone()])
        .map_err(|e| e.to_string())?
        .remove(&id)
        .unwrap_or_default();

    Ok(Bookmark {
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
    })
}

#[tauri::command]
fn delete_bookmark(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_folders(state: State<'_, AppState>) -> Result<Vec<Folder>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT id, name, parent_id, created_at FROM folders ORDER BY name")
        .map_err(|e| e.to_string())?;
    let folders = stmt
        .query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(folders)
}

#[tauri::command]
fn add_folder(
    name: String,
    parent_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Folder, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let ts = now();
    db.execute(
        "INSERT INTO folders (id, name, parent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, parent_id, ts],
    )
    .map_err(|e| e.to_string())?;
    Ok(Folder {
        id,
        name,
        parent_id,
        created_at: ts,
    })
}

#[tauri::command]
fn delete_folder(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute("DELETE FROM folders WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_tags(state: State<'_, AppState>) -> Result<Vec<Tag>, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let mut stmt = db
        .prepare("SELECT id, name, color, created_at FROM tags ORDER BY name")
        .map_err(|e| e.to_string())?;
    let tags = stmt
        .query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(tags)
}

#[tauri::command]
fn add_tag(name: String, color: String, state: State<'_, AppState>) -> Result<Tag, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let id = Uuid::new_v4().to_string();
    let ts = now();
    db.execute(
        "INSERT OR IGNORE INTO tags (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, color, ts],
    )
    .map_err(|e| e.to_string())?;

    // SELECT after INSERT OR IGNORE so we always return the actual record (handles name conflict)
    db.query_row(
        "SELECT id, name, color, created_at FROM tags WHERE name = ?1",
        params![name],
        |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                created_at: row.get(3)?,
            })
        },
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_tag(id: String, state: State<'_, AppState>) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    db.execute("DELETE FROM tags WHERE id = ?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_api_token(state: State<'_, AppState>) -> String {
    state.api_token.clone()
}

#[tauri::command]
fn export_opml(state: State<'_, AppState>) -> Result<String, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    let folders: Vec<Folder> = {
        let mut stmt = db
            .prepare("SELECT id, name, parent_id, created_at FROM folders ORDER BY name")
            .map_err(|e| e.to_string())?;
        stmt.query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect()
    };

    let bookmarks: Vec<RawBookmark> = {
        let mut stmt = db
            .prepare(
                "SELECT id, url, title, description, favicon_url, feed_url, folder_id, \
                 created_at, updated_at FROM bookmarks ORDER BY created_at",
            )
            .map_err(|e| e.to_string())?;
        stmt.query_map([], row_to_raw)
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
    };

    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <opml version=\"2.0\">\n\
         <head><title>Ferrico Bookmarks</title></head>\n\
         <body>\n",
    );

    append_folder_tree(&mut xml, &folders, &bookmarks, None, 2);

    xml.push_str("</body>\n</opml>");
    Ok(xml)
}

fn append_folder_tree(
    xml: &mut String,
    folders: &[Folder],
    bookmarks: &[RawBookmark],
    parent_id: Option<&str>,
    indent: usize,
) {
    let pad = " ".repeat(indent);
    for folder in folders
        .iter()
        .filter(|f| f.parent_id.as_deref() == parent_id)
    {
        xml.push_str(&format!(
            "{}<outline text=\"{}\">\n",
            pad,
            xml_escape(&folder.name)
        ));
        for b in bookmarks
            .iter()
            .filter(|b| b.folder_id.as_deref() == Some(&folder.id))
        {
            append_outline(xml, b, indent + 2);
        }
        append_folder_tree(xml, folders, bookmarks, Some(&folder.id), indent + 2);
        xml.push_str(&format!("{}</outline>\n", pad));
    }
    if parent_id.is_none() {
        for b in bookmarks.iter().filter(|b| b.folder_id.is_none()) {
            append_outline(xml, b, indent);
        }
    }
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

    match tokio::net::TcpListener::bind("127.0.0.1:59432").await {
        Ok(listener) => {
            if let Err(e) = axum::serve(listener, app).await {
                eprintln!("HTTP server error: {e}");
            }
        }
        Err(e) => eprintln!("HTTP server failed to bind 127.0.0.1:59432: {e}"),
    }
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

// ─── Main ────────────────────────────────────────────────────────────────────

fn main() {
    let data_dir = dirs::data_dir()
        .map(|d| d.join("ferrico"))
        .unwrap_or_else(|| PathBuf::from("."));
    fs::create_dir_all(&data_dir).ok();

    let conn = init_db(&data_dir).expect("init db");
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
