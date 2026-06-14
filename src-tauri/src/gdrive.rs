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

    fn export_local(&self) -> Result<String, AppError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?;
        crate::io::export_json(&conn)
    }

    fn replace_local(&self, json: &str) -> Result<usize, AppError> {
        let conn = self
            .db
            .lock()
            .map_err(|e| AppError::Lock { message: e.to_string() })?;
        crate::db::db_clear_all_data(&conn)?;
        let r = crate::io::import_json(&conn, json)?;
        Ok(r.imported)
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

    // ── sync ─────────────────────────────────────────────────────────────────────

    /// Upload the local DB, overwriting the remote snapshot.
    pub async fn push(&self) -> Result<BackupStatus, AppError> {
        let folder_id = self
            .cfg()?
            .folder_id
            .ok_or_else(|| berr("no backup folder selected"))?;
        self.app.emit("backup-syncing", serde_json::json!({ "op": "push" })).ok();
        let result = self.push_inner(&folder_id).await;
        self.emit_result("push", result.as_ref().map(|_| false));
        result?;
        self.status()
    }

    async fn push_inner(&self, folder_id: &str) -> Result<(), AppError> {
        let token = self.access_token().await?;
        let json = self.export_local()?;

        let mut file_id = self.cfg()?.file_id;
        if file_id.is_none() {
            file_id = match drive_find_backup(&self.http, &token, folder_id).await? {
                Some(f) => Some(f.id),
                None => Some(drive_create_empty(&self.http, &token, folder_id).await?.id),
            };
        }
        let fid = file_id.expect("file_id resolved above");
        let meta = drive_update_content(&self.http, &token, &fid, &json).await?;
        self.update_cfg(|c| {
            c.file_id = Some(fid.clone());
            c.last_sync = meta.modified_time.clone();
        })?;
        Ok(())
    }

    /// Pull the remote snapshot and replace the local DB iff the remote is newer
    /// than our last reconciliation point. Returns `true` if the DB was replaced.
    pub async fn pull(&self) -> Result<bool, AppError> {
        let c = self.cfg()?;
        let folder_id = c.folder_id.ok_or_else(|| berr("no backup folder selected"))?;
        self.app.emit("backup-syncing", serde_json::json!({ "op": "pull" })).ok();
        let result = self.pull_inner(&folder_id, c.last_sync).await;
        self.emit_result("pull", result.as_ref().copied());
        result
    }

    async fn pull_inner(
        &self,
        folder_id: &str,
        last_sync: Option<String>,
    ) -> Result<bool, AppError> {
        let token = self.access_token().await?;
        let file = match drive_find_backup(&self.http, &token, folder_id).await? {
            Some(f) => f,
            None => return Ok(false), // first-ever sync — nothing remote yet
        };
        // LWW: skip if the remote hasn't advanced past what we already have.
        // RFC-3339 `Z` timestamps compare correctly lexicographically.
        if let (Some(last), Some(remote)) = (&last_sync, &file.modified_time) {
            if remote.as_str() <= last.as_str() {
                return Ok(false);
            }
        }
        let content = drive_download(&self.http, &token, &file.id).await?;
        self.replace_local(&content)?;
        self.update_cfg(|c| {
            c.file_id = Some(file.id.clone());
            c.last_sync = file.modified_time.clone();
        })?;
        Ok(true)
    }

    /// Manual "Sync now": reconcile down, then push the resulting state up.
    pub async fn sync_now(&self) -> Result<BackupStatus, AppError> {
        self.pull().await?;
        self.push().await
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
            if let Err(e) = self.pull().await {
                eprintln!("backup pull failed: {e}");
            }
        }
    }

    pub async fn push_if_active(&self) {
        if self.is_active() {
            if let Err(e) = self.push().await {
                eprintln!("backup push failed: {e}");
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
