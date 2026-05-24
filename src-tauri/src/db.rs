use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

use crate::error::AppError;

// ─── Types ────────────────────────────────────────────────────────────────────

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
    pub deleted_at: Option<i64>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bookmark_count: Option<i64>,
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

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub imported: usize,
    pub errors: Vec<String>,
}

#[derive(Deserialize)]
pub struct InboxSortAssignment {
    pub bookmark_id: String,
    pub folder_name: String,
}

#[derive(Serialize)]
pub struct InboxSortResult {
    pub moved: usize,
}

/// Input type for bulk CSV import. Unlike CreateBookmarkInput, this carries
/// raw folder/tag *names* which are resolved to IDs inside the transaction.
#[derive(Deserialize)]
pub struct ImportRowInput {
    pub url: String,
    pub title: String,
    pub description: Option<String>,
    pub favicon_url: Option<String>,
    pub feed_url: Option<String>,
    /// Raw folder name (top-level). Found or created once per unique name.
    pub folder_name: Option<String>,
    /// Comma- or semicolon-separated tag names. Each is found or created.
    pub tag_names: Option<String>,
}

pub(crate) struct RawBookmark {
    pub id: String,
    pub url: String,
    pub title: String,
    pub description: Option<String>,
    pub favicon_url: Option<String>,
    pub feed_url: Option<String>,
    pub folder_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

// ─── Schema ───────────────────────────────────────────────────────────────────

pub fn init_schema(conn: &Connection) -> Result<(), AppError> {
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
           updated_at  INTEGER NOT NULL,
           deleted_at  INTEGER
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
         );

         CREATE INDEX IF NOT EXISTS idx_bookmarks_deleted ON bookmarks(deleted_at);
         CREATE INDEX IF NOT EXISTS idx_bookmarks_folder  ON bookmarks(folder_id);
         CREATE INDEX IF NOT EXISTS idx_bt_tag            ON bookmark_tags(tag_id);",
    )?;

    // Migration: add deleted_at column to existing databases that predate the bin feature
    let has_deleted_at: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('bookmarks') WHERE name='deleted_at'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_deleted_at {
        conn.execute("ALTER TABLE bookmarks ADD COLUMN deleted_at INTEGER", [])?;
    }

    Ok(())
}

pub fn open_db(data_dir: &Path) -> Result<Connection, AppError> {
    let conn = Connection::open(data_dir.join("ferrico.db"))?;
    init_schema(&conn)?;
    Ok(conn)
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

pub(crate) fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn validate_url(url: &str) -> Result<(), AppError> {
    if url.trim().is_empty() {
        return Err(AppError::Validation { message: "url is required".into() });
    }
    let lower = url.trim().to_lowercase();
    for scheme in ["javascript:", "data:", "vbscript:"] {
        if lower.starts_with(scheme) {
            return Err(AppError::Validation { message: "unsafe URL scheme".into() });
        }
    }
    Ok(())
}

pub(crate) fn row_to_raw(row: &rusqlite::Row) -> rusqlite::Result<RawBookmark> {
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
        deleted_at: row.get(9)?,
    })
}

fn get_tags_batch(
    conn: &Connection,
    bookmark_ids: &[String],
) -> Result<HashMap<String, Vec<Tag>>, AppError> {
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
                bookmark_count: None,
            },
        ))
    })?;
    for row in rows.collect::<Result<Vec<_>, _>>()? {
        map.entry(row.0).or_default().push(row.1);
    }
    Ok(map)
}

fn enrich_batch(raws: Vec<RawBookmark>, conn: &Connection) -> Result<Vec<Bookmark>, AppError> {
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
                deleted_at: r.deleted_at,
            }
        })
        .collect())
}

// ─── OPML Helpers ─────────────────────────────────────────────────────────────

pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub(crate) fn append_folder_tree(
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

// ─── DB Operations ────────────────────────────────────────────────────────────

pub fn db_get_bookmarks(
    conn: &Connection,
    folder_id: Option<&str>,
    tag_id: Option<&str>,
    search: Option<&str>,
    inbox_only: bool,
) -> Result<Vec<Bookmark>, AppError> {
    // Build query dynamically so all filter combinations share one code path.
    let mut join = String::new();
    let mut wheres = vec!["b.deleted_at IS NULL".to_string()];
    let mut params: Vec<String> = Vec::new();

    if let Some(tid) = tag_id {
        join.push_str(" JOIN bookmark_tags bt ON bt.bookmark_id = b.id");
        wheres.push("bt.tag_id = ?".into());
        params.push(tid.to_string());
    }

    if inbox_only {
        wheres.push("b.folder_id IS NULL".into());
    } else if let Some(fid) = folder_id {
        wheres.push("b.folder_id = ?".into());
        params.push(fid.to_string());
    }

    // Push search into SQL so we never load a full table just to filter in Rust.
    let search_pat = search.map(|s| format!("%{}%", s.to_lowercase()));
    if let Some(ref pat) = search_pat {
        wheres.push(
            "(LOWER(b.title) LIKE ? \
              OR LOWER(b.url) LIKE ? \
              OR LOWER(COALESCE(b.description,'')) LIKE ?)"
                .into(),
        );
        params.extend([pat.clone(), pat.clone(), pat.clone()]);
    }

    let sql = format!(
        "SELECT b.id, b.url, b.title, b.description, b.favicon_url, b.feed_url, \
         b.folder_id, b.created_at, b.updated_at, b.deleted_at \
         FROM bookmarks b{} WHERE {} ORDER BY b.created_at DESC",
        join,
        wheres.join(" AND ")
    );

    let mut stmt = conn.prepare(&sql)?;
    let raws = stmt
        .query_map(params_from_iter(params.iter()), row_to_raw)?
        .collect::<Result<Vec<_>, _>>()?;

    enrich_batch(raws, conn)
}

pub fn db_get_bookmark_count(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row("SELECT COUNT(*) FROM bookmarks WHERE deleted_at IS NULL", [], |r| r.get(0))?)
}

pub fn db_add_bookmark(
    conn: &Connection,
    input: CreateBookmarkInput,
) -> Result<Bookmark, AppError> {
    validate_url(&input.url)?;
    if input.title.trim().is_empty() {
        return Err(AppError::Validation { message: "title is required".into() });
    }

    let id = Uuid::new_v4().to_string();
    let ts = now();

    conn.execute(
        "INSERT INTO bookmarks \
         (id, url, title, description, favicon_url, feed_url, folder_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id, input.url, input.title, input.description,
            input.favicon_url, input.feed_url, input.folder_id, ts, ts
        ],
    )?;

    if let Some(tag_ids) = &input.tag_ids {
        for tid in tag_ids {
            conn.execute(
                "INSERT OR IGNORE INTO bookmark_tags (bookmark_id, tag_id) VALUES (?1, ?2)",
                params![id, tid],
            )?;
        }
    }

    let tags = get_tags_batch(conn, std::slice::from_ref(&id))?.remove(&id).unwrap_or_default();

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
        deleted_at: None,
    })
}

pub fn db_delete_bookmark(conn: &Connection, id: &str) -> Result<(), AppError> {
    let n = conn.execute(
        "UPDATE bookmarks SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![now(), id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound { message: format!("bookmark {id}") });
    }
    Ok(())
}

pub fn db_get_bin_bookmarks(conn: &Connection) -> Result<Vec<Bookmark>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, url, title, description, favicon_url, feed_url, folder_id, \
         created_at, updated_at, deleted_at FROM bookmarks \
         WHERE deleted_at IS NOT NULL ORDER BY deleted_at DESC",
    )?;
    let raws = stmt.query_map([], row_to_raw)?
        .collect::<Result<Vec<_>, _>>()?;
    enrich_batch(raws, conn)
}

pub fn db_get_bin_count(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM bookmarks WHERE deleted_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?)
}

pub fn db_restore_bookmark(conn: &Connection, id: &str) -> Result<(), AppError> {
    let n = conn.execute(
        "UPDATE bookmarks SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL",
        params![id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound { message: format!("bookmark {id}") });
    }
    Ok(())
}

pub fn db_move_bookmark(conn: &Connection, id: &str, folder_id: Option<&str>) -> Result<(), AppError> {
    let n = conn.execute(
        "UPDATE bookmarks SET folder_id = ?1, updated_at = ?2 WHERE id = ?3 AND deleted_at IS NULL",
        params![folder_id, now(), id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound { message: format!("bookmark {id}") });
    }
    Ok(())
}

pub fn db_permanently_delete_bookmark(conn: &Connection, id: &str) -> Result<(), AppError> {
    let n = conn.execute(
        "DELETE FROM bookmarks WHERE id = ?1 AND deleted_at IS NOT NULL",
        params![id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound { message: format!("bookmark {id}") });
    }
    Ok(())
}

pub fn db_empty_bin(conn: &Connection) -> Result<(), AppError> {
    conn.execute("DELETE FROM bookmarks WHERE deleted_at IS NOT NULL", [])?;
    Ok(())
}

pub fn db_purge_expired_bin(conn: &Connection, days: i64) -> Result<(), AppError> {
    let cutoff = now() - days * 86400;
    conn.execute(
        "DELETE FROM bookmarks WHERE deleted_at IS NOT NULL AND deleted_at < ?1",
        params![cutoff],
    )?;
    Ok(())
}

/// Look up a top-level folder by name, or create it. Caches results so the
/// same name only hits the DB once per import call.
pub(crate) fn find_or_create_folder(
    conn: &Connection,
    name: &str,
    cache: &mut HashMap<String, String>,
) -> Result<String, AppError> {
    if let Some(id) = cache.get(name) {
        return Ok(id.clone());
    }
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM folders WHERE name = ?1 AND parent_id IS NULL LIMIT 1",
            params![name],
            |r| r.get(0),
        )
        .optional()?;
    let id = match existing {
        Some(id) => id,
        None => db_add_folder(conn, name.to_string(), None)?.id,
    };
    cache.insert(name.to_string(), id.clone());
    Ok(id)
}

/// Parse and resolve tag names from a raw CSV cell ("rust, systems, tools").
/// Splits on commas or semicolons, trims whitespace, finds or creates each tag.
pub(crate) fn find_or_create_tags(
    conn: &Connection,
    raw: &str,
    cache: &mut HashMap<String, String>,
) -> Result<Vec<String>, AppError> {
    let mut ids = Vec::new();
    for name in raw.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(id) = cache.get(name) {
            ids.push(id.clone());
        } else {
            // db_add_tag uses INSERT OR IGNORE — safe to call even if name exists
            let tag = db_add_tag(conn, name.to_string(), "#6366f1".to_string())?;
            cache.insert(name.to_string(), tag.id.clone());
            ids.push(tag.id);
        }
    }
    Ok(ids)
}

pub fn db_import_bookmarks(
    conn: &Connection,
    inputs: Vec<ImportRowInput>,
) -> Result<ImportResult, AppError> {
    // One transaction for the whole batch — orders of magnitude faster than per-row auto-commits
    let tx = conn.unchecked_transaction()?;
    let mut imported = 0usize;
    let mut errors = Vec::new();
    let mut folder_cache: HashMap<String, String> = HashMap::new();
    let mut tag_cache: HashMap<String, String> = HashMap::new();

    for (i, input) in inputs.into_iter().enumerate() {
        let folder_id = match input.folder_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(name) => match find_or_create_folder(&tx, name, &mut folder_cache) {
                Ok(id) => Some(id),
                Err(e) => { errors.push(format!("Row {}: folder: {e}", i + 1)); continue; }
            },
            None => None,
        };

        let tag_ids = match input.tag_names.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(raw) => match find_or_create_tags(&tx, raw, &mut tag_cache) {
                Ok(ids) => if ids.is_empty() { None } else { Some(ids) },
                Err(e) => { errors.push(format!("Row {}: tags: {e}", i + 1)); continue; }
            },
            None => None,
        };

        let create_input = CreateBookmarkInput {
            url: input.url,
            title: input.title,
            description: input.description,
            favicon_url: input.favicon_url,
            feed_url: input.feed_url,
            folder_id,
            tag_ids,
        };

        match db_add_bookmark(&tx, create_input) {
            Ok(_) => imported += 1,
            Err(e) => errors.push(format!("Row {}: {e}", i + 1)),
        }
    }

    tx.commit()?;
    Ok(ImportResult { imported, errors })
}

pub fn db_get_folders(conn: &Connection) -> Result<Vec<Folder>, AppError> {
    let mut stmt =
        conn.prepare("SELECT id, name, parent_id, created_at FROM folders ORDER BY name")?;
    let folders = stmt
        .query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(folders)
}

pub fn db_add_folder(
    conn: &Connection,
    name: String,
    parent_id: Option<String>,
) -> Result<Folder, AppError> {
    if name.trim().is_empty() {
        return Err(AppError::Validation { message: "folder name is required".into() });
    }
    let id = Uuid::new_v4().to_string();
    let ts = now();
    conn.execute(
        "INSERT INTO folders (id, name, parent_id, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, parent_id, ts],
    )?;
    Ok(Folder { id, name, parent_id, created_at: ts })
}

pub fn db_delete_folder(conn: &Connection, id: &str) -> Result<(), AppError> {
    let n = conn.execute("DELETE FROM folders WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound { message: format!("folder {id}") });
    }
    Ok(())
}

pub fn db_get_inbox_count(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM bookmarks WHERE folder_id IS NULL AND deleted_at IS NULL",
        [],
        |r| r.get(0),
    )?)
}

pub fn db_apply_inbox_sort(
    conn: &Connection,
    assignments: Vec<InboxSortAssignment>,
) -> Result<InboxSortResult, AppError> {
    let tx = conn.unchecked_transaction()?;
    let mut folder_cache: HashMap<String, String> = HashMap::new();
    let mut moved = 0usize;

    for assignment in assignments {
        let folder_id = find_or_create_folder(&tx, &assignment.folder_name, &mut folder_cache)?;
        let n = tx.execute(
            "UPDATE bookmarks SET folder_id = ?1, updated_at = ?2 \
             WHERE id = ?3 AND deleted_at IS NULL",
            params![folder_id, now(), assignment.bookmark_id],
        )?;
        moved += n;
    }

    tx.commit()?;
    Ok(InboxSortResult { moved })
}

pub fn db_get_tags(conn: &Connection) -> Result<Vec<Tag>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.color, t.created_at, \
         COUNT(bt.bookmark_id) as bookmark_count \
         FROM tags t \
         LEFT JOIN bookmark_tags bt ON bt.tag_id = t.id \
         GROUP BY t.id \
         ORDER BY t.name",
    )?;
    let tags = stmt
        .query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                created_at: row.get(3)?,
                bookmark_count: Some(row.get(4)?),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tags)
}

pub fn db_add_tag(
    conn: &Connection,
    name: String,
    color: String,
) -> Result<Tag, AppError> {
    if name.trim().is_empty() {
        return Err(AppError::Validation { message: "tag name is required".into() });
    }
    let id = Uuid::new_v4().to_string();
    let ts = now();
    conn.execute(
        "INSERT OR IGNORE INTO tags (id, name, color, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, color, ts],
    )?;
    // SELECT after INSERT OR IGNORE so we always return the actual record (handles name conflict)
    Ok(conn.query_row(
        "SELECT id, name, color, created_at FROM tags WHERE name = ?1",
        params![name],
        |row| {
            Ok(Tag {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                created_at: row.get(3)?,
                bookmark_count: None,
            })
        },
    )?)
}

/// Like `db_add_tag` but uses the supplied `color` for *new* tags.
/// If the tag already exists by name, the existing record (including its color)
/// is returned unchanged — same semantics as `db_add_tag`.
/// Used by JSON import to preserve exported tag colours.
pub(crate) fn db_add_tag_with_color(
    conn: &Connection,
    name: &str,
    color: &str,
) -> Result<Tag, AppError> {
    db_add_tag(conn, name.to_string(), color.to_string())
}

pub fn db_delete_tag(conn: &Connection, id: &str) -> Result<(), AppError> {
    let n = conn.execute("DELETE FROM tags WHERE id = ?1", params![id])?;
    if n == 0 {
        return Err(AppError::NotFound { message: format!("tag {id}") });
    }
    Ok(())
}

pub fn db_clear_all_data(conn: &Connection) -> Result<(), AppError> {
    // Delete junction table first, then leaf tables, then folders
    conn.execute_batch(
        "DELETE FROM bookmark_tags;
         DELETE FROM bookmarks;
         DELETE FROM tags;
         DELETE FROM folders;",
    )?;
    Ok(())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn mk_bookmark(conn: &Connection, url: &str, title: &str) -> Bookmark {
        db_add_bookmark(
            conn,
            CreateBookmarkInput {
                url: url.to_string(),
                title: title.to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap()
    }

    fn mk_folder(conn: &Connection, name: &str) -> Folder {
        db_add_folder(conn, name.to_string(), None).unwrap()
    }

    fn mk_tag(conn: &Connection, name: &str) -> Tag {
        db_add_tag(conn, name.to_string(), "#ff0000".to_string()).unwrap()
    }

    // ── Schema ────────────────────────────────────────────────────────────────

    #[test]
    fn schema_initializes_cleanly() {
        mem();
    }

    #[test]
    fn schema_is_idempotent() {
        let conn = mem();
        // Running init_schema twice must not fail (CREATE TABLE IF NOT EXISTS)
        init_schema(&conn).unwrap();
    }

    // ── Bookmarks ─────────────────────────────────────────────────────────────

    #[test]
    fn add_bookmark_roundtrip() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://example.com", "Example");
        assert_eq!(b.url, "https://example.com");
        assert_eq!(b.title, "Example");
        assert!(b.tags.is_empty());
        assert!(b.created_at > 0);
    }

    #[test]
    fn add_bookmark_with_all_fields() {
        let conn = mem();
        let folder = mk_folder(&conn, "Work");
        let tag = mk_tag(&conn, "rust");
        let b = db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://rust-lang.org".to_string(),
                title: "Rust".to_string(),
                description: Some("A systems language".to_string()),
                favicon_url: Some("https://rust-lang.org/favicon.ico".to_string()),
                feed_url: None,
                folder_id: Some(folder.id.clone()),
                tag_ids: Some(vec![tag.id.clone()]),
            },
        )
        .unwrap();

        assert_eq!(b.description.as_deref(), Some("A systems language"));
        assert_eq!(b.folder_id.as_deref(), Some(folder.id.as_str()));
        assert_eq!(b.tags.len(), 1);
        assert_eq!(b.tags[0].id, tag.id);
    }

    #[test]
    fn add_bookmark_empty_url_is_validation_error() {
        let conn = mem();
        let err = db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "  ".to_string(),
                title: "Title".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn add_bookmark_javascript_scheme_is_validation_error() {
        let conn = mem();
        for url in &["javascript:alert(1)", "JAVASCRIPT:void(0)", "JavaScript:x"] {
            let err = db_add_bookmark(
                &conn,
                CreateBookmarkInput {
                    url: url.to_string(),
                    title: "Bad".to_string(),
                    description: None,
                    favicon_url: None,
                    feed_url: None,
                    folder_id: None,
                    tag_ids: None,
                },
            )
            .unwrap_err();
            assert!(matches!(err, AppError::Validation { .. }), "expected Validation for {url}");
        }
    }

    #[test]
    fn add_bookmark_data_scheme_is_validation_error() {
        let conn = mem();
        let err = db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "data:text/html,<h1>hi</h1>".to_string(),
                title: "Data".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn search_combined_with_inbox_only() {
        let conn = mem();
        let folder = mk_folder(&conn, "Work");

        // In inbox, matches search
        mk_bookmark(&conn, "https://rust-lang.org", "Rust Language");
        // In inbox, doesn't match search
        mk_bookmark(&conn, "https://python.org", "Python");
        // In folder — excluded even though title matches search
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-book.com".to_string(),
            title: "Rust Book".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(folder.id.clone()),
            tag_ids: None,
        }).unwrap();

        let results = db_get_bookmarks(&conn, None, None, Some("rust"), true).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust Language");
    }

    #[test]
    fn search_combined_with_tag_filter() {
        let conn = mem();
        let tag = mk_tag(&conn, "systems");
        let b1 = mk_bookmark(&conn, "https://rust-lang.org", "Rust");
        let _b2 = mk_bookmark(&conn, "https://go.dev", "Go");
        let b3 = mk_bookmark(&conn, "https://other.com", "systems journal");

        // Tag both rust and "systems journal"
        for bid in &[&b1.id, &b3.id] {
            conn.execute(
                "INSERT INTO bookmark_tags (bookmark_id, tag_id) VALUES (?1, ?2)",
                params![bid, tag.id],
            ).unwrap();
        }

        // Tag filter + search: must have the tag AND match "rust"
        let results = db_get_bookmarks(&conn, None, Some(&tag.id), Some("rust"), false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, b1.id);
    }

    #[test]
    fn add_bookmark_empty_title_is_validation_error() {
        let conn = mem();
        let err = db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://example.com".to_string(),
                title: "".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn get_bookmarks_all() {
        let conn = mem();
        mk_bookmark(&conn, "https://a.com", "A");
        mk_bookmark(&conn, "https://b.com", "B");
        let all = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(all.len(), 2);
        let titles: std::collections::HashSet<&str> = all.iter().map(|b| b.title.as_str()).collect();
        assert!(titles.contains("A"));
        assert!(titles.contains("B"));
    }

    #[test]
    fn get_bookmarks_by_folder() {
        let conn = mem();
        let folder = mk_folder(&conn, "Work");
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://work.com".to_string(),
                title: "Work".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: Some(folder.id.clone()),
                tag_ids: None,
            },
        )
        .unwrap();
        mk_bookmark(&conn, "https://other.com", "Other");

        let results = db_get_bookmarks(&conn, Some(&folder.id), None, None, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Work");
    }

    #[test]
    fn get_bookmarks_by_tag() {
        let conn = mem();
        let tag = mk_tag(&conn, "rust");
        let b1 = mk_bookmark(&conn, "https://rust-lang.org", "Rust");
        mk_bookmark(&conn, "https://python.org", "Python");

        conn.execute(
            "INSERT INTO bookmark_tags (bookmark_id, tag_id) VALUES (?1, ?2)",
            params![b1.id, tag.id],
        )
        .unwrap();

        let results = db_get_bookmarks(&conn, None, Some(&tag.id), None, false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Rust");
    }

    #[test]
    fn get_bookmarks_search_title() {
        let conn = mem();
        mk_bookmark(&conn, "https://rust-lang.org", "The Rust Programming Language");
        mk_bookmark(&conn, "https://python.org", "Python");

        let results = db_get_bookmarks(&conn, None, None, Some("rust"), false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "The Rust Programming Language");
    }

    #[test]
    fn get_bookmarks_search_url() {
        let conn = mem();
        mk_bookmark(&conn, "https://docs.rs/tokio", "Tokio docs");
        mk_bookmark(&conn, "https://crates.io", "Crates");

        let results = db_get_bookmarks(&conn, None, None, Some("docs.rs"), false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://docs.rs/tokio");
    }

    #[test]
    fn get_bookmarks_search_description() {
        let conn = mem();
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://example.com".to_string(),
                title: "Example".to_string(),
                description: Some("async runtime notes".to_string()),
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap();
        mk_bookmark(&conn, "https://other.com", "Other");

        let results = db_get_bookmarks(&conn, None, None, Some("async runtime"), false).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn get_bookmarks_search_case_insensitive() {
        let conn = mem();
        mk_bookmark(&conn, "https://rust-lang.org", "The Rust Language");
        let results = db_get_bookmarks(&conn, None, None, Some("RUST"), false).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn get_bookmark_count() {
        let conn = mem();
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 0);
        mk_bookmark(&conn, "https://a.com", "A");
        mk_bookmark(&conn, "https://b.com", "B");
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 2);
    }

    #[test]
    fn delete_bookmark_removes_it() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://example.com", "Example");
        db_delete_bookmark(&conn, &b.id).unwrap();
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 0);
    }

    #[test]
    fn delete_bookmark_soft_delete_preserves_tags() {
        let conn = mem();
        let tag = mk_tag(&conn, "rust");
        let b = db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://rust-lang.org".to_string(),
                title: "Rust".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: Some(vec![tag.id.clone()]),
            },
        )
        .unwrap();

        db_delete_bookmark(&conn, &b.id).unwrap();

        // Soft delete — bookmark_tags survive so restore brings tags back
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bookmark_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn empty_bin_cascades_bookmark_tags() {
        let conn = mem();
        let tag = mk_tag(&conn, "rust");
        let b = db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://rust-lang.org".to_string(),
                title: "Rust".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: Some(vec![tag.id.clone()]),
            },
        )
        .unwrap();

        db_delete_bookmark(&conn, &b.id).unwrap();
        db_empty_bin(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bookmark_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_bookmark_not_found_returns_error() {
        let conn = mem();
        let err = db_delete_bookmark(&conn, "nonexistent-id").unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    // ── Folders ───────────────────────────────────────────────────────────────

    #[test]
    fn add_and_get_folder() {
        let conn = mem();
        let f = mk_folder(&conn, "Reading List");
        assert_eq!(f.name, "Reading List");
        assert!(f.parent_id.is_none());

        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].id, f.id);
    }

    #[test]
    fn add_nested_folder() {
        let conn = mem();
        let parent = mk_folder(&conn, "Work");
        let child =
            db_add_folder(&conn, "Projects".to_string(), Some(parent.id.clone())).unwrap();

        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    }

    #[test]
    fn add_folder_empty_name_is_validation_error() {
        let conn = mem();
        let err = db_add_folder(&conn, "".to_string(), None).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn delete_folder_sets_bookmark_folder_null() {
        let conn = mem();
        let folder = mk_folder(&conn, "Work");
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://work.com".to_string(),
                title: "Work".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: Some(folder.id.clone()),
                tag_ids: None,
            },
        )
        .unwrap();

        db_delete_folder(&conn, &folder.id).unwrap();

        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks.len(), 1);
        // folder_id SET NULL on folder delete
        assert!(bookmarks[0].folder_id.is_none());
    }

    #[test]
    fn delete_folder_not_found_returns_error() {
        let conn = mem();
        let err = db_delete_folder(&conn, "nonexistent").unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    // ── Tags ──────────────────────────────────────────────────────────────────

    #[test]
    fn add_and_get_tag() {
        let conn = mem();
        let t = mk_tag(&conn, "design");
        assert_eq!(t.name, "design");
        assert_eq!(t.color, "#ff0000");

        let tags = db_get_tags(&conn).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].id, t.id);
    }

    #[test]
    fn add_tag_duplicate_name_returns_existing() {
        let conn = mem();
        let t1 = mk_tag(&conn, "rust");
        let t2 = db_add_tag(&conn, "rust".to_string(), "#00ff00".to_string()).unwrap();
        // Same id — INSERT OR IGNORE kept the first one
        assert_eq!(t1.id, t2.id);
        // Original color preserved
        assert_eq!(t2.color, "#ff0000");
    }

    #[test]
    fn add_tag_empty_name_is_validation_error() {
        let conn = mem();
        let err = db_add_tag(&conn, "".to_string(), "#ff0000".to_string()).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn delete_tag_removes_it() {
        let conn = mem();
        let t = mk_tag(&conn, "rust");
        db_delete_tag(&conn, &t.id).unwrap();
        assert_eq!(db_get_tags(&conn).unwrap().len(), 0);
    }

    #[test]
    fn delete_tag_cascades_bookmark_tags() {
        let conn = mem();
        let tag = mk_tag(&conn, "rust");
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://rust-lang.org".to_string(),
                title: "Rust".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: Some(vec![tag.id.clone()]),
            },
        )
        .unwrap();

        db_delete_tag(&conn, &tag.id).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM bookmark_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn delete_tag_not_found_returns_error() {
        let conn = mem();
        let err = db_delete_tag(&conn, "nonexistent").unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    // ── OPML ──────────────────────────────────────────────────────────────────

    #[test]
    fn xml_escape_special_chars() {
        assert_eq!(xml_escape("a & b"), "a &amp; b");
        assert_eq!(xml_escape("say \"hi\""), "say &quot;hi&quot;");
        assert_eq!(xml_escape("<tag>"), "&lt;tag&gt;");
        assert_eq!(xml_escape("plain text"), "plain text");
    }

    #[test]
    fn export_opml_empty_db() {
        let conn = mem();
        let opml = crate::io::export_opml(&conn).unwrap();
        assert!(opml.contains("<?xml"));
        assert!(opml.contains("<opml version=\"2.0\">"));
        assert!(opml.contains("<body>"));
        assert!(opml.contains("</body>"));
    }

    #[test]
    fn export_opml_flat_bookmarks() {
        let conn = mem();
        mk_bookmark(&conn, "https://example.com", "Example");
        let opml = crate::io::export_opml(&conn).unwrap();
        assert!(opml.contains("url=\"https://example.com\""));
        assert!(opml.contains("text=\"Example\""));
    }

    #[test]
    fn export_opml_with_folder() {
        let conn = mem();
        let folder = mk_folder(&conn, "Work");
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://work.com".to_string(),
                title: "Work Site".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: Some(folder.id.clone()),
                tag_ids: None,
            },
        )
        .unwrap();

        let opml = crate::io::export_opml(&conn).unwrap();
        assert!(opml.contains("<outline text=\"Work\">"));
        assert!(opml.contains("url=\"https://work.com\""));
    }

    #[test]
    fn export_opml_escapes_special_chars_in_title() {
        let conn = mem();
        mk_bookmark(&conn, "https://example.com", "A & B <test>");
        let opml = crate::io::export_opml(&conn).unwrap();
        assert!(opml.contains("text=\"A &amp; B &lt;test&gt;\""));
        assert!(!opml.contains("text=\"A & B <test>\""));
    }

    #[test]
    fn export_opml_includes_description_and_feed_url() {
        let conn = mem();
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://blog.example.com".to_string(),
                title: "Blog".to_string(),
                description: Some("A great blog".to_string()),
                favicon_url: None,
                feed_url: Some("https://blog.example.com/feed.xml".to_string()),
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap();

        let opml = crate::io::export_opml(&conn).unwrap();
        assert!(opml.contains("description=\"A great blog\""));
        assert!(opml.contains("xmlUrl=\"https://blog.example.com/feed.xml\""));
    }

    // ── Error type ────────────────────────────────────────────────────────────

    #[test]
    fn app_error_display_messages() {
        assert!(AppError::Db { message: "oops".into() }.to_string().contains("oops"));
        assert!(AppError::Lock { message: "poisoned".into() }.to_string().contains("lock"));
        assert!(AppError::NotFound { message: "x".into() }.to_string().contains("not found"));
        assert!(AppError::Validation { message: "bad".into() }.to_string().contains("bad"));
    }

    #[test]
    fn rusqlite_error_converts_to_app_error() {
        let err: AppError = rusqlite::Error::QueryReturnedNoRows.into();
        assert!(matches!(err, AppError::Db { .. }));
    }

    // ── Bin ───────────────────────────────────────────────────────────────────

    #[test]
    fn delete_bookmark_moves_to_bin() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://example.com", "Example");
        db_delete_bookmark(&conn, &b.id).unwrap();

        // Active list is empty
        assert_eq!(db_get_bookmarks(&conn, None, None, None, false).unwrap().len(), 0);
        // Bin has the item
        let bin = db_get_bin_bookmarks(&conn).unwrap();
        assert_eq!(bin.len(), 1);
        assert_eq!(bin[0].id, b.id);
        assert!(bin[0].deleted_at.is_some());
    }

    #[test]
    fn delete_already_binned_bookmark_returns_not_found() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://example.com", "Example");
        db_delete_bookmark(&conn, &b.id).unwrap();
        let err = db_delete_bookmark(&conn, &b.id).unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[test]
    fn get_bin_count() {
        let conn = mem();
        assert_eq!(db_get_bin_count(&conn).unwrap(), 0);
        let b1 = mk_bookmark(&conn, "https://a.com", "A");
        let b2 = mk_bookmark(&conn, "https://b.com", "B");
        db_delete_bookmark(&conn, &b1.id).unwrap();
        assert_eq!(db_get_bin_count(&conn).unwrap(), 1);
        db_delete_bookmark(&conn, &b2.id).unwrap();
        assert_eq!(db_get_bin_count(&conn).unwrap(), 2);
    }

    #[test]
    fn restore_bookmark_returns_to_active_list() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://example.com", "Example");
        db_delete_bookmark(&conn, &b.id).unwrap();
        db_restore_bookmark(&conn, &b.id).unwrap();

        let active = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(active.len(), 1);
        assert!(active[0].deleted_at.is_none());
        assert_eq!(db_get_bin_bookmarks(&conn).unwrap().len(), 0);
    }

    #[test]
    fn restore_bookmark_not_in_bin_returns_not_found() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://example.com", "Example");
        let err = db_restore_bookmark(&conn, &b.id).unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[test]
    fn empty_bin_removes_all_binned() {
        let conn = mem();
        let b1 = mk_bookmark(&conn, "https://a.com", "A");
        let b2 = mk_bookmark(&conn, "https://b.com", "B");
        db_delete_bookmark(&conn, &b1.id).unwrap();
        db_delete_bookmark(&conn, &b2.id).unwrap();

        db_empty_bin(&conn).unwrap();
        assert_eq!(db_get_bin_count(&conn).unwrap(), 0);
    }

    #[test]
    fn purge_expired_bin_removes_old_only() {
        let conn = mem();
        let b1 = mk_bookmark(&conn, "https://old.com", "Old");
        let b2 = mk_bookmark(&conn, "https://new.com", "New");

        // Manually insert b1 with a deleted_at 31 days ago
        let old_ts = now() - 31 * 86400;
        conn.execute(
            "UPDATE bookmarks SET deleted_at = ?1 WHERE id = ?2",
            params![old_ts, b1.id],
        )
        .unwrap();
        // b2 deleted just now via soft-delete
        db_delete_bookmark(&conn, &b2.id).unwrap();

        db_purge_expired_bin(&conn, 30).unwrap();

        let bin = db_get_bin_bookmarks(&conn).unwrap();
        assert_eq!(bin.len(), 1);
        assert_eq!(bin[0].id, b2.id);
    }

    #[test]
    fn active_bookmark_count_excludes_bin() {
        let conn = mem();
        mk_bookmark(&conn, "https://a.com", "A");
        let b2 = mk_bookmark(&conn, "https://b.com", "B");
        db_delete_bookmark(&conn, &b2.id).unwrap();
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 1);
    }

    // ── db_import_bookmarks ───────────────────────────────────────────────────

    fn mk_input(url: &str, title: &str) -> ImportRowInput {
        ImportRowInput {
            url: url.to_string(),
            title: title.to_string(),
            description: None,
            favicon_url: None,
            feed_url: None,
            folder_name: None,
            tag_names: None,
        }
    }

    #[test]
    fn import_bookmarks_inserts_all_valid_rows() {
        let conn = mem();
        let inputs = vec![
            mk_input("https://a.com", "A"),
            mk_input("https://b.com", "B"),
            mk_input("https://c.com", "C"),
        ];
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(result.imported, 3);
        assert!(result.errors.is_empty());
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 3);
    }

    #[test]
    fn import_bookmarks_skips_rows_with_empty_url() {
        let conn = mem();
        let inputs = vec![
            mk_input("https://a.com", "A"),
            mk_input("", "Empty URL"),
            mk_input("https://b.com", "B"),
        ];
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(result.imported, 2);
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Row 2"));
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 2);
    }

    #[test]
    fn import_bookmarks_skips_rows_with_empty_title() {
        let conn = mem();
        let inputs = vec![
            mk_input("https://a.com", "A"),
            mk_input("https://b.com", ""),
        ];
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(result.imported, 1);
        assert_eq!(result.errors.len(), 1);
    }

    #[test]
    fn import_bookmarks_all_invalid_produces_zero_imported() {
        let conn = mem();
        let inputs = vec![
            mk_input("", "No URL"),
            mk_input("https://b.com", ""),
        ];
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(result.imported, 0);
        assert_eq!(result.errors.len(), 2);
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 0);
    }

    #[test]
    fn import_bookmarks_empty_list_returns_zero() {
        let conn = mem();
        let result = db_import_bookmarks(&conn, vec![]).unwrap();
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn import_bookmarks_creates_folder_by_name() {
        let conn = mem();
        let inputs = vec![ImportRowInput {
            url: "https://a.com".to_string(),
            title: "A".to_string(),
            folder_name: Some("Work".to_string()),
            ..mk_input("", "")
        }];
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(result.imported, 1);
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Work");
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks[0].folder_id.as_deref(), Some(folders[0].id.as_str()));
    }

    #[test]
    fn import_bookmarks_reuses_existing_folder() {
        let conn = mem();
        let existing = mk_folder(&conn, "Work");
        let inputs = vec![
            ImportRowInput { url: "https://a.com".to_string(), title: "A".to_string(), folder_name: Some("Work".to_string()), ..mk_input("", "") },
            ImportRowInput { url: "https://b.com".to_string(), title: "B".to_string(), folder_name: Some("Work".to_string()), ..mk_input("", "") },
        ];
        db_import_bookmarks(&conn, inputs).unwrap();
        // Still only one folder
        assert_eq!(db_get_folders(&conn).unwrap().len(), 1);
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert!(bookmarks.iter().all(|b| b.folder_id.as_deref() == Some(existing.id.as_str())));
    }

    #[test]
    fn import_bookmarks_creates_tags_by_name() {
        let conn = mem();
        let inputs = vec![ImportRowInput {
            url: "https://a.com".to_string(),
            title: "A".to_string(),
            tag_names: Some("rust, systems".to_string()),
            ..mk_input("", "")
        }];
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(result.imported, 1);
        let tags = db_get_tags(&conn).unwrap();
        let names: std::collections::HashSet<&str> = tags.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains("rust"));
        assert!(names.contains("systems"));
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks[0].tags.len(), 2);
    }

    #[test]
    fn import_bookmarks_semicolon_separated_tags() {
        let conn = mem();
        let inputs = vec![ImportRowInput {
            url: "https://a.com".to_string(),
            title: "A".to_string(),
            tag_names: Some("design;ux;css".to_string()),
            ..mk_input("", "")
        }];
        db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(db_get_tags(&conn).unwrap().len(), 3);
    }

    #[test]
    fn import_bookmarks_reuses_existing_tags() {
        let conn = mem();
        mk_tag(&conn, "rust");
        let inputs = vec![
            ImportRowInput { url: "https://a.com".to_string(), title: "A".to_string(), tag_names: Some("rust, systems".to_string()), ..mk_input("", "") },
            ImportRowInput { url: "https://b.com".to_string(), title: "B".to_string(), tag_names: Some("rust".to_string()), ..mk_input("", "") },
        ];
        db_import_bookmarks(&conn, inputs).unwrap();
        // Only 2 unique tags total ("rust" pre-existed, "systems" is new)
        assert_eq!(db_get_tags(&conn).unwrap().len(), 2);
    }

    // ── Inbox ─────────────────────────────────────────────────────────────────

    #[test]
    fn inbox_count_only_counts_active_unfoldered_bookmarks() {
        let conn = mem();
        let folder = mk_folder(&conn, "Work");

        // In inbox (no folder, not deleted)
        mk_bookmark(&conn, "https://a.com", "A");
        mk_bookmark(&conn, "https://b.com", "B");

        // Has a folder — not inbox
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://c.com".to_string(),
            title: "C".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(folder.id.clone()),
            tag_ids: None,
        }).unwrap();

        // Deleted (in bin) — not inbox even though folder_id IS NULL
        let d = mk_bookmark(&conn, "https://d.com", "D");
        db_delete_bookmark(&conn, &d.id).unwrap();

        assert_eq!(db_get_inbox_count(&conn).unwrap(), 2);
    }

    #[test]
    fn inbox_view_excludes_binned_and_foldered_bookmarks() {
        let conn = mem();
        let folder = mk_folder(&conn, "Work");

        let a = mk_bookmark(&conn, "https://a.com", "A"); // inbox
        let _b = db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://b.com".to_string(),
            title: "B".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(folder.id.clone()),
            tag_ids: None,
        }).unwrap(); // in folder, not inbox
        let c = mk_bookmark(&conn, "https://c.com", "C"); // will be binned
        db_delete_bookmark(&conn, &c.id).unwrap();

        let inbox = db_get_bookmarks(&conn, None, None, None, true).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].id, a.id);
    }

    #[test]
    fn apply_inbox_sort_moves_bookmark_to_folder() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://a.com", "A");

        let result = db_apply_inbox_sort(&conn, vec![
            InboxSortAssignment { bookmark_id: b.id.clone(), folder_name: "Tech".to_string() },
        ]).unwrap();

        assert_eq!(result.moved, 1);
        let inbox = db_get_bookmarks(&conn, None, None, None, true).unwrap();
        assert_eq!(inbox.len(), 0);
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Tech");
    }

    #[test]
    fn apply_inbox_sort_reuses_existing_folder() {
        let conn = mem();
        let existing = mk_folder(&conn, "Tech");
        let b = mk_bookmark(&conn, "https://a.com", "A");

        db_apply_inbox_sort(&conn, vec![
            InboxSortAssignment { bookmark_id: b.id.clone(), folder_name: "Tech".to_string() },
        ]).unwrap();

        assert_eq!(db_get_folders(&conn).unwrap().len(), 1);
        let bookmarks = db_get_bookmarks(&conn, Some(&existing.id), None, None, false).unwrap();
        assert_eq!(bookmarks.len(), 1);
    }

    #[test]
    fn apply_inbox_sort_skips_binned_bookmarks() {
        let conn = mem();
        let b = mk_bookmark(&conn, "https://a.com", "A");
        db_delete_bookmark(&conn, &b.id).unwrap(); // move to bin

        let result = db_apply_inbox_sort(&conn, vec![
            InboxSortAssignment { bookmark_id: b.id.clone(), folder_name: "Tech".to_string() },
        ]).unwrap();

        // Binned bookmark must not be moved — the UPDATE hits deleted_at IS NULL guard
        assert_eq!(result.moved, 0);
        assert_eq!(db_get_bin_count(&conn).unwrap(), 1);
    }

    #[test]
    fn inbox_count_decreases_when_bookmark_sorted() {
        let conn = mem();
        mk_bookmark(&conn, "https://a.com", "A");
        let b = mk_bookmark(&conn, "https://b.com", "B");
        assert_eq!(db_get_inbox_count(&conn).unwrap(), 2);

        db_apply_inbox_sort(&conn, vec![
            InboxSortAssignment { bookmark_id: b.id.clone(), folder_name: "Work".to_string() },
        ]).unwrap();

        assert_eq!(db_get_inbox_count(&conn).unwrap(), 1);
    }

    #[test]
    fn inbox_count_decreases_when_bookmark_deleted() {
        let conn = mem();
        mk_bookmark(&conn, "https://a.com", "A");
        let b = mk_bookmark(&conn, "https://b.com", "B");
        assert_eq!(db_get_inbox_count(&conn).unwrap(), 2);

        db_delete_bookmark(&conn, &b.id).unwrap();

        assert_eq!(db_get_inbox_count(&conn).unwrap(), 1);
    }

    #[test]
    fn import_bookmarks_1000_rows_completes_quickly() {
        // Verifies the transaction path: 1000 inserts should take well under 5s
        // even in debug mode. Without a transaction each auto-commit can take
        // milliseconds (fsync), so 1000 rows could take 10-30s without this fix.
        let conn = mem();
        let inputs: Vec<_> = (0..1000)
            .map(|i| mk_input(&format!("https://example.com/{i}"), &format!("Bookmark {i}")))
            .collect();

        let start = std::time::Instant::now();
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(result.imported, 1000);
        assert!(result.errors.is_empty());
        assert_eq!(db_get_bookmark_count(&conn).unwrap(), 1000);
        assert!(elapsed.as_secs() < 5, "1000-row import took {elapsed:?}, expected < 5s");
    }
}
