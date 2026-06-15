use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    pub cover_url: Option<String>,
    pub feed_url: Option<String>,
    pub folder_id: Option<String>,
    pub tags: Vec<Tag>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    pub is_broken: bool,
    pub last_checked_at: Option<i64>,
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
    pub cover_url: Option<String>,
    pub feed_url: Option<String>,
    pub folder_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    pub is_broken: bool,
    pub last_checked_at: Option<i64>,
}

/// Sidebar badge counts. Computed in a single table scan rather than four
/// separate `COUNT(*)` queries, then shipped to the frontend in one payload.
#[derive(Debug, Serialize)]
pub struct Counts {
    pub total: i64,
    pub inbox: i64,
    pub bin: i64,
    pub broken: i64,
}

/// Everything the sidebar needs in one round trip: folder + tag lists and all
/// badge counts. Folded into a single command so navigation never has to fire a
/// fan-out of independent `invoke`s that each contend on the DB mutex.
#[derive(Debug, Serialize)]
pub struct SidebarData {
    pub folders: Vec<Folder>,
    pub tags: Vec<Tag>,
    pub counts: Counts,
}

// ─── Schema ───────────────────────────────────────────────────────────────────

pub fn init_schema(conn: &Connection) -> Result<(), AppError> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         -- Performance pragmas. synchronous=NORMAL is safe under WAL (only risks
         -- losing the last commit on OS crash, never corruption) and removes an
         -- fsync from every write. The rest keep temp data and the page cache in
         -- memory and let readers mmap the file instead of syscalling per page.
         PRAGMA synchronous = NORMAL;
         PRAGMA temp_store = MEMORY;
         PRAGMA cache_size = -16384;
         PRAGMA mmap_size = 134217728;
         PRAGMA busy_timeout = 5000;

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

         CREATE INDEX IF NOT EXISTS idx_bookmarks_folder  ON bookmarks(folder_id);
         CREATE INDEX IF NOT EXISTS idx_bookmarks_created ON bookmarks(created_at);
         CREATE INDEX IF NOT EXISTS idx_bt_tag            ON bookmark_tags(tag_id);",
    )?;

    // Migration: add deleted_at column to existing databases that predate the bin feature.
    // The idx_bookmarks_deleted index must be created after this migration because SQLite
    // will reject CREATE INDEX on a column that doesn't exist yet.
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
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_bookmarks_deleted ON bookmarks(deleted_at);",
    )?;

    // Migration: add is_broken and last_checked_at for link health checking.
    // Each column is checked independently — a partial migration may leave one present
    // but not the other, and a combined `if` would silently skip the missing one.
    let has_is_broken: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('bookmarks') WHERE name='is_broken'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_is_broken {
        conn.execute(
            "ALTER TABLE bookmarks ADD COLUMN is_broken INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    let has_last_checked_at: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('bookmarks') WHERE name='last_checked_at'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_last_checked_at {
        conn.execute("ALTER TABLE bookmarks ADD COLUMN last_checked_at INTEGER", [])?;
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_bookmarks_broken ON bookmarks(is_broken) WHERE is_broken = 1;",
    )?;

    // Migration: add cover_url for OG image / screenshot covers.
    let has_cover_url: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM pragma_table_info('bookmarks') WHERE name='cover_url'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap_or(0)
        > 0;
    if !has_cover_url {
        conn.execute("ALTER TABLE bookmarks ADD COLUMN cover_url TEXT", [])?;
    }

    // Migration: per-record sync (multi-machine merge) needs an `updated_at`
    // clock and a `deleted_at` tombstone on folders and tags too — bookmarks
    // already carry both. Additive + idempotent. Existing rows backfill
    // `updated_at` from `created_at` so the merge has a clock to compare; a
    // freshly-migrated DB therefore loses no races against a remote that has
    // genuinely newer edits.
    for table in ["folders", "tags"] {
        let has_updated_at: bool = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='updated_at'"
                ),
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !has_updated_at {
            conn.execute(
                &format!("ALTER TABLE {table} ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0"),
                [],
            )?;
            conn.execute(
                &format!("UPDATE {table} SET updated_at = created_at WHERE updated_at = 0"),
                [],
            )?;
        }
        let has_deleted_at: bool = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='deleted_at'"
                ),
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if !has_deleted_at {
            conn.execute(&format!("ALTER TABLE {table} ADD COLUMN deleted_at INTEGER"), [])?;
        }
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
        cover_url: row.get(5)?,
        feed_url: row.get(6)?,
        folder_id: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        deleted_at: row.get(10)?,
        is_broken: row.get::<_, i64>(11)? != 0,
        last_checked_at: row.get(12)?,
    })
}

fn get_tags_batch(
    conn: &Connection,
    bookmark_ids: &[&str],
) -> Result<HashMap<String, Vec<Tag>>, AppError> {
    let mut map: HashMap<String, Vec<Tag>> = HashMap::new();
    if bookmark_ids.is_empty() {
        return Ok(map);
    }
    // SQLite caps bound parameters at SQLITE_MAX_VARIABLE_NUMBER (~32k), so chunk
    // the id list — otherwise the "All" view errors outright on a huge library.
    // A bookmark's rows are never split across chunks, so tag order is preserved.
    const CHUNK: usize = 10_000;
    for chunk in bookmark_ids.chunks(CHUNK) {
        let placeholders = (1..=chunk.len())
            .map(|i| format!("?{i}"))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT bt.bookmark_id, t.id, t.name, t.color, t.created_at \
             FROM tags t JOIN bookmark_tags bt ON bt.tag_id = t.id \
             WHERE bt.bookmark_id IN ({placeholders}) AND t.deleted_at IS NULL \
             ORDER BY t.name"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(chunk.iter().copied()), |row| {
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
    }
    Ok(map)
}

fn enrich_batch(raws: Vec<RawBookmark>, conn: &Connection) -> Result<Vec<Bookmark>, AppError> {
    let ids: Vec<&str> = raws.iter().map(|r| r.id.as_str()).collect();
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
                cover_url: r.cover_url,
                feed_url: r.feed_url,
                folder_id: r.folder_id,
                tags,
                created_at: r.created_at,
                updated_at: r.updated_at,
                deleted_at: r.deleted_at,
                is_broken: r.is_broken,
                last_checked_at: r.last_checked_at,
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
    let mut visited = HashSet::new();
    append_folder_tree_inner(xml, folders, bookmarks, parent_id, indent, &mut visited);
}

fn append_folder_tree_inner(
    xml: &mut String,
    folders: &[Folder],
    bookmarks: &[RawBookmark],
    parent_id: Option<&str>,
    indent: usize,
    visited: &mut HashSet<String>,
) {
    let pad = " ".repeat(indent);
    for folder in folders
        .iter()
        .filter(|f| f.parent_id.as_deref() == parent_id)
    {
        if !visited.insert(folder.id.clone()) {
            continue; // cycle in folder tree — skip to avoid infinite recursion
        }
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
        append_folder_tree_inner(xml, folders, bookmarks, Some(&folder.id), indent + 2, visited);
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

    // Scope filters (folder/tag/inbox) run in SQL. The text query is applied
    // afterwards as a fuzzy rank in Rust (see below), so it is not part of the
    // WHERE clause — fuzzy matching can't be expressed as a SQL LIKE.
    let sql = format!(
        "SELECT b.id, b.url, b.title, b.description, b.favicon_url, b.cover_url, b.feed_url, \
         b.folder_id, b.created_at, b.updated_at, b.deleted_at, b.is_broken, b.last_checked_at \
         FROM bookmarks b{} WHERE {} ORDER BY b.created_at DESC",
        join,
        wheres.join(" AND ")
    );

    let mut stmt = conn.prepare(&sql)?;
    let raws = stmt
        .query_map(params_from_iter(params.iter()), row_to_raw)?
        .collect::<Result<Vec<_>, _>>()?;

    // When a (non-blank) query is present, fuzzy-rank the scope-filtered
    // candidates by relevance and drop non-matches. Tags are only hydrated for
    // the surviving rows, so enrichment cost stays bound to the result set.
    let raws = match search.map(str::trim).filter(|q| !q.is_empty()) {
        None => raws,
        Some(query) => rank_fuzzy(query, raws),
    };

    enrich_batch(raws, conn)
}

// Field weights for combining per-field fuzzy scores. Title dominates; the body
// (description) only nudges ties so a long note never outranks a title hit.
const FUZZY_TITLE_WEIGHT: u32 = 3;
const FUZZY_URL_WEIGHT: u32 = 2;
const FUZZY_DESC_WEIGHT: u32 = 1;

/// Fuzzy-rank `raws` against `query`, keeping only matches, best score first.
/// Equal scores retain the input order (newest-first from the SQL `ORDER BY`),
/// since `sort_by` is stable.
fn rank_fuzzy(query: &str, raws: Vec<RawBookmark>) -> Vec<RawBookmark> {
    let mut matcher = Matcher::new(Config::DEFAULT);
    // Always case-insensitive (preserves the old `LOWER(..) LIKE` semantics);
    // smart unicode normalization so e.g. "cafe" still matches "café".
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let mut buf: Vec<char> = Vec::new();

    let mut scored: Vec<(u32, RawBookmark)> = raws
        .into_iter()
        .filter_map(|raw| {
            fuzzy_score(&mut matcher, &pattern, &mut buf, &raw).map(|score| (score, raw))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().map(|(_, raw)| raw).collect()
}

/// Combined fuzzy relevance of one bookmark. Scores title, url and description
/// independently and sums the fields that matched (each weighted). Returns
/// `None` only when no field matches at all, so the row can be filtered out.
fn fuzzy_score(
    matcher: &mut Matcher,
    pattern: &Pattern,
    buf: &mut Vec<char>,
    raw: &RawBookmark,
) -> Option<u32> {
    let title = score_field(matcher, pattern, buf, &raw.title, FUZZY_TITLE_WEIGHT);
    let url = score_field(matcher, pattern, buf, &raw.url, FUZZY_URL_WEIGHT);
    let desc = raw
        .description
        .as_deref()
        .and_then(|d| score_field(matcher, pattern, buf, d, FUZZY_DESC_WEIGHT));

    [title, url, desc]
        .into_iter()
        .flatten()
        .reduce(|a, b| a.saturating_add(b))
}

fn score_field(
    matcher: &mut Matcher,
    pattern: &Pattern,
    buf: &mut Vec<char>,
    text: &str,
    weight: u32,
) -> Option<u32> {
    pattern
        .score(Utf32Str::new(text, buf), matcher)
        .map(|s| s.saturating_mul(weight))
}

pub fn db_get_bookmark_count(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row("SELECT COUNT(*) FROM bookmarks WHERE deleted_at IS NULL", [], |r| r.get(0))?)
}

/// All four sidebar counts in one scan of the bookmarks table. `COUNT(*) FILTER`
/// lets SQLite tally every bucket in a single pass instead of four queries.
pub fn db_get_counts(conn: &Connection) -> Result<Counts, AppError> {
    conn.query_row(
        "SELECT \
           COUNT(*) FILTER (WHERE deleted_at IS NULL) AS total, \
           COUNT(*) FILTER (WHERE deleted_at IS NULL AND folder_id IS NULL) AS inbox, \
           COUNT(*) FILTER (WHERE deleted_at IS NOT NULL) AS bin, \
           COUNT(*) FILTER (WHERE deleted_at IS NULL AND is_broken = 1) AS broken \
         FROM bookmarks",
        [],
        |r| Ok(Counts { total: r.get(0)?, inbox: r.get(1)?, bin: r.get(2)?, broken: r.get(3)? }),
    )
    .map_err(Into::into)
}

/// Folders, tags and counts for the sidebar, gathered under a single mutex lock.
pub fn db_get_sidebar(conn: &Connection) -> Result<SidebarData, AppError> {
    Ok(SidebarData {
        folders: db_get_folders(conn)?,
        tags: db_get_tags(conn)?,
        counts: db_get_counts(conn)?,
    })
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

    let tags = get_tags_batch(conn, &[id.as_str()])?.remove(&id).unwrap_or_default();

    Ok(Bookmark {
        id,
        url: input.url,
        title: input.title,
        description: input.description,
        favicon_url: input.favicon_url,
        cover_url: None,
        feed_url: input.feed_url,
        folder_id: input.folder_id,
        tags,
        created_at: ts,
        updated_at: ts,
        deleted_at: None,
        is_broken: false,
        last_checked_at: None,
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
        "SELECT id, url, title, description, favicon_url, cover_url, feed_url, folder_id, \
         created_at, updated_at, deleted_at, is_broken, last_checked_at FROM bookmarks \
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

/// Resolve a folder *path* to a folder id, creating any missing levels.
///
/// A `/` in the name denotes nesting, so `"Work / Projects"` finds or creates
/// `Work` at the top level and `Projects` underneath it, returning the id of the
/// deepest segment. A plain name (no `/`) behaves as a single top-level folder.
/// Segments are trimmed and empty ones are skipped, so `"a/ /b"` → `a → b`.
/// Nesting is capped at [`MAX_FOLDER_DEPTH`]: extra segments collapse into the
/// deepest allowed folder rather than erroring the whole import row.
///
/// Results are cached per resolved path prefix so repeated paths (and shared
/// prefixes like `"a/b"` and `"a/c"`) only hit the DB once.
pub(crate) fn find_or_create_folder(
    conn: &Connection,
    name: &str,
    cache: &mut HashMap<String, String>,
) -> Result<String, AppError> {
    let segments: Vec<&str> = name.split('/').map(str::trim).filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return Err(AppError::Validation { message: "folder name is required".into() });
    }

    let mut parent: Option<String> = None;
    let mut path_key = String::new();
    for (depth, seg) in segments.into_iter().enumerate() {
        if depth as i64 >= MAX_FOLDER_DEPTH {
            break; // honor the nesting cap — deeper segments land in the deepest folder
        }
        if !path_key.is_empty() {
            path_key.push('/');
        }
        path_key.push_str(seg);

        if let Some(id) = cache.get(&path_key) {
            parent = Some(id.clone());
            continue;
        }
        let existing: Option<String> = match &parent {
            Some(pid) => conn
                .query_row(
                    "SELECT id FROM folders WHERE name = ?1 AND parent_id = ?2 LIMIT 1",
                    params![seg, pid],
                    |r| r.get(0),
                )
                .optional()?,
            None => conn
                .query_row(
                    "SELECT id FROM folders WHERE name = ?1 AND parent_id IS NULL LIMIT 1",
                    params![seg],
                    |r| r.get(0),
                )
                .optional()?,
        };
        let id = match existing {
            Some(id) => id,
            None => db_add_folder(conn, seg.to_string(), parent.clone())?.id,
        };
        cache.insert(path_key.clone(), id.clone());
        parent = Some(id);
    }

    // Non-empty `segments` guarantees at least one iteration set `parent`.
    parent.ok_or_else(|| AppError::Validation { message: "folder name is required".into() })
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
    let mut stmt = conn.prepare(
        "SELECT id, name, parent_id, created_at FROM folders \
         WHERE deleted_at IS NULL ORDER BY name",
    )?;
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

/// Maximum folder nesting depth (1-based). A top-level folder is at depth 1, so
/// `MAX_FOLDER_DEPTH = 3` allows folders → subfolders → sub-subfolders.
pub(crate) const MAX_FOLDER_DEPTH: i64 = 3;

/// 1-based depth of a folder: a root folder (parent_id IS NULL) is 1, its child
/// is 2, and so on. Walks up the parent chain. Returns 0 if the folder is missing.
/// The loop is bounded by `MAX_FOLDER_DEPTH + 1` so a corrupt cycle can't hang.
fn folder_depth(conn: &Connection, id: &str) -> Result<i64, AppError> {
    let mut depth = 0i64;
    let mut current: Option<String> = Some(id.to_string());
    while let Some(fid) = current {
        let parent: Option<Option<String>> = conn
            .query_row("SELECT parent_id FROM folders WHERE id = ?1", params![fid], |r| r.get(0))
            .optional()?;
        match parent {
            Some(p) => {
                depth += 1;
                current = p;
            }
            None => break, // folder row not found — stop walking
        }
        if depth > MAX_FOLDER_DEPTH + 1 {
            break;
        }
    }
    Ok(depth)
}

/// Height of the subtree rooted at `id`: 1 for a leaf, 2 if it has children, etc.
fn subtree_height(conn: &Connection, id: &str) -> Result<i64, AppError> {
    let children: Vec<String> = conn
        .prepare("SELECT id FROM folders WHERE parent_id = ?1")?
        .query_map(params![id], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut max_child = 0i64;
    for child in children {
        max_child = max_child.max(subtree_height(conn, &child)?);
    }
    Ok(1 + max_child)
}

/// True if `candidate` is `ancestor` itself or any descendant of `ancestor`.
/// Used to reject moves that would create a cycle.
fn is_self_or_descendant(conn: &Connection, ancestor: &str, candidate: &str) -> Result<bool, AppError> {
    let mut current: Option<String> = Some(candidate.to_string());
    let mut steps = 0i64;
    while let Some(fid) = current {
        if fid == ancestor {
            return Ok(true);
        }
        current = conn
            .query_row("SELECT parent_id FROM folders WHERE id = ?1", params![fid], |r| r.get(0))
            .optional()?
            .flatten();
        steps += 1;
        if steps > MAX_FOLDER_DEPTH + 1 {
            break; // guard against a pre-existing cycle
        }
    }
    Ok(false)
}

pub fn db_add_folder(
    conn: &Connection,
    name: String,
    parent_id: Option<String>,
) -> Result<Folder, AppError> {
    if name.trim().is_empty() {
        return Err(AppError::Validation { message: "folder name is required".into() });
    }
    if let Some(pid) = parent_id.as_deref() {
        if folder_depth(conn, pid)? >= MAX_FOLDER_DEPTH {
            return Err(AppError::Validation {
                message: format!("folders can only be nested {MAX_FOLDER_DEPTH} levels deep"),
            });
        }
    }
    let id = Uuid::new_v4().to_string();
    let ts = now();
    conn.execute(
        "INSERT INTO folders (id, name, parent_id, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![id, name, parent_id, ts],
    )?;
    Ok(Folder { id, name, parent_id, created_at: ts })
}

/// Re-parent a folder. `new_parent_id == None` moves it to the top level.
/// Rejects: missing folder, self-parenting, cycles (moving a folder under one of
/// its own descendants), and moves that would push the subtree past
/// `MAX_FOLDER_DEPTH`.
pub fn db_move_folder(
    conn: &Connection,
    id: &str,
    new_parent_id: Option<&str>,
) -> Result<(), AppError> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM folders WHERE id = ?1 AND deleted_at IS NULL",
            params![id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::NotFound { message: format!("folder {id}") });
    }

    if let Some(pid) = new_parent_id {
        if pid == id {
            return Err(AppError::Validation { message: "a folder cannot be its own parent".into() });
        }
        let parent_exists: bool = conn
            .query_row(
                "SELECT 1 FROM folders WHERE id = ?1 AND deleted_at IS NULL",
                params![pid],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !parent_exists {
            return Err(AppError::NotFound { message: format!("folder {pid}") });
        }
        if is_self_or_descendant(conn, id, pid)? {
            return Err(AppError::Validation {
                message: "cannot move a folder into one of its own subfolders".into(),
            });
        }
        // New depth of `id` = parent depth + 1; the deepest descendant sits
        // (subtree_height - 1) levels below that. Keep the whole subtree in bounds.
        let new_base_depth = folder_depth(conn, pid)? + 1;
        let height = subtree_height(conn, id)?;
        if new_base_depth + height - 1 > MAX_FOLDER_DEPTH {
            return Err(AppError::Validation {
                message: format!("folders can only be nested {MAX_FOLDER_DEPTH} levels deep"),
            });
        }
    }

    conn.execute(
        "UPDATE folders SET parent_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![new_parent_id, now(), id],
    )?;
    Ok(())
}

/// Soft-delete a folder and its whole subtree, detaching their bookmarks.
///
/// Folders used to be hard-deleted, leaning on `ON DELETE CASCADE` (subfolders)
/// and `ON DELETE SET NULL` (bookmarks). Per-record sync needs the deletion to
/// *propagate*, so it must leave a tombstone instead of vanishing — which also
/// means the cascade has to be reproduced by hand: tombstone every descendant
/// folder and move their bookmarks back to the inbox, bumping `updated_at`
/// everywhere so the merge carries the change to the other machines.
pub fn db_delete_folder(conn: &Connection, id: &str) -> Result<(), AppError> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM folders WHERE id = ?1 AND deleted_at IS NULL",
            params![id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::NotFound { message: format!("folder {id}") });
    }

    let ts = now();
    // Collect the subtree (self + descendants) via a breadth-first walk, bounded
    // by the schema's max depth so a corrupt cycle can't loop forever.
    let mut subtree = vec![id.to_string()];
    let mut frontier = vec![id.to_string()];
    for _ in 0..=MAX_FOLDER_DEPTH {
        let mut next = Vec::new();
        for fid in &frontier {
            let children: Vec<String> = conn
                .prepare("SELECT id FROM folders WHERE parent_id = ?1 AND deleted_at IS NULL")?
                .query_map(params![fid], |r| r.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            next.extend(children);
        }
        if next.is_empty() {
            break;
        }
        subtree.extend(next.iter().cloned());
        frontier = next;
    }

    for fid in &subtree {
        conn.execute(
            "UPDATE bookmarks SET folder_id = NULL, updated_at = ?1 \
             WHERE folder_id = ?2 AND deleted_at IS NULL",
            params![ts, fid],
        )?;
        conn.execute(
            "UPDATE folders SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![ts, fid],
        )?;
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
         WHERE t.deleted_at IS NULL \
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

/// Suggest tags that frequently co-occur with the given `tag_ids` across the
/// user's *active* (non-binned) bookmarks. Used by the browser extension to
/// propose context-aware tags once the user has picked at least one.
///
/// Ranking: by the number of shared bookmarks (descending), then name. The
/// input tags themselves are excluded from the result. `bookmark_count` carries
/// the co-occurrence count so callers can show / weight it.
pub fn db_related_tags(
    conn: &Connection,
    tag_ids: &[String],
    limit: usize,
) -> Result<Vec<Tag>, AppError> {
    if tag_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=tag_ids.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(", ");
    // Two IN-lists referencing the same ids: seed tags (bt_in) and the tags to
    // exclude from the output (bt_other). We bind the id slice twice.
    let limit_ph = tag_ids.len() * 2 + 1;
    let sql = format!(
        "SELECT t.id, t.name, t.color, t.created_at, COUNT(*) AS cooccur \
         FROM bookmark_tags bt_in \
         JOIN bookmark_tags bt_other ON bt_other.bookmark_id = bt_in.bookmark_id \
         JOIN bookmarks b ON b.id = bt_in.bookmark_id AND b.deleted_at IS NULL \
         JOIN tags t ON t.id = bt_other.tag_id \
         WHERE bt_in.tag_id IN ({placeholders}) \
           AND bt_other.tag_id NOT IN ({placeholders}) \
           AND t.deleted_at IS NULL \
         GROUP BY t.id \
         ORDER BY cooccur DESC, t.name \
         LIMIT ?{limit_ph}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let bound = tag_ids
        .iter()
        .map(|s| s.as_str())
        .chain(tag_ids.iter().map(|s| s.as_str()))
        .map(|s| s.to_string())
        .chain(std::iter::once(limit.to_string()))
        .collect::<Vec<_>>();
    let tags = stmt
        .query_map(params_from_iter(bound.iter()), |row| {
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
        "INSERT OR IGNORE INTO tags (id, name, color, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![id, name, color, ts],
    )?;
    // A previously soft-deleted tag of the same name still holds the UNIQUE(name)
    // slot, so the insert above is ignored and the SELECT would otherwise return
    // a tombstone. Revive it (clear the tombstone, bump the clock) so re-adding a
    // deleted tag behaves like creating it — and the resurrection syncs out too.
    conn.execute(
        "UPDATE tags SET deleted_at = NULL, updated_at = ?1 \
         WHERE name = ?2 AND deleted_at IS NOT NULL",
        params![ts, name],
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
    // Soft-delete (tombstone) so the deletion propagates through per-record sync
    // instead of silently vanishing on one machine. The `bookmark_tags` junction
    // rows are left in place; every read filters dead tags out, and a merge
    // matches tags by id, so the tombstone is all the other machines need.
    let ts = now();
    let n = conn.execute(
        "UPDATE tags SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![ts, id],
    )?;
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

// ─── Per-record sync snapshot (multi-machine merge) ─────────────────────────────

/// Read the whole database — INCLUDING soft-deleted tombstones — as a
/// `SyncSnapshot` for the merge engine. Unlike the user-facing JSON export
/// (`io::export_json`, active rows only, deduped by URL), tombstones MUST travel
/// so deletions propagate to the other machines.
pub fn db_export_sync_snapshot(conn: &Connection) -> Result<crate::merge::SyncSnapshot, AppError> {
    let folders = conn
        .prepare("SELECT id, name, parent_id, created_at, updated_at, deleted_at FROM folders")?
        .query_map([], |r| {
            Ok(crate::merge::SyncFolder {
                id: r.get(0)?,
                name: r.get(1)?,
                parent_id: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
                deleted_at: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let tags = conn
        .prepare("SELECT id, name, color, created_at, updated_at, deleted_at FROM tags")?
        .query_map([], |r| {
            Ok(crate::merge::SyncTag {
                id: r.get(0)?,
                name: r.get(1)?,
                color: r.get(2)?,
                created_at: r.get(3)?,
                updated_at: r.get(4)?,
                deleted_at: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    // tag_ids per bookmark — every association (apply-time drops dead/absent tags).
    let mut tags_by_bookmark: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn.prepare("SELECT bookmark_id, tag_id FROM bookmark_tags")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (bid, tid) = row?;
            tags_by_bookmark.entry(bid).or_default().push(tid);
        }
    }

    let bookmarks = conn
        .prepare(
            "SELECT id, url, title, description, favicon_url, feed_url, cover_url, folder_id, \
                    created_at, updated_at, deleted_at FROM bookmarks",
        )?
        .query_map([], |r| {
            Ok(crate::merge::SyncBookmark {
                id: r.get(0)?,
                url: r.get(1)?,
                title: r.get(2)?,
                description: r.get(3)?,
                favicon_url: r.get(4)?,
                feed_url: r.get(5)?,
                cover_url: r.get(6)?,
                folder_id: r.get(7)?,
                tag_ids: Vec::new(),
                created_at: r.get(8)?,
                updated_at: r.get(9)?,
                deleted_at: r.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|mut b| {
            if let Some(ids) = tags_by_bookmark.remove(&b.id) {
                b.tag_ids = ids;
            }
            b
        })
        .collect();

    Ok(crate::merge::SyncSnapshot { folders, tags, bookmarks })
}

/// Replace the entire database with a merged `SyncSnapshot`, in one transaction.
/// Tombstones are written too, so future merges still observe the deletions.
///
/// Two schema realities are handled here:
/// * `tags.name` is UNIQUE across *all* rows (the inline constraint can't be made
///   partial without a table rebuild), yet a legitimate merge can yield a live
///   tag and a dead tag sharing a name. Dead tags are stored with their name set
///   to their id — unique, and never displayed (every read filters tombstones).
/// * a bookmark may point at a folder the merge tombstoned/omitted, or carry a
///   tag_id that resolved to a tombstone; those references are dropped so the
///   foreign keys and the "no dead tags on a bookmark" invariant both hold.
pub fn db_apply_sync_snapshot(
    conn: &Connection,
    snap: &crate::merge::SyncSnapshot,
) -> Result<(), AppError> {
    use std::collections::HashSet;
    let live_tag_ids: HashSet<&str> = snap
        .tags
        .iter()
        .filter(|t| t.deleted_at.is_none())
        .map(|t| t.id.as_str())
        .collect();
    let folder_ids: HashSet<&str> = snap.folders.iter().map(|f| f.id.as_str()).collect();

    let tx = conn.unchecked_transaction()?;
    db_clear_all_data(&tx)?;

    for f in &snap.folders {
        tx.execute(
            "INSERT INTO folders (id, name, parent_id, created_at, updated_at, deleted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![f.id, f.name, f.parent_id, f.created_at, f.updated_at, f.deleted_at],
        )?;
    }

    for t in &snap.tags {
        // Dead tag → store its id as the name to dodge UNIQUE(name); it's hidden.
        let name = if t.deleted_at.is_some() { &t.id } else { &t.name };
        tx.execute(
            "INSERT INTO tags (id, name, color, created_at, updated_at, deleted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![t.id, name, t.color, t.created_at, t.updated_at, t.deleted_at],
        )?;
    }

    for b in &snap.bookmarks {
        // Drop a folder reference the merge didn't carry (e.g. folder tombstoned
        // on another machine) so the FK holds and the bookmark falls to the inbox.
        let folder_id = b.folder_id.as_deref().filter(|fid| folder_ids.contains(fid));
        tx.execute(
            "INSERT INTO bookmarks (id, url, title, description, favicon_url, feed_url, \
                                    cover_url, folder_id, created_at, updated_at, deleted_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                b.id, b.url, b.title, b.description, b.favicon_url, b.feed_url, b.cover_url,
                folder_id, b.created_at, b.updated_at, b.deleted_at
            ],
        )?;
        for tid in &b.tag_ids {
            if live_tag_ids.contains(tid.as_str()) {
                tx.execute(
                    "INSERT OR IGNORE INTO bookmark_tags (bookmark_id, tag_id) VALUES (?1, ?2)",
                    params![b.id, tid],
                )?;
            }
        }
    }

    tx.commit()?;
    Ok(())
}

// ─── Deduplication ────────────────────────────────────────────────────────────

pub fn db_find_duplicate_bookmarks(conn: &Connection) -> Result<Vec<Vec<Bookmark>>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.url, b.title, b.description, b.favicon_url, b.cover_url, b.feed_url, \
                b.folder_id, b.created_at, b.updated_at, b.deleted_at, b.is_broken, b.last_checked_at \
         FROM bookmarks b \
         WHERE b.deleted_at IS NULL \
           AND b.url IN ( \
             SELECT url FROM bookmarks WHERE deleted_at IS NULL \
             GROUP BY url HAVING COUNT(*) > 1 \
           ) \
         ORDER BY b.url, b.created_at",
    )?;
    let raws: Vec<RawBookmark> = stmt
        .query_map([], row_to_raw)?
        .collect::<Result<Vec<_>, _>>()?;

    let bookmarks = enrich_batch(raws, conn)?;

    let mut groups: Vec<Vec<Bookmark>> = Vec::new();
    let mut current_url = String::new();
    for b in bookmarks {
        if b.url != current_url {
            current_url = b.url.clone();
            groups.push(vec![b]);
        } else {
            groups.last_mut().unwrap().push(b);
        }
    }

    Ok(groups)
}

pub fn db_merge_bookmark_duplicates(
    conn: &Connection,
    keeper_id: &str,
    discard_ids: &[String],
) -> Result<(), AppError> {
    if discard_ids.is_empty() {
        return Ok(());
    }
    // Copy all tags from each discard to the keeper
    for discard_id in discard_ids {
        conn.execute(
            "INSERT OR IGNORE INTO bookmark_tags (bookmark_id, tag_id) \
             SELECT ?1, tag_id FROM bookmark_tags WHERE bookmark_id = ?2",
            params![keeper_id, discard_id],
        )?;
    }
    // Soft-delete the discards
    let ts = now();
    for discard_id in discard_ids {
        conn.execute(
            "UPDATE bookmarks SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            params![ts, discard_id],
        )?;
    }
    Ok(())
}

// ─── Health Check ─────────────────────────────────────────────────────────────

pub fn db_get_broken_bookmarks(conn: &Connection) -> Result<Vec<Bookmark>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, url, title, description, favicon_url, cover_url, feed_url, folder_id, \
         created_at, updated_at, deleted_at, is_broken, last_checked_at FROM bookmarks \
         WHERE is_broken = 1 AND deleted_at IS NULL ORDER BY last_checked_at DESC",
    )?;
    let raws = stmt.query_map([], row_to_raw)?.collect::<Result<Vec<_>, _>>()?;
    enrich_batch(raws, conn)
}

pub fn db_get_broken_count(conn: &Connection) -> Result<i64, AppError> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM bookmarks WHERE is_broken = 1 AND deleted_at IS NULL",
        [],
        |r| r.get(0),
    )?)
}

/// How long a healthy (is_broken = 0) result is cached before we re-check.
/// Known-broken bookmarks are always re-checked regardless of this threshold.
const RECHECK_HEALTHY_AFTER_SECS: i64 = 24 * 3600;

/// Returns (id, url) pairs that need checking:
/// - always: known-broken bookmarks (they may have been fixed)
/// - always: never-checked bookmarks
/// - only if stale: healthy bookmarks not checked within RECHECK_HEALTHY_AFTER_SECS
pub fn db_get_urls_for_health_check(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let cutoff = now() - RECHECK_HEALTHY_AFTER_SECS;
    let mut stmt = conn.prepare(
        "SELECT id, url FROM bookmarks \
         WHERE deleted_at IS NULL \
           AND (is_broken = 1 OR last_checked_at IS NULL OR last_checked_at < ?1) \
         ORDER BY is_broken DESC, created_at",
    )?;
    let rows = stmt
        .query_map(params![cutoff], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Apply many health-check results in a single transaction, reusing one prepared
/// statement — so a scan of N bookmarks is one commit instead of N auto-commits.
pub fn db_update_bookmark_health_batch(
    conn: &Connection,
    updates: &[(String, bool, i64)],
) -> Result<(), AppError> {
    if updates.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "UPDATE bookmarks SET is_broken = ?1, last_checked_at = ?2 WHERE id = ?3",
        )?;
        for (id, is_broken, checked_at) in updates {
            stmt.execute(params![*is_broken as i64, checked_at, id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Returns (id, url) pairs for bookmarks that have no cover image yet.
pub fn db_get_bookmarks_without_cover(conn: &Connection) -> Result<Vec<(String, String)>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, url FROM bookmarks WHERE deleted_at IS NULL AND cover_url IS NULL ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn db_update_cover_url(conn: &Connection, id: &str, cover_url: &str) -> Result<(), AppError> {
    conn.execute(
        "UPDATE bookmarks SET cover_url = ?1 WHERE id = ?2",
        params![cover_url, id],
    )?;
    Ok(())
}

/// Soft-deletes multiple bookmarks atomically. Already-deleted IDs are silently skipped.
pub fn db_delete_bookmarks(conn: &Connection, ids: &[String]) -> Result<(), AppError> {
    if ids.is_empty() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    let ts = now();
    for id in ids {
        tx.execute(
            "UPDATE bookmarks SET deleted_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
            params![ts, id],
        )?;
    }
    tx.commit()?;
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
    fn update_bookmark_health_batch_marks_only_listed_rows() {
        let conn = mem();
        let a = mk_bookmark(&conn, "https://a.com", "A");
        let b = mk_bookmark(&conn, "https://b.com", "B");
        let c = mk_bookmark(&conn, "https://c.com", "C");

        db_update_bookmark_health_batch(
            &conn,
            &[(a.id.clone(), true, 1000), (b.id.clone(), false, 1001)],
        )
        .unwrap();

        let broken = db_get_broken_bookmarks(&conn).unwrap();
        assert_eq!(broken.len(), 1, "only A should be broken");
        assert_eq!(broken[0].id, a.id);
        assert_eq!(broken[0].last_checked_at, Some(1000));

        // C was not in the batch — it stays untouched.
        let all = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        let cc = all.iter().find(|x| x.id == c.id).unwrap();
        assert!(!cc.is_broken);
        assert_eq!(cc.last_checked_at, None);
    }

    #[test]
    fn update_bookmark_health_batch_empty_is_noop() {
        let conn = mem();
        db_update_bookmark_health_batch(&conn, &[]).unwrap();
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
    fn fuzzy_search_matches_subsequence() {
        let conn = mem();
        mk_bookmark(&conn, "https://react.dev", "React Documentation");
        mk_bookmark(&conn, "https://python.org", "Python");

        // Non-contiguous subsequence with gaps ("rctdoc" → "ReaCT DOCumentation").
        // A substring LIKE would miss this entirely.
        let results = db_get_bookmarks(&conn, None, None, Some("rctdoc"), false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "React Documentation");
    }

    #[test]
    fn fuzzy_search_tolerates_dropped_character() {
        let conn = mem();
        mk_bookmark(&conn, "https://docs.rs", "Documentation");
        mk_bookmark(&conn, "https://python.org", "Python");

        // A dropped-letter typo (missing "en") still matches as a subsequence.
        let results = db_get_bookmarks(&conn, None, None, Some("documtation"), false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Documentation");
    }

    #[test]
    fn fuzzy_search_matches_out_of_order_words() {
        let conn = mem();
        mk_bookmark(&conn, "https://tokio.rs", "async rust runtime");
        mk_bookmark(&conn, "https://go.dev", "Go concurrency");

        // Words given in the opposite order to the title.
        let results = db_get_bookmarks(&conn, None, None, Some("rust async"), false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "async rust runtime");
    }

    #[test]
    fn fuzzy_search_matches_body_only() {
        let conn = mem();
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://example.com".to_string(),
                title: "Untitled clip".to_string(),
                description: Some("notes on borrow checker lifetimes".to_string()),
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap();
        mk_bookmark(&conn, "https://other.com", "Other");

        // The term only appears in the description/body.
        let results = db_get_bookmarks(&conn, None, None, Some("borrow checker"), false).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Untitled clip");
    }

    #[test]
    fn fuzzy_search_ranks_title_above_body() {
        let conn = mem();
        // Body-only match.
        db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://blog.example.com".to_string(),
                title: "Weekly digest".to_string(),
                description: Some("a paragraph mentioning kubernetes once".to_string()),
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: None,
            },
        )
        .unwrap();
        // Title match — should rank first.
        mk_bookmark(&conn, "https://k8s.io", "Kubernetes");

        let results = db_get_bookmarks(&conn, None, None, Some("kubernetes"), false).unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Kubernetes");
        assert_eq!(results[1].title, "Weekly digest");
    }

    #[test]
    fn fuzzy_search_excludes_non_matches() {
        let conn = mem();
        mk_bookmark(&conn, "https://rust-lang.org", "Rust");
        mk_bookmark(&conn, "https://go.dev", "Go");

        let results = db_get_bookmarks(&conn, None, None, Some("zzzqqq"), false).unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn fuzzy_search_blank_query_returns_all() {
        let conn = mem();
        mk_bookmark(&conn, "https://a.com", "A");
        mk_bookmark(&conn, "https://b.com", "B");

        // Whitespace-only query is treated as no query (returns everything).
        let results = db_get_bookmarks(&conn, None, None, Some("   "), false).unwrap();
        assert_eq!(results.len(), 2);
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

    // ── Subfolders ──────────────────────────────────────────────────────────────

    /// Build a chain of folders nested `depth` levels deep, returning every id
    /// from the root down. `depth` must be ≤ MAX_FOLDER_DEPTH.
    fn nest_chain(conn: &Connection, depth: i64) -> Vec<String> {
        let mut ids = Vec::new();
        let mut parent: Option<String> = None;
        for i in 0..depth {
            let f = db_add_folder(conn, format!("L{i}"), parent.clone()).unwrap();
            parent = Some(f.id.clone());
            ids.push(f.id);
        }
        ids
    }

    #[test]
    fn add_subfolder_at_max_depth_is_rejected() {
        let conn = mem();
        let chain = nest_chain(&conn, MAX_FOLDER_DEPTH); // deepest is at MAX_FOLDER_DEPTH
        let leaf = chain.last().unwrap();
        let err = db_add_folder(&conn, "too deep".to_string(), Some(leaf.clone())).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn move_folder_to_root_clears_parent() {
        let conn = mem();
        let parent = mk_folder(&conn, "Work");
        let child = db_add_folder(&conn, "Projects".to_string(), Some(parent.id.clone())).unwrap();

        db_move_folder(&conn, &child.id, None).unwrap();

        let folders = db_get_folders(&conn).unwrap();
        let moved = folders.iter().find(|f| f.id == child.id).unwrap();
        assert!(moved.parent_id.is_none());
    }

    #[test]
    fn move_folder_under_another_sets_parent() {
        let conn = mem();
        let a = mk_folder(&conn, "A");
        let b = mk_folder(&conn, "B");

        db_move_folder(&conn, &b.id, Some(&a.id)).unwrap();

        let folders = db_get_folders(&conn).unwrap();
        let moved = folders.iter().find(|f| f.id == b.id).unwrap();
        assert_eq!(moved.parent_id.as_deref(), Some(a.id.as_str()));
    }

    #[test]
    fn move_folder_into_self_is_rejected() {
        let conn = mem();
        let a = mk_folder(&conn, "A");
        let err = db_move_folder(&conn, &a.id, Some(&a.id)).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn move_folder_into_own_descendant_is_rejected() {
        let conn = mem();
        let parent = mk_folder(&conn, "Parent");
        let child = db_add_folder(&conn, "Child".to_string(), Some(parent.id.clone())).unwrap();
        // Moving the parent under its own child would create a cycle.
        let err = db_move_folder(&conn, &parent.id, Some(&child.id)).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn move_folder_exceeding_max_depth_is_rejected() {
        let conn = mem();
        // A two-level subtree (X → Y) cannot go under a folder already at the
        // deepest allowed parent depth, since Y would land one level too deep.
        let chain = nest_chain(&conn, MAX_FOLDER_DEPTH - 1);
        let deepest_parent = chain.last().unwrap();
        let x = mk_folder(&conn, "X");
        db_add_folder(&conn, "Y".to_string(), Some(x.id.clone())).unwrap();

        let err = db_move_folder(&conn, &x.id, Some(deepest_parent)).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn move_folder_not_found_returns_error() {
        let conn = mem();
        let err = db_move_folder(&conn, "nonexistent", None).unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    #[test]
    fn delete_parent_folder_cascades_to_subfolders() {
        let conn = mem();
        let parent = mk_folder(&conn, "Parent");
        db_add_folder(&conn, "Child".to_string(), Some(parent.id.clone())).unwrap();

        db_delete_folder(&conn, &parent.id).unwrap();

        let folders = db_get_folders(&conn).unwrap();
        assert!(folders.is_empty());
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
    fn delete_tag_soft_deletes_and_hides_it() {
        // Per-record sync changed tag deletion from a hard `DELETE` (which
        // cascaded the junction rows away) to a tombstone, so the deletion can
        // propagate to other machines. The junction rows now persist; the
        // contract is that the tag becomes invisible everywhere instead.
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

        // Row remains but carries a tombstone.
        let deleted_at: Option<i64> = conn
            .query_row("SELECT deleted_at FROM tags WHERE id = ?1", params![tag.id], |r| r.get(0))
            .unwrap();
        assert!(deleted_at.is_some(), "tag should be tombstoned, not removed");

        // Gone from the tag list...
        assert!(db_get_tags(&conn).unwrap().iter().all(|t| t.id != tag.id));

        // ...and stripped from the bookmark's enriched tags.
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert!(bms[0].tags.iter().all(|t| t.id != tag.id));
    }

    #[test]
    fn delete_tag_not_found_returns_error() {
        let conn = mem();
        let err = db_delete_tag(&conn, "nonexistent").unwrap_err();
        assert!(matches!(err, AppError::NotFound { .. }));
    }

    // ── per-record sync snapshot bridge ──────────────────────────────────────

    fn add_bm(conn: &Connection, url: &str, tag_ids: Option<Vec<String>>, folder: Option<String>) -> Bookmark {
        db_add_bookmark(
            conn,
            CreateBookmarkInput {
                url: url.to_string(),
                title: url.to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: folder,
                tag_ids,
            },
        )
        .unwrap()
    }

    #[test]
    fn sync_snapshot_round_trips_through_db() {
        let conn = mem();
        let folder = db_add_folder(&conn, "Reading".into(), None).unwrap();
        let tag = mk_tag(&conn, "rust");
        add_bm(&conn, "https://a.test", Some(vec![tag.id.clone()]), Some(folder.id.clone()));

        let snap = db_export_sync_snapshot(&conn).unwrap();

        // Apply the snapshot into a *fresh* DB and re-export — the data survives.
        let conn2 = mem();
        db_apply_sync_snapshot(&conn2, &snap).unwrap();
        let snap2 = db_export_sync_snapshot(&conn2).unwrap();

        assert_eq!(snap2.folders.len(), 1);
        assert_eq!(snap2.tags.len(), 1);
        assert_eq!(snap2.bookmarks.len(), 1);
        let b = &snap2.bookmarks[0];
        assert_eq!(b.url, "https://a.test");
        assert_eq!(b.folder_id.as_deref(), Some(folder.id.as_str()));
        assert_eq!(b.tag_ids, vec![tag.id.clone()]);
    }

    #[test]
    fn sync_apply_preserves_tombstones_but_hides_them() {
        let conn = mem();
        let tag = mk_tag(&conn, "rust");
        add_bm(&conn, "https://a.test", Some(vec![tag.id.clone()]), None);
        db_delete_tag(&conn, &tag.id).unwrap(); // tombstone

        let snap = db_export_sync_snapshot(&conn).unwrap();
        assert_eq!(snap.tags.len(), 1, "tombstone travels in the snapshot");
        assert!(snap.tags[0].deleted_at.is_some());

        let conn2 = mem();
        db_apply_sync_snapshot(&conn2, &snap).unwrap();
        // Hidden in the UI...
        assert!(db_get_tags(&conn2).unwrap().is_empty());
        // ...but still present as a tombstone so the deletion can't be undone by
        // a stale machine re-adding the row in a later merge.
        let reexport = db_export_sync_snapshot(&conn2).unwrap();
        assert_eq!(reexport.tags.len(), 1);
        assert!(reexport.tags[0].deleted_at.is_some());
    }

    #[test]
    fn sync_apply_survives_live_and_dead_tag_same_name() {
        // The merge can legitimately produce a live tag and a dead tag sharing a
        // name. Applying both must not trip UNIQUE(name).
        let conn = mem();
        let snap = crate::merge::SyncSnapshot {
            folders: vec![],
            tags: vec![
                crate::merge::SyncTag {
                    id: "live".into(),
                    name: "rust".into(),
                    color: "#fff".into(),
                    created_at: 1,
                    updated_at: 20,
                    deleted_at: None,
                },
                crate::merge::SyncTag {
                    id: "dead".into(),
                    name: "rust".into(),
                    color: "#fff".into(),
                    created_at: 1,
                    updated_at: 10,
                    deleted_at: Some(10),
                },
            ],
            bookmarks: vec![],
        };
        db_apply_sync_snapshot(&conn, &snap).unwrap();
        let live = db_get_tags(&conn).unwrap();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "live");
    }

    #[test]
    fn sync_apply_drops_reference_to_missing_folder() {
        // A bookmark whose folder was tombstoned/omitted on another machine must
        // land in the inbox (folder_id NULL), not break the foreign key.
        let conn = mem();
        let snap = crate::merge::SyncSnapshot {
            folders: vec![], // folder intentionally absent
            tags: vec![],
            bookmarks: vec![crate::merge::SyncBookmark {
                id: "b1".into(),
                url: "https://a.test".into(),
                title: "A".into(),
                description: None,
                favicon_url: None,
                feed_url: None,
                cover_url: None,
                folder_id: Some("ghost-folder".into()),
                tag_ids: vec![],
                created_at: 1,
                updated_at: 1,
                deleted_at: None,
            }],
        };
        db_apply_sync_snapshot(&conn, &snap).unwrap();
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms.len(), 1);
        assert_eq!(bms[0].folder_id, None);
    }

    #[test]
    fn related_tags_ranks_by_cooccurrence() {
        let conn = mem();
        let rust = mk_tag(&conn, "rust");
        let prog = mk_tag(&conn, "programming");
        let web = mk_tag(&conn, "web");
        let cooking = mk_tag(&conn, "cooking");

        let with_tags = |url: &str, tag_ids: Vec<String>| {
            db_add_bookmark(
                &conn,
                CreateBookmarkInput {
                    url: url.to_string(),
                    title: "t".to_string(),
                    description: None,
                    favicon_url: None,
                    feed_url: None,
                    folder_id: None,
                    tag_ids: Some(tag_ids),
                },
            )
            .unwrap()
        };

        // rust+programming co-occur 2x, rust+web 1x, cooking unrelated
        with_tags("https://a.com", vec![rust.id.clone(), prog.id.clone()]);
        with_tags("https://b.com", vec![rust.id.clone(), prog.id.clone(), web.id.clone()]);
        with_tags("https://c.com", vec![cooking.id.clone()]);

        let related = db_related_tags(&conn, &[rust.id.clone()], 10).unwrap();
        let names: Vec<&str> = related.iter().map(|t| t.name.as_str()).collect();

        // programming (2) ranks above web (1); rust excluded; cooking absent
        assert_eq!(names, vec!["programming", "web"]);
        assert_eq!(related[0].bookmark_count, Some(2));
        assert_eq!(related[1].bookmark_count, Some(1));
    }

    #[test]
    fn related_tags_excludes_seed_and_binned_bookmarks() {
        let conn = mem();
        let rust = mk_tag(&conn, "rust");
        let prog = mk_tag(&conn, "programming");

        let b = db_add_bookmark(
            &conn,
            CreateBookmarkInput {
                url: "https://a.com".to_string(),
                title: "t".to_string(),
                description: None,
                favicon_url: None,
                feed_url: None,
                folder_id: None,
                tag_ids: Some(vec![rust.id.clone(), prog.id.clone()]),
            },
        )
        .unwrap();

        // Bin the only bookmark — its co-occurrences must no longer count.
        db_delete_bookmark(&conn, &b.id).unwrap();
        assert!(db_related_tags(&conn, &[rust.id.clone()], 10).unwrap().is_empty());
    }

    #[test]
    fn related_tags_empty_input_returns_empty() {
        let conn = mem();
        mk_tag(&conn, "rust");
        assert!(db_related_tags(&conn, &[], 10).unwrap().is_empty());
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
    fn find_or_create_folder_splits_path_into_subfolders() {
        let conn = mem();
        let mut cache = HashMap::new();
        let leaf = find_or_create_folder(&conn, "Work / Projects", &mut cache).unwrap();

        let folders = db_get_folders(&conn).unwrap();
        let work = folders.iter().find(|f| f.name == "Work").unwrap();
        let projects = folders.iter().find(|f| f.name == "Projects").unwrap();
        assert!(work.parent_id.is_none());
        assert_eq!(projects.parent_id.as_deref(), Some(work.id.as_str()));
        assert_eq!(leaf, projects.id);
    }

    #[test]
    fn find_or_create_folder_reuses_shared_path_prefix() {
        let conn = mem();
        let mut cache = HashMap::new();
        find_or_create_folder(&conn, "Work / Projects", &mut cache).unwrap();
        find_or_create_folder(&conn, "Work / Archive", &mut cache).unwrap();

        let folders = db_get_folders(&conn).unwrap();
        // One "Work", with two distinct children — not a duplicate "Work".
        assert_eq!(folders.iter().filter(|f| f.name == "Work").count(), 1);
        let work = folders.iter().find(|f| f.name == "Work").unwrap();
        let children = folders.iter().filter(|f| f.parent_id.as_deref() == Some(work.id.as_str())).count();
        assert_eq!(children, 2);
    }

    #[test]
    fn find_or_create_folder_caps_path_at_max_depth() {
        let conn = mem();
        let mut cache = HashMap::new();
        // Four segments, cap is 3 — the 4th collapses into the deepest folder.
        find_or_create_folder(&conn, "a / b / c / d", &mut cache).unwrap();
        let folders = db_get_folders(&conn).unwrap();
        assert!(folders.iter().any(|f| f.name == "c"));
        assert!(!folders.iter().any(|f| f.name == "d"));
    }

    #[test]
    fn find_or_create_folder_plain_name_is_top_level() {
        let conn = mem();
        let mut cache = HashMap::new();
        find_or_create_folder(&conn, "Reading", &mut cache).unwrap();
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Reading");
        assert!(folders[0].parent_id.is_none());
    }

    #[test]
    fn import_bookmarks_creates_nested_folders_from_path() {
        let conn = mem();
        let inputs = vec![ImportRowInput {
            url: "https://a.com".to_string(),
            title: "A".to_string(),
            folder_name: Some("Work / Projects".to_string()),
            ..mk_input("", "")
        }];
        let result = db_import_bookmarks(&conn, inputs).unwrap();
        assert_eq!(result.imported, 1);
        let folders = db_get_folders(&conn).unwrap();
        let projects = folders.iter().find(|f| f.name == "Projects").unwrap();
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks[0].folder_id.as_deref(), Some(projects.id.as_str()));
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
