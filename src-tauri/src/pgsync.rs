//! Neon/Postgres sync — the primary multi-machine sync backend.
//!
//! Strategy: same per-record merge semantics as the old Drive backup (see
//! [`crate::merge`]), but the remote is a user-owned Postgres database holding
//! **real rows** instead of one JSON blob, and transfer is **incremental**:
//!
//! * every remote row carries a `seq` assigned from one global Postgres
//!   sequence by a trigger on INSERT/UPDATE — a server-side, monotonic,
//!   gap-tolerant change counter that no client clock can skew;
//! * each device keeps a cursor (`last_seq`); a pull is
//!   `WHERE seq > cursor` — only rows that moved since the device last looked;
//! * each device tracks its own local changes in SQLite (`sync_dirty`,
//!   maintained by triggers — see `db::init_schema`) and pushes only those,
//!   plus any rows the merge/normalize decided the remote has wrong.
//!
//! One sync cycle runs inside a single Postgres transaction holding an
//! advisory lock, so concurrent devices serialize; the cursor is read from the
//! sequence *inside* the lock, which also means a device never re-pulls its own
//! pushes. Conflict resolution is byte-for-byte the existing [`crate::merge`]
//! rank/normalize — this module only changes how rows travel, not who wins.
//!
//! The remote schema is app-managed (idempotent DDL on connect), and plain
//! Postgres role credentials are the only auth — no Neon-specific API is used,
//! so any reachable Postgres works. See `docs/neon-sync-plan.md`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::error::AppError;
use crate::merge::{self, SyncSnapshot};

pub use crate::gdrive::SyncMode;

fn serr(msg: impl std::fmt::Display) -> AppError {
    AppError::Backup { message: msg.to_string() }
}

// ─── Persisted config (settings.json → "neon") ─────────────────────────────────

/// Build-time defaults (todo-app pattern): bake a host/db/user into the binary
/// via env vars so a self-built app comes preconfigured. The settings UI can
/// always override them; release binaries just start blank.
pub const ENV_HOST: Option<&str> = option_env!("FERRICO_NEON_HOST");
pub const ENV_DBNAME: Option<&str> = option_env!("FERRICO_NEON_DB");
pub const ENV_USER: Option<&str> = option_env!("FERRICO_NEON_USER");

pub const DEFAULT_DBNAME: &str = "neondb";

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct NeonConfig {
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub dbname: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    /// Password, stored here only where the OS keyring is unavailable (mobile:
    /// the file lives in the app-private sandbox; desktop keeps this `None`
    /// and uses the keyring instead — see `secret.rs`).
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    /// Periodic full-sync interval in minutes; `0` disables the periodic pull.
    /// (Pushes don't wait for this — local changes push within seconds.)
    #[serde(default)]
    pub interval_min: u64,
    /// Highest remote `seq` this device has reconciled. `0` = never synced.
    #[serde(default)]
    pub last_seq: i64,
    /// Epoch seconds of the last successful sync (display only).
    #[serde(default)]
    pub last_sync: Option<i64>,
}

impl NeonConfig {
    /// Effective connection fields: explicit settings first, then the
    /// build-time env defaults.
    pub fn effective_host(&self) -> Option<String> {
        self.host.clone().filter(|s| !s.trim().is_empty()).or_else(|| ENV_HOST.map(Into::into))
    }
    pub fn effective_dbname(&self) -> String {
        self.dbname
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| ENV_DBNAME.map(Into::into))
            .unwrap_or_else(|| DEFAULT_DBNAME.into())
    }
    pub fn effective_user(&self) -> Option<String> {
        self.user.clone().filter(|s| !s.trim().is_empty()).or_else(|| ENV_USER.map(Into::into))
    }
    /// Configured = every connection field present (password is checked at
    /// connect time — it lives in the keyring on desktop).
    pub fn is_configured(&self) -> bool {
        self.effective_host().is_some() && self.effective_user().is_some()
    }
}

/// View model for the frontend — never exposes the password.
#[derive(Serialize)]
pub struct NeonStatus {
    pub configured: bool,
    pub enabled: bool,
    pub host: Option<String>,
    pub dbname: String,
    pub user: Option<String>,
    pub interval_min: u64,
    pub last_seq: i64,
    pub last_sync: Option<i64>,
}

pub fn load_config(data_dir: &Path) -> NeonConfig {
    let path = data_dir.join("settings.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| v.get("neon").cloned())
        .and_then(|b| serde_json::from_value::<NeonConfig>(b).ok())
        .unwrap_or_default()
}

/// Read-modify-write of the settings root so sibling keys (`api_token`,
/// `backup`) survive — same pattern as `gdrive::save_config`.
pub fn save_config(data_dir: &Path, cfg: &NeonConfig) {
    let path = data_dir.join("settings.json");
    let mut root = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    root["neon"] = serde_json::to_value(cfg).unwrap_or(serde_json::Value::Null);
    if let Ok(s) = serde_json::to_string_pretty(&root) {
        std::fs::write(&path, s).ok();
    }
}

// ─── Store seam ─────────────────────────────────────────────────────────────────

/// The remote operations one sync cycle needs, in call order. Abstracting them
/// lets [`sync_once`] run against an in-memory fake in tests — and later
/// against an HTTP transport (Data API / thin server) for a web build —
/// without touching the sync logic.
///
/// Contract: `begin` opens a transaction and takes a global advisory lock, so
/// everything until `commit` sees and produces a serialized view; `current_seq`
/// inside that window is therefore a safe next cursor (nothing can commit a
/// higher seq before our commit).
#[allow(async_fn_in_trait)]
pub trait SyncStore {
    async fn begin(&mut self) -> Result<(), AppError>;
    /// All rows with `seq > cursor`, as a (partial) snapshot.
    async fn pull_since(&mut self, cursor: i64) -> Result<SyncSnapshot, AppError>;
    /// Upsert the given rows; the remote assigns fresh `seq`s.
    async fn push(&mut self, changes: &SyncSnapshot) -> Result<(), AppError>;
    /// Highest `seq` handed out so far (including our own pushes).
    async fn current_seq(&mut self) -> Result<i64, AppError>;
    async fn commit(&mut self) -> Result<(), AppError>;
}

// ─── The sync cycle ─────────────────────────────────────────────────────────────

/// Outcome of one cycle. The engine applies `merged` locally (only when
/// `changed_local`), persists `new_cursor`, and clears the dirty entries it
/// captured before calling.
pub struct SyncOutcome {
    pub merged: SyncSnapshot,
    pub changed_local: bool,
    pub new_cursor: i64,
    pub pushed: usize,
}

fn sorted(mut snap: SyncSnapshot) -> SyncSnapshot {
    snap.folders.sort_by(|a, b| a.id.cmp(&b.id));
    snap.tags.sort_by(|a, b| a.id.cmp(&b.id));
    snap.bookmarks.sort_by(|a, b| a.id.cmp(&b.id));
    snap
}

/// Decide which merged rows the remote still needs. A row is pushed when the
/// remote's latest known version differs from the merged winner AND we have a
/// reason to believe the remote is behind:
/// * the row is locally dirty (a user edit),
/// * or the merge/normalize produced something the raw local row didn't have
///   (a cross-row repair like a folder collapse),
/// * or the remote itself just sent a version that lost the merge (its copy is
///   stale — push the correction).
///
/// Everything else was already in sync before this cycle and is skipped, which
/// is what makes the transfer incremental.
fn push_set<T: Clone + PartialEq, F: Fn(&T) -> &str>(
    merged: &[T],
    local: &[T],
    pulled: &[T],
    dirty_ids: &HashSet<&str>,
    id_of: F,
) -> Vec<T> {
    let local_by_id: HashMap<&str, &T> = local.iter().map(|r| (id_of(r), r)).collect();
    let pulled_by_id: HashMap<&str, &T> = pulled.iter().map(|r| (id_of(r), r)).collect();
    merged
        .iter()
        .filter(|r| {
            let id = id_of(r);
            let remote_has_it = pulled_by_id.get(id).map(|p| *p == *r).unwrap_or(false);
            if remote_has_it {
                return false;
            }
            dirty_ids.contains(id)
                || pulled_by_id.contains_key(id)
                || local_by_id.get(id).map(|l| *l != *r).unwrap_or(true)
        })
        .cloned()
        .collect()
}

/// One reconcile cycle against a [`SyncStore`]. Pure orchestration: no SQLite,
/// no Tauri, no clock — every input is passed in.
///
/// `dirty` is the `(kind, id)` set captured from `sync_dirty` *before* the
/// call; `cursor` is the device's `last_seq` (an empty local dataset forces a
/// full pull regardless, mirroring the Drive engine's "empty local must never
/// win" rule).
pub async fn sync_once<S: SyncStore>(
    store: &mut S,
    local: SyncSnapshot,
    dirty: &[(String, String)],
    cursor: i64,
    mode: SyncMode,
) -> Result<SyncOutcome, AppError> {
    let local = sorted(local);
    let local_empty =
        local.bookmarks.is_empty() && local.folders.is_empty() && local.tags.is_empty();
    // A wiped/fresh local DB may carry a stale cursor in settings.json; pulling
    // from 0 re-reads everything so the remote dataset wins, never absence.
    let cursor = if local_empty { 0 } else { cursor };

    let dirty_folders: HashSet<&str> = dirty
        .iter()
        .filter(|(k, _)| k == "folder")
        .map(|(_, id)| id.as_str())
        .collect();
    let dirty_tags: HashSet<&str> =
        dirty.iter().filter(|(k, _)| k == "tag").map(|(_, id)| id.as_str()).collect();
    let dirty_bookmarks: HashSet<&str> = dirty
        .iter()
        .filter(|(k, _)| k == "bookmark")
        .map(|(_, id)| id.as_str())
        .collect();

    store.begin().await?;
    let pulled = store.pull_since(cursor).await?;

    // `merge` unions by id, so a *partial* remote snapshot is safe: rows the
    // pull didn't include simply keep their local version.
    let merged = merge::merge(local.clone(), pulled.clone());
    let changed_local = merged != local;

    let mut pushed = 0usize;
    if mode == SyncMode::Full {
        let changes = SyncSnapshot {
            folders: push_set(&merged.folders, &local.folders, &pulled.folders, &dirty_folders, |f| &f.id),
            tags: push_set(&merged.tags, &local.tags, &pulled.tags, &dirty_tags, |t| &t.id),
            bookmarks: push_set(
                &merged.bookmarks,
                &local.bookmarks,
                &pulled.bookmarks,
                &dirty_bookmarks,
                |b| &b.id,
            ),
        };
        pushed = changes.folders.len() + changes.tags.len() + changes.bookmarks.len();
        if pushed > 0 {
            store.push(&changes).await?;
        }
    }

    let new_cursor = store.current_seq().await?;
    store.commit().await?;

    Ok(SyncOutcome { merged, changed_local, new_cursor, pushed })
}

// ─── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::{SyncBookmark, SyncFolder, SyncTag};
    use std::sync::{Arc, Mutex};

    // ── In-memory Postgres stand-in: rows + a global seq counter ─────────────

    #[derive(Default)]
    struct FakeInner {
        folders: HashMap<String, (SyncFolder, i64)>,
        tags: HashMap<String, (SyncTag, i64)>,
        bookmarks: HashMap<String, (SyncBookmark, i64)>,
        seq: i64,
        locked: bool,
    }

    #[derive(Clone, Default)]
    struct FakeStore {
        inner: Arc<Mutex<FakeInner>>,
        push_count: Arc<Mutex<usize>>,
    }

    impl FakeStore {
        fn total_pushed_rows(&self) -> usize {
            *self.push_count.lock().unwrap()
        }
        fn remote_bookmark(&self, id: &str) -> Option<SyncBookmark> {
            self.inner.lock().unwrap().bookmarks.get(id).map(|(b, _)| b.clone())
        }
        fn remote_len(&self) -> usize {
            let g = self.inner.lock().unwrap();
            g.folders.len() + g.tags.len() + g.bookmarks.len()
        }
    }

    impl SyncStore for FakeStore {
        async fn begin(&mut self) -> Result<(), AppError> {
            let mut g = self.inner.lock().unwrap();
            assert!(!g.locked, "advisory lock is not re-entrant in the fake");
            g.locked = true;
            Ok(())
        }
        async fn pull_since(&mut self, cursor: i64) -> Result<SyncSnapshot, AppError> {
            let g = self.inner.lock().unwrap();
            assert!(g.locked, "pull outside begin/commit");
            Ok(SyncSnapshot {
                folders: g
                    .folders
                    .values()
                    .filter(|(_, s)| *s > cursor)
                    .map(|(f, _)| f.clone())
                    .collect(),
                tags: g.tags.values().filter(|(_, s)| *s > cursor).map(|(t, _)| t.clone()).collect(),
                bookmarks: g
                    .bookmarks
                    .values()
                    .filter(|(_, s)| *s > cursor)
                    .map(|(b, _)| b.clone())
                    .collect(),
            })
        }
        async fn push(&mut self, changes: &SyncSnapshot) -> Result<(), AppError> {
            let mut g = self.inner.lock().unwrap();
            assert!(g.locked, "push outside begin/commit");
            let mut n = 0;
            for f in &changes.folders {
                g.seq += 1;
                let s = g.seq;
                g.folders.insert(f.id.clone(), (f.clone(), s));
                n += 1;
            }
            for t in &changes.tags {
                g.seq += 1;
                let s = g.seq;
                g.tags.insert(t.id.clone(), (t.clone(), s));
                n += 1;
            }
            for b in &changes.bookmarks {
                g.seq += 1;
                let s = g.seq;
                g.bookmarks.insert(b.id.clone(), (b.clone(), s));
                n += 1;
            }
            *self.push_count.lock().unwrap() += n;
            Ok(())
        }
        async fn current_seq(&mut self) -> Result<i64, AppError> {
            Ok(self.inner.lock().unwrap().seq)
        }
        async fn commit(&mut self) -> Result<(), AppError> {
            self.inner.lock().unwrap().locked = false;
            Ok(())
        }
    }

    // ── One simulated device: local snapshot + dirty log + cursor ────────────

    struct Device {
        local: SyncSnapshot,
        dirty: Vec<(String, String)>,
        cursor: i64,
    }

    impl Device {
        fn new() -> Self {
            Device { local: SyncSnapshot::default(), dirty: Vec::new(), cursor: 0 }
        }
        fn edit_bookmark(&mut self, b: SyncBookmark) {
            self.dirty.push(("bookmark".into(), b.id.clone()));
            self.local.bookmarks.retain(|x| x.id != b.id);
            self.local.bookmarks.push(b);
        }
        fn edit_folder(&mut self, f: SyncFolder) {
            self.dirty.push(("folder".into(), f.id.clone()));
            self.local.folders.retain(|x| x.id != f.id);
            self.local.folders.push(f);
        }
        /// Mirrors `SyncEngine::run_sync`: bootstrap-mark-all when the cursor
        /// is 0, sync, apply, advance cursor, clear captured dirty entries.
        async fn sync(&mut self, store: &FakeStore) -> SyncOutcome {
            self.sync_with(store, SyncMode::Full).await
        }
        async fn sync_with(&mut self, store: &FakeStore, mode: SyncMode) -> SyncOutcome {
            if self.cursor == 0 {
                for f in &self.local.folders {
                    self.dirty.push(("folder".into(), f.id.clone()));
                }
                for t in &self.local.tags {
                    self.dirty.push(("tag".into(), t.id.clone()));
                }
                for b in &self.local.bookmarks {
                    self.dirty.push(("bookmark".into(), b.id.clone()));
                }
            }
            let captured = self.dirty.clone();
            let mut s = store.clone();
            let outcome =
                sync_once(&mut s, self.local.clone(), &captured, self.cursor, mode).await.unwrap();
            if outcome.changed_local {
                self.local = outcome.merged.clone();
            }
            self.cursor = outcome.new_cursor;
            self.dirty.retain(|e| !captured.contains(e));
            outcome
        }
        fn title_of(&self, id: &str) -> Option<&str> {
            self.local
                .bookmarks
                .iter()
                .find(|b| b.id == id && b.deleted_at.is_none())
                .map(|b| b.title.as_str())
        }
    }

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

    fn fld(id: &str, name: &str, updated_at: i64) -> SyncFolder {
        SyncFolder {
            id: id.into(),
            name: name.into(),
            parent_id: None,
            created_at: 1,
            updated_at,
            deleted_at: None,
        }
    }

    #[tokio::test]
    async fn two_devices_disjoint_edits_both_survive() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let mut b = Device::new();

        a.edit_bookmark(bm("X", "x", 10, None));
        a.sync(&store).await;

        b.edit_bookmark(bm("Y", "y", 10, None));
        b.sync(&store).await; // pulls X, pushes Y

        a.sync(&store).await; // pulls Y

        assert_eq!(a.title_of("X"), Some("x"));
        assert_eq!(a.title_of("Y"), Some("y"));
        assert_eq!(b.title_of("X"), Some("x"));
        assert_eq!(b.title_of("Y"), Some("y"));
    }

    #[tokio::test]
    async fn newer_edit_wins_on_both_devices() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let mut b = Device::new();

        a.edit_bookmark(bm("X", "old", 10, None));
        a.sync(&store).await;
        b.sync(&store).await;

        b.edit_bookmark(bm("X", "new", 20, None));
        b.sync(&store).await;
        a.sync(&store).await;

        assert_eq!(a.title_of("X"), Some("new"));
        assert_eq!(store.remote_bookmark("X").unwrap().title, "new");
    }

    #[tokio::test]
    async fn delete_tombstone_propagates() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let mut b = Device::new();

        a.edit_bookmark(bm("X", "x", 10, None));
        a.sync(&store).await;
        b.sync(&store).await;
        assert_eq!(b.title_of("X"), Some("x"));

        b.edit_bookmark(bm("X", "x", 20, Some(20)));
        b.sync(&store).await;
        a.sync(&store).await;

        assert_eq!(a.title_of("X"), None, "tombstone must land on A");
        assert!(store.remote_bookmark("X").unwrap().deleted_at.is_some());
    }

    /// The whole point of incremental sync: a device that changed one row out
    /// of many pushes exactly that row, and an in-sync partner pulls exactly
    /// that row (plus pushes nothing).
    #[tokio::test]
    async fn steady_state_transfers_only_what_changed() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let mut b = Device::new();

        for i in 0..50 {
            a.edit_bookmark(bm(&format!("B{i:02}"), "t", 10, None));
        }
        a.sync(&store).await;
        b.sync(&store).await;

        let baseline = store.total_pushed_rows();
        // B in steady state: nothing dirty, nothing new remotely.
        let out = b.sync(&store).await;
        assert_eq!(out.pushed, 0, "clean device must push nothing");
        assert!(!out.changed_local);

        a.edit_bookmark(bm("B07", "edited", 20, None));
        let out = a.sync(&store).await;
        assert_eq!(out.pushed, 1, "exactly the edited row");
        assert_eq!(store.total_pushed_rows(), baseline + 1);

        let out = b.sync(&store).await;
        assert!(out.changed_local);
        assert_eq!(out.pushed, 0, "receiving a change must not re-push it");
        assert_eq!(b.title_of("B07"), Some("edited"));
    }

    /// Existing DB adopting sync (cursor 0, empty remote): everything pushes.
    #[tokio::test]
    async fn bootstrap_pushes_full_dataset() {
        let store = FakeStore::default();
        let mut a = Device::new();
        a.local.folders.push(fld("F", "News", 5));
        a.local.bookmarks.push(bm("X", "x", 10, None));
        // NOT via edit_bookmark — simulates rows that predate dirty tracking.

        let out = a.sync(&store).await;
        assert_eq!(out.pushed, 2);
        assert_eq!(store.remote_len(), 2);
    }

    /// Fresh install joining an existing remote: pulls everything, pushes nothing.
    #[tokio::test]
    async fn fresh_device_pulls_full_dataset() {
        let store = FakeStore::default();
        let mut a = Device::new();
        a.edit_bookmark(bm("X", "x", 10, None));
        a.edit_folder(fld("F", "News", 5));
        a.sync(&store).await;

        let mut b = Device::new();
        let out = b.sync(&store).await;
        assert!(out.changed_local);
        assert_eq!(out.pushed, 0);
        assert_eq!(b.title_of("X"), Some("x"));
        assert_eq!(b.local.folders.len(), 1);
    }

    /// A wiped local DB with a stale cursor must re-pull the world, not
    /// blank the remote.
    #[tokio::test]
    async fn wiped_local_with_stale_cursor_repulls_everything() {
        let store = FakeStore::default();
        let mut a = Device::new();
        a.edit_bookmark(bm("X", "x", 10, None));
        a.sync(&store).await;

        a.local = SyncSnapshot::default(); // wipe, cursor stays stale
        a.dirty.clear();
        let out = a.sync(&store).await;
        assert_eq!(a.title_of("X"), Some("x"), "remote dataset must win over absence");
        assert_eq!(out.pushed, 0);
    }

    /// Concurrent same-second conflict: both devices converge on the same
    /// winner (rank order), and the remote holds it too.
    #[tokio::test]
    async fn conflicting_edits_converge() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let mut b = Device::new();

        a.edit_bookmark(bm("X", "base", 10, None));
        a.sync(&store).await;
        b.sync(&store).await;

        a.edit_bookmark(bm("X", "from-a", 20, None));
        b.edit_bookmark(bm("X", "from-b", 20, None));
        a.sync(&store).await;
        b.sync(&store).await; // B pulls A's version, merge ranks, pushes if B won
        a.sync(&store).await; // A picks up the outcome

        assert_eq!(a.local, b.local, "devices must converge");
        assert_eq!(
            store.remote_bookmark("X").unwrap(),
            a.local.bookmarks.iter().find(|x| x.id == "X").unwrap().clone(),
            "remote must hold the winner"
        );
    }

    /// The stale-loser correction: when a pulled row loses the merge, the
    /// winner is pushed back so the remote doesn't keep serving the loser.
    #[tokio::test]
    async fn stale_remote_version_gets_corrected() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let mut b = Device::new();

        a.edit_bookmark(bm("X", "newer", 30, None));
        a.sync(&store).await;

        // B holds a NEWER local edit than what it pulls, but B's row is not
        // dirty in the pathological case of a restored settings file. Simulate:
        // B has the row locally with a higher clock, cursor 0 marks it dirty
        // anyway via bootstrap — so instead test via an explicit older remote:
        b.edit_bookmark(bm("X", "newest", 40, None));
        let out = b.sync(&store).await;
        assert!(out.pushed >= 1, "B's winner must reach the remote");
        assert_eq!(store.remote_bookmark("X").unwrap().title, "newest");

        a.sync(&store).await;
        assert_eq!(a.title_of("X"), Some("newest"));
    }

    /// Normalize repairs (e.g. two machines minting the same folder name)
    /// must both apply locally and push to the remote.
    #[tokio::test]
    async fn folder_name_collision_collapses_and_propagates() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let mut b = Device::new();

        let mut ba = bm("A", "a", 10, None);
        ba.folder_id = Some("F-a".into());
        a.edit_folder(fld("F-a", "News", 10));
        a.edit_bookmark(ba);
        a.sync(&store).await;

        let mut bb = bm("Z", "z", 10, None);
        bb.folder_id = Some("F-b".into());
        b.edit_folder(fld("F-b", "News", 20)); // newer wins the name
        b.edit_bookmark(bb);
        b.sync(&store).await;
        a.sync(&store).await;

        for d in [&a, &b] {
            let live: Vec<_> =
                d.local.folders.iter().filter(|f| f.deleted_at.is_none()).collect();
            assert_eq!(live.len(), 1, "one live News folder");
            assert_eq!(live[0].id, "F-b");
            for x in &d.local.bookmarks {
                assert_eq!(x.folder_id.as_deref(), Some("F-b"));
            }
        }
        assert_eq!(store.remote_bookmark("A").unwrap().folder_id.as_deref(), Some("F-b"));
    }

    /// Mobile mode: the merge lands locally, nothing is ever pushed.
    #[tokio::test]
    async fn pull_only_never_pushes() {
        let store = FakeStore::default();
        let mut desktop = Device::new();
        desktop.edit_bookmark(bm("X", "x", 10, None));
        desktop.sync(&store).await;

        let mut phone = Device::new();
        phone.edit_bookmark(bm("LOCAL", "phone-only", 10, None));
        let out = phone.sync_with(&store, SyncMode::PullOnly).await;
        assert_eq!(out.pushed, 0);
        assert_eq!(phone.title_of("X"), Some("x"));
        assert!(store.remote_bookmark("LOCAL").is_none(), "phone must not upload");
    }

    /// Cursor semantics: after a device pushes, its cursor covers its own
    /// writes — the next sync must not re-pull (or re-push) them.
    #[tokio::test]
    async fn own_pushes_are_not_repulled() {
        let store = FakeStore::default();
        let mut a = Device::new();
        a.edit_bookmark(bm("X", "x", 10, None));
        a.sync(&store).await;

        let out = a.sync(&store).await;
        assert_eq!(out.pushed, 0);
        assert!(!out.changed_local);
    }

    #[tokio::test]
    async fn empty_both_sides_is_a_noop() {
        let store = FakeStore::default();
        let mut a = Device::new();
        let out = a.sync(&store).await;
        assert_eq!(out.pushed, 0);
        assert!(!out.changed_local);
        assert_eq!(out.new_cursor, 0);
    }
}
