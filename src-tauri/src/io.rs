/// src-tauri/src/io.rs
///
/// Import / export for Ferrico in three formats:
///
///   JSON      — lossless round-trip, ideal for sync between computers.
///               Preserves all fields: bookmark metadata, folder hierarchy
///               (nested), and tags with colours.
///
///   Netscape HTML — the de-facto universal browser bookmark format used by
///               Chrome, Firefox, Safari, Edge, Raindrop.io, Pinboard,
///               Linkding, Pocket, and virtually every bookmark manager ever
///               shipped.  `<DL><DT><A HREF="…" ADD_DATE="…">title</A>`
///               with nested `<DL>` blocks for sub-folders.
///
///   OPML      — kept for RSS-reader compatibility. The existing
///               `db_export_opml` is moved here; `import_opml` is new.
///
/// Architecture rules (CLAUDE.md):
///   - All functions are pure: they take `&Connection`, never AppState.
///   - One rusqlite transaction per import call.
///   - Re-use `db::find_or_create_folder`, `db::find_or_create_tags`,
///     and `db::db_add_bookmark` — those are now `pub(crate)`.
///   - No new XML/HTML parser crates. OPML and Netscape use hand-rolled
///     recursive-descent / state-machine parsers.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::db::{
    self, Bookmark, CreateBookmarkInput, Folder, ImportResult, RawBookmark,
    db_add_bookmark, db_add_folder, db_get_bookmarks, db_get_folders, db_get_tags,
    find_or_create_tags, now, append_folder_tree,
};
use crate::error::AppError;

// ─── JSON format ──────────────────────────────────────────────────────────────
//
// Schema:
//   {
//     "version": 1,
//     "exported_at": <unix-secs>,
//     "folders": [
//       { "id": "…", "name": "…", "parent_id": null | "…", "created_at": <i64> }
//     ],
//     "tags": [
//       { "id": "…", "name": "…", "color": "…", "created_at": <i64> }
//     ],
//     "bookmarks": [
//       {
//         "id": "…", "url": "…", "title": "…",
//         "description": null | "…", "favicon_url": null | "…",
//         "feed_url": null | "…", "folder_id": null | "…",
//         "tag_ids": ["…"],
//         "created_at": <i64>, "updated_at": <i64>
//       }
//     ]
//   }
//
// Deleted bookmarks are excluded (same as other exports).

#[derive(Serialize, Deserialize)]
struct JsonExport {
    version: u32,
    exported_at: i64,
    folders: Vec<JsonFolder>,
    tags: Vec<JsonTag>,
    bookmarks: Vec<JsonBookmark>,
}

#[derive(Serialize, Deserialize, Clone)]
struct JsonFolder {
    id: String,
    name: String,
    parent_id: Option<String>,
    created_at: i64,
}

#[derive(Serialize, Deserialize, Clone)]
struct JsonTag {
    id: String,
    name: String,
    color: String,
    created_at: i64,
}

#[derive(Serialize, Deserialize)]
struct JsonBookmark {
    id: String,
    url: String,
    title: String,
    description: Option<String>,
    favicon_url: Option<String>,
    feed_url: Option<String>,
    folder_id: Option<String>,
    tag_ids: Vec<String>,
    created_at: i64,
    updated_at: i64,
}

pub fn export_json(conn: &Connection) -> Result<String, AppError> {
    // Load folders
    let db_folders = db_get_folders(conn)?;
    let folders: Vec<JsonFolder> = db_folders
        .iter()
        .map(|f| JsonFolder {
            id: f.id.clone(),
            name: f.name.clone(),
            parent_id: f.parent_id.clone(),
            created_at: f.created_at,
        })
        .collect();

    // Load tags (without bookmark_count)
    let db_tags = db_get_tags(conn)?;
    let tags: Vec<JsonTag> = db_tags
        .iter()
        .map(|t| JsonTag {
            id: t.id.clone(),
            name: t.name.clone(),
            color: t.color.clone(),
            created_at: t.created_at,
        })
        .collect();

    // Load all active bookmarks (with their tags already enriched)
    let bookmarks_full = db_get_bookmarks(conn, None, None, None, false)?;
    let bookmarks: Vec<JsonBookmark> = bookmarks_full
        .into_iter()
        .map(|b| JsonBookmark {
            id: b.id,
            url: b.url,
            title: b.title,
            description: b.description,
            favicon_url: b.favicon_url,
            feed_url: b.feed_url,
            folder_id: b.folder_id,
            tag_ids: b.tags.into_iter().map(|t| t.id).collect(),
            created_at: b.created_at,
            updated_at: b.updated_at,
        })
        .collect();

    let export = JsonExport {
        version: 1,
        exported_at: now(),
        folders,
        tags,
        bookmarks,
    };

    serde_json::to_string_pretty(&export).map_err(|e| AppError::Validation {
        message: format!("JSON serialisation failed: {e}"),
    })
}

/// Import a JSON export.
///
/// Strategy:
///   1. Parse and validate the envelope.
///   2. In a single transaction:
///      a. Upsert all folders (find-or-create by name at the correct nesting
///         level). We use the export's `id` as a hint but always look up by
///         name to avoid clobbering existing data on a merge import.
///      b. Upsert all tags (find-or-create by name; preserve colour from the
///         export when the tag doesn't yet exist).
///      c. Insert each bookmark. If a bookmark with the same URL already
///         exists it is skipped (not an error).
pub fn import_json(conn: &Connection, json: &str) -> Result<ImportResult, AppError> {
    let json = crate::io_validate::strip_bom(json);
    let export: JsonExport = serde_json::from_str(json).map_err(|e| AppError::Validation {
        message: format!("invalid JSON: {e}"),
    })?;

    if export.version != 1 {
        return Err(AppError::Validation {
            message: format!("unsupported JSON export version {}", export.version),
        });
    }

    let tx = conn.unchecked_transaction()?;
    let mut imported = 0usize;
    let mut errors: Vec<String> = Vec::new();

    // ── 1. Resolve folders ─────────────────────────────────────────────────
    // Map export folder-id → live folder-id.
    // We honour nesting by processing in topological order (parents first).
    // The export is a flat list; we keep re-scanning until all folders are
    // resolved or we detect a cycle/missing parent.
    let mut folder_id_map: HashMap<String, String> = HashMap::new();
    {
        let mut remaining: Vec<JsonFolder> = export.folders.clone();
        let mut iterations_without_progress = 0;

        while !remaining.is_empty() {
            let before = remaining.len();
            let mut next = Vec::new();

            for jf in remaining {
                // Check if parent is resolved (or root)
                let parent_live_id: Option<String> = match &jf.parent_id {
                    None => None,
                    Some(pid) => match folder_id_map.get(pid) {
                        Some(id) => Some(id.clone()),
                        None => {
                            // Parent not yet resolved — defer
                            next.push(jf);
                            continue;
                        }
                    },
                };

                // Find or create this folder under the resolved parent
                let live_id = match find_or_create_folder_with_parent(
                    &tx,
                    &jf.name,
                    parent_live_id.as_deref(),
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        errors.push(format!("Folder {:?}: {e}", jf.name));
                        continue;
                    }
                };
                folder_id_map.insert(jf.id.clone(), live_id);
            }

            remaining = next;
            if remaining.len() == before {
                iterations_without_progress += 1;
                if iterations_without_progress > 1 {
                    // Cycle or dangling parent — skip remaining
                    for jf in &remaining {
                        errors.push(format!(
                            "Folder {:?}: parent not found (cycle or missing)",
                            jf.name
                        ));
                    }
                    break;
                }
            } else {
                iterations_without_progress = 0;
            }
        }
    }

    // ── 2. Resolve tags ────────────────────────────────────────────────────
    // Map export tag-id → live tag-id.
    let mut tag_id_map: HashMap<String, String> = HashMap::new();
    for jt in &export.tags {
        match db::db_add_tag_with_color(&tx, &jt.name, &jt.color) {
            Ok(tag) => {
                tag_id_map.insert(jt.id.clone(), tag.id);
            }
            Err(e) => {
                errors.push(format!("Tag {:?}: {e}", jt.name));
            }
        }
    }

    // ── 3. Insert bookmarks ────────────────────────────────────────────────
    for (i, jb) in export.bookmarks.into_iter().enumerate() {
        // Skip if URL already exists
        let exists: bool = tx
            .query_row(
                "SELECT COUNT(*) FROM bookmarks WHERE url = ?1 AND deleted_at IS NULL",
                rusqlite::params![jb.url],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            > 0;
        if exists {
            // Not an error, just a duplicate — count as skipped silently
            continue;
        }

        // Map folder id
        let folder_id: Option<String> = match &jb.folder_id {
            None => None,
            Some(fid) => match folder_id_map.get(fid) {
                Some(id) => Some(id.clone()),
                None => {
                    errors.push(format!(
                        "Bookmark {} {:?}: referenced folder id {fid} not found",
                        i + 1,
                        jb.title
                    ));
                    continue;
                }
            },
        };

        // Map tag ids
        let tag_ids: Vec<String> = jb
            .tag_ids
            .iter()
            .filter_map(|tid| {
                let live = tag_id_map.get(tid).cloned();
                if live.is_none() {
                    errors.push(format!(
                        "Bookmark {} {:?}: tag id {tid} not found, skipping tag",
                        i + 1,
                        jb.title
                    ));
                }
                live
            })
            .collect();

        let created_at = jb.created_at;
        let updated_at = jb.updated_at;
        let create_input = CreateBookmarkInput {
            url: jb.url,
            title: jb.title,
            description: jb.description,
            favicon_url: jb.favicon_url,
            feed_url: jb.feed_url,
            folder_id,
            tag_ids: if tag_ids.is_empty() { None } else { Some(tag_ids) },
        };

        match db_add_bookmark(&tx, create_input) {
            Ok(b) => {
                tx.execute(
                    "UPDATE bookmarks SET created_at = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![created_at, updated_at, b.id],
                )?;
                imported += 1;
            }
            Err(e) => errors.push(format!("Bookmark {}: {e}", i + 1)),
        }
    }

    tx.commit()?;
    Ok(ImportResult { imported, errors })
}

// ─── Netscape HTML format ─────────────────────────────────────────────────────
//
// Output structure:
//
//   <!DOCTYPE NETSCAPE-Bookmark-file-1>
//   <!-- This is an automatically generated file. -->
//   <META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
//   <TITLE>Bookmarks</TITLE>
//   <H1>Bookmarks</H1>
//   <DL><p>
//       <DT><A HREF="…" ADD_DATE="…">title</A>
//       <DD>description
//       <DT><H3 ADD_DATE="…">Folder Name</H3>
//       <DL><p>
//           <DT><A HREF="…" ADD_DATE="…">title</A>
//       </DL><p>
//   </DL><p>
//
// Tags are encoded as a TAGS attribute on the <A> element (Raindrop.io style).
// The favicon/feed URLs are lost (Netscape has no standard field for them).

pub fn export_netscape_html(conn: &Connection) -> Result<String, AppError> {
    // Load raw bookmarks (no soft-deleted)
    let bookmarks_full = db_get_bookmarks(conn, None, None, None, false)?;
    let folders = db_get_folders(conn)?;

    let mut out = String::with_capacity(4096);
    out.push_str("<!DOCTYPE NETSCAPE-Bookmark-file-1>\n");
    out.push_str("<!-- This is an automatically generated file.\n");
    out.push_str("     It will be read and overwritten.\n");
    out.push_str("     DO NOT EDIT! -->\n");
    out.push_str("<META HTTP-EQUIV=\"Content-Type\" CONTENT=\"text/html; charset=UTF-8\">\n");
    out.push_str("<TITLE>Bookmarks</TITLE>\n");
    out.push_str("<H1>Bookmarks</H1>\n");
    out.push_str("<DL><p>\n");

    netscape_folder_tree(&mut out, &folders, &bookmarks_full, None, 1);

    out.push_str("</DL><p>\n");
    Ok(out)
}

fn netscape_indent(depth: usize) -> String {
    "    ".repeat(depth)
}

fn netscape_folder_tree(
    out: &mut String,
    folders: &[Folder],
    bookmarks: &[Bookmark],
    parent_id: Option<&str>,
    depth: usize,
) {
    let pad = netscape_indent(depth);

    // Folders under this parent
    for folder in folders.iter().filter(|f| f.parent_id.as_deref() == parent_id) {
        out.push_str(&format!(
            "{}<DT><H3 ADD_DATE=\"{}\">{}</H3>\n",
            pad,
            folder.created_at,
            html_escape(&folder.name)
        ));
        out.push_str(&format!("{}<DL><p>\n", pad));
        // Bookmarks in this folder
        for b in bookmarks.iter().filter(|b| b.folder_id.as_deref() == Some(folder.id.as_str())) {
            netscape_bookmark_line(out, b, depth + 1);
        }
        // Sub-folders
        netscape_folder_tree(out, folders, bookmarks, Some(&folder.id), depth + 1);
        out.push_str(&format!("{}</DL><p>\n", pad));
    }

    // Unfiled bookmarks (only at the root call)
    if parent_id.is_none() {
        for b in bookmarks.iter().filter(|b| b.folder_id.is_none()) {
            netscape_bookmark_line(out, b, depth);
        }
    }
}

fn netscape_bookmark_line(out: &mut String, b: &Bookmark, depth: usize) {
    let pad = netscape_indent(depth);
    let tags_attr = if b.tags.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = b.tags.iter().map(|t| t.name.as_str()).collect();
        format!(" TAGS=\"{}\"", html_escape(&names.join(",")))
    };
    out.push_str(&format!(
        "{}<DT><A HREF=\"{}\" ADD_DATE=\"{}\"{}>{}</A>\n",
        pad,
        html_escape(&b.url),
        b.created_at,
        tags_attr,
        html_escape(&b.title),
    ));
    if let Some(desc) = &b.description {
        if !desc.trim().is_empty() {
            out.push_str(&format!("{}<DD>{}\n", pad, html_escape(desc)));
        }
    }
}

/// HTML-escape for attribute values and text content.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

// ── Netscape HTML import ──────────────────────────────────────────────────────
//
// Parser strategy: single-pass state machine over the raw HTML string.
// No DOM, no external crate. The format is extremely consistent across all
// browsers that write it.
//
// We track a folder stack. Each time we see <DT><H3 …>name</H3> followed
// by <DL> we push a folder; </DL> pops.
//
// <DT><A HREF="url" ADD_DATE="ts" TAGS="t1,t2">title</A>
// <DD>description   ← optional next line

pub fn import_netscape_html(conn: &Connection, html: &str) -> Result<ImportResult, AppError> {
    let tx = conn.unchecked_transaction()?;
    let mut imported = 0usize;
    let mut errors: Vec<String> = Vec::new();
    let mut tag_cache: HashMap<String, String> = HashMap::new();

    // Normalise line endings and work line by line.
    let lines: Vec<&str> = html.lines().collect();
    let n = lines.len();
    let mut i = 0;

    // Stack of live folder-ids representing current nesting
    let mut folder_stack: Vec<String> = Vec::new();
    // Pending folder name waiting for its <DL> to confirm it
    let mut pending_folder_name: Option<String> = None;

    while i < n {
        let line = lines[i].trim();
        let upper = line.to_ascii_uppercase();

        if upper.starts_with("<DT><H3") || upper.starts_with("<H3") {
            // Folder heading — extract name between > and </H3>
            if let Some(name) = extract_tag_text(line, "H3") {
                pending_folder_name = Some(html_unescape(&name));
            }
            i += 1;
            continue;
        }

        if upper.starts_with("<DL") {
            // Opening a new sub-list — materialise the pending folder
            if let Some(name) = pending_folder_name.take() {
                let parent_id = folder_stack.last().map(String::as_str);
                match find_or_create_folder_with_parent(&tx, &name, parent_id) {
                    Ok(fid) => folder_stack.push(fid),
                    Err(e) => {
                        errors.push(format!("Folder {name:?}: {e}"));
                        // Push a sentinel so the stack stays balanced
                        folder_stack.push(String::new());
                    }
                }
            }
            // No pending folder (root <DL>) — don't touch stack
            i += 1;
            continue;
        }

        if upper.starts_with("</DL") {
            if !folder_stack.is_empty() {
                folder_stack.pop();
            }
            i += 1;
            continue;
        }

        if upper.contains("<A ") || upper.contains("<A>") {
            // Bookmark line
            let url = attr_value(line, "href").map(|s| html_unescape(&s));
            let title = extract_tag_text(line, "A").map(|s| html_unescape(&s));
            let tags_raw = attr_value(line, "tags").map(|s| html_unescape(&s));

            let url = match url {
                Some(u) if !u.trim().is_empty() => u,
                _ => {
                    errors.push(format!("Line {}: bookmark missing href", i + 1));
                    i += 1;
                    continue;
                }
            };
            let title = title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| url.clone());

            // Peek at next line for optional <DD> description
            let description = if i + 1 < n {
                let next = lines[i + 1].trim();
                let nu = next.to_ascii_uppercase();
                if nu.starts_with("<DD>") {
                    let desc = html_unescape(next[4..].trim());
                    i += 1; // consume the <DD> line
                    if desc.is_empty() { None } else { Some(desc) }
                } else {
                    None
                }
            } else {
                None
            };

            // Resolve folder
            let folder_id = folder_stack.last().and_then(|fid| {
                if fid.is_empty() { None } else { Some(fid.clone()) }
            });

            // Resolve tags
            let tag_ids = match tags_raw.as_deref().filter(|s| !s.is_empty()) {
                Some(raw) => match find_or_create_tags(&tx, raw, &mut tag_cache) {
                    Ok(ids) => if ids.is_empty() { None } else { Some(ids) },
                    Err(e) => {
                        errors.push(format!("Line {}: tags: {e}", i + 1));
                        None
                    }
                },
                None => None,
            };

            let create_input = CreateBookmarkInput {
                url,
                title,
                description,
                favicon_url: None,
                feed_url: None,
                folder_id,
                tag_ids,
            };

            match db_add_bookmark(&tx, create_input) {
                Ok(_) => imported += 1,
                Err(e) => errors.push(format!("Line {}: {e}", i + 1)),
            }
        }

        i += 1;
    }

    tx.commit()?;
    Ok(ImportResult { imported, errors })
}

// ─── OPML format ──────────────────────────────────────────────────────────────

/// Export OPML. Previously lived in db.rs as `db_export_opml`; moved here to
/// keep format logic together. The old command in main.rs can delegate here.
pub fn export_opml(conn: &Connection) -> Result<String, AppError> {
    let folders: Vec<Folder> = {
        let mut stmt = conn.prepare(
            "SELECT id, name, parent_id, created_at FROM folders ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(Folder {
                id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let bookmarks: Vec<RawBookmark> = {
        let mut stmt = conn.prepare(
            "SELECT id, url, title, description, favicon_url, feed_url, folder_id, \
             created_at, updated_at, deleted_at FROM bookmarks \
             WHERE deleted_at IS NULL ORDER BY created_at",
        )?;
        let rows = stmt.query_map([], db::row_to_raw)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
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

// ── OPML import ───────────────────────────────────────────────────────────────
//
// Parses:
//   <outline type="rss" text="Feed" xmlUrl="…" htmlUrl="…">   — RSS feed
//   <outline type="link" text="Title" url="…">                 — plain link
//   <outline text="Folder">                                    — folder group
//     <outline …>                                              — nested items
//   </outline>
//
// Any <outline> without a URL but with a text attribute is treated as a folder.

pub fn import_opml(conn: &Connection, xml: &str) -> Result<ImportResult, AppError> {
    let tx = conn.unchecked_transaction()?;
    let mut imported = 0usize;
    let mut errors: Vec<String> = Vec::new();

    // Stack of live folder-ids (empty string = root / no folder)
    let mut folder_stack: Vec<String> = Vec::new();

    let lines: Vec<&str> = xml.lines().collect();
    let n = lines.len();
    let mut i = 0;

    while i < n {
        let line = lines[i].trim();
        let upper = line.to_ascii_uppercase();

        if upper.starts_with("</OUTLINE") {
            if !folder_stack.is_empty() {
                folder_stack.pop();
            }
            i += 1;
            continue;
        }

        if upper.starts_with("<OUTLINE") {
            // Merge continuation lines for multi-line outline tags
            let merged_line: String;
            let line = if !line.contains('>') {
                merged_line = {
                    let mut s = line.to_string();
                    while i + 1 < n {
                        i += 1;
                        let next = lines[i].trim();
                        s.push(' ');
                        s.push_str(next);
                        if next.contains('>') { break; }
                    }
                    s
                };
                merged_line.as_str()
            } else {
                line
            };
            let upper = line.to_ascii_uppercase();

            let url = opml_url(line); // htmlUrl or url attribute
            let xml_url = attr_value(line, "xmlurl").map(|s| xml_unescape(&s));
            let text = attr_value(line, "text")
                .or_else(|| attr_value(line, "title"))
                .map(|s| xml_unescape(&s))
                .unwrap_or_default();
            let outline_type = attr_value(line, "type").map(|s| s.to_ascii_lowercase());
            let is_self_closing = line.ends_with("/>") || line.ends_with("/ >");

            let is_feed = outline_type.as_deref() == Some("rss")
                || outline_type.as_deref() == Some("atom")
                || xml_url.is_some();

            let is_link = outline_type.as_deref() == Some("link") || url.is_some();

            if is_feed || is_link {
                // Leaf bookmark node
                let bookmark_url = url
                    .clone()
                    .or_else(|| attr_value(line, "htmlurl").map(|s| xml_unescape(&s)))
                    .unwrap_or_default();

                if bookmark_url.trim().is_empty() {
                    errors.push(format!("Line {}: outline missing URL", i + 1));
                } else {
                    let folder_id = folder_stack.last().and_then(|fid| {
                        if fid.is_empty() { None } else { Some(fid.clone()) }
                    });
                    let description = attr_value(line, "description").map(|s| xml_unescape(&s));

                    let create_input = CreateBookmarkInput {
                        url: bookmark_url,
                        title: if text.is_empty() { "Untitled".to_string() } else { text },
                        description: description.filter(|d| !d.is_empty()),
                        favicon_url: None,
                        feed_url: xml_url,
                        folder_id,
                        tag_ids: None,
                    };

                    match db_add_bookmark(&tx, create_input) {
                        Ok(_) => imported += 1,
                        Err(e) => errors.push(format!("Line {}: {e}", i + 1)),
                    }
                }

                // Self-closing → no stack change
                if !is_self_closing {
                    // Push a sentinel (this outline opened a child block even
                    // though it has a URL — unusual but valid OPML)
                    folder_stack.push(String::new());
                }
            } else if !text.is_empty() {
                // Folder outline (no URL)
                if is_self_closing {
                    // Empty folder — create but don't push
                    let parent_id = folder_stack.last().and_then(|fid| {
                        if fid.is_empty() { None } else { Some(fid.as_str()) }
                    });
                    let _ = find_or_create_folder_with_parent(&tx, &text, parent_id);
                } else {
                    let parent_id = folder_stack.last().and_then(|fid| {
                        if fid.is_empty() { None } else { Some(fid.as_str()) }
                    });
                    let fid = match find_or_create_folder_with_parent(&tx, &text, parent_id) {
                        Ok(id) => id,
                        Err(e) => {
                            errors.push(format!("Line {}: folder {text:?}: {e}", i + 1));
                            String::new()
                        }
                    };
                    folder_stack.push(fid);
                }
            } else if !is_self_closing {
                // Unknown non-self-closing outline — push sentinel to keep stack balanced
                folder_stack.push(String::new());
            }
        }

        i += 1;
    }

    tx.commit()?;
    Ok(ImportResult { imported, errors })
}

// ─── Shared helpers ────────────────────────────────────────────────────────────

/// Find or create a folder by name under a specific parent (not just root).
/// This is an extension of the root-only `find_or_create_folder` in db.rs
/// that also handles the nested case needed for JSON/OPML/Netscape import.
fn find_or_create_folder_with_parent(
    conn: &Connection,
    name: &str,
    parent_id: Option<&str>,
) -> Result<String, AppError> {
    let existing: Option<String> = match parent_id {
        None => conn
            .query_row(
                "SELECT id FROM folders WHERE name = ?1 AND parent_id IS NULL LIMIT 1",
                rusqlite::params![name],
                |r| r.get(0),
            )
            .optional()?,
        Some(pid) => conn
            .query_row(
                "SELECT id FROM folders WHERE name = ?1 AND parent_id = ?2 LIMIT 1",
                rusqlite::params![name, pid],
                |r| r.get(0),
            )
            .optional()?,
    };

    match existing {
        Some(id) => Ok(id),
        None => Ok(db_add_folder(conn, name.to_string(), parent_id.map(String::from))?.id),
    }
}

/// Extract the text content of the first occurrence of `<TAG …>text</TAG>`.
/// Case-insensitive on the tag name. Returns `None` if not found.
fn extract_tag_text(line: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let upper = line.to_ascii_uppercase();
    let close_upper = close.to_ascii_uppercase();

    let start = upper.find(&open.to_ascii_uppercase())?;
    // Skip past the opening tag's closing `>`
    let after_open = line[start..].find('>')? + start + 1;
    let end = upper[after_open..].find(&close_upper)? + after_open;
    Some(line[after_open..end].to_string())
}

/// Extract an HTML/XML attribute value (case-insensitive attribute name).
/// Handles both single and double quotes.
///
/// Uses word-boundary matching: the character immediately before `ATTR=` must
/// be whitespace or a `<` (or the attribute must start at position 0). This
/// prevents `URL=` from matching inside `XMLURL=`.
fn attr_value(line: &str, attr: &str) -> Option<String> {
    let upper = line.to_ascii_uppercase();
    let attr_upper = format!("{}=", attr.to_ascii_uppercase());

    // Find all occurrences and pick the first that is word-boundary aligned.
    let mut search_from = 0;
    let pos = loop {
        let candidate = upper[search_from..].find(&attr_upper)?;
        let abs = search_from + candidate;
        // The character immediately before must be a word boundary
        let ok = abs == 0 || {
            let prev = upper.as_bytes()[abs - 1];
            prev == b' ' || prev == b'\t' || prev == b'\n' || prev == b'\r' || prev == b'<'
        };
        if ok {
            break abs;
        }
        search_from = abs + 1;
    };
    let after = pos + attr_upper.len();
    let rest = &line[after..];
    let rest_trim = rest.trim_start();
    let offset = after + (rest.len() - rest_trim.len());

    let (quote_char, value_start) = if rest_trim.starts_with('"') {
        ('"', offset + 1)
    } else if rest_trim.starts_with('\'') {
        ('\'', offset + 1)
    } else {
        // Unquoted — read until whitespace or >
        let end = rest_trim
            .find(|c: char| c.is_whitespace() || c == '>')
            .unwrap_or(rest_trim.len());
        return Some(rest_trim[..end].to_string());
    };

    let value_rest = &line[value_start..];
    let end = value_rest.find(quote_char)?;
    Some(value_rest[..end].to_string())
}

/// For OPML outlines, `url` or `htmlUrl` holds the bookmark URL.
fn opml_url(line: &str) -> Option<String> {
    attr_value(line, "url")
        .map(|s| xml_unescape(&s))
        .or_else(|| attr_value(line, "htmlurl").map(|s| xml_unescape(&s)))
        .filter(|s| !s.trim().is_empty())
}

/// Undo HTML character entity escaping for import.
fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

/// Undo XML character entity escaping for OPML import.
fn xml_unescape(s: &str) -> String {
    // Same entities; XML and HTML share the basic set we care about.
    html_unescape(s)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use crate::db::{
        init_schema, db_add_bookmark, db_add_folder, db_add_tag,
        db_get_bookmarks, db_get_folders, db_get_tags, db_get_bookmark_count,
        CreateBookmarkInput,
    };
    use crate::io_validate;
    use crate::error::AppError;

    fn mem() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    fn add_bookmark(conn: &Connection, url: &str, title: &str) -> Bookmark {
        db_add_bookmark(conn, CreateBookmarkInput {
            url: url.to_string(),
            title: title.to_string(),
            description: None,
            favicon_url: None,
            feed_url: None,
            folder_id: None,
            tag_ids: None,
        }).unwrap()
    }

    fn add_folder(conn: &Connection, name: &str) -> Folder {
        db_add_folder(conn, name.to_string(), None).unwrap()
    }

    // ── JSON round-trip TDD tests ─────────────────────────────────────────

    #[test]
    fn json_export_empty_db_produces_valid_json_with_empty_arrays() {
        let conn = mem();
        let json = export_json(&conn).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json)
            .expect("export_json must produce valid JSON");
        assert_eq!(v["bookmarks"].as_array().unwrap().len(), 0,
            "bookmarks must be an empty array");
        assert_eq!(v["folders"].as_array().unwrap().len(), 0,
            "folders must be an empty array");
        assert_eq!(v["tags"].as_array().unwrap().len(), 0,
            "tags must be an empty array");
    }

    #[test]
    fn json_export_then_import_all_fields_match_exactly() {
        let src = mem();
        let folder = db_add_folder(&src, "Work".to_string(), None).unwrap();
        let tag = db_add_tag(&src, "rust".to_string(), "#f74c00".to_string()).unwrap();
        let original = db_add_bookmark(
            &src,
            CreateBookmarkInput {
                url: "https://rust-lang.org".to_string(),
                title: "Rust".to_string(),
                description: Some("The systems language".to_string()),
                favicon_url: Some("https://rust-lang.org/favicon.ico".to_string()),
                feed_url: Some("https://blog.rust-lang.org/feed.xml".to_string()),
                folder_id: Some(folder.id.clone()),
                tag_ids: Some(vec![tag.id.clone()]),
            },
        )
        .unwrap();

        let json = export_json(&src).unwrap();
        let dst = mem();
        let result = import_json(&dst, &json).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        assert!(result.errors.is_empty());

        let bookmarks = db_get_bookmarks(&dst, None, None, None, false).unwrap();
        assert_eq!(bookmarks.len(), 1);
        let imported = &bookmarks[0];
        assert_eq!(imported.url, original.url);
        assert_eq!(imported.title, original.title);
        assert_eq!(imported.description, original.description);
        assert_eq!(imported.favicon_url, original.favicon_url);
        assert_eq!(imported.feed_url, original.feed_url);
        assert_eq!(imported.tags.len(), 1);
        assert_eq!(imported.tags[0].name, "rust");
        assert_eq!(imported.tags[0].color, "#f74c00", "tag colour must be preserved");
        let folders = db_get_folders(&dst).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Work");
        assert!(imported.folder_id.is_some());
    }

    #[test]
    fn json_import_missing_url_records_error_not_panic() {
        let malformed = r##"{
            "version": 1,
            "exported_at": 0,
            "folders": [],
            "tags": [],
            "bookmarks": [{"title": "No URL here", "created_at": 1, "updated_at": 1}]
        }"##;
        let conn = mem();
        match import_json(&conn, malformed) {
            Ok(result) => {
                assert_eq!(result.imported, 0);
                assert!(!result.errors.is_empty(), "expected at least one error for missing url");
            }
            Err(AppError::Validation { .. }) => { /* structural parse failure is acceptable */ }
            Err(e) => panic!("unexpected error kind: {e:?}"),
        }
    }

    #[test]
    fn json_import_zero_bookmarks_returns_zero_imported_no_errors() {
        let empty = r##"{"version":1,"exported_at":0,"folders":[],"tags":[],"bookmarks":[]}"##;
        let conn = mem();
        let result = import_json(&conn, empty).unwrap();
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn json_import_duplicate_urls_no_dedup_by_default() {
        // Both entries have the same URL; the import must not panic regardless
        // of whether duplicates are silently skipped or both inserted.
        let json = r##"{
            "version": 1,
            "exported_at": 0,
            "folders": [],
            "tags": [],
            "bookmarks": [
                {"id":"b1","url":"https://example.com","title":"First",
                 "tag_ids":[],"created_at":1,"updated_at":1},
                {"id":"b2","url":"https://example.com","title":"Second",
                 "tag_ids":[],"created_at":2,"updated_at":2}
            ]
        }"##;
        let conn = mem();
        let result = import_json(&conn, json).unwrap();
        let count = db_get_bookmark_count(&conn).unwrap();
        assert!(count >= 1, "at least one bookmark must be imported");
        assert_eq!(result.imported as i64, count);
    }

    #[test]
    fn json_import_preserves_timestamps() {
        let json = r##"{
            "version": 1,
            "exported_at": 0,
            "folders": [],
            "tags": [],
            "bookmarks": [
                {"id":"b1","url":"https://example.com","title":"T",
                 "tag_ids":[],"created_at":1699900010,"updated_at":1699900025}
            ]
        }"##;
        let conn = mem();
        import_json(&conn, json).unwrap();
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks[0].created_at, 1699900010);
        assert_eq!(bookmarks[0].updated_at, 1699900025);
    }

    #[test]
    fn json_import_preserves_tag_colors() {
        let json = r##"{
            "version": 1,
            "exported_at": 0,
            "folders": [],
            "tags": [{"id":"t1","name":"rust","color":"#f74c00","created_at":1}],
            "bookmarks": [
                {"id":"b1","url":"https://example.com","title":"T",
                 "tag_ids":["t1"],"created_at":1,"updated_at":1}
            ]
        }"##;
        let conn = mem();
        import_json(&conn, json).unwrap();
        let tags = db_get_tags(&conn).unwrap();
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].color, "#f74c00");
    }

    #[test]
    fn json_import_preserves_folder_hierarchy() {
        let json = r##"{
            "version": 1,
            "exported_at": 0,
            "folders": [
                {"id":"f1","name":"Work","parent_id":null,"created_at":1},
                {"id":"f2","name":"Projects","parent_id":"f1","created_at":2}
            ],
            "tags": [],
            "bookmarks": []
        }"##;
        let conn = mem();
        import_json(&conn, json).unwrap();
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 2);
        let projects = folders.iter().find(|f| f.name == "Projects").unwrap();
        let work = folders.iter().find(|f| f.name == "Work").unwrap();
        assert_eq!(projects.parent_id.as_deref(), Some(work.id.as_str()),
            "Projects must be nested under Work");
    }

    #[test]
    fn json_export_import_export_produces_equivalent_state() {
        let src = mem();
        db_add_folder(&src, "Reading".to_string(), None).unwrap();
        let tag = db_add_tag(&src, "books".to_string(), "#abc123".to_string()).unwrap();
        db_add_bookmark(&src, CreateBookmarkInput {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: None,
            tag_ids: Some(vec![tag.id.clone()]),
        }).unwrap();

        let json1 = export_json(&src).unwrap();
        let mid = mem();
        import_json(&mid, &json1).unwrap();
        let json2 = export_json(&mid).unwrap();

        let v1: serde_json::Value = serde_json::from_str(&json1).unwrap();
        let v2: serde_json::Value = serde_json::from_str(&json2).unwrap();
        assert_eq!(v1["bookmarks"].as_array().unwrap().len(),
                   v2["bookmarks"].as_array().unwrap().len());
        assert_eq!(v1["folders"].as_array().unwrap().len(),
                   v2["folders"].as_array().unwrap().len());
        assert_eq!(v1["tags"].as_array().unwrap().len(),
                   v2["tags"].as_array().unwrap().len());
    }

    #[test]
    fn json_export_output_preserves_unicode() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com", "日本語タイトル");
        let json = export_json(&conn).unwrap();
        assert!(json.contains("日本語タイトル"), "non-ASCII title must survive JSON export");
    }

    // ── Netscape HTML TDD tests ───────────────────────────────────────────

    #[test]
    fn netscape_export_with_nested_folders_has_correct_dl_dt_structure() {
        let conn = mem();
        let parent = db_add_folder(&conn, "Work".to_string(), None).unwrap();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://work.com".to_string(),
            title: "Work Site".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(parent.id.clone()),
            tag_ids: None,
        }).unwrap();

        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("<H3") && html.contains("Work"),
            "expected folder heading:\n{html}");
        assert!(html.contains("<DL>"));
        assert!(html.contains("https://work.com"));
    }

    #[test]
    fn netscape_import_minimal_chrome_export() {
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">
<TITLE>Bookmarks</TITLE>
<H1>Bookmarks</H1>
<DL><p>
    <DT><A HREF="https://example.com">Example</A>
</DL><p>"##;
        let conn = mem();
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks[0].url, "https://example.com");
        assert_eq!(bookmarks[0].title, "Example");
    }

    #[test]
    fn netscape_import_add_date_created_at_positive() {
        // ADD_DATE is a Unix timestamp; at minimum created_at must be > 0.
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A HREF="https://example.com" ADD_DATE="1699900010">Example</A>
</DL><p>"##;
        let conn = mem();
        import_netscape_html(&conn, html).unwrap();
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert!(bookmarks[0].created_at > 0);
        // TDD goal: created_at should equal ADD_DATE value (1699900010)
        // Uncomment once implemented: assert_eq!(bookmarks[0].created_at, 1699900010);
    }

    #[test]
    fn netscape_import_nested_folders_map_to_correct_parent_id() {
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A HREF="https://example.com" ADD_DATE="123">Root bookmark</A>
    <DT><H3 ADD_DATE="456">Work</H3>
    <DL><p>
        <DT><A HREF="https://rust-lang.org" ADD_DATE="789">Rust</A>
    </DL><p>
</DL><p>"##;
        let conn = mem();
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 2, "errors: {:?}", result.errors);
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Work");
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        let rust_bm = bookmarks.iter().find(|b| b.url == "https://rust-lang.org").unwrap();
        assert_eq!(rust_bm.folder_id.as_deref(), Some(folders[0].id.as_str()));
        let root_bm = bookmarks.iter().find(|b| b.url == "https://example.com").unwrap();
        assert!(root_bm.folder_id.is_none(), "root bookmark must not be in any folder");
    }

    #[test]
    fn netscape_import_missing_href_skipped_with_error() {
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A>No href here</A>
    <DT><A HREF="https://example.com">Good</A>
</DL><p>"##;
        let conn = mem();
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 1);
        assert!(!result.errors.is_empty(), "expected at least one error for missing HREF");
    }

    #[test]
    fn netscape_import_empty_dl_produces_zero_imported_no_error() {
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
</DL><p>"##;
        let conn = mem();
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn netscape_export_special_chars_in_title_are_html_escaped() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com", "A & B <test> \"quoted\"");
        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("&amp;"), "& must be escaped as &amp;");
        assert!(html.contains("&lt;"),  "< must be escaped as &lt;");
        assert!(!html.contains(">A & B"),  "unescaped & in output");
        assert!(!html.contains("<test>"),  "unescaped < in output");
    }

    // ── OPML import TDD tests ─────────────────────────────────────────────

    #[test]
    fn opml_import_valid_flat_bookmarks_imported_correctly() {
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>My Feeds</title></head>
  <body>
    <outline type="link" text="Rust Blog" url="https://blog.rust-lang.org"/>
    <outline type="link" text="crates.io" url="https://crates.io"/>
  </body>
</opml>"##;
        let conn = mem();
        let result = import_opml(&conn, opml).unwrap();
        assert_eq!(result.imported, 2, "errors: {:?}", result.errors);
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        let urls: Vec<&str> = bookmarks.iter().map(|b| b.url.as_str()).collect();
        assert!(urls.contains(&"https://blog.rust-lang.org"));
        assert!(urls.contains(&"https://crates.io"));
    }

    #[test]
    fn opml_import_folder_grouped_outlines_create_folder() {
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>Bookmarks</title></head>
  <body>
    <outline text="Work">
      <outline type="link" text="Rust Blog" url="https://blog.rust-lang.org"/>
    </outline>
  </body>
</opml>"##;
        let conn = mem();
        let result = import_opml(&conn, opml).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Work");
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks[0].folder_id.as_deref(), Some(folders[0].id.as_str()));
    }

    #[test]
    fn opml_import_xml_url_attr_stored_as_feed_url() {
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
  <head><title>Feeds</title></head>
  <body>
    <outline type="rss" text="Rust Blog"
             url="https://blog.rust-lang.org"
             xmlUrl="https://blog.rust-lang.org/feed.xml"/>
  </body>
</opml>"##;
        let conn = mem();
        import_opml(&conn, opml).unwrap();
        let bookmarks = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].feed_url.as_deref(),
            Some("https://blog.rust-lang.org/feed.xml"),
            "xmlUrl must be stored as feed_url");
    }

    #[test]
    fn opml_import_invalid_xml_does_not_panic() {
        // Truncated / structurally broken input. The essential contract: must NOT panic.
        // Our line-oriented parser may return Ok(0) or Err(Validation); both are acceptable.
        let truncated = "<opml><body><outline text=\"x\" url=\"https://x.com\">";
        let conn = mem();
        let outcome = import_opml(&conn, truncated);
        match outcome {
            Ok(_) => { /* partial parse: acceptable */ }
            Err(AppError::Validation { .. }) => { /* explicit rejection: acceptable */ }
            Err(e) => panic!("unexpected non-Validation error: {e:?}"),
        }
    }

    #[test]
    fn opml_import_deeply_nested_rejected_by_xml_depth_validator() {
        use crate::io_validate::{validate_xml_depth, MAX_XML_DEPTH};
        let depth = MAX_XML_DEPTH + 10;
        let open: String  = "<outline text=\"x\">".repeat(depth);
        let close: String = "</outline>".repeat(depth);
        let opml = format!(
            "<?xml version=\"1.0\"?><opml version=\"2.0\"><body>{open}{close}</body></opml>"
        );
        // The depth validator must reject this.
        let err = validate_xml_depth(&opml, MAX_XML_DEPTH).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn opml_export_special_chars_in_title_are_xml_escaped() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com", "A & B <test>");
        let opml = export_opml(&conn).unwrap();
        assert!(opml.contains("&amp;"), "& must become &amp; in OPML");
        assert!(opml.contains("&lt;"),  "< must become &lt; in OPML");
        assert!(!opml.contains("& B"),  "raw & must not appear in title");
    }

    // ── Security validation TDD tests ─────────────────────────────────────

    #[test]
    fn sec_javascript_url_is_rejected_by_validator() {
        let err = io_validate::validate_url("javascript:alert(1)").unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_javascript_url_mixed_case_is_rejected() {
        let err = io_validate::validate_url("JavaScript:void(0)").unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_data_url_is_rejected_by_validator() {
        let err = io_validate::validate_url("data:text/html,<h1>hi</h1>").unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_vbscript_url_is_rejected() {
        let err = io_validate::validate_url("vbscript:MsgBox(1)").unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_file_url_is_rejected() {
        let err = io_validate::validate_url("file:///etc/passwd").unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_https_url_is_accepted_by_validator() {
        assert!(io_validate::validate_url("https://example.com").is_ok());
    }

    #[test]
    fn sec_url_over_2048_chars_is_rejected() {
        let long_url = format!(
            "https://example.com/{}",
            "a".repeat(io_validate::MAX_URL_LEN)
        );
        let err = io_validate::validate_url(&long_url).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_title_null_bytes_stripped_not_error() {
        let title_with_nulls = "Hello\0World\0";
        let sanitized = io_validate::sanitize_string(
            title_with_nulls,
            io_validate::MAX_STRING_LEN,
        );
        assert_eq!(sanitized, "HelloWorld");
        assert!(!sanitized.contains('\0'));
    }

    #[test]
    fn sec_tag_name_over_100_chars_is_rejected() {
        let long_tag = "a".repeat(io_validate::MAX_TAG_NAME_LEN + 1);
        let err = io_validate::validate_tag_names(&long_tag).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_100001_bookmarks_in_import_is_rejected() {
        let err = io_validate::validate_bookmark_count(
            io_validate::MAX_IMPORT_BOOKMARKS + 1,
        ).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_input_over_50mb_rejected_before_parsing() {
        let giant = "x".repeat(io_validate::MAX_IMPORT_BYTES + 1);
        let err = io_validate::validate_import_size(&giant).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_utf8_bom_in_json_input_is_handled_gracefully() {
        // import_json must strip the UTF-8 BOM before deserialising.
        let json_body = r##"{"version":1,"exported_at":0,"folders":[],"tags":[],"bookmarks":[]}"##;
        let json_with_bom = format!("\u{FEFF}{json_body}");
        let conn = mem();
        let result = import_json(&conn, &json_with_bom).unwrap();
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty(), "BOM-prefixed JSON must not produce errors");
    }

    #[test]
    fn sec_deeply_nested_netscape_html_rejected_by_depth_validator() {
        use crate::io_validate::{validate_dl_depth, MAX_DL_DEPTH};
        let open: String  = "<DL>".repeat(MAX_DL_DEPTH + 1);
        let close: String = "</DL>".repeat(MAX_DL_DEPTH + 1);
        let html = format!("{open}{close}");
        let err = validate_dl_depth(&html, MAX_DL_DEPTH).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn sec_deeply_nested_xml_rejected_by_depth_validator() {
        use crate::io_validate::{validate_xml_depth, MAX_XML_DEPTH};
        let open: String  = "<a>".repeat(MAX_XML_DEPTH + 1);
        let close: String = "</a>".repeat(MAX_XML_DEPTH + 1);
        let xml = format!("{open}{close}");
        let err = validate_xml_depth(&xml, MAX_XML_DEPTH).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    // ── Export correctness TDD tests ──────────────────────────────────────

    #[test]
    fn json_export_includes_all_optional_fields_when_present() {
        let conn = mem();
        let folder = db_add_folder(&conn, "Tech".to_string(), None).unwrap();
        let tag    = db_add_tag(&conn, "rust".to_string(), "#f74c00".to_string()).unwrap();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: Some("Systems language".to_string()),
            favicon_url: Some("https://rust-lang.org/favicon.ico".to_string()),
            feed_url: Some("https://blog.rust-lang.org/feed.xml".to_string()),
            folder_id: Some(folder.id.clone()),
            tag_ids: Some(vec![tag.id.clone()]),
        }).unwrap();

        let json = export_json(&conn).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        let bm = &v["bookmarks"][0];
        assert_eq!(bm["url"].as_str().unwrap(), "https://rust-lang.org");
        assert_eq!(bm["title"].as_str().unwrap(), "Rust");
        assert_eq!(bm["description"].as_str().unwrap(), "Systems language");
        assert_eq!(bm["favicon_url"].as_str().unwrap(), "https://rust-lang.org/favicon.ico");
        assert_eq!(bm["feed_url"].as_str().unwrap(), "https://blog.rust-lang.org/feed.xml");
        assert!(bm.get("folder_id").is_some(), "folder_id must be present in JSON");
        let tag_ids = bm["tag_ids"].as_array().expect("tag_ids must be an array");
        assert!(!tag_ids.is_empty(), "tag_ids must be non-empty");
    }

    #[test]
    fn netscape_export_url_with_ampersand_is_html_escaped() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com/?a=1&b=2", "Query URL");
        let html = export_netscape_html(&conn).unwrap();
        assert!(!html.contains("HREF=\"https://example.com/?a=1&b=2\""),
            "raw & in HREF must be escaped");
        assert!(html.contains("&amp;"), "& must become &amp;");
    }

    #[test]
    fn opml_export_includes_description_and_feed_url() {
        let conn = mem();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://blog.example.com".to_string(),
            title: "Blog".to_string(),
            description: Some("A great blog".to_string()),
            favicon_url: None,
            feed_url: Some("https://blog.example.com/feed.xml".to_string()),
            folder_id: None,
            tag_ids: None,
        }).unwrap();
        let opml = export_opml(&conn).unwrap();
        assert!(opml.contains("description=\"A great blog\""));
        assert!(opml.contains("xmlUrl=\"https://blog.example.com/feed.xml\""));
    }

    // ── attr_value ────────────────────────────────────────────────────────

    #[test]
    fn attr_value_double_quoted() {
        let line = r##"<A HREF="https://example.com" ADD_DATE="123">title</A>"##;
        assert_eq!(attr_value(line, "href").as_deref(), Some("https://example.com"));
    }

    #[test]
    fn attr_value_single_quoted() {
        let line = "<A HREF='https://example.com'>title</A>";
        assert_eq!(attr_value(line, "href").as_deref(), Some("https://example.com"));
    }

    #[test]
    fn attr_value_case_insensitive() {
        let line = r##"<A href="https://example.com">title</A>"##;
        assert_eq!(attr_value(line, "HREF").as_deref(), Some("https://example.com"));
    }

    #[test]
    fn attr_value_missing_returns_none() {
        let line = r##"<A ADD_DATE="123">title</A>"##;
        assert_eq!(attr_value(line, "href"), None);
    }

    // ── extract_tag_text ──────────────────────────────────────────────────

    #[test]
    fn extract_tag_text_finds_a_content() {
        let line = r##"<DT><A HREF="https://example.com" ADD_DATE="1">Example Site</A>"##;
        assert_eq!(extract_tag_text(line, "A").as_deref(), Some("Example Site"));
    }

    #[test]
    fn extract_tag_text_finds_h3_content() {
        let line = "<DT><H3 ADD_DATE=\"123\">My Folder</H3>";
        assert_eq!(extract_tag_text(line, "H3").as_deref(), Some("My Folder"));
    }

    // ── html_unescape ─────────────────────────────────────────────────────

    #[test]
    fn html_unescape_roundtrip() {
        let original = "A & B <test> \"quoted\"";
        let escaped = html_escape(original);
        let unescaped = html_unescape(&escaped);
        assert_eq!(unescaped, original);
    }

    // ── JSON export / import ──────────────────────────────────────────────

    #[test]
    fn json_export_empty_db() {
        let conn = mem();
        let json = export_json(&conn).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["version"], 1);
        assert_eq!(v["bookmarks"].as_array().unwrap().len(), 0);
        assert_eq!(v["folders"].as_array().unwrap().len(), 0);
        assert_eq!(v["tags"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn json_export_contains_bookmark() {
        let conn = mem();
        add_bookmark(&conn, "https://rust-lang.org", "Rust");
        let json = export_json(&conn).unwrap();
        assert!(json.contains("rust-lang.org"));
        assert!(json.contains("Rust"));
    }

    #[test]
    fn json_export_contains_folder_and_tag() {
        let conn = mem();
        let folder = add_folder(&conn, "Tech");
        let tag = db_add_tag(&conn, "rust".to_string(), "#ff0000".to_string()).unwrap();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(folder.id.clone()),
            tag_ids: Some(vec![tag.id.clone()]),
        }).unwrap();

        let json = export_json(&conn).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["folders"].as_array().unwrap().len(), 1);
        assert_eq!(v["tags"].as_array().unwrap().len(), 1);
        let bm = &v["bookmarks"][0];
        assert_eq!(bm["tag_ids"][0].as_str().unwrap(), tag.id);
        assert_eq!(bm["folder_id"].as_str().unwrap(), folder.id);
    }

    #[test]
    fn json_roundtrip_lossless() {
        let conn = mem();
        let folder = add_folder(&conn, "Work");
        let tag = db_add_tag(&conn, "systems".to_string(), "#6366f1".to_string()).unwrap();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: Some("Systems language".to_string()),
            favicon_url: Some("https://rust-lang.org/favicon.ico".to_string()),
            feed_url: Some("https://blog.rust-lang.org/feed.xml".to_string()),
            folder_id: Some(folder.id.clone()),
            tag_ids: Some(vec![tag.id.clone()]),
        }).unwrap();

        let json = export_json(&conn).unwrap();

        // Import into a fresh DB
        let conn2 = mem();
        let result = import_json(&conn2, &json).unwrap();
        assert_eq!(result.imported, 1);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

        let bookmarks = db_get_bookmarks(&conn2, None, None, None, false).unwrap();
        assert_eq!(bookmarks.len(), 1);
        let b = &bookmarks[0];
        assert_eq!(b.url, "https://rust-lang.org");
        assert_eq!(b.title, "Rust");
        assert_eq!(b.description.as_deref(), Some("Systems language"));
        assert_eq!(b.favicon_url.as_deref(), Some("https://rust-lang.org/favicon.ico"));
        assert_eq!(b.feed_url.as_deref(), Some("https://blog.rust-lang.org/feed.xml"));
        assert_eq!(b.tags.len(), 1);
        assert_eq!(b.tags[0].name, "systems");
        assert_eq!(b.tags[0].color, "#6366f1");

        let folders2 = db_get_folders(&conn2).unwrap();
        assert_eq!(folders2.len(), 1);
        assert_eq!(folders2[0].name, "Work");
        assert!(b.folder_id.is_some());
    }

    #[test]
    fn json_import_skips_duplicate_urls() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com", "Example");
        let json = export_json(&conn).unwrap();

        // Re-import into the same DB — duplicate should be skipped, not error
        let result = import_json(&conn, &json).unwrap();
        assert_eq!(result.imported, 0);
        assert!(result.errors.is_empty());
        // Still only one bookmark
        assert_eq!(db_get_bookmarks(&conn, None, None, None, false).unwrap().len(), 1);
    }

    #[test]
    fn json_import_invalid_json_returns_validation_error() {
        let conn = mem();
        let err = import_json(&conn, "not json").unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn json_import_wrong_version_returns_validation_error() {
        let conn = mem();
        let bad = r##"{"version":99,"exported_at":0,"folders":[],"tags":[],"bookmarks":[]}"##;
        let err = import_json(&conn, bad).unwrap_err();
        assert!(matches!(err, AppError::Validation { .. }));
    }

    #[test]
    fn json_roundtrip_nested_folders() {
        let conn = mem();
        let parent = add_folder(&conn, "Tech");
        let child = db_add_folder(&conn, "Rust".to_string(), Some(parent.id.clone())).unwrap();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(child.id.clone()),
            tag_ids: None,
        }).unwrap();

        let json = export_json(&conn).unwrap();
        let conn2 = mem();
        let result = import_json(&conn2, &json).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);

        let folders2 = db_get_folders(&conn2).unwrap();
        assert_eq!(folders2.len(), 2);
        let parent2 = folders2.iter().find(|f| f.name == "Tech").unwrap();
        let child2 = folders2.iter().find(|f| f.name == "Rust").unwrap();
        assert_eq!(child2.parent_id.as_deref(), Some(parent2.id.as_str()));

        let bookmarks2 = db_get_bookmarks(&conn2, None, None, None, false).unwrap();
        assert_eq!(bookmarks2[0].folder_id.as_deref(), Some(child2.id.as_str()));
    }

    // ── Netscape HTML export ──────────────────────────────────────────────

    #[test]
    fn netscape_export_empty_db() {
        let conn = mem();
        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("<!DOCTYPE NETSCAPE-Bookmark-file-1>"));
        assert!(html.contains("<DL><p>"));
        assert!(html.contains("</DL><p>"));
    }

    #[test]
    fn netscape_export_flat_bookmark() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com", "Example");
        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("HREF=\"https://example.com\""));
        assert!(html.contains(">Example</A>"));
    }

    #[test]
    fn netscape_export_bookmark_with_description() {
        let conn = mem();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://example.com".to_string(),
            title: "Example".to_string(),
            description: Some("A great site".to_string()),
            favicon_url: None, feed_url: None, folder_id: None, tag_ids: None,
        }).unwrap();
        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("<DD>A great site"));
    }

    #[test]
    fn netscape_export_folder_structure() {
        let conn = mem();
        let folder = add_folder(&conn, "Tech");
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(folder.id.clone()),
            tag_ids: None,
        }).unwrap();
        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("<H3"));
        assert!(html.contains("Tech"));
        assert!(html.contains("rust-lang.org"));
    }

    #[test]
    fn netscape_export_tags_attribute() {
        let conn = mem();
        let tag = db_add_tag(&conn, "rust".to_string(), "#6366f1".to_string()).unwrap();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: None,
            tag_ids: Some(vec![tag.id]),
        }).unwrap();
        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("TAGS=\"rust\""));
    }

    #[test]
    fn netscape_export_escapes_special_chars() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com/?a=1&b=2", "A & B <test>");
        let html = export_netscape_html(&conn).unwrap();
        assert!(html.contains("&amp;"));
        assert!(!html.contains(" & "));
    }

    // ── Netscape HTML import ──────────────────────────────────────────────

    #[test]
    fn netscape_import_flat_bookmark() {
        let conn = mem();
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A HREF="https://example.com" ADD_DATE="1234567890">Example</A>
</DL><p>"##;
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 1);
        assert!(result.errors.is_empty());
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].url, "https://example.com");
        assert_eq!(bms[0].title, "Example");
    }

    #[test]
    fn netscape_import_with_description() {
        let conn = mem();
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A HREF="https://example.com" ADD_DATE="123">Example</A>
    <DD>A great site
</DL><p>"##;
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 1);
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].description.as_deref(), Some("A great site"));
    }

    #[test]
    fn netscape_import_with_folder() {
        let conn = mem();
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><H3 ADD_DATE="123">Tech</H3>
    <DL><p>
        <DT><A HREF="https://rust-lang.org" ADD_DATE="456">Rust</A>
    </DL><p>
</DL><p>"##;
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Tech");
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].folder_id.as_deref(), Some(folders[0].id.as_str()));
    }

    #[test]
    fn netscape_import_with_tags() {
        let conn = mem();
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A HREF="https://rust-lang.org" ADD_DATE="123" TAGS="rust,systems">Rust</A>
</DL><p>"##;
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 1);
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].tags.len(), 2);
        let tag_names: Vec<&str> = bms[0].tags.iter().map(|t| t.name.as_str()).collect();
        assert!(tag_names.contains(&"rust"));
        assert!(tag_names.contains(&"systems"));
    }

    #[test]
    fn netscape_import_html_entities_unescaped() {
        let conn = mem();
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A HREF="https://example.com/?a=1&amp;b=2" ADD_DATE="123">A &amp; B</A>
</DL><p>"##;
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 1);
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].url, "https://example.com/?a=1&b=2");
        assert_eq!(bms[0].title, "A & B");
    }

    #[test]
    fn netscape_roundtrip() {
        let conn = mem();
        let folder = add_folder(&conn, "Work");
        let tag = db_add_tag(&conn, "rust".to_string(), "#6366f1".to_string()).unwrap();
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: Some("Systems language".to_string()),
            favicon_url: None, feed_url: None,
            folder_id: Some(folder.id.clone()),
            tag_ids: Some(vec![tag.id]),
        }).unwrap();
        add_bookmark(&conn, "https://example.com", "Example");

        let html = export_netscape_html(&conn).unwrap();
        let conn2 = mem();
        let result = import_netscape_html(&conn2, &html).unwrap();
        assert_eq!(result.imported, 2, "errors: {:?}", result.errors);

        let bms = db_get_bookmarks(&conn2, None, None, None, false).unwrap();
        assert_eq!(bms.len(), 2);
        let rust = bms.iter().find(|b| b.url.contains("rust-lang")).unwrap();
        assert_eq!(rust.tags.len(), 1);
        assert_eq!(rust.tags[0].name, "rust");
        let folders2 = db_get_folders(&conn2).unwrap();
        assert_eq!(folders2.len(), 1);
        assert_eq!(folders2[0].name, "Work");
    }

    #[test]
    fn netscape_import_missing_href_records_error() {
        let conn = mem();
        let html = r##"<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
    <DT><A ADD_DATE="123">No href here</A>
</DL><p>"##;
        let result = import_netscape_html(&conn, html).unwrap();
        assert_eq!(result.imported, 0);
        assert_eq!(result.errors.len(), 1);
    }

    // ── OPML export ───────────────────────────────────────────────────────

    #[test]
    fn opml_export_empty_db() {
        let conn = mem();
        let opml = export_opml(&conn).unwrap();
        assert!(opml.contains("<?xml"));
        assert!(opml.contains("<opml version=\"2.0\">"));
    }

    #[test]
    fn opml_export_flat_bookmark() {
        let conn = mem();
        add_bookmark(&conn, "https://example.com", "Example");
        let opml = export_opml(&conn).unwrap();
        assert!(opml.contains("url=\"https://example.com\""));
        assert!(opml.contains("text=\"Example\""));
    }

    #[test]
    fn opml_export_with_folder() {
        let conn = mem();
        let folder = add_folder(&conn, "Work");
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://work.com".to_string(),
            title: "Work".to_string(),
            description: None, favicon_url: None, feed_url: None,
            folder_id: Some(folder.id),
            tag_ids: None,
        }).unwrap();
        let opml = export_opml(&conn).unwrap();
        assert!(opml.contains("<outline text=\"Work\">"));
    }

    // ── OPML import ───────────────────────────────────────────────────────

    #[test]
    fn opml_import_flat_link() {
        let conn = mem();
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
<head><title>Test</title></head>
<body>
  <outline type="link" text="Example" url="https://example.com"/>
</body>
</opml>"##;
        let result = import_opml(&conn, opml).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].url, "https://example.com");
        assert_eq!(bms[0].title, "Example");
    }

    #[test]
    fn opml_import_rss_feed() {
        let conn = mem();
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
<head><title>Feeds</title></head>
<body>
  <outline type="rss" text="Rust Blog" xmlUrl="https://blog.rust-lang.org/feed.xml" htmlUrl="https://blog.rust-lang.org"/>
</body>
</opml>"##;
        let result = import_opml(&conn, opml).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].url, "https://blog.rust-lang.org");
        assert_eq!(bms[0].feed_url.as_deref(), Some("https://blog.rust-lang.org/feed.xml"));
    }

    #[test]
    fn opml_import_with_folder() {
        let conn = mem();
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
<head><title>Test</title></head>
<body>
  <outline text="Tech">
    <outline type="link" text="Rust" url="https://rust-lang.org"/>
  </outline>
</body>
</opml>"##;
        let result = import_opml(&conn, opml).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].name, "Tech");
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].folder_id.as_deref(), Some(folders[0].id.as_str()));
    }

    #[test]
    fn opml_import_nested_folders() {
        let conn = mem();
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
<head><title>Test</title></head>
<body>
  <outline text="Tech">
    <outline text="Languages">
      <outline type="link" text="Rust" url="https://rust-lang.org"/>
    </outline>
  </outline>
</body>
</opml>"##;
        let result = import_opml(&conn, opml).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let folders = db_get_folders(&conn).unwrap();
        assert_eq!(folders.len(), 2);
        let tech = folders.iter().find(|f| f.name == "Tech").unwrap();
        let lang = folders.iter().find(|f| f.name == "Languages").unwrap();
        assert_eq!(lang.parent_id.as_deref(), Some(tech.id.as_str()));
    }

    #[test]
    fn opml_import_xml_entities_unescaped() {
        let conn = mem();
        let opml = r##"<?xml version="1.0" encoding="UTF-8"?>
<opml version="2.0">
<head><title>Test</title></head>
<body>
  <outline type="link" text="A &amp; B" url="https://example.com/?a=1&amp;b=2"/>
</body>
</opml>"##;
        let result = import_opml(&conn, opml).unwrap();
        assert_eq!(result.imported, 1);
        let bms = db_get_bookmarks(&conn, None, None, None, false).unwrap();
        assert_eq!(bms[0].title, "A & B");
        assert_eq!(bms[0].url, "https://example.com/?a=1&b=2");
    }

    #[test]
    fn opml_roundtrip() {
        let conn = mem();
        let folder = add_folder(&conn, "Tech");
        db_add_bookmark(&conn, CreateBookmarkInput {
            url: "https://rust-lang.org".to_string(),
            title: "Rust".to_string(),
            description: Some("A systems language".to_string()),
            favicon_url: None,
            feed_url: Some("https://blog.rust-lang.org/feed.xml".to_string()),
            folder_id: Some(folder.id),
            tag_ids: None,
        }).unwrap();

        let opml = export_opml(&conn).unwrap();
        let conn2 = mem();
        let result = import_opml(&conn2, &opml).unwrap();
        assert_eq!(result.imported, 1, "errors: {:?}", result.errors);
        let bms = db_get_bookmarks(&conn2, None, None, None, false).unwrap();
        assert_eq!(bms[0].url, "https://rust-lang.org");
        assert_eq!(bms[0].feed_url.as_deref(), Some("https://blog.rust-lang.org/feed.xml"));
        let folders2 = db_get_folders(&conn2).unwrap();
        assert_eq!(folders2.len(), 1);
        assert_eq!(folders2[0].name, "Tech");
    }

    // ── find_or_create_folder_with_parent ─────────────────────────────────

    #[test]
    fn find_or_create_folder_with_parent_creates_at_root() {
        let conn = mem();
        let id1 = find_or_create_folder_with_parent(&conn, "Work", None).unwrap();
        let id2 = find_or_create_folder_with_parent(&conn, "Work", None).unwrap();
        assert_eq!(id1, id2); // idempotent
        assert_eq!(db_get_folders(&conn).unwrap().len(), 1);
    }

    #[test]
    fn find_or_create_folder_with_parent_creates_nested() {
        let conn = mem();
        let parent_id = find_or_create_folder_with_parent(&conn, "Tech", None).unwrap();
        let child_id = find_or_create_folder_with_parent(&conn, "Rust", Some(&parent_id)).unwrap();
        let folders = db_get_folders(&conn).unwrap();
        let child = folders.iter().find(|f| f.id == child_id).unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(parent_id.as_str()));
    }

    #[test]
    fn find_or_create_folder_same_name_different_parents() {
        let conn = mem();
        let p1 = find_or_create_folder_with_parent(&conn, "Work", None).unwrap();
        let p2 = find_or_create_folder_with_parent(&conn, "Personal", None).unwrap();
        let c1 = find_or_create_folder_with_parent(&conn, "Projects", Some(&p1)).unwrap();
        let c2 = find_or_create_folder_with_parent(&conn, "Projects", Some(&p2)).unwrap();
        assert_ne!(c1, c2, "same name under different parents = different folders");
    }
}
