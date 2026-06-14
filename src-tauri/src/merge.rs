//! Per-record merge for multi-machine sync.
//!
//! This replaces the old full-snapshot *last-write-wins*, where `push` blindly
//! overwrote the remote file with the local DB. That model silently dropped a
//! client's edits whenever another client pushed after it (and an idle client's
//! periodic push could stomp fresh remote edits with stale data).
//!
//! Model: every syncable row carries `updated_at` (ms epoch, bumped on *every*
//! mutation including deletion) and an optional `deleted_at` tombstone. Merge is
//! a **pure** function over two snapshots: union by UUID, the row with the
//! greater rank wins. A delete is just a row whose `updated_at` advanced and
//! `deleted_at` got set, so deletes and edits race on the same clock — no
//! special case. Because every row carries a globally-unique UUID (`uuid::v4`,
//! minted client-side), two machines never collide on identity, and edits to
//! *different* records both survive.
//!
//! The rank is a total order, so `merge` is **commutative**: both machines
//! compute the same result regardless of who pulled whom.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SyncFolder {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SyncTag {
    pub id: String,
    pub name: String,
    pub color: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SyncBookmark {
    pub id: String,
    pub url: String,
    pub title: String,
    pub description: Option<String>,
    pub favicon_url: Option<String>,
    pub feed_url: Option<String>,
    pub cover_url: Option<String>,
    pub folder_id: Option<String>,
    pub tag_ids: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SyncSnapshot {
    pub folders: Vec<SyncFolder>,
    pub tags: Vec<SyncTag>,
    pub bookmarks: Vec<SyncBookmark>,
}

/// Wire format version for the Drive snapshot. v1 was the old active-only
/// `io::JsonExport` (no tombstones); v2 is the tombstone-carrying `SyncSnapshot`
/// that per-record merge requires.
pub const SYNC_FORMAT_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
struct SyncFile {
    version: u32,
    snapshot: SyncSnapshot,
}

/// Serialize a snapshot to the versioned JSON stored on Drive.
pub fn to_json(snapshot: &SyncSnapshot) -> Result<String, serde_json::Error> {
    serde_json::to_string(&SyncFile {
        version: SYNC_FORMAT_VERSION,
        snapshot: snapshot.clone(),
    })
}

/// Parse a v2 snapshot. Returns `Ok(None)` for any JSON that isn't a v2
/// `SyncFile` (e.g. an old active-only export) so the caller can fall back to
/// the legacy import path instead of erroring.
pub fn from_json(s: &str) -> Option<SyncSnapshot> {
    match serde_json::from_str::<SyncFile>(s) {
        Ok(f) if f.version == SYNC_FORMAT_VERSION => Some(f.snapshot),
        _ => None,
    }
}

/// A row that can take part in a per-record merge.
trait Mergeable: Serialize + Clone {
    fn id(&self) -> &str;
    fn updated_at(&self) -> i64;
    fn deleted_at(&self) -> Option<i64>;

    /// Total, order-independent ranking. Greater rank wins a same-id contest.
    ///
    /// 1. newer `updated_at` wins;
    /// 2. tie → a delete beats an edit (conservative: a deletion at the same
    ///    instant as an edit is honored);
    /// 3. tie → larger `deleted_at`;
    /// 4. final tie → lexicographically greater JSON. Arbitrary, but a *total*
    ///    order, which is what makes the merge commutative across machines.
    fn rank(&self) -> (i64, bool, i64, String) {
        (
            self.updated_at(),
            self.deleted_at().is_some(),
            self.deleted_at().unwrap_or(i64::MIN),
            serde_json::to_string(self).unwrap_or_default(),
        )
    }
}

impl Mergeable for SyncFolder {
    fn id(&self) -> &str {
        &self.id
    }
    fn updated_at(&self) -> i64 {
        self.updated_at
    }
    fn deleted_at(&self) -> Option<i64> {
        self.deleted_at
    }
}

impl Mergeable for SyncTag {
    fn id(&self) -> &str {
        &self.id
    }
    fn updated_at(&self) -> i64 {
        self.updated_at
    }
    fn deleted_at(&self) -> Option<i64> {
        self.deleted_at
    }
}

impl Mergeable for SyncBookmark {
    fn id(&self) -> &str {
        &self.id
    }
    fn updated_at(&self) -> i64 {
        self.updated_at
    }
    fn deleted_at(&self) -> Option<i64> {
        self.deleted_at
    }
}

/// Union two row sets by id, keeping the higher-ranked row for each id.
/// Output is sorted by id so the result is deterministic and round-trips stably.
fn merge_rows<T: Mergeable>(local: Vec<T>, remote: Vec<T>) -> Vec<T> {
    let mut by_id: HashMap<String, T> = HashMap::new();
    for row in local.into_iter().chain(remote.into_iter()) {
        match by_id.get(row.id()) {
            Some(existing) if existing.rank() >= row.rank() => {}
            _ => {
                by_id.insert(row.id().to_string(), row);
            }
        }
    }
    let mut out: Vec<T> = by_id.into_values().collect();
    out.sort_by(|a, b| a.id().cmp(b.id()));
    out
}

/// Merge two snapshots into one. Pure and commutative.
pub fn merge(local: SyncSnapshot, remote: SyncSnapshot) -> SyncSnapshot {
    let folders = merge_rows(local.folders, remote.folders);
    let mut tags = merge_rows(local.tags, remote.tags);
    let bookmarks = merge_rows(local.bookmarks, remote.bookmarks);

    // `tags.name` is UNIQUE in the schema. Two machines may have independently
    // minted *different* UUIDs for the same tag name. Collapse such collisions
    // onto the higher-ranked tag and rewrite bookmark references to match,
    // otherwise applying the merged snapshot would hit a UNIQUE violation.
    let remap = resolve_tag_name_collisions(&mut tags);
    let bookmarks = apply_tag_remap(bookmarks, &remap);

    SyncSnapshot {
        folders,
        tags,
        bookmarks,
    }
}

/// Drop duplicate **live** tags sharing a name, keeping the higher-ranked one.
/// Returns a map `dropped_id -> kept_id` for rewriting bookmark tag references.
fn resolve_tag_name_collisions(tags: &mut Vec<SyncTag>) -> HashMap<String, String> {
    let mut winner_by_name: HashMap<String, usize> = HashMap::new();
    let mut remap: HashMap<String, String> = HashMap::new();
    let mut drop: Vec<usize> = Vec::new();

    for i in 0..tags.len() {
        if tags[i].deleted_at.is_some() {
            continue; // tombstones don't contend for a name
        }
        let name = tags[i].name.clone();
        match winner_by_name.get(&name).copied() {
            None => {
                winner_by_name.insert(name, i);
            }
            Some(w) => {
                let (keep, lose) = if tags[i].rank() > tags[w].rank() {
                    winner_by_name.insert(name, i);
                    (i, w)
                } else {
                    (w, i)
                };
                remap.insert(tags[lose].id.clone(), tags[keep].id.clone());
                drop.push(lose);
            }
        }
    }

    drop.sort_unstable();
    drop.dedup();
    for idx in drop.into_iter().rev() {
        tags.remove(idx);
    }
    remap
}

/// Rewrite bookmark tag references through `remap`, de-duplicating the result
/// while preserving order.
fn apply_tag_remap(
    bookmarks: Vec<SyncBookmark>,
    remap: &HashMap<String, String>,
) -> Vec<SyncBookmark> {
    if remap.is_empty() {
        return bookmarks;
    }
    bookmarks
        .into_iter()
        .map(|mut b| {
            let mut seen: Vec<String> = Vec::with_capacity(b.tag_ids.len());
            for t in b.tag_ids.into_iter() {
                let mapped = remap.get(&t).cloned().unwrap_or(t);
                if !seen.contains(&mapped) {
                    seen.push(mapped);
                }
            }
            b.tag_ids = seen;
            b
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bm(id: &str, title: &str, updated_at: i64, deleted_at: Option<i64>) -> SyncBookmark {
        SyncBookmark {
            id: id.into(),
            url: format!("https://example.com/{id}"),
            title: title.into(),
            description: None,
            favicon_url: None,
            feed_url: None,
            cover_url: None,
            folder_id: None,
            tag_ids: vec![],
            created_at: 1,
            updated_at,
            deleted_at,
        }
    }

    fn tag(id: &str, name: &str, updated_at: i64, deleted_at: Option<i64>) -> SyncTag {
        SyncTag {
            id: id.into(),
            name: name.into(),
            color: "#fff".into(),
            created_at: 1,
            updated_at,
            deleted_at,
        }
    }

    fn snap_bm(bms: Vec<SyncBookmark>) -> SyncSnapshot {
        SyncSnapshot {
            bookmarks: bms,
            ..Default::default()
        }
    }

    fn find<'a>(s: &'a SyncSnapshot, id: &str) -> Option<&'a SyncBookmark> {
        s.bookmarks.iter().find(|b| b.id == id)
    }

    /// Disjoint edits on two machines: BOTH records survive. This is the core
    /// multi-client guarantee the old snapshot-LWW broke.
    #[test]
    fn disjoint_edits_both_survive() {
        let local = snap_bm(vec![bm("A", "local-A", 10, None)]);
        let remote = snap_bm(vec![bm("B", "remote-B", 10, None)]);
        let m = merge(local, remote);
        assert_eq!(m.bookmarks.len(), 2);
        assert_eq!(find(&m, "A").unwrap().title, "local-A");
        assert_eq!(find(&m, "B").unwrap().title, "remote-B");
    }

    #[test]
    fn same_id_newer_remote_wins() {
        let local = snap_bm(vec![bm("A", "old", 10, None)]);
        let remote = snap_bm(vec![bm("A", "new", 20, None)]);
        let m = merge(local, remote);
        assert_eq!(m.bookmarks.len(), 1);
        assert_eq!(find(&m, "A").unwrap().title, "new");
    }

    #[test]
    fn same_id_newer_local_wins() {
        let local = snap_bm(vec![bm("A", "new", 30, None)]);
        let remote = snap_bm(vec![bm("A", "old", 20, None)]);
        let m = merge(local, remote);
        assert_eq!(find(&m, "A").unwrap().title, "new");
    }

    /// A delete made on one machine propagates to the other.
    #[test]
    fn delete_propagates() {
        let local = snap_bm(vec![bm("A", "alive", 10, None)]);
        let remote = snap_bm(vec![bm("A", "alive", 20, Some(20))]);
        let m = merge(local, remote);
        assert_eq!(find(&m, "A").unwrap().deleted_at, Some(20));
    }

    /// A delete LOSES to a later edit on the other machine (edit resurrects).
    #[test]
    fn newer_edit_beats_older_delete() {
        let local = snap_bm(vec![bm("A", "edited", 30, None)]);
        let remote = snap_bm(vec![bm("A", "gone", 20, Some(20))]);
        let m = merge(local, remote);
        let a = find(&m, "A").unwrap();
        assert_eq!(a.deleted_at, None);
        assert_eq!(a.title, "edited");
    }

    /// Equal `updated_at`: delete wins the tie (conservative).
    #[test]
    fn tie_delete_beats_edit() {
        let local = snap_bm(vec![bm("A", "edit", 50, None)]);
        let remote = snap_bm(vec![bm("A", "del", 50, Some(50))]);
        let m = merge(local, remote);
        assert!(find(&m, "A").unwrap().deleted_at.is_some());
    }

    /// merge(a, b) == merge(b, a) — order independence across machines.
    #[test]
    fn commutative() {
        let a = snap_bm(vec![
            bm("A", "a1", 10, None),
            bm("B", "b-local", 30, None),
            bm("C", "c-del", 40, Some(40)),
        ]);
        let b = snap_bm(vec![
            bm("A", "a2", 20, None),
            bm("B", "b-remote", 15, None),
            bm("C", "c-alive", 10, None),
            bm("D", "d-new", 5, None),
        ]);
        assert_eq!(merge(a.clone(), b.clone()), merge(b, a));
    }

    /// Tag UUIDs differ across machines but the name is the same: collapse to
    /// one tag and rewrite the bookmark's reference (else UNIQUE(name) blows up).
    #[test]
    fn tag_name_collision_remapped() {
        let mut local = SyncSnapshot::default();
        local.tags.push(tag("T1", "rust", 10, None));
        let mut blocal = bm("A", "post", 10, None);
        blocal.tag_ids = vec!["T1".into()];
        local.bookmarks.push(blocal);

        let mut remote = SyncSnapshot::default();
        remote.tags.push(tag("T2", "rust", 20, None)); // newer wins the name
        let mut bremote = bm("Z", "other", 10, None);
        bremote.tag_ids = vec!["T2".into()];
        remote.bookmarks.push(bremote);

        let m = merge(local, remote);
        let live: Vec<_> = m.tags.iter().filter(|t| t.deleted_at.is_none()).collect();
        assert_eq!(live.len(), 1, "one live 'rust' tag survives");
        assert_eq!(live[0].id, "T2");
        assert_eq!(find(&m, "A").unwrap().tag_ids, vec!["T2".to_string()]);
        assert_eq!(find(&m, "Z").unwrap().tag_ids, vec!["T2".to_string()]);
    }

    #[test]
    fn json_round_trips() {
        let mut snap = snap_bm(vec![bm("A", "keep", 7, None), bm("B", "dead", 9, Some(9))]);
        snap.tags.push(tag("T", "rust", 3, None));
        let json = to_json(&snap).unwrap();
        assert_eq!(from_json(&json), Some(snap));
    }

    #[test]
    fn from_json_rejects_non_v2() {
        // An old active-only export (no `version: 2` SyncFile wrapper) → None,
        // so the caller falls back to the legacy import path.
        assert_eq!(from_json(r#"{"version":1,"bookmarks":[]}"#), None);
        assert_eq!(from_json("not json"), None);
    }

    #[test]
    fn empty_snapshots_merge_to_empty() {
        let m = merge(SyncSnapshot::default(), SyncSnapshot::default());
        assert!(m.folders.is_empty() && m.tags.is_empty() && m.bookmarks.is_empty());
    }

    #[test]
    fn output_sorted_by_id_for_determinism() {
        let local = snap_bm(vec![bm("C", "c", 1, None), bm("A", "a", 1, None)]);
        let remote = snap_bm(vec![bm("B", "b", 1, None)]);
        let m = merge(local, remote);
        let ids: Vec<_> = m.bookmarks.iter().map(|b| b.id.as_str()).collect();
        assert_eq!(ids, vec!["A", "B", "C"]);
    }
}
