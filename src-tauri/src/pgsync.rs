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

/// How a sync cycle may touch the remote. With `PullOnly` the merge still
/// runs (remote rows land locally), but nothing is ever uploaded. Every
/// platform now syncs `Full` — mobile was pull-only until v0.16; the variant
/// stays for the test harness and a possible future per-device read-only
/// setting.
#[derive(Clone, Copy, PartialEq)]
pub enum SyncMode {
    Full,
    /// Only constructed by tests today (see the doc comment above).
    #[allow(dead_code)]
    PullOnly,
}

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
    /// One-shot upgrade marker for mobile builds. Pull-only builds (< v0.16)
    /// cleared dirty marks without ever pushing, so rows created or edited on
    /// this device have no pending-push record. The first sync of a
    /// write-capable build re-marks everything dirty once; `push_set` then
    /// uploads only rows that actually differ from the remote.
    #[serde(default)]
    pub mobile_write_migrated: bool,
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

// ─── Password storage ───────────────────────────────────────────────────────────
//
// Desktop: OS keyring (Keychain / Credential Manager / Secret Service), keyed by
// user@host so switching Neon projects keeps credentials apart. If the keyring
// is unavailable (e.g. a Linux session without a Secret Service), fall back to
// the settings.json field — same trust level as the Drive refresh token that
// already lives there. Mobile: always settings.json; the file sits in the
// app-private sandbox and there is no cross-platform keyring backend.

#[cfg(desktop)]
const KEYRING_SERVICE: &str = "ferrico-neon";

#[cfg(desktop)]
fn keyring_entry(user: &str, host: &str) -> Result<keyring::Entry, keyring::Error> {
    keyring::Entry::new(KEYRING_SERVICE, &format!("{user}@{host}"))
}

/// Store the password as securely as the platform allows. Returns the value to
/// persist in `NeonConfig.password` (`None` when the keyring took it).
pub fn store_password(user: &str, host: &str, password: &str) -> Option<String> {
    #[cfg(desktop)]
    {
        match keyring_entry(user, host).and_then(|e| e.set_password(password)) {
            Ok(()) => return None,
            Err(e) => {
                eprintln!("keyring unavailable ({e}); storing Neon password in settings.json");
            }
        }
    }
    Some(password.to_string())
}

/// Resolve the password for a connection attempt.
pub fn load_password(cfg: &NeonConfig) -> Option<String> {
    if let Some(p) = cfg.password.clone().filter(|p| !p.is_empty()) {
        return Some(p);
    }
    #[cfg(desktop)]
    {
        if let (Some(user), Some(host)) = (cfg.effective_user(), cfg.effective_host()) {
            if let Ok(p) = keyring_entry(&user, &host).and_then(|e| e.get_password()) {
                return Some(p);
            }
        }
    }
    None
}

/// Remove a stored password everywhere.
pub fn delete_password(cfg: &NeonConfig) {
    #[cfg(desktop)]
    {
        if let (Some(user), Some(host)) = (cfg.effective_user(), cfg.effective_host()) {
            if let Ok(e) = keyring_entry(&user, &host) {
                e.delete_credential().ok();
            }
        }
    }
    let _ = cfg; // silence unused on mobile — the settings field is cleared by the caller
}

// ─── Real Postgres store ────────────────────────────────────────────────────────

/// Remote schema version, recorded in `schema_meta`. Bump when a migration is
/// added; `init_remote_schema` is idempotent DDL, so re-running it is the
/// migration mechanism (mirrors `db::init_schema`).
const REMOTE_SCHEMA_VERSION: &str = "1";

/// Advisory-lock key serializing Ferrico sync cycles per database
/// (arbitrary constant; FNV-1a-64 of "ferrico-sync" truncated to i64 range).
const SYNC_LOCK_KEY: i64 = 0x6665_7272_6963_6f31;

/// Split a pasted `postgres://user:pass@host[:port][/db][?params]` connection
/// string. Returns `None` when the input is not URL-shaped (a bare hostname).
/// No percent-decoding — Neon-generated passwords are URL-safe; a hand-crafted
/// password containing `%xx` should be entered via the password field instead.
fn split_conn_string(raw: &str) -> Option<(String, Option<String>, Option<String>, Option<String>)> {
    let rest = raw
        .strip_prefix("postgresql://")
        .or_else(|| raw.strip_prefix("postgres://"))?;
    // rsplit: passwords may contain '@' but hostnames never do.
    let (creds, host_part) = match rest.rsplit_once('@') {
        Some((c, h)) => (Some(c), h),
        None => (None, rest),
    };
    let (host_port, db_part) = match host_part.split_once('/') {
        Some((h, d)) => (h, Some(d)),
        None => (host_part, None),
    };
    let host = host_port.split(':').next().unwrap_or_default().to_string();
    let dbname = db_part
        .and_then(|d| d.split('?').next())
        .filter(|s| !s.is_empty())
        .map(String::from);
    let (user, password) = match creds {
        Some(c) => match c.split_once(':') {
            Some((u, p)) => (Some(u.to_string()), Some(p.to_string())),
            None => (Some(c.to_string()), None),
        },
        None => (None, None),
    };
    Some((host, dbname, user, password))
}

/// tokio-postgres `Display` hides the cause ("error connecting to server"
/// says nothing about DNS vs refused vs TLS) — walk the source chain so the
/// settings UI shows something diagnosable.
fn pgerr(e: tokio_postgres::Error) -> AppError {
    let mut msg = format!("Postgres: {e}");
    let mut source = std::error::Error::source(&e);
    while let Some(s) = source {
        msg.push_str(&format!(": {s}"));
        source = s.source();
    }
    serr(msg)
}

fn make_tls() -> Result<tokio_postgres_rustls::MakeRustlsConnect, AppError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // Explicit ring provider: both reqwest and this module link rustls, and a
    // process-default lookup panics if more than one provider is compiled in.
    let config = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(serr)?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(tokio_postgres_rustls::MakeRustlsConnect::new(config))
}

/// A live connection implementing [`SyncStore`] over the Postgres wire
/// protocol. One instance = one sync cycle; the transaction state is plain
/// `BEGIN`/`COMMIT` SQL because the client is exclusively ours.
pub struct PgStore {
    client: tokio_postgres::Client,
    // Drives the connection I/O; aborted when the store drops.
    io: tokio::task::JoinHandle<()>,
}

impl Drop for PgStore {
    fn drop(&mut self) {
        // An uncommitted transaction rolls back when the connection closes.
        self.io.abort();
    }
}

impl PgStore {
    /// Connect (TLS required — Neon rejects plaintext) and ensure the remote
    /// schema exists and is current.
    pub async fn connect(
        host: &str,
        dbname: &str,
        user: &str,
        password: &str,
    ) -> Result<Self, AppError> {
        let mut cfg = tokio_postgres::Config::new();
        cfg.host(host)
            .port(5432)
            .user(user)
            .password(password)
            .dbname(dbname)
            .ssl_mode(tokio_postgres::config::SslMode::Require)
            .connect_timeout(std::time::Duration::from_secs(15));
        let (client, connection) = cfg.connect(make_tls()?).await.map_err(pgerr)?;
        let io = tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("neon connection error: {e}");
            }
        });
        let store = PgStore { client, io };
        store.init_remote_schema().await?;
        Ok(store)
    }

    /// Idempotent DDL, app-managed (the user never runs SQL). `seq` is set by a
    /// BEFORE INSERT OR UPDATE trigger from ONE global sequence, giving a
    /// database-wide monotonic change counter for cursor pulls. No foreign keys
    /// on purpose: tombstones travel and `merge::normalize` repairs cross-row
    /// invariants at merge time — remote FKs would only fight upsert order.
    /// `tag_ids` is JSONB on the bookmark row (matching the sync wire format)
    /// instead of a junction table.
    async fn init_remote_schema(&self) -> Result<(), AppError> {
        self.client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS schema_meta (
                   key   TEXT PRIMARY KEY,
                   value TEXT NOT NULL
                 );

                 CREATE SEQUENCE IF NOT EXISTS ferrico_seq;

                 CREATE TABLE IF NOT EXISTS folders (
                   id         TEXT PRIMARY KEY,
                   name       TEXT NOT NULL,
                   parent_id  TEXT,
                   created_at BIGINT NOT NULL,
                   updated_at BIGINT NOT NULL DEFAULT 0,
                   deleted_at BIGINT,
                   seq        BIGINT NOT NULL DEFAULT 0
                 );

                 CREATE TABLE IF NOT EXISTS tags (
                   id         TEXT PRIMARY KEY,
                   name       TEXT NOT NULL,
                   color      TEXT NOT NULL,
                   created_at BIGINT NOT NULL,
                   updated_at BIGINT NOT NULL DEFAULT 0,
                   deleted_at BIGINT,
                   seq        BIGINT NOT NULL DEFAULT 0
                 );

                 CREATE TABLE IF NOT EXISTS bookmarks (
                   id          TEXT PRIMARY KEY,
                   url         TEXT NOT NULL,
                   title       TEXT NOT NULL,
                   description TEXT,
                   favicon_url TEXT,
                   feed_url    TEXT,
                   cover_url   TEXT,
                   folder_id   TEXT,
                   tag_ids     JSONB NOT NULL DEFAULT '[]',
                   created_at  BIGINT NOT NULL,
                   updated_at  BIGINT NOT NULL,
                   deleted_at  BIGINT,
                   purged_at   BIGINT,
                   seq         BIGINT NOT NULL DEFAULT 0
                 );

                 CREATE OR REPLACE FUNCTION ferrico_bump_seq() RETURNS trigger AS $$
                 BEGIN
                   NEW.seq := nextval('ferrico_seq');
                   RETURN NEW;
                 END
                 $$ LANGUAGE plpgsql;

                 DROP TRIGGER IF EXISTS folders_seq ON folders;
                 CREATE TRIGGER folders_seq BEFORE INSERT OR UPDATE ON folders
                   FOR EACH ROW EXECUTE FUNCTION ferrico_bump_seq();
                 DROP TRIGGER IF EXISTS tags_seq ON tags;
                 CREATE TRIGGER tags_seq BEFORE INSERT OR UPDATE ON tags
                   FOR EACH ROW EXECUTE FUNCTION ferrico_bump_seq();
                 DROP TRIGGER IF EXISTS bookmarks_seq ON bookmarks;
                 CREATE TRIGGER bookmarks_seq BEFORE INSERT OR UPDATE ON bookmarks
                   FOR EACH ROW EXECUTE FUNCTION ferrico_bump_seq();

                 CREATE INDEX IF NOT EXISTS folders_seq_idx   ON folders (seq);
                 CREATE INDEX IF NOT EXISTS tags_seq_idx      ON tags (seq);
                 CREATE INDEX IF NOT EXISTS bookmarks_seq_idx ON bookmarks (seq);",
            )
            .await
            .map_err(pgerr)?;
        self.client
            .execute(
                "INSERT INTO schema_meta (key, value) VALUES ('schema_version', $1)
                 ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
                &[&REMOTE_SCHEMA_VERSION],
            )
            .await
            .map_err(pgerr)?;
        Ok(())
    }

    fn tag_ids_from_row(row: &tokio_postgres::Row, idx: usize) -> Vec<String> {
        row.get::<_, serde_json::Value>(idx)
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default()
    }
}

impl SyncStore for PgStore {
    async fn begin(&mut self) -> Result<(), AppError> {
        // The advisory lock serializes concurrent devices for the whole cycle;
        // it releases automatically at COMMIT/ROLLBACK (xact-scoped).
        self.client.batch_execute("BEGIN").await.map_err(pgerr)?;
        self.client
            .execute("SELECT pg_advisory_xact_lock($1)", &[&SYNC_LOCK_KEY])
            .await
            .map_err(pgerr)?;
        Ok(())
    }

    async fn pull_since(&mut self, cursor: i64) -> Result<SyncSnapshot, AppError> {
        let folders = self
            .client
            .query(
                "SELECT id, name, parent_id, created_at, updated_at, deleted_at
                 FROM folders WHERE seq > $1",
                &[&cursor],
            )
            .await
            .map_err(pgerr)?
            .iter()
            .map(|r| merge::SyncFolder {
                id: r.get(0),
                name: r.get(1),
                parent_id: r.get(2),
                created_at: r.get(3),
                updated_at: r.get(4),
                deleted_at: r.get(5),
            })
            .collect();
        let tags = self
            .client
            .query(
                "SELECT id, name, color, created_at, updated_at, deleted_at
                 FROM tags WHERE seq > $1",
                &[&cursor],
            )
            .await
            .map_err(pgerr)?
            .iter()
            .map(|r| merge::SyncTag {
                id: r.get(0),
                name: r.get(1),
                color: r.get(2),
                created_at: r.get(3),
                updated_at: r.get(4),
                deleted_at: r.get(5),
            })
            .collect();
        let bookmarks = self
            .client
            .query(
                "SELECT id, url, title, description, favicon_url, feed_url, cover_url,
                        folder_id, tag_ids, created_at, updated_at, deleted_at, purged_at
                 FROM bookmarks WHERE seq > $1",
                &[&cursor],
            )
            .await
            .map_err(pgerr)?
            .iter()
            .map(|r| merge::SyncBookmark {
                id: r.get(0),
                url: r.get(1),
                title: r.get(2),
                description: r.get(3),
                favicon_url: r.get(4),
                feed_url: r.get(5),
                cover_url: r.get(6),
                folder_id: r.get(7),
                tag_ids: Self::tag_ids_from_row(r, 8),
                created_at: r.get(9),
                updated_at: r.get(10),
                deleted_at: r.get(11),
                purged_at: r.get(12),
            })
            .collect();
        Ok(SyncSnapshot { folders, tags, bookmarks })
    }

    async fn push(&mut self, changes: &SyncSnapshot) -> Result<(), AppError> {
        let fstmt = self
            .client
            .prepare(
                "INSERT INTO folders (id, name, parent_id, created_at, updated_at, deleted_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO UPDATE SET
                   name = EXCLUDED.name, parent_id = EXCLUDED.parent_id,
                   created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at,
                   deleted_at = EXCLUDED.deleted_at",
            )
            .await
            .map_err(pgerr)?;
        for f in &changes.folders {
            self.client
                .execute(
                    &fstmt,
                    &[&f.id, &f.name, &f.parent_id, &f.created_at, &f.updated_at, &f.deleted_at],
                )
                .await
                .map_err(pgerr)?;
        }

        let tstmt = self
            .client
            .prepare(
                "INSERT INTO tags (id, name, color, created_at, updated_at, deleted_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 ON CONFLICT (id) DO UPDATE SET
                   name = EXCLUDED.name, color = EXCLUDED.color,
                   created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at,
                   deleted_at = EXCLUDED.deleted_at",
            )
            .await
            .map_err(pgerr)?;
        for t in &changes.tags {
            self.client
                .execute(
                    &tstmt,
                    &[&t.id, &t.name, &t.color, &t.created_at, &t.updated_at, &t.deleted_at],
                )
                .await
                .map_err(pgerr)?;
        }

        let bstmt = self
            .client
            .prepare(
                "INSERT INTO bookmarks (id, url, title, description, favicon_url, feed_url,
                                        cover_url, folder_id, tag_ids, created_at, updated_at,
                                        deleted_at, purged_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)
                 ON CONFLICT (id) DO UPDATE SET
                   url = EXCLUDED.url, title = EXCLUDED.title,
                   description = EXCLUDED.description, favicon_url = EXCLUDED.favicon_url,
                   feed_url = EXCLUDED.feed_url, cover_url = EXCLUDED.cover_url,
                   folder_id = EXCLUDED.folder_id, tag_ids = EXCLUDED.tag_ids,
                   created_at = EXCLUDED.created_at, updated_at = EXCLUDED.updated_at,
                   deleted_at = EXCLUDED.deleted_at, purged_at = EXCLUDED.purged_at",
            )
            .await
            .map_err(pgerr)?;
        for b in &changes.bookmarks {
            let tag_ids = serde_json::Value::from(b.tag_ids.clone());
            self.client
                .execute(
                    &bstmt,
                    &[
                        &b.id,
                        &b.url,
                        &b.title,
                        &b.description,
                        &b.favicon_url,
                        &b.feed_url,
                        &b.cover_url,
                        &b.folder_id,
                        &tag_ids,
                        &b.created_at,
                        &b.updated_at,
                        &b.deleted_at,
                        &b.purged_at,
                    ],
                )
                .await
                .map_err(pgerr)?;
        }
        Ok(())
    }

    async fn current_seq(&mut self) -> Result<i64, AppError> {
        // Inside the advisory lock no other Ferrico device can call nextval,
        // so the sequence's last handed-out value is a safe cursor. Before any
        // nextval, `is_called` is false and `last_value` is a phantom 1.
        let row = self
            .client
            .query_one("SELECT last_value, is_called FROM ferrico_seq", &[])
            .await
            .map_err(pgerr)?;
        let last: i64 = row.get(0);
        let called: bool = row.get(1);
        Ok(if called { last } else { 0 })
    }

    async fn commit(&mut self) -> Result<(), AppError> {
        self.client.batch_execute("COMMIT").await.map_err(pgerr)
    }
}

// ─── Engine ─────────────────────────────────────────────────────────────────────

use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

/// Shared, cheaply-cloneable handle wiring the DB, persisted config and the
/// Tauri app handle — the Neon counterpart of `gdrive::BackupEngine`. Held in
/// managed state and by the lifecycle tasks (open-pull, change-driven push
/// loop, close-push).
#[derive(Clone)]
pub struct SyncEngine {
    db: Arc<Mutex<Connection>>,
    config: Arc<Mutex<NeonConfig>>,
    data_dir: Arc<PathBuf>,
    app: AppHandle,
    /// Serializes sync cycles within this process (the Postgres advisory lock
    /// serializes across devices).
    running: Arc<tokio::sync::Mutex<()>>,
}

impl SyncEngine {
    pub fn new(db: Arc<Mutex<Connection>>, data_dir: PathBuf, app: AppHandle) -> Self {
        let config = load_config(&data_dir);
        SyncEngine {
            db,
            config: Arc::new(Mutex::new(config)),
            data_dir: Arc::new(data_dir),
            app,
            running: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    // ── config access ──────────────────────────────────────────────────────────

    fn cfg(&self) -> Result<NeonConfig, AppError> {
        Ok(self
            .config
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?
            .clone())
    }

    fn update_cfg(&self, f: impl FnOnce(&mut NeonConfig)) -> Result<NeonConfig, AppError> {
        let mut guard = self
            .config
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?;
        f(&mut guard);
        let snapshot = guard.clone();
        save_config(self.data_dir.as_path(), &snapshot);
        Ok(snapshot)
    }

    pub fn status(&self) -> Result<NeonStatus, AppError> {
        let c = self.cfg()?;
        Ok(NeonStatus {
            configured: c.is_configured(),
            enabled: c.enabled,
            host: c.effective_host(),
            dbname: c.effective_dbname(),
            user: c.effective_user(),
            interval_min: c.interval_min,
            last_seq: c.last_seq,
            last_sync: c.last_sync,
        })
    }

    pub fn is_active(&self) -> bool {
        self.cfg().map(|c| c.enabled && c.is_configured()).unwrap_or(false)
    }

    /// Current config, for the v2 pairing exporter (`crate::pairing`).
    pub fn config_snapshot(&self) -> Result<NeonConfig, AppError> {
        self.cfg()
    }

    // ── settings commands ───────────────────────────────────────────────────────

    /// Update connection settings. An empty `password` keeps the stored one; a
    /// changed host or user resets the cursor (a different remote's `seq`
    /// numbering means nothing to us) and re-homes the stored password.
    ///
    /// The host field tolerates the natural mistake of pasting Neon's full
    /// connection string: `postgres://user:pass@host/db` is split into its
    /// parts (filling only fields the user left blank), and a stray `:5432`
    /// is stripped.
    pub fn set_config(
        &self,
        host: String,
        dbname: String,
        user: String,
        password: String,
    ) -> Result<NeonStatus, AppError> {
        let mut host = host.trim().to_string();
        let mut dbname = dbname.trim().to_string();
        let mut user = user.trim().to_string();
        let mut password = password.trim().to_string();
        if let Some((h, db, u, p)) = split_conn_string(&host) {
            host = h;
            if dbname.is_empty() {
                dbname = db.unwrap_or_default();
            }
            if user.is_empty() {
                user = u.unwrap_or_default();
            }
            if password.is_empty() {
                password = p.unwrap_or_default();
            }
        }
        if let Some((h, _)) = host.split_once(':') {
            host = h.to_string();
        }
        if host.is_empty() || user.is_empty() {
            return Err(serr("host and user are required"));
        }

        let old = self.cfg()?;
        let target_changed = old.effective_host().as_deref() != Some(host.as_str())
            || old.effective_user().as_deref() != Some(user.as_str())
            || old.effective_dbname() != dbname;

        let stored_password = if password.is_empty() {
            if target_changed {
                // Old credentials belong to the old target; require a fresh one.
                return Err(serr("password is required when changing host or user"));
            }
            old.password.clone()
        } else {
            if target_changed {
                delete_password(&old);
            }
            store_password(&user, &host, &password)
        };

        self.update_cfg(|c| {
            c.host = Some(host);
            c.dbname = if dbname.is_empty() { None } else { Some(dbname) };
            c.user = Some(user);
            c.password = stored_password;
            if target_changed {
                c.last_seq = 0;
                c.last_sync = None;
            }
        })?;
        self.status()
    }

    /// Connect + schema init + trivial round trip, without syncing.
    pub async fn test_connection(&self) -> Result<(), AppError> {
        let c = self.cfg()?;
        let store = self.connect(&c).await?;
        drop(store);
        Ok(())
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<NeonStatus, AppError> {
        self.update_cfg(|c| c.enabled = enabled)?;
        self.status()
    }

    pub fn set_interval(&self, interval_min: u64) -> Result<NeonStatus, AppError> {
        self.update_cfg(|c| c.interval_min = interval_min)?;
        self.status()
    }

    /// Forget the connection: password everywhere, enabled off. Host/db/user
    /// stay for an easy reconnect.
    pub fn disconnect(&self) -> Result<NeonStatus, AppError> {
        let c = self.cfg()?;
        delete_password(&c);
        self.update_cfg(|c| {
            c.password = None;
            c.enabled = false;
        })?;
        self.status()
    }

    /// Adopt pairing data from a desktop (QR/paste). Stores the password with
    /// platform-appropriate secrecy and resets the cursor — this device has
    /// never seen this remote.
    pub fn apply_pairing(
        &self,
        host: String,
        dbname: Option<String>,
        user: String,
        password: String,
    ) -> Result<NeonStatus, AppError> {
        let stored = store_password(&user, &host, &password);
        self.update_cfg(|c| {
            c.host = Some(host);
            c.dbname = dbname;
            c.user = Some(user);
            c.password = stored;
            c.enabled = true;
            c.last_seq = 0;
            c.last_sync = None;
        })?;
        self.status()
    }

    // ── sync ────────────────────────────────────────────────────────────────────

    async fn connect(&self, c: &NeonConfig) -> Result<PgStore, AppError> {
        let host = c.effective_host().ok_or_else(|| serr("Neon host is not configured"))?;
        let user = c.effective_user().ok_or_else(|| serr("Neon user is not configured"))?;
        let password = load_password(c).ok_or_else(|| {
            serr("no stored Neon password — enter it in Settings → Sync")
        })?;
        PgStore::connect(&host, &c.effective_dbname(), &user, &password).await
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AppError> {
        self.db.lock().map_err(|e| AppError::Lock { message: e.to_string() })
    }

    /// One full cycle: capture dirty + local snapshot, exchange with the
    /// remote, apply the merge, then persist cursor and clear exactly the
    /// captured dirty entries (marks added mid-flight survive for the next
    /// cycle). Emits the same events as the old Drive engine so the frontend
    /// sync indicator keeps working unchanged.
    async fn run_sync(&self, op: &str) -> Result<(bool, usize), AppError> {
        let _guard = self.running.lock().await;
        let cfg = self.cfg()?;
        self.app.emit("backup-syncing", serde_json::json!({ "op": op })).ok();

        let result = async {
            if cfg!(mobile) && !cfg.mobile_write_migrated {
                // See `NeonConfig::mobile_write_migrated`. Marks land in the
                // persistent `sync_dirty` table, so setting the flag before the
                // cycle succeeds is safe — a failed sync keeps the marks.
                let conn = self.lock_conn()?;
                crate::db::db_mark_all_dirty(&conn)?;
                drop(conn);
                self.update_cfg(|c| c.mobile_write_migrated = true)?;
            }

            let (dirty, fence, local) = {
                let conn = self.lock_conn()?;
                // First contact with this remote: rows predating dirty tracking
                // exist only in the tables, so everything must count as dirty.
                if cfg.last_seq == 0 {
                    crate::db::db_mark_all_dirty(&conn)?;
                }
                (
                    crate::db::db_get_dirty(&conn)?,
                    crate::db::db_dirty_fence(&conn)?,
                    crate::db::db_export_sync_snapshot(&conn)?,
                )
            };

            let mut store = self.connect(&cfg).await?;
            let outcome = sync_once(&mut store, local, &dirty, cfg.last_seq, SyncMode::Full).await?;

            {
                let conn = self.lock_conn()?;
                if outcome.changed_local {
                    crate::db::db_apply_sync_snapshot(&conn, &outcome.merged)?;
                }
                // Fence-bounded: an edit that raced this cycle re-marked its
                // row above the fence, so its mark survives and the next cycle
                // pushes it.
                crate::db::db_clear_dirty(&conn, &dirty, fence)?;
            }
            self.update_cfg(|c| {
                c.last_seq = outcome.new_cursor;
                c.last_sync = Some(crate::db::now());
            })?;
            Ok::<(bool, usize), AppError>((outcome.changed_local, outcome.pushed))
        }
        .await;

        match &result {
            Ok((changed, pushed)) => {
                self.app
                    .emit(
                        "backup-synced",
                        serde_json::json!({ "op": op, "changed": changed, "pushed": pushed }),
                    )
                    .ok();
            }
            Err(e) => {
                self.app
                    .emit("backup-error", serde_json::json!({ "op": op, "message": e.to_string() }))
                    .ok();
            }
        }
        result
    }

    pub async fn sync_now(&self) -> Result<NeonStatus, AppError> {
        self.run_sync("sync").await?;
        self.status()
    }

    // ── lifecycle entry points (best-effort, errors only logged/emitted) ────────

    pub async fn pull_if_active(&self) {
        if self.is_active() {
            if let Err(e) = self.run_sync("pull").await {
                eprintln!("neon sync (open) failed: {e}");
            }
        }
    }

    pub async fn push_if_active(&self) {
        if self.is_active() {
            if let Err(e) = self.run_sync("push").await {
                eprintln!("neon sync (close) failed: {e}");
            }
        }
    }

    /// Change-driven near-realtime push + periodic pull, in one loop.
    ///
    /// Every `TICK` it checks `sync_dirty`; pending local changes trigger a
    /// full cycle within seconds of the user's edit (the debounce is the tick
    /// itself). Independently, `interval_min` forces a periodic cycle so
    /// *remote* changes land without a local edit; `0` disables that part.
    /// After a failure (offline, bad credentials) it backs off before
    /// retrying so a closed laptop lid doesn't hammer the network.
    ///
    /// Runs on both platforms. Mobile uses a slower tick — Android suspends
    /// the process when backgrounded anyway, and the frontend flushes pending
    /// changes on `visibilitychange`, so sub-5s latency buys nothing there.
    pub async fn run_change_loop(self) {
        let tick = std::time::Duration::from_secs(if cfg!(mobile) { 15 } else { 5 });
        const ERROR_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);
        let mut last_full = std::time::Instant::now();
        loop {
            tokio::time::sleep(tick).await;
            if !self.is_active() {
                continue;
            }
            let dirty = {
                match self.lock_conn() {
                    Ok(conn) => crate::db::db_get_dirty(&conn).map(|d| d.len()).unwrap_or(0),
                    Err(_) => 0,
                }
            };
            let interval = self.cfg().map(|c| c.interval_min).unwrap_or(0);
            let interval_due =
                interval > 0 && last_full.elapsed() >= std::time::Duration::from_secs(interval * 60);
            if dirty == 0 && !interval_due {
                continue;
            }
            let op = if dirty > 0 { "auto" } else { "interval" };
            match self.run_sync(op).await {
                Ok(_) => last_full = std::time::Instant::now(),
                Err(_) => {
                    // run_sync already emitted backup-error.
                    tokio::time::sleep(ERROR_BACKOFF).await;
                }
            }
        }
    }
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

    #[test]
    fn conn_string_paste_is_split_into_parts() {
        let (h, db, u, p) = split_conn_string(
            "postgresql://neondb_owner:npg_abc123@ep-cool-name-123456.eu-central-1.aws.neon.tech/neondb?sslmode=require",
        )
        .unwrap();
        assert_eq!(h, "ep-cool-name-123456.eu-central-1.aws.neon.tech");
        assert_eq!(db.as_deref(), Some("neondb"));
        assert_eq!(u.as_deref(), Some("neondb_owner"));
        assert_eq!(p.as_deref(), Some("npg_abc123"));

        // Port and missing db/creds variants.
        let (h, db, u, p) = split_conn_string("postgres://host.example:5432").unwrap();
        assert_eq!(h, "host.example");
        assert!(db.is_none() && u.is_none() && p.is_none());

        // Password containing '@' still splits on the LAST '@'.
        let (h, _, u, p) = split_conn_string("postgres://u:p@ss@host.example/db").unwrap();
        assert_eq!(h, "host.example");
        assert_eq!(u.as_deref(), Some("u"));
        assert_eq!(p.as_deref(), Some("p@ss"));

        // A bare hostname is not URL-shaped.
        assert!(split_conn_string("ep-cool-name.aws.neon.tech").is_none());
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
