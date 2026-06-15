//! Google Drive backup + multi-machine sync.
//!
//! Strategy: **full-snapshot last-write-wins**. The whole dataset is exported as
//! the lossless JSON produced by [`crate::io::export_json`] and stored as a single
//! file (`ferrico-backup.json`) inside a user-chosen Drive folder.
//!
//! The LWW clock is Drive's own `modifiedTime` (RFC-3339, server-side) — this
//! sidesteps clock skew between machines. `last_sync` records the `modifiedTime`
//! we last reconciled with:
//!   - **pull** (app open): if the remote file's `modifiedTime` is newer than
//!     `last_sync`, the local DB is wiped and replaced with the remote snapshot.
//!   - **push** (app close + periodic): the local DB is uploaded, overwriting the
//!     remote file; `last_sync` advances to the new `modifiedTime`.
//!
//! Concurrent offline edits on two machines lose the older writer's changes — the
//! accepted trade-off of snapshot LWW (vs. per-record merge).
//!
//! Auth: OAuth2 for native apps — PKCE + loopback redirect. Scope is `drive.file`
//! (non-sensitive: the app only ever touches files it created, so no Google
//! verification and no 7-day refresh-token expiry). The user supplies a Desktop
//! OAuth client id/secret from Google Cloud Console.

use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::AppError;

const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const DRIVE_FILES: &str = "https://www.googleapis.com/drive/v3/files";
const DRIVE_UPLOAD: &str = "https://www.googleapis.com/upload/drive/v3/files";
const SCOPE: &str = "https://www.googleapis.com/auth/drive.file openid email";
const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
const BACKUP_FILENAME: &str = "ferrico-backup.json";

// ─── Persisted config (settings.json → "backup") ───────────────────────────────

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct BackupConfig {
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub client_secret: Option<String>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub account_email: Option<String>,
    #[serde(default)]
    pub folder_id: Option<String>,
    #[serde(default)]
    pub folder_name: Option<String>,
    #[serde(default)]
    pub file_id: Option<String>,
    /// Drive `modifiedTime` we last reconciled with (RFC-3339 string).
    #[serde(default)]
    pub last_sync: Option<String>,
    /// Periodic autosave interval in minutes; `0` disables periodic push.
    #[serde(default)]
    pub interval_min: u64,
    #[serde(default)]
    pub enabled: bool,
    /// FNV digest of the snapshot we last published to Drive. Lets an idle
    /// client skip a redundant upload, which would otherwise bump the remote
    /// `modifiedTime` and make every other client needlessly re-pull (and the
    /// two could ping-pong forever).
    #[serde(default)]
    pub last_pushed_digest: Option<String>,
}

/// View model handed to the frontend (never exposes the OAuth secret/token).
#[derive(Serialize)]
pub struct BackupStatus {
    pub has_credentials: bool,
    pub connected: bool,
    pub account_email: Option<String>,
    pub folder_id: Option<String>,
    pub folder_name: Option<String>,
    pub last_sync: Option<String>,
    pub interval_min: u64,
    pub enabled: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DriveFolder {
    pub id: String,
    pub name: String,
}

// ─── Drive / OAuth wire types ───────────────────────────────────────────────────

#[derive(Deserialize)]
struct DriveFileMeta {
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "modifiedTime")]
    modified_time: Option<String>,
}

#[derive(Deserialize)]
struct DriveFileList {
    #[serde(default)]
    files: Vec<DriveFileMeta>,
}

#[derive(Deserialize)]
struct TokenResp {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
struct UserInfo {
    #[serde(default)]
    email: Option<String>,
}

// ─── Helpers ────────────────────────────────────────────────────────────────────

fn berr(msg: impl std::fmt::Display) -> AppError {
    AppError::Backup { message: msg.to_string() }
}

/// Turn a non-2xx response into a useful error. Google returns the real reason
/// in the JSON body (`{ "error": { "message": "…" } }` or OAuth's
/// `{ "error": "…", "error_description": "…" }`); `reqwest::error_for_status`
/// throws that away, so we read the body ourselves.
async fn check(resp: reqwest::Response) -> Result<reqwest::Response, AppError> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| {
            v["error"]["message"]
                .as_str()
                .or_else(|| v["error_description"].as_str())
                .or_else(|| v["error"].as_str())
                .map(str::to_string)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| body.chars().take(300).collect());
    Err(berr(format!("Google API {status}: {msg}")))
}

fn b64url(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// RFC-7636 PKCE pair: `(code_verifier, code_challenge)` using S256.
fn gen_pkce() -> (String, String) {
    let mut verifier_bytes = [0u8; 32];
    OsRng.fill_bytes(&mut verifier_bytes);
    let verifier = b64url(&verifier_bytes);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = b64url(&hasher.finalize());
    (verifier, challenge)
}

fn gen_state() -> String {
    let mut b = [0u8; 16];
    OsRng.fill_bytes(&mut b);
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex_val(bytes[i + 1]), hex_val(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 3;
                }
                _ => {
                    out.push(bytes[i]);
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn parse_query(qs: &str) -> HashMap<String, String> {
    qs.split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=')?;
            Some((k.to_string(), percent_decode(v)))
        })
        .collect()
}

// ─── Config persistence (merged into settings.json, preserving api_token) ───────

pub fn load_config(data_dir: &Path) -> BackupConfig {
    let path = data_dir.join("settings.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .and_then(|v| v.get("backup").cloned())
        .and_then(|b| serde_json::from_value::<BackupConfig>(b).ok())
        .unwrap_or_default()
}

pub fn save_config(data_dir: &Path, cfg: &BackupConfig) {
    let path = data_dir.join("settings.json");
    let mut root = std::fs::read_to_string(&path)
        .ok()
        .and_then(|c| serde_json::from_str::<serde_json::Value>(&c).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    if !root.is_object() {
        root = serde_json::json!({});
    }
    root["backup"] = serde_json::to_value(cfg).unwrap_or(serde_json::Value::Null);
    if let Ok(s) = serde_json::to_string_pretty(&root) {
        std::fs::write(&path, s).ok();
    }
}

// ─── OAuth ──────────────────────────────────────────────────────────────────────

/// Runs the full PKCE loopback flow. Returns `(refresh_token, account_email)`.
/// Blocks (async) until the browser redirects back or the 5-minute timeout hits.
async fn run_oauth(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
) -> Result<(Option<String>, Option<String>), AppError> {
    let (verifier, challenge) = gen_pkce();
    let state = gen_state();

    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(berr)?;
    let port = listener.local_addr().map_err(berr)?.port();
    let redirect = format!("http://127.0.0.1:{port}");

    let url = reqwest::Url::parse_with_params(
        AUTH_URL,
        &[
            ("client_id", client_id),
            ("redirect_uri", redirect.as_str()),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state.as_str()),
        ],
    )
    .map_err(berr)?;

    open::that(url.as_str()).map_err(|e| berr(format!("could not open browser: {e}")))?;

    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(300), listener.accept())
        .await
        .map_err(|_| berr("timed out waiting for Google authorization (5 min)"))?
        .map_err(berr)?;

    let mut buf = [0u8; 8192];
    let n = socket.read(&mut buf).await.map_err(berr)?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    // "GET /?code=…&state=… HTTP/1.1"
    let path = first_line.split_whitespace().nth(1).unwrap_or("");
    let qs = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params = parse_query(qs);

    let body = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Ferrico</title></head>\
        <body style=\"font-family:system-ui,sans-serif;text-align:center;padding-top:4rem;background:#16161a;color:#e8e8ea\">\
        <h2>Ferrico is connected to Google Drive</h2>\
        <p>You can close this tab and return to the app.</p></body></html>";
    let resp = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    socket.write_all(resp.as_bytes()).await.ok();
    socket.flush().await.ok();

    if params.get("state").map(String::as_str) != Some(state.as_str()) {
        return Err(berr("OAuth state mismatch (possible CSRF) — try again"));
    }
    if let Some(err) = params.get("error") {
        return Err(berr(format!("Google authorization denied: {err}")));
    }
    let code = params
        .get("code")
        .ok_or_else(|| berr("no authorization code returned by Google"))?;

    let resp = http
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code.as_str()),
            ("code_verifier", verifier.as_str()),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect.as_str()),
        ])
        .send()
        .await
        .map_err(berr)?;
    let token: TokenResp = check(resp).await?.json().await.map_err(berr)?;

    // Best-effort: fetch the account email for display.
    let email = match http
        .get(USERINFO_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await
    {
        Ok(r) => match r.error_for_status() {
            Ok(r) => r.json::<UserInfo>().await.ok().and_then(|u| u.email),
            Err(_) => None,
        },
        Err(_) => None,
    };

    Ok((token.refresh_token, email))
}

/// Mints a fresh short-lived access token from the stored refresh token.
async fn refresh_access_token(
    http: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    refresh: &str,
) -> Result<String, AppError> {
    let resp = http
        .post(TOKEN_URL)
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("refresh_token", refresh),
            ("grant_type", "refresh_token"),
        ])
        .send()
        .await
        .map_err(berr)?;
    let token: TokenResp = check(resp).await?.json().await.map_err(berr)?;
    Ok(token.access_token)
}

// ─── Drive REST ─────────────────────────────────────────────────────────────────

async fn drive_list_folders(
    http: &reqwest::Client,
    token: &str,
) -> Result<Vec<DriveFolder>, AppError> {
    let resp = http
        .get(DRIVE_FILES)
        .bearer_auth(token)
        .query(&[
            ("q", "mimeType = 'application/vnd.google-apps.folder' and trashed = false"),
            ("fields", "files(id,name)"),
            ("spaces", "drive"),
            ("pageSize", "100"),
            ("orderBy", "name"),
        ])
        .send()
        .await
        .map_err(berr)?;
    let list: DriveFileList = check(resp).await?.json().await.map_err(berr)?;
    Ok(list
        .files
        .into_iter()
        .map(|f| DriveFolder { id: f.id, name: f.name })
        .collect())
}

async fn drive_create_folder(
    http: &reqwest::Client,
    token: &str,
    name: &str,
) -> Result<DriveFolder, AppError> {
    let resp = http
        .post(DRIVE_FILES)
        .bearer_auth(token)
        .query(&[("fields", "id,name")])
        .json(&serde_json::json!({ "name": name, "mimeType": FOLDER_MIME }))
        .send()
        .await
        .map_err(berr)?;
    let meta: DriveFileMeta = check(resp).await?.json().await.map_err(berr)?;
    Ok(DriveFolder { id: meta.id, name: meta.name })
}

async fn drive_find_backup(
    http: &reqwest::Client,
    token: &str,
    folder_id: &str,
) -> Result<Option<DriveFileMeta>, AppError> {
    let q = format!(
        "name = '{}' and '{}' in parents and trashed = false",
        BACKUP_FILENAME, folder_id
    );
    let resp = http
        .get(DRIVE_FILES)
        .bearer_auth(token)
        .query(&[
            ("q", q.as_str()),
            ("fields", "files(id,name,modifiedTime)"),
            ("spaces", "drive"),
        ])
        .send()
        .await
        .map_err(berr)?;
    let list: DriveFileList = check(resp).await?.json().await.map_err(berr)?;
    Ok(list.files.into_iter().next())
}

async fn drive_create_empty(
    http: &reqwest::Client,
    token: &str,
    folder_id: &str,
) -> Result<DriveFileMeta, AppError> {
    let resp = http
        .post(DRIVE_FILES)
        .bearer_auth(token)
        .query(&[("fields", "id,name,modifiedTime")])
        .json(&serde_json::json!({
            "name": BACKUP_FILENAME,
            "parents": [folder_id],
            "mimeType": "application/json",
        }))
        .send()
        .await
        .map_err(berr)?;
    let meta: DriveFileMeta = check(resp).await?.json().await.map_err(berr)?;
    Ok(meta)
}

async fn drive_download(
    http: &reqwest::Client,
    token: &str,
    file_id: &str,
) -> Result<String, AppError> {
    let resp = http
        .get(format!("{}/{}", DRIVE_FILES, file_id))
        .bearer_auth(token)
        .query(&[("alt", "media")])
        .send()
        .await
        .map_err(berr)?;
    check(resp).await?.text().await.map_err(berr)
}

async fn drive_update_content(
    http: &reqwest::Client,
    token: &str,
    file_id: &str,
    content: &str,
) -> Result<DriveFileMeta, AppError> {
    let resp = http
        .patch(format!("{}/{}", DRIVE_UPLOAD, file_id))
        .bearer_auth(token)
        .query(&[("uploadType", "media"), ("fields", "id,name,modifiedTime")])
        .header("Content-Type", "application/json")
        .body(content.to_string())
        .send()
        .await
        .map_err(berr)?;
    let meta: DriveFileMeta = check(resp).await?.json().await.map_err(berr)?;
    Ok(meta)
}

// ─── Engine ───────────────────────────────────────────────────────────────────

/// Shared, cheaply-cloneable handle wiring the DB, persisted config, HTTP client
/// and the Tauri app handle (for progress events). Held in Tauri managed state
/// and by the lifecycle tasks (open-pull, periodic-push, close-push).
#[derive(Clone)]
pub struct BackupEngine {
    db: Arc<Mutex<Connection>>,
    config: Arc<Mutex<BackupConfig>>,
    data_dir: Arc<PathBuf>,
    http: reqwest::Client,
    app: AppHandle,
}

impl BackupEngine {
    pub fn new(db: Arc<Mutex<Connection>>, data_dir: PathBuf, app: AppHandle) -> Self {
        let config = load_config(&data_dir);
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_default();
        BackupEngine {
            db,
            config: Arc::new(Mutex::new(config)),
            data_dir: Arc::new(data_dir),
            http,
            app,
        }
    }

    // ── config access ──────────────────────────────────────────────────────────

    fn cfg(&self) -> Result<BackupConfig, AppError> {
        Ok(self
            .config
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?
            .clone())
    }

    /// Mutate the in-memory config and persist it to disk atomically under the lock.
    fn update_cfg(&self, f: impl FnOnce(&mut BackupConfig)) -> Result<BackupConfig, AppError> {
        let mut guard = self
            .config
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?;
        f(&mut guard);
        let snapshot = guard.clone();
        save_config(self.data_dir.as_path(), &snapshot);
        Ok(snapshot)
    }

    pub fn status(&self) -> Result<BackupStatus, AppError> {
        let c = self.cfg()?;
        Ok(BackupStatus {
            has_credentials: c.client_id.is_some() && c.client_secret.is_some(),
            connected: c.refresh_token.is_some(),
            account_email: c.account_email,
            folder_id: c.folder_id,
            folder_name: c.folder_name,
            last_sync: c.last_sync,
            interval_min: c.interval_min,
            enabled: c.enabled,
        })
    }

    /// Backup is "active" (eligible for automatic open/close/periodic sync) only
    /// when enabled, connected, and a target folder is selected.
    pub fn is_active(&self) -> bool {
        self.cfg()
            .map(|c| c.enabled && c.refresh_token.is_some() && c.folder_id.is_some())
            .unwrap_or(false)
    }

    async fn access_token(&self) -> Result<String, AppError> {
        let c = self.cfg()?;
        match (c.client_id, c.client_secret, c.refresh_token) {
            (Some(id), Some(secret), Some(refresh)) => {
                refresh_access_token(&self.http, &id, &secret, &refresh).await
            }
            _ => Err(berr("Google Drive is not connected")),
        }
    }

    // ── DB bridge (lock held only for the sync section, never across an await) ──

    /// Read the local DB (incl. tombstones) as a merge snapshot.
    fn export_local_snapshot(&self) -> Result<crate::merge::SyncSnapshot, AppError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?;
        crate::db::db_export_sync_snapshot(&conn)
    }

    /// Replace the local DB with a merged snapshot.
    fn apply_local_snapshot(&self, snap: &crate::merge::SyncSnapshot) -> Result<(), AppError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?;
        crate::db::db_apply_sync_snapshot(&conn, snap)
    }

    // ── settings commands ────────────────────────────────────────────────────────

    pub fn set_credentials(
        &self,
        client_id: String,
        client_secret: String,
    ) -> Result<BackupStatus, AppError> {
        let id = client_id.trim().to_string();
        let secret = client_secret.trim().to_string();
        if id.is_empty() || secret.is_empty() {
            return Err(berr("client id and secret are required"));
        }
        self.update_cfg(|c| {
            c.client_id = Some(id);
            c.client_secret = Some(secret);
        })?;
        self.status()
    }

    pub async fn connect(&self) -> Result<BackupStatus, AppError> {
        let c = self.cfg()?;
        let (id, secret) = match (c.client_id, c.client_secret) {
            (Some(i), Some(s)) => (i, s),
            _ => return Err(berr("set the Google OAuth client id and secret first")),
        };
        let (refresh, email) = run_oauth(&self.http, &id, &secret).await?;
        if refresh.is_none() {
            return Err(berr(
                "Google did not return a refresh token — revoke Ferrico's access at \
                 myaccount.google.com/permissions and reconnect",
            ));
        }
        self.update_cfg(|c| {
            c.refresh_token = refresh;
            c.account_email = email;
        })?;
        self.status()
    }

    pub fn disconnect(&self) -> Result<BackupStatus, AppError> {
        // Keep client credentials + folder choice so reconnecting is one click;
        // only the refresh token / identity and the enabled flag are cleared.
        self.update_cfg(|c| {
            c.refresh_token = None;
            c.account_email = None;
            c.enabled = false;
        })?;
        self.status()
    }

    pub async fn list_folders(&self) -> Result<Vec<DriveFolder>, AppError> {
        let token = self.access_token().await?;
        drive_list_folders(&self.http, &token).await
    }

    pub async fn create_folder(&self, name: String) -> Result<DriveFolder, AppError> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(berr("folder name is required"));
        }
        let token = self.access_token().await?;
        let folder = drive_create_folder(&self.http, &token, &name).await?;
        // Auto-select the new folder; reset file/sync state for the fresh target.
        self.update_cfg(|c| {
            c.folder_id = Some(folder.id.clone());
            c.folder_name = Some(folder.name.clone());
            c.file_id = None;
            c.last_sync = None;
        })?;
        Ok(folder)
    }

    pub fn select_folder(
        &self,
        folder_id: String,
        folder_name: String,
    ) -> Result<BackupStatus, AppError> {
        self.update_cfg(|c| {
            c.folder_id = Some(folder_id);
            c.folder_name = Some(folder_name);
            c.file_id = None;
            c.last_sync = None;
        })?;
        self.status()
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<BackupStatus, AppError> {
        self.update_cfg(|c| c.enabled = enabled)?;
        self.status()
    }

    pub fn set_interval(&self, interval_min: u64) -> Result<BackupStatus, AppError> {
        self.update_cfg(|c| c.interval_min = interval_min)?;
        self.status()
    }

    // ── sync (per-record merge; see `merge.rs`) ──────────────────────────────────

    /// Full reconcile: pull the remote, MERGE it with the local DB
    /// record-by-record, and write the union back to both sides. Unlike the old
    /// snapshot last-write-wins (a blind overwrite), this cannot clobber another
    /// client's edits — a stale push merges the newer remote in first, so a
    /// just-opened or idle client can never stomp fresh remote changes. Returns
    /// `true` if the local DB changed.
    async fn run_sync(&self, op: &str) -> Result<bool, AppError> {
        let cfg = self.cfg()?;
        let folder_id = cfg
            .folder_id
            .clone()
            .ok_or_else(|| berr("no backup folder selected"))?;
        self.app.emit("backup-syncing", serde_json::json!({ "op": op })).ok();

        let result = async {
            let token = self.access_token().await?;
            let store = HttpDrive { http: self.http.clone(), token };
            let local = self.export_local_snapshot()?;
            let outcome = sync_once(
                &store,
                &folder_id,
                local,
                cfg.last_sync.clone(),
                cfg.file_id.clone(),
                cfg.last_pushed_digest.clone(),
            )
            .await?;

            if outcome.changed_local {
                self.apply_local_snapshot(&outcome.merged)?;
            }
            self.update_cfg(|c| {
                c.last_sync = outcome.new_last_sync.clone();
                c.file_id = outcome.new_file_id.clone();
                c.last_pushed_digest = outcome.new_digest.clone();
            })?;
            Ok::<bool, AppError>(outcome.changed_local)
        }
        .await;

        self.emit_result(op, result.as_ref().copied());
        result
    }

    /// Manual "Sync now" command. The open/close/periodic lifecycle hooks run the
    /// very same full merge cycle — there is no longer a push-only path that
    /// could overwrite the remote without first reconciling it.
    pub async fn sync_now(&self) -> Result<BackupStatus, AppError> {
        self.run_sync("sync").await?;
        self.status()
    }

    fn emit_result(&self, op: &str, outcome: Result<bool, &AppError>) {
        match outcome {
            Ok(changed) => {
                self.app
                    .emit("backup-synced", serde_json::json!({ "op": op, "changed": changed }))
                    .ok();
            }
            Err(e) => {
                self.app
                    .emit("backup-error", serde_json::json!({ "op": op, "message": e.to_string() }))
                    .ok();
            }
        }
    }

    // ── lifecycle entry points (best-effort, errors only logged) ──────────────────

    pub async fn pull_if_active(&self) {
        if self.is_active() {
            if let Err(e) = self.run_sync("pull").await {
                eprintln!("backup sync (open) failed: {e}");
            }
        }
    }

    pub async fn push_if_active(&self) {
        if self.is_active() {
            if let Err(e) = self.run_sync("push").await {
                eprintln!("backup sync (close) failed: {e}");
            }
        }
    }

    /// Periodic autosave loop. Re-reads the interval each tick so changes in
    /// settings take effect without a restart; `interval_min == 0` idles.
    pub async fn run_autosave(self) {
        loop {
            let interval = self.cfg().map(|c| c.interval_min).unwrap_or(0);
            if interval == 0 {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
            tokio::time::sleep(Duration::from_secs(interval * 60)).await;
            self.push_if_active().await;
        }
    }
}

// ─── Drive transport seam (lets the merge sync be unit-tested with a fake) ──────

/// The four Drive operations the sync core needs. Abstracting them behind a
/// trait lets `sync_once` run against an in-memory fake in tests, with no
/// network and no Tauri `AppHandle`.
#[allow(async_fn_in_trait)]
trait DriveStore {
    async fn find_backup(&self, folder_id: &str) -> Result<Option<DriveFileMeta>, AppError>;
    async fn create_empty(&self, folder_id: &str) -> Result<DriveFileMeta, AppError>;
    async fn download(&self, file_id: &str) -> Result<String, AppError>;
    async fn update_content(&self, file_id: &str, content: &str)
        -> Result<DriveFileMeta, AppError>;
}

/// Production transport: the real Drive REST calls, with one access token held
/// for the duration of a single sync.
struct HttpDrive {
    http: reqwest::Client,
    token: String,
}

impl DriveStore for HttpDrive {
    async fn find_backup(&self, folder_id: &str) -> Result<Option<DriveFileMeta>, AppError> {
        drive_find_backup(&self.http, &self.token, folder_id).await
    }
    async fn create_empty(&self, folder_id: &str) -> Result<DriveFileMeta, AppError> {
        drive_create_empty(&self.http, &self.token, folder_id).await
    }
    async fn download(&self, file_id: &str) -> Result<String, AppError> {
        drive_download(&self.http, &self.token, file_id).await
    }
    async fn update_content(
        &self,
        file_id: &str,
        content: &str,
    ) -> Result<DriveFileMeta, AppError> {
        drive_update_content(&self.http, &self.token, file_id, content).await
    }
}

/// Outcome of one reconcile pass. The engine applies `merged` to the DB (only
/// when `changed_local`) and persists the three config fields.
struct SyncOutcome {
    merged: crate::merge::SyncSnapshot,
    changed_local: bool,
    new_last_sync: Option<String>,
    new_file_id: Option<String>,
    new_digest: Option<String>,
}

/// FNV-1a 64-bit, hex. Deterministic across machines and runs (unlike the std
/// hasher), so it's safe to persist as the "snapshot we last published" marker.
fn digest(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// True if the remote `modifiedTime` is strictly newer than our last
/// reconciliation point (or we've never reconciled). RFC-3339 `Z` timestamps
/// order correctly lexicographically.
fn rfc3339_after(remote: &Option<String>, last: &Option<String>) -> bool {
    match (remote, last) {
        (Some(r), Some(l)) => r.as_str() > l.as_str(),
        (Some(_), None) => true,
        _ => false,
    }
}

/// Parse a remote file into a snapshot, tolerating the legacy v1 (active-only)
/// export so an existing Drive backup upgrades cleanly on the first merge.
fn parse_remote_snapshot(json: &str) -> crate::merge::SyncSnapshot {
    if let Some(s) = crate::merge::from_json(json) {
        return s;
    }
    crate::io::legacy_export_to_snapshot(json).unwrap_or_default()
}

/// The reconcile core: pure orchestration over a `DriveStore`. No DB, no Tauri,
/// no clock — every input is passed in, so it runs identically against the real
/// Drive and against the in-memory fake in the tests below.
///
/// 1. If the remote advanced past `last_sync`, download and **merge** it into
///    the local snapshot (never overwrite — merge can't lose either side).
/// 2. Publish the merged snapshot when it adds anything the remote lacks: after
///    a merge, unless the union already equals the remote; otherwise only when
///    the local snapshot changed since our last push (the digest gate stops two
///    idle clients ping-ponging `modifiedTime` bumps).
/// 3. Reuse the existing remote file id when present, so concurrent clients
///    converge on a single backup file instead of spawning duplicates.
async fn sync_once<S: DriveStore>(
    store: &S,
    folder_id: &str,
    local: crate::merge::SyncSnapshot,
    last_sync: Option<String>,
    file_id: Option<String>,
    last_pushed_digest: Option<String>,
) -> Result<SyncOutcome, AppError> {
    let local_json = crate::merge::to_json(&local).map_err(berr)?;
    let local_digest = digest(&local_json);

    let remote = store.find_backup(folder_id).await?;

    let pulled_json = match &remote {
        Some(f) if rfc3339_after(&f.modified_time, &last_sync) => {
            Some(store.download(&f.id).await?)
        }
        _ => None,
    };
    let merged = match &pulled_json {
        Some(json) => crate::merge::merge(local.clone(), parse_remote_snapshot(json)),
        None => local.clone(),
    };
    let merged_json = crate::merge::to_json(&merged).map_err(berr)?;
    let merged_digest = digest(&merged_json);
    let changed_local = merged_digest != local_digest;

    let should_push = match &pulled_json {
        Some(remote_json) => merged_digest != digest(remote_json),
        None => Some(&merged_digest) != last_pushed_digest.as_ref(),
    };

    let mut new_last_sync = last_sync;
    let mut new_file_id = file_id.or_else(|| remote.as_ref().map(|f| f.id.clone()));

    if should_push {
        let fid = match remote.as_ref() {
            Some(f) => f.id.clone(),
            None => store.create_empty(folder_id).await?.id,
        };
        let meta = store.update_content(&fid, &merged_json).await?;
        new_last_sync = meta.modified_time;
        new_file_id = Some(fid);
    } else if pulled_json.is_some() {
        // Nothing new to publish, but advance our point to the remote we read so
        // we don't re-download it next time.
        if let Some(f) = &remote {
            new_last_sync = f.modified_time.clone();
        }
    }

    Ok(SyncOutcome {
        merged,
        changed_local,
        new_last_sync,
        new_file_id,
        new_digest: Some(merged_digest),
    })
}

#[cfg(test)]
mod sync_tests {
    use super::*;
    use crate::merge::{SyncBookmark, SyncSnapshot};
    use std::sync::Mutex;

    // ── in-memory Drive: one shared folder, monotonic server clock ───────────

    struct FakeFile {
        id: String,
        content: String,
        modified_time: String,
    }
    struct FakeInner {
        files: Vec<FakeFile>,
        clock: u64,
    }
    struct FakeDrive {
        inner: Mutex<FakeInner>,
    }
    impl FakeDrive {
        fn new() -> Self {
            FakeDrive { inner: Mutex::new(FakeInner { files: Vec::new(), clock: 0 }) }
        }
        // Monotonic, zero-padded so it orders the same lexicographically and
        // numerically — exactly the property the real Drive `modifiedTime` has.
        fn tick(inner: &mut FakeInner) -> String {
            inner.clock += 1;
            format!("2026-01-01T00:00:00.{:06}Z", inner.clock)
        }
        fn file_count(&self) -> usize {
            self.inner.lock().unwrap().files.len()
        }
        fn remote(&self) -> SyncSnapshot {
            let inner = self.inner.lock().unwrap();
            crate::merge::from_json(&inner.files[0].content).unwrap()
        }
        fn remote_mtime(&self) -> String {
            self.inner.lock().unwrap().files[0].modified_time.clone()
        }
    }
    impl DriveStore for FakeDrive {
        async fn find_backup(&self, _folder_id: &str) -> Result<Option<DriveFileMeta>, AppError> {
            let inner = self.inner.lock().unwrap();
            Ok(inner.files.first().map(|f| DriveFileMeta {
                id: f.id.clone(),
                name: BACKUP_FILENAME.to_string(),
                modified_time: Some(f.modified_time.clone()),
            }))
        }
        async fn create_empty(&self, _folder_id: &str) -> Result<DriveFileMeta, AppError> {
            let mut inner = self.inner.lock().unwrap();
            let mt = Self::tick(&mut inner);
            let id = format!("file-{}", inner.files.len() + 1);
            inner.files.push(FakeFile {
                id: id.clone(),
                content: String::new(),
                modified_time: mt.clone(),
            });
            Ok(DriveFileMeta { id, name: BACKUP_FILENAME.to_string(), modified_time: Some(mt) })
        }
        async fn download(&self, file_id: &str) -> Result<String, AppError> {
            let inner = self.inner.lock().unwrap();
            inner
                .files
                .iter()
                .find(|f| f.id == file_id)
                .map(|f| f.content.clone())
                .ok_or_else(|| berr("fake: no such file"))
        }
        async fn update_content(
            &self,
            file_id: &str,
            content: &str,
        ) -> Result<DriveFileMeta, AppError> {
            let mut inner = self.inner.lock().unwrap();
            let mt = Self::tick(&mut inner);
            let f = inner
                .files
                .iter_mut()
                .find(|f| f.id == file_id)
                .ok_or_else(|| berr("fake: no such file"))?;
            f.content = content.to_string();
            f.modified_time = mt.clone();
            Ok(DriveFileMeta {
                id: file_id.to_string(),
                name: BACKUP_FILENAME.to_string(),
                modified_time: Some(mt),
            })
        }
    }

    /// One machine: its own local snapshot + persisted sync state. `sync` mirrors
    /// exactly what `BackupEngine::run_sync` does (apply merged, persist config).
    struct Client {
        local: SyncSnapshot,
        last_sync: Option<String>,
        file_id: Option<String>,
        digest: Option<String>,
    }
    impl Client {
        fn new() -> Self {
            Client { local: SyncSnapshot::default(), last_sync: None, file_id: None, digest: None }
        }
        async fn sync(&mut self, drive: &FakeDrive) -> bool {
            let outcome = sync_once(
                drive,
                "folder",
                self.local.clone(),
                self.last_sync.clone(),
                self.file_id.clone(),
                self.digest.clone(),
            )
            .await
            .unwrap();
            if outcome.changed_local {
                self.local = outcome.merged.clone();
            }
            self.last_sync = outcome.new_last_sync;
            self.file_id = outcome.new_file_id;
            self.digest = outcome.new_digest;
            outcome.changed_local
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
        }
    }
    fn snap(bms: Vec<SyncBookmark>) -> SyncSnapshot {
        SyncSnapshot { bookmarks: bms, ..Default::default() }
    }

    /// THE reported bug. Two clients edit different bookmarks "from time to
    /// time"; with the old blind-overwrite push, whoever pushed last erased the
    /// other's bookmark. With merge, both survive.
    #[tokio::test]
    async fn two_clients_disjoint_edits_both_survive() {
        let drive = FakeDrive::new();
        let mut a = Client::new();
        let mut b = Client::new();

        a.local = snap(vec![bm("X", "x", 10, None)]);
        a.sync(&drive).await; // A publishes X

        b.local = snap(vec![bm("Y", "y", 10, None)]);
        b.sync(&drive).await; // B pulls X, merges, publishes {X,Y}

        a.sync(&drive).await; // A pulls the union back

        assert_eq!(a.title_of("X"), Some("x"));
        assert_eq!(a.title_of("Y"), Some("y"));
        assert_eq!(b.title_of("X"), Some("x"));
        assert_eq!(b.title_of("Y"), Some("y"));
        let r = drive.remote();
        assert_eq!(r.bookmarks.len(), 2);
    }

    /// The idle-client clobber: A edits and pushes; B has been sitting open with
    /// stale data and its periodic sync fires. The old push-only autosave would
    /// stomp A's edit with B's stale copy. Now B reconciles instead.
    #[tokio::test]
    async fn idle_client_does_not_clobber_fresh_remote() {
        let drive = FakeDrive::new();
        let mut a = Client::new();
        let mut b = Client::new();

        a.local = snap(vec![bm("X", "x1", 10, None)]);
        b.local = snap(vec![bm("X", "x1", 10, None)]);
        a.sync(&drive).await; // remote = x1
        b.sync(&drive).await; // B now in sync at x1, idle

        // A edits X and pushes a newer version.
        a.local = snap(vec![bm("X", "x2", 20, None)]);
        a.sync(&drive).await;

        // B's periodic sync fires with NO local change.
        let changed = b.sync(&drive).await;

        assert!(changed, "B should have pulled A's newer edit");
        assert_eq!(b.title_of("X"), Some("x2"));
        assert_eq!(drive.remote().bookmarks[0].title, "x2", "remote not clobbered");
    }

    /// A deletion on one machine reaches the other (tombstone travels).
    #[tokio::test]
    async fn delete_propagates_across_clients() {
        let drive = FakeDrive::new();
        let mut a = Client::new();
        let mut b = Client::new();

        a.local = snap(vec![bm("X", "x", 10, None)]);
        a.sync(&drive).await;
        b.sync(&drive).await; // B has X live

        a.local = snap(vec![bm("X", "x", 20, Some(20))]); // delete on A
        a.sync(&drive).await;

        b.sync(&drive).await;
        assert_eq!(b.title_of("X"), None, "X should be tombstoned on B");
    }

    /// Concurrent first sync converges on ONE backup file, not a duplicate per
    /// client (the old `find ? : create` race produced two files).
    #[tokio::test]
    async fn first_sync_converges_on_single_file() {
        let drive = FakeDrive::new();
        let mut a = Client::new();
        let mut b = Client::new();

        a.local = snap(vec![bm("A", "a", 10, None)]);
        b.local = snap(vec![bm("B", "b", 10, None)]);
        a.sync(&drive).await;
        b.sync(&drive).await;

        assert_eq!(drive.file_count(), 1, "must reuse the one backup file");
        assert_eq!(drive.remote().bookmarks.len(), 2);
    }

    /// A truly idle re-sync must not re-upload — otherwise it bumps the remote
    /// `modifiedTime` and makes every other client needlessly re-pull (and two
    /// idle clients would ping-pong forever).
    #[tokio::test]
    async fn idle_resync_is_a_no_op() {
        let drive = FakeDrive::new();
        let mut a = Client::new();
        a.local = snap(vec![bm("X", "x", 10, None)]);
        a.sync(&drive).await;
        let mtime_after_first = drive.remote_mtime();

        let changed = a.sync(&drive).await;

        assert!(!changed);
        assert_eq!(drive.remote_mtime(), mtime_after_first, "no redundant upload");
    }
}
