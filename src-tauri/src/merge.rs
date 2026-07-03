//! Per-record merge for multi-machine sync.
//!
//! This replaces the old full-snapshot *last-write-wins*, where `push` blindly
//! overwrote the remote file with the local DB. That model silently dropped a
//! client's edits whenever another client pushed after it (and an idle client's
//! periodic push could stomp fresh remote edits with stale data).
//!
//! Model: every syncable row carries `updated_at` (seconds epoch, bumped on
//! *every* mutation including deletion) and an optional `deleted_at` tombstone.
//! Merge is a **pure** function over two snapshots: union by UUID, the row with
//! the greater rank wins. A delete is just a row whose `updated_at` advanced and
//! `deleted_at` got set, so deletes and edits race on the same clock — no
//! special case. Because every row carries a globally-unique UUID (`uuid::v4`,
//! minted client-side), two machines never collide on identity, and edits to
//! *different* records both survive.
//!
//! The rank is a total order, so `merge` is **commutative**: both machines
//! compute the same result regardless of who pulled whom.
//!
//! A row-wise union cannot see problems that exist *between* rows, so every
//! merge ends with [`normalize`]:
//! * live folders sharing (parent, name) collapse onto the higher-ranked one,
//!   and every bookmark / child-folder reference follows — two machines that
//!   each minted their own UUID for "News" otherwise end up with the folder
//!   twice, the bookmarks split between the two copies;
//! * parent cycles from concurrent folder moves are broken (the stalest move
//!   is undone) — a cycle makes every folder in it unreachable from the root;
//! * live rows never point at tombstoned or missing containers: folders fall
//!   to their nearest live ancestor, bookmarks to the inbox — mirroring what
//!   `db_delete_folder` does locally;
//! * live tags sharing a name collapse (the schema's UNIQUE(name)) and
//!   bookmark `tag_ids` are rewritten through the *transitive* remap;
//! * `tag_ids` keep only live tags, de-duplicated and sorted, so equal content
//!   always serializes identically (stable digests across machines);
//! * a purged bookmark (permanently deleted from the bin, content redacted,
//!   `purged_at` set) is forced to also carry `deleted_at`, so it can never
//!   surface in a view.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
    /// Set when the row was purged from the bin ("permanently deleted"): the
    /// content is redacted and the row is hidden from every view *including*
    /// the bin, but the tombstone still travels — a hard DELETE would just
    /// resurrect on the next merge, because every other machine still carries
    /// the row and absence always loses a union. `#[serde(default)]` keeps
    /// snapshots written by older builds parseable.
    #[serde(default)]
    pub purged_at: Option<i64>,
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

    /// Type-specific tiebreak applied just before the JSON tiebreak; see
    /// `SyncBookmark`'s cover-image preference.
    fn boost(&self) -> u8 {
        0
    }

    /// Total, order-independent ranking. Greater rank wins a same-id contest.
    ///
    /// 1. newer `updated_at` wins;
    /// 2. tie → a delete beats an edit (conservative: a deletion at the same
    ///    instant as an edit is honored);
    /// 3. tie → larger `deleted_at`;
    /// 4. tie → larger `boost()` (type-specific enrichment preference);
    /// 5. final tie → lexicographically greater JSON. Arbitrary, but a *total*
    ///    order, which is what makes the merge commutative across machines.
    fn rank(&self) -> (i64, bool, i64, u8, String) {
        (
            self.updated_at(),
            self.deleted_at().is_some(),
            self.deleted_at().unwrap_or(i64::MIN),
            self.boost(),
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
    /// Equal-clock preferences (the clock is whole seconds, so same-clock
    /// contests are routine):
    /// * a PURGED tombstone beats a plain one — the further deletion state
    ///   wins, mirroring the delete-beats-edit tie rule; otherwise a purge in
    ///   the same second as the delete could lose the JSON tiebreak and never
    ///   propagate;
    /// * a row WITH a cover image beats its cover-less twin. Covers are set by
    ///   a background scanner that deliberately does NOT bump `updated_at` (a
    ///   bulk backfill must never beat real user edits), so two machines
    ///   routinely hold same-clock rows differing only in `cover_url`; if the
    ///   cover-less row could win the tiebreak, the loser's scanner refetches
    ///   and the machines ping-pong the cover on every sync, forever.
    fn boost(&self) -> u8 {
        if self.purged_at.is_some() {
            2
        } else {
            self.cover_url.is_some() as u8
        }
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

/// Merge two snapshots into one. Pure and commutative: the row-wise union is
/// order-independent, and [`normalize`] is a deterministic function of that
/// union, so both machines still compute the identical result.
pub fn merge(local: SyncSnapshot, remote: SyncSnapshot) -> SyncSnapshot {
    normalize(SyncSnapshot {
        folders: merge_rows(local.folders, remote.folders),
        tags: merge_rows(local.tags, remote.tags),
        bookmarks: merge_rows(local.bookmarks, remote.bookmarks),
    })
}

/// Repair the cross-row invariants a row-wise merge cannot see (see the module
/// docs for the list). Deterministic and idempotent. The sync engine also runs
/// this on push-only cycles, so a snapshot that predates these rules — e.g.
/// one already carrying duplicate folders — heals on its next sync even when
/// there is nothing to pull.
pub fn normalize(mut snap: SyncSnapshot) -> SyncSnapshot {
    // A purged bookmark is always also a deleted one (purge = redacted
    // tombstone); enforce it in case a foreign snapshot disagrees.
    for b in &mut snap.bookmarks {
        if b.purged_at.is_some() && b.deleted_at.is_none() {
            b.deleted_at = b.purged_at;
        }
    }

    // `tags.name` is UNIQUE in the schema. Two machines may have independently
    // minted *different* UUIDs for the same tag name. Collapse such collisions
    // onto the higher-ranked tag and rewrite bookmark references to match,
    // otherwise applying the merged snapshot would hit a UNIQUE violation.
    let mut tag_remap = resolve_tag_name_collisions(&mut snap.tags);
    compress_remap(&mut tag_remap);

    reparent_to_live_ancestors(&mut snap.folders);
    break_parent_cycles(&mut snap.folders);
    let mut folder_remap = collapse_folder_name_collisions(&mut snap.folders);
    compress_remap(&mut folder_remap);

    let live_folder_ids: HashSet<&str> = snap
        .folders
        .iter()
        .filter(|f| f.deleted_at.is_none())
        .map(|f| f.id.as_str())
        .collect();
    let live_tag_ids: HashSet<&str> = snap
        .tags
        .iter()
        .filter(|t| t.deleted_at.is_none())
        .map(|t| t.id.as_str())
        .collect();

    for b in &mut snap.bookmarks {
        // Folder reference: follow a collapse, then require a live target — a
        // bookmark in a tombstoned/missing folder falls to the inbox, exactly
        // like `db_delete_folder` does for a local deletion.
        let folder_id = b
            .folder_id
            .take()
            .map(|fid| folder_remap.get(&fid).cloned().unwrap_or(fid));
        b.folder_id = folder_id.filter(|fid| live_folder_ids.contains(fid.as_str()));

        // Tag references: follow collapses, keep live targets only, and store
        // in canonical (sorted, deduped) form so equal content always
        // serializes to equal JSON regardless of which machine wrote it.
        let mut tag_ids: Vec<String> = std::mem::take(&mut b.tag_ids)
            .into_iter()
            .map(|tid| tag_remap.get(&tid).cloned().unwrap_or(tid))
            .filter(|tid| live_tag_ids.contains(tid.as_str()))
            .collect();
        tag_ids.sort();
        tag_ids.dedup();
        b.tag_ids = tag_ids;
    }

    snap.folders.sort_by(|a, b| a.id.cmp(&b.id));
    snap.tags.sort_by(|a, b| a.id.cmp(&b.id));
    snap.bookmarks.sort_by(|a, b| a.id.cmp(&b.id));
    snap
}

/// Point every live folder at its nearest LIVE ancestor: tombstoned parents in
/// the chain are skipped (a deleted parent must not orphan a live subfolder),
/// dangling ids and cycles-through-tombstones fall to the root. Tombstoned
/// folders are left untouched — nothing renders them.
fn reparent_to_live_ancestors(folders: &mut [SyncFolder]) {
    let by_id: HashMap<String, (bool, Option<String>)> = folders
        .iter()
        .map(|f| (f.id.clone(), (f.deleted_at.is_none(), f.parent_id.clone())))
        .collect();
    for f in folders.iter_mut() {
        if f.deleted_at.is_some() {
            continue;
        }
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(f.id.clone());
        let mut cur = f.parent_id.clone();
        f.parent_id = loop {
            match cur {
                None => break None,
                Some(pid) => {
                    if !seen.insert(pid.clone()) {
                        break None; // walked into a cycle — fall to the root
                    }
                    match by_id.get(&pid) {
                        None => break None, // dangling reference
                        Some((true, _)) => break Some(pid),
                        Some((false, next)) => cur = next.clone(), // skip tombstone
                    }
                }
            }
        };
    }
}

/// Break parent cycles among live folders. Two concurrent moves can each be
/// locally valid yet form a cycle once merged (A moves X under Y while B moves
/// Y under X); the sidebar can then reach none of the cycle's folders from the
/// root. Detach the lowest-ranked member of each cycle — the stalest move is
/// the one undone. Folder order is fixed (sorted ids) so every machine breaks
/// the same cycles the same way.
fn break_parent_cycles(folders: &mut [SyncFolder]) {
    loop {
        let parent_of: HashMap<String, Option<String>> = folders
            .iter()
            .filter(|f| f.deleted_at.is_none())
            .map(|f| (f.id.clone(), f.parent_id.clone()))
            .collect();
        let mut order: Vec<&String> = parent_of.keys().collect();
        order.sort();

        let mut acyclic: HashSet<String> = HashSet::new();
        let mut victim: Option<String> = None;
        'scan: for start in order {
            let mut path: Vec<String> = Vec::new();
            let mut cur = Some(start.clone());
            while let Some(id) = cur {
                if acyclic.contains(&id) {
                    break; // joins a chain already known to reach the root
                }
                if let Some(pos) = path.iter().position(|p| p == &id) {
                    // `path[pos..]` is the cycle. Undo the stalest move in it.
                    victim = path[pos..]
                        .iter()
                        .min_by_key(|fid| folders.iter().find(|f| &&f.id == fid).map(|f| f.rank()))
                        .cloned();
                    break 'scan;
                }
                path.push(id.clone());
                cur = parent_of.get(&id).cloned().flatten();
            }
            acyclic.extend(path);
        }

        match victim.take() {
            Some(id) => {
                if let Some(f) = folders.iter_mut().find(|f| f.id == id) {
                    f.parent_id = None;
                }
            }
            None => break,
        }
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

/// Collapse live folders sharing (parent, name) onto the higher-ranked one.
/// Two machines independently creating "News" each mint their own UUID; a
/// row-wise merge keeps both, splitting the bookmarks between two identical-
/// looking folders. Children re-parent to the winner, which can turn two
/// formerly-separate subtrees into siblings — so the pass loops to a fixpoint
/// and nested duplicate trees collapse level by level.
///
/// Returns `dropped_id -> kept_id`. Chains are possible across rounds (a
/// round-1 winner may lose round 2); callers must [`compress_remap`].
fn collapse_folder_name_collisions(folders: &mut Vec<SyncFolder>) -> HashMap<String, String> {
    let mut remap: HashMap<String, String> = HashMap::new();
    loop {
        let mut groups: HashMap<(Option<String>, String), Vec<usize>> = HashMap::new();
        for (i, f) in folders.iter().enumerate() {
            if f.deleted_at.is_none() {
                groups
                    .entry((f.parent_id.clone(), f.name.clone()))
                    .or_default()
                    .push(i);
            }
        }

        let mut drop: Vec<usize> = Vec::new();
        for idxs in groups.into_values().filter(|v| v.len() > 1) {
            let winner = *idxs.iter().max_by_key(|&&i| folders[i].rank()).unwrap();
            for &i in &idxs {
                if i != winner {
                    remap.insert(folders[i].id.clone(), folders[winner].id.clone());
                    drop.push(i);
                }
            }
        }
        if drop.is_empty() {
            return remap;
        }
        drop.sort_unstable();
        for i in drop.into_iter().rev() {
            folders.remove(i);
        }

        // Re-point parent references at the survivors so the next grouping
        // round sees the merged tree. Values in `remap` always outrank their
        // keys, so this chain-following walk cannot loop.
        for f in folders.iter_mut() {
            while let Some(pid) = &f.parent_id {
                match remap.get(pid) {
                    Some(winner) => f.parent_id = Some(winner.clone()),
                    None => break,
                }
            }
        }
    }
}

/// Path-compress a `dropped -> kept` map so every entry points at its FINAL
/// survivor. Without this, a chain (A→B, B→C, where B itself lost a later
/// contest) leaves references rewritten onto the already-dropped B — which is
/// how a bookmark could silently lose its tag when three machines minted the
/// same tag name.
fn compress_remap(remap: &mut HashMap<String, String>) {
    let keys: Vec<String> = remap.keys().cloned().collect();
    for k in keys {
        let mut target = remap[&k].clone();
        let mut hops = 0usize;
        while let Some(next) = remap.get(&target) {
            target = next.clone();
            hops += 1;
            if hops > remap.len() {
                break; // defensive: ranks make cycles impossible, never hang
            }
        }
        remap.insert(k, target);
    }
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
            purged_at: None,
        }
    }

    fn fld(id: &str, name: &str, parent_id: Option<&str>, updated_at: i64, deleted_at: Option<i64>) -> SyncFolder {
        SyncFolder {
            id: id.into(),
            name: name.into(),
            parent_id: parent_id.map(Into::into),
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

    // ── folder name collision collapse ────────────────────────────────────────

    fn live_folders(s: &SyncSnapshot) -> Vec<&SyncFolder> {
        s.folders.iter().filter(|f| f.deleted_at.is_none()).collect()
    }

    /// THE reported bug: both machines created "News" independently (different
    /// UUIDs) and filed different bookmarks inside. A row-wise merge kept both
    /// folders — "the same folder twice, each with different bookmarks". They
    /// must collapse into one folder holding the union of the bookmarks.
    #[test]
    fn duplicate_folder_names_collapse_and_bookmarks_unite() {
        let mut local = SyncSnapshot::default();
        local.folders.push(fld("F-local", "News", None, 10, None));
        let mut b1 = bm("A", "article-1", 10, None);
        b1.folder_id = Some("F-local".into());
        local.bookmarks.push(b1);

        let mut remote = SyncSnapshot::default();
        remote.folders.push(fld("F-remote", "News", None, 20, None)); // newer wins
        let mut b2 = bm("Z", "article-2", 10, None);
        b2.folder_id = Some("F-remote".into());
        remote.bookmarks.push(b2);

        let m = merge(local.clone(), remote.clone());
        let live = live_folders(&m);
        assert_eq!(live.len(), 1, "one live 'News' folder must survive");
        assert_eq!(live[0].id, "F-remote");
        assert_eq!(find(&m, "A").unwrap().folder_id.as_deref(), Some("F-remote"));
        assert_eq!(find(&m, "Z").unwrap().folder_id.as_deref(), Some("F-remote"));

        assert_eq!(m, merge(remote, local), "collapse must be commutative");
    }

    /// Same name under *different* parents is NOT a duplicate (Work/Projects
    /// vs Personal/Projects must both survive — same identity rule the
    /// io.rs importers use).
    #[test]
    fn same_name_under_different_parents_survives() {
        let mut snap = SyncSnapshot::default();
        snap.folders.push(fld("P1", "Work", None, 10, None));
        snap.folders.push(fld("P2", "Personal", None, 10, None));
        snap.folders.push(fld("C1", "Projects", Some("P1"), 10, None));
        snap.folders.push(fld("C2", "Projects", Some("P2"), 10, None));
        let m = merge(snap, SyncSnapshot::default());
        assert_eq!(live_folders(&m).len(), 4);
    }

    /// A live folder and a tombstoned folder may share a name — tombstones
    /// don't contend.
    #[test]
    fn dead_folder_does_not_contend_for_name() {
        let mut snap = SyncSnapshot::default();
        snap.folders.push(fld("F1", "News", None, 10, Some(10)));
        snap.folders.push(fld("F2", "News", None, 5, None));
        let m = merge(snap, SyncSnapshot::default());
        assert_eq!(m.folders.len(), 2);
        assert_eq!(live_folders(&m).len(), 1);
    }

    /// Nested duplicate trees: both machines built News/Tech. Collapsing the
    /// roots makes the two "Tech" children siblings, which must then collapse
    /// too (fixpoint), re-pointing grandchildren and bookmarks transitively.
    #[test]
    fn nested_duplicate_trees_collapse_to_fixpoint() {
        let mut local = SyncSnapshot::default();
        local.folders.push(fld("N1", "News", None, 10, None));
        local.folders.push(fld("T1", "Tech", Some("N1"), 10, None));
        let mut b1 = bm("A", "a", 10, None);
        b1.folder_id = Some("T1".into());
        local.bookmarks.push(b1);

        let mut remote = SyncSnapshot::default();
        remote.folders.push(fld("N2", "News", None, 20, None));
        remote.folders.push(fld("T2", "Tech", Some("N2"), 20, None));
        let mut b2 = bm("Z", "z", 10, None);
        b2.folder_id = Some("T2".into());
        remote.bookmarks.push(b2);

        let m = merge(local, remote);
        let live = live_folders(&m);
        assert_eq!(live.len(), 2, "News and Tech, once each: {live:?}");
        let news = live.iter().find(|f| f.name == "News").unwrap();
        let tech = live.iter().find(|f| f.name == "Tech").unwrap();
        assert_eq!(news.id, "N2");
        assert_eq!(tech.id, "T2");
        assert_eq!(tech.parent_id.as_deref(), Some("N2"));
        assert_eq!(find(&m, "A").unwrap().folder_id.as_deref(), Some("T2"));
        assert_eq!(find(&m, "Z").unwrap().folder_id.as_deref(), Some("T2"));
    }

    /// Three copies of the same folder (three machines): remap chains must be
    /// compressed so no reference lands on a dropped intermediate id.
    #[test]
    fn three_way_folder_collapse_leaves_no_dangling_refs() {
        let mut snap = SyncSnapshot::default();
        snap.folders.push(fld("F1", "News", None, 10, None));
        snap.folders.push(fld("F2", "News", None, 20, None));
        snap.folders.push(fld("F3", "News", None, 30, None));
        for (i, f) in ["F1", "F2", "F3"].iter().enumerate() {
            let mut b = bm(&format!("B{i}"), "b", 10, None);
            b.folder_id = Some((*f).into());
            snap.bookmarks.push(b);
        }
        let m = merge(snap, SyncSnapshot::default());
        assert_eq!(live_folders(&m).len(), 1);
        for b in &m.bookmarks {
            assert_eq!(b.folder_id.as_deref(), Some("F3"), "bookmark {} dangles", b.id);
        }
    }

    /// Healing an ALREADY-duplicated dataset: after the original bug both the
    /// local DB and the remote carry the same two duplicate folders, so the
    /// dupes are no longer "one per side". Normalization must still collapse
    /// them — this is what repairs existing user data on the first sync.
    #[test]
    fn preexisting_duplicates_on_both_sides_heal() {
        let mut side = SyncSnapshot::default();
        side.folders.push(fld("F1", "News", None, 10, None));
        side.folders.push(fld("F2", "News", None, 20, None));
        let mut b1 = bm("A", "a", 10, None);
        b1.folder_id = Some("F1".into());
        let mut b2 = bm("Z", "z", 10, None);
        b2.folder_id = Some("F2".into());
        side.bookmarks.push(b1);
        side.bookmarks.push(b2);

        let m = merge(side.clone(), side);
        assert_eq!(live_folders(&m).len(), 1);
        assert_eq!(find(&m, "A").unwrap().folder_id.as_deref(), Some("F2"));
        assert_eq!(find(&m, "Z").unwrap().folder_id.as_deref(), Some("F2"));
    }

    // ── parent cycles / tombstoned parents ────────────────────────────────────

    /// Machine A moved X under Y; machine B moved Y under X. Merged row-wise
    /// both edits survive → cycle → the sidebar reaches neither from the root.
    /// The stalest move (Y→X at t=20) is undone; the newer one is kept.
    #[test]
    fn concurrent_moves_forming_cycle_are_broken() {
        let mut local = SyncSnapshot::default();
        local.folders.push(fld("X", "x", Some("Y"), 30, None)); // newer move
        local.folders.push(fld("Y", "y", None, 5, None));
        let mut remote = SyncSnapshot::default();
        remote.folders.push(fld("X", "x", None, 5, None));
        remote.folders.push(fld("Y", "y", Some("X"), 20, None)); // stalest move

        let m = merge(local.clone(), remote.clone());
        let x = m.folders.iter().find(|f| f.id == "X").unwrap();
        let y = m.folders.iter().find(|f| f.id == "Y").unwrap();
        assert_eq!(x.parent_id.as_deref(), Some("Y"), "newer move survives");
        assert_eq!(y.parent_id, None, "stalest move is undone");
        assert_eq!(m, merge(remote, local), "cycle break must be commutative");
    }

    #[test]
    fn self_parent_is_detached() {
        let mut snap = SyncSnapshot::default();
        snap.folders.push(fld("X", "x", Some("X"), 10, None));
        let m = merge(snap, SyncSnapshot::default());
        assert_eq!(m.folders[0].parent_id, None);
    }

    /// Deleting a parent folder on machine A while machine B created a child
    /// under it: the live child must climb to the nearest LIVE ancestor.
    #[test]
    fn live_folder_under_tombstoned_parent_climbs_to_live_ancestor() {
        let mut snap = SyncSnapshot::default();
        snap.folders.push(fld("Root", "root", None, 10, None));
        snap.folders.push(fld("Mid", "mid", Some("Root"), 20, Some(20))); // deleted
        snap.folders.push(fld("Leaf", "leaf", Some("Mid"), 15, None)); // live child
        let m = merge(snap, SyncSnapshot::default());
        let leaf = m.folders.iter().find(|f| f.id == "Leaf").unwrap();
        assert_eq!(leaf.parent_id.as_deref(), Some("Root"));
    }

    #[test]
    fn live_folder_with_missing_parent_falls_to_root() {
        let mut snap = SyncSnapshot::default();
        snap.folders.push(fld("Leaf", "leaf", Some("gone"), 15, None));
        let m = merge(snap, SyncSnapshot::default());
        assert_eq!(m.folders[0].parent_id, None);
    }

    /// A bookmark filed into a folder that another machine deleted must fall
    /// to the inbox (folder_id = None) — matching db_delete_folder's local
    /// behavior — instead of pointing at an invisible tombstone.
    #[test]
    fn bookmark_in_tombstoned_folder_falls_to_inbox() {
        let mut local = SyncSnapshot::default();
        let mut b = bm("A", "a", 15, None);
        b.folder_id = Some("F".into());
        local.bookmarks.push(b);
        let mut remote = SyncSnapshot::default();
        remote.folders.push(fld("F", "news", None, 20, Some(20)));

        let m = merge(local, remote);
        assert_eq!(find(&m, "A").unwrap().folder_id, None);
    }

    #[test]
    fn bookmark_with_missing_folder_falls_to_inbox() {
        let mut local = SyncSnapshot::default();
        let mut b = bm("A", "a", 15, None);
        b.folder_id = Some("never-synced".into());
        local.bookmarks.push(b);
        let m = merge(local, SyncSnapshot::default());
        assert_eq!(find(&m, "A").unwrap().folder_id, None);
    }

    // ── tag remap chains / canonical tag_ids ──────────────────────────────────

    /// Three machines each minted "rust". The old one-hop remap could rewrite
    /// a bookmark onto an id that itself lost a later contest — silently
    /// detaching the tag. Every reference must land on the FINAL winner.
    #[test]
    fn three_way_tag_collision_no_dangling_refs() {
        let mut snap = SyncSnapshot::default();
        // Insertion order ascending by rank exercises the chain: winner flips
        // twice (T1 → T2 → T3) while bookmarks reference every generation.
        snap.tags.push(tag("T1", "rust", 10, None));
        snap.tags.push(tag("T2", "rust", 20, None));
        snap.tags.push(tag("T3", "rust", 30, None));
        for (i, t) in ["T1", "T2", "T3"].iter().enumerate() {
            let mut b = bm(&format!("B{i}"), "b", 10, None);
            b.tag_ids = vec![(*t).to_string()];
            snap.bookmarks.push(b);
        }
        let m = merge(snap, SyncSnapshot::default());
        let live: Vec<_> = m.tags.iter().filter(|t| t.deleted_at.is_none()).collect();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].id, "T3");
        for b in &m.bookmarks {
            assert_eq!(b.tag_ids, vec!["T3".to_string()], "bookmark {} dangles", b.id);
        }
    }

    #[test]
    fn tag_ids_are_deduped_sorted_and_live_only() {
        let mut snap = SyncSnapshot::default();
        snap.tags.push(tag("T-b", "beta", 10, None));
        snap.tags.push(tag("T-a", "alpha", 10, None));
        snap.tags.push(tag("T-dead", "dead", 10, Some(10)));
        let mut b = bm("A", "a", 10, None);
        b.tag_ids = vec![
            "T-b".into(),
            "T-missing".into(),
            "T-a".into(),
            "T-dead".into(),
            "T-b".into(),
        ];
        snap.bookmarks.push(b);
        let m = merge(snap, SyncSnapshot::default());
        assert_eq!(
            find(&m, "A").unwrap().tag_ids,
            vec!["T-a".to_string(), "T-b".to_string()]
        );
    }

    // ── purge / cover rank ────────────────────────────────────────────────────

    /// At equal clocks the row WITH a cover image must win, no matter which
    /// side it sits on — otherwise the cover-less row can win the JSON
    /// tiebreak and the machines ping-pong the cover forever (the scanner
    /// refetches it without bumping updated_at).
    #[test]
    fn cover_presence_wins_equal_clock_tie() {
        let plain = bm("A", "a", 10, None);
        let mut covered = bm("A", "a", 10, None);
        covered.cover_url = Some("https://covers.example/a.png".into());

        let m1 = merge(snap_bm(vec![plain.clone()]), snap_bm(vec![covered.clone()]));
        let m2 = merge(snap_bm(vec![covered]), snap_bm(vec![plain]));
        assert!(find(&m1, "A").unwrap().cover_url.is_some());
        assert_eq!(m1, m2);
    }

    /// A newer real edit still beats an older row that merely has a cover.
    #[test]
    fn newer_edit_beats_older_cover() {
        let mut covered = bm("A", "old-title", 10, None);
        covered.cover_url = Some("https://covers.example/a.png".into());
        let edited = bm("A", "new-title", 20, None);
        let m = merge(snap_bm(vec![covered]), snap_bm(vec![edited]));
        assert_eq!(find(&m, "A").unwrap().title, "new-title");
    }

    #[test]
    fn purged_bookmark_is_forced_deleted() {
        let mut b = bm("A", "", 30, None); // purged but deleted_at lost somehow
        b.purged_at = Some(30);
        let m = merge(snap_bm(vec![b]), SyncSnapshot::default());
        let a = find_any(&m, "A").unwrap();
        assert_eq!(a.deleted_at, Some(30));
        assert_eq!(a.purged_at, Some(30));
    }

    /// The clock is whole seconds, so "delete, sync, purge" can land the purge
    /// in the same second as the tombstone the other machines hold. The purged
    /// row must win that tie or the purge never propagates.
    #[test]
    fn purged_tombstone_beats_plain_tombstone_at_equal_clock() {
        let plain = bm("A", "gone", 50, Some(50));
        let mut purged = bm("A", "", 50, Some(50));
        purged.purged_at = Some(50);

        let m1 = merge(snap_bm(vec![plain.clone()]), snap_bm(vec![purged.clone()]));
        let m2 = merge(snap_bm(vec![purged]), snap_bm(vec![plain]));
        assert!(find_any(&m1, "A").unwrap().purged_at.is_some());
        assert_eq!(m1, m2);
    }

    #[test]
    fn purge_beats_stale_live_copy_and_round_trips() {
        let stale = bm("A", "still-alive", 10, None);
        let mut purged = bm("A", "", 30, Some(20));
        purged.purged_at = Some(30);
        let m = merge(snap_bm(vec![stale]), snap_bm(vec![purged]));
        let a = find_any(&m, "A").unwrap();
        assert_eq!(a.purged_at, Some(30), "purge must propagate");

        let json = to_json(&m).unwrap();
        assert_eq!(from_json(&json), Some(m), "purged_at survives the wire");
    }

    /// Older snapshots (no `purged_at` key) still parse — the field defaults.
    #[test]
    fn v2_snapshot_without_purged_at_still_parses() {
        let json = r#"{"version":2,"snapshot":{"folders":[],"tags":[],
            "bookmarks":[{"id":"X","url":"https://e.com","title":"x",
            "description":null,"favicon_url":null,"feed_url":null,"cover_url":null,
            "folder_id":null,"tag_ids":[],"created_at":1,"updated_at":1,
            "deleted_at":null}]}}"#;
        let snap = from_json(json).expect("legacy v2 must parse");
        assert_eq!(snap.bookmarks[0].purged_at, None);
    }

    // ── normalization properties ──────────────────────────────────────────────

    fn messy_snapshot() -> SyncSnapshot {
        let mut s = SyncSnapshot::default();
        s.folders.push(fld("F1", "News", None, 10, None));
        s.folders.push(fld("F2", "News", None, 20, None));
        s.folders.push(fld("Dead", "old", None, 30, Some(30)));
        s.folders.push(fld("Orphan", "kid", Some("Dead"), 10, None));
        s.folders.push(fld("CycA", "a", Some("CycB"), 12, None));
        s.folders.push(fld("CycB", "b", Some("CycA"), 11, None));
        s.tags.push(tag("T1", "rust", 10, None));
        s.tags.push(tag("T2", "rust", 20, None));
        let mut b = bm("A", "a", 10, None);
        b.folder_id = Some("F1".into());
        b.tag_ids = vec!["T1".into(), "T2".into()];
        s.bookmarks.push(b);
        let mut d = bm("D", "d", 15, None);
        d.folder_id = Some("Dead".into());
        s.bookmarks.push(d);
        s
    }

    #[test]
    fn normalize_is_idempotent() {
        let once = normalize(messy_snapshot());
        let twice = normalize(once.clone());
        assert_eq!(once, twice);
    }

    #[test]
    fn merge_of_messy_snapshots_is_commutative() {
        let a = messy_snapshot();
        let mut b = messy_snapshot();
        b.folders.push(fld("F3", "News", None, 15, None));
        b.bookmarks.push(bm("Z", "z", 9, None));
        assert_eq!(merge(a.clone(), b.clone()), merge(b, a));
    }

    fn find_any<'a>(s: &'a SyncSnapshot, id: &str) -> Option<&'a SyncBookmark> {
        s.bookmarks.iter().find(|b| b.id == id)
    }
}
