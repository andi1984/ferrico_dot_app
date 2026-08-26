//! Google Drive backup — a MANUAL export/import fallback.
//!
//! Since Neon/Postgres became the sync backend (see [`crate::pgsync`]), Drive
//! no longer auto-syncs. What remains is a user-triggered, one-way pair of
//! operations on the same versioned `SyncSnapshot` JSON file
//! (`ferrico-backup.json`) in a user-chosen Drive folder:
//!
//! * **Export**: overwrite the remote backup file with the local dataset
//!   (tombstones included);
//! * **Import (restore)**: replace the local database with the remote file's
//!   contents — destructive, so the frontend confirms first and the engine
//!   writes a local safety export before applying.
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

// ─── Pairing (desktop exports, phone imports — no OAuth on mobile) ──────────────
//
// The phone gets Drive access by importing a pairing payload exported from an
// already-connected desktop. `refresh_access_token` is plain HTTPS and works on
// any platform; only the initial loopback-redirect OAuth is desktop-bound.

const PAIRING_PREFIX: &str = "ferrico-pair:v1:";

#[derive(Serialize, Deserialize)]
pub struct PairingPayload {
    /// Payload format version; currently always 1.
    pub v: u32,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub account_email: Option<String>,
    pub folder_id: String,
    pub folder_name: Option<String>,
    pub file_id: Option<String>,
}

/// Serialize the connected config as `"ferrico-pair:v1:" + base64(json)`.
/// Requires an established connection: OAuth client credentials, a refresh
/// token, and a selected backup folder.
///
/// Production export moved to the v2 format (`crate::pairing`); this stays so
/// the v1 round-trip remains covered by tests against real import code.
#[cfg_attr(not(test), allow(dead_code))]
pub fn export_pairing(cfg: &BackupConfig) -> Result<String, AppError> {
    let (client_id, client_secret) = match (&cfg.client_id, &cfg.client_secret) {
        (Some(i), Some(s)) => (i.clone(), s.clone()),
        _ => return Err(berr("Google Drive is not connected — set credentials first")),
    };
    let refresh_token = cfg
        .refresh_token
        .clone()
        .ok_or_else(|| berr("Google Drive is not connected"))?;
    let folder_id = cfg
        .folder_id
        .clone()
        .ok_or_else(|| berr("no backup folder selected"))?;
    let payload = PairingPayload {
        v: 1,
        client_id,
        client_secret,
        refresh_token,
        account_email: cfg.account_email.clone(),
        folder_id,
        folder_name: cfg.folder_name.clone(),
        file_id: cfg.file_id.clone(),
    };
    let json = serde_json::to_string(&payload).map_err(berr)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(json);
    Ok(format!("{PAIRING_PREFIX}{b64}"))
}

/// Parse and validate a pairing string produced by [`export_pairing`].
pub fn import_pairing(s: &str) -> Result<PairingPayload, AppError> {
    let b64 = s
        .trim()
        .strip_prefix(PAIRING_PREFIX)
        .ok_or_else(|| berr("not a Ferrico pairing code (expected \"ferrico-pair:v1:…\")"))?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| berr(format!("invalid pairing code: {e}")))?;
    let payload: PairingPayload = serde_json::from_slice(&bytes)
        .map_err(|e| berr(format!("invalid pairing payload: {e}")))?;
    if payload.v != 1 {
        return Err(berr(format!("unsupported pairing version {}", payload.v)));
    }
    if payload.client_id.trim().is_empty()
        || payload.client_secret.trim().is_empty()
        || payload.refresh_token.trim().is_empty()
        || payload.folder_id.trim().is_empty()
    {
        return Err(berr("pairing payload is missing required fields"));
    }
    Ok(payload)
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

    tauri_plugin_opener::open_url(url.as_str(), None::<&str>)
        .map_err(|e| berr(format!("could not open browser: {e}")))?;

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

    // ── pairing ─────────────────────────────────────────────────────────────────

    /// Current config, for the v2 pairing exporter (`crate::pairing`).
    pub fn config_snapshot(&self) -> Result<BackupConfig, AppError> {
        self.cfg()
    }

    /// Adopt a paired Drive connection (from a v1 or v2 pairing code). Drive is
    /// a manual export/import fallback now, so nothing auto-enables; `last_sync`
    /// and the push digest reset because this device never reconciled anything.
    pub fn adopt_pairing(&self, p: &crate::pairing::DrivePairing) -> Result<(), AppError> {
        self.update_cfg(|c| {
            c.client_id = Some(p.client_id.clone());
            c.client_secret = Some(p.client_secret.clone());
            c.refresh_token = Some(p.refresh_token.clone());
            c.account_email = p.account_email.clone();
            c.folder_id = Some(p.folder_id.clone());
            c.folder_name = p.folder_name.clone();
            c.file_id = p.file_id.clone();
            c.enabled = false;
            c.interval_min = 0;
            c.last_sync = None;
            c.last_pushed_digest = None;
        })?;
        Ok(())
    }

    // ── manual export / import (one-way; no merge, no auto triggers) ────────────

    /// Overwrite the remote backup file with the local dataset. One-way: the
    /// remote's previous contents are NOT consulted.
    pub async fn export_to_drive(&self) -> Result<BackupStatus, AppError> {
        let cfg = self.cfg()?;
        let folder_id = cfg
            .folder_id
            .clone()
            .ok_or_else(|| berr("no backup folder selected"))?;
        self.app.emit("backup-syncing", serde_json::json!({ "op": "export" })).ok();

        let result = async {
            let token = self.access_token().await?;
            let store = HttpDrive { http: self.http.clone(), token };
            let local = self.export_local_snapshot()?;
            let json = crate::merge::to_json(&local).map_err(berr)?;
            let meta = export_once(&store, &folder_id, &json).await?;
            self.update_cfg(|c| {
                c.file_id = Some(meta.id.clone());
                c.last_sync = meta.modified_time.clone();
            })?;
            Ok::<bool, AppError>(false)
        }
        .await;

        self.emit_result("export", result.as_ref().copied());
        result?;
        self.status()
    }

    /// Replace the local database with the remote backup file (restore). The
    /// frontend shows the confirm dialog; this method still writes a local
    /// safety export (`pre-restore-backup-<ts>.json` beside the DB) before
    /// touching anything, so a mis-click is recoverable.
    ///
    /// Note: this restores the LOCAL device only. If Neon sync is enabled, the
    /// next cycle merges the remote Neon state back in — restore does not
    /// rewrite sync history.
    pub async fn import_from_drive(&self) -> Result<BackupStatus, AppError> {
        let cfg = self.cfg()?;
        let folder_id = cfg
            .folder_id
            .clone()
            .ok_or_else(|| berr("no backup folder selected"))?;
        self.app.emit("backup-syncing", serde_json::json!({ "op": "restore" })).ok();

        let result = async {
            let token = self.access_token().await?;
            let store = HttpDrive { http: self.http.clone(), token };
            let (snapshot, modified_time) = import_once(&store, &folder_id).await?;

            let local = self.export_local_snapshot()?;
            let safety = crate::merge::to_json(&local).map_err(berr)?;
            let path = self
                .data_dir
                .join(format!("pre-restore-backup-{}.json", crate::db::now()));
            std::fs::write(&path, safety)
                .map_err(|e| berr(format!("could not write safety export {path:?}: {e}")))?;

            self.apply_local_snapshot(&snapshot)?;
            self.update_cfg(|c| c.last_sync = modified_time.clone())?;
            Ok::<bool, AppError>(true)
        }
        .await;

        self.emit_result("restore", result.as_ref().copied());
        result?;
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

/// Find-or-create the backup file, then overwrite its contents. Generic over
/// [`DriveStore`] so the tests run it against the in-memory fake.
async fn export_once<S: DriveStore>(
    store: &S,
    folder_id: &str,
    json: &str,
) -> Result<DriveFileMeta, AppError> {
    let file = match store.find_backup(folder_id).await? {
        Some(f) => f,
        None => store.create_empty(folder_id).await?,
    };
    store.update_content(&file.id, json).await
}

/// Download and parse the backup file for a restore. Errors when no backup
/// file exists or the file is non-empty but unreadable (never silently treats
/// a corrupt backup as empty data).
async fn import_once<S: DriveStore>(
    store: &S,
    folder_id: &str,
) -> Result<(crate::merge::SyncSnapshot, Option<String>), AppError> {
    let file = store
        .find_backup(folder_id)
        .await?
        .ok_or_else(|| berr("no ferrico-backup.json found in the selected Drive folder"))?;
    let json = store.download(&file.id).await?;
    Ok((parse_remote_snapshot(&json)?, file.modified_time))
}

/// Parse a remote file into a snapshot, tolerating the legacy v1 (active-only)
/// export so an existing Drive backup upgrades cleanly on the first merge.
///
/// A blank file is a legitimately empty snapshot (e.g. a backup file just
/// created by another client that hasn't written it yet). A **non-empty** file
/// we cannot parse is an error — we must never silently turn an unreadable
/// remote into an empty snapshot, because the caller would then overwrite the
/// real backup with nothing. (That was the "it erased my backup" bug.)
fn parse_remote_snapshot(json: &str) -> Result<crate::merge::SyncSnapshot, AppError> {
    if json.trim().is_empty() {
        return Ok(crate::merge::SyncSnapshot::default());
    }
    if let Some(s) = crate::merge::from_json(json) {
        return Ok(s);
    }
    if let Ok(s) = crate::io::legacy_export_to_snapshot(json) {
        return Ok(s);
    }
    Err(berr(
        "the remote backup file exists but could not be read (unrecognized or \
         corrupt format) — refusing to overwrite it. Inspect ferrico-backup.json \
         in your Drive folder, then retry.",
    ))
}

#[cfg(test)]
mod pairing_tests {
    use super::*;

    fn connected_cfg() -> BackupConfig {
        BackupConfig {
            client_id: Some("id-123".into()),
            client_secret: Some("secret-456".into()),
            refresh_token: Some("refresh-789".into()),
            account_email: Some("user@example.com".into()),
            folder_id: Some("folder-abc".into()),
            folder_name: Some("Ferrico Backup".into()),
            file_id: Some("file-def".into()),
            last_sync: Some("2026-01-01T00:00:00Z".into()),
            interval_min: 30,
            enabled: true,
            last_pushed_digest: Some("deadbeef".into()),
        }
    }

    #[test]
    fn export_import_round_trip_preserves_fields() {
        let cfg = connected_cfg();
        let code = export_pairing(&cfg).unwrap();
        assert!(code.starts_with("ferrico-pair:v1:"));

        let p = import_pairing(&code).unwrap();
        assert_eq!(p.v, 1);
        assert_eq!(p.client_id, "id-123");
        assert_eq!(p.client_secret, "secret-456");
        assert_eq!(p.refresh_token, "refresh-789");
        assert_eq!(p.account_email.as_deref(), Some("user@example.com"));
        assert_eq!(p.folder_id, "folder-abc");
        assert_eq!(p.folder_name.as_deref(), Some("Ferrico Backup"));
        assert_eq!(p.file_id.as_deref(), Some("file-def"));
    }

    #[test]
    fn import_tolerates_surrounding_whitespace() {
        let code = export_pairing(&connected_cfg()).unwrap();
        let p = import_pairing(&format!("  {code}\n")).unwrap();
        assert_eq!(p.folder_id, "folder-abc");
    }

    #[test]
    fn import_rejects_garbage() {
        assert!(import_pairing("hello world").is_err(), "no prefix");
        assert!(import_pairing("ferrico-pair:v1:!!!not-base64!!!").is_err(), "bad base64");
        let not_json = base64::engine::general_purpose::STANDARD.encode("not json");
        assert!(import_pairing(&format!("ferrico-pair:v1:{not_json}")).is_err(), "bad json");
    }

    #[test]
    fn import_rejects_wrong_version() {
        let json = r#"{"v":2,"client_id":"i","client_secret":"s","refresh_token":"r",
            "account_email":null,"folder_id":"f","folder_name":null,"file_id":null}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        assert!(import_pairing(&format!("ferrico-pair:v1:{b64}")).is_err());
    }

    #[test]
    fn import_rejects_missing_or_empty_required_fields() {
        // folder_id key absent entirely → serde rejects
        let json = r#"{"v":1,"client_id":"i","client_secret":"s","refresh_token":"r",
            "account_email":null,"folder_name":null,"file_id":null}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        assert!(import_pairing(&format!("ferrico-pair:v1:{b64}")).is_err());

        // folder_id present but blank → field validation rejects
        let json = r#"{"v":1,"client_id":"i","client_secret":"s","refresh_token":"r",
            "account_email":null,"folder_id":"  ","folder_name":null,"file_id":null}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        assert!(import_pairing(&format!("ferrico-pair:v1:{b64}")).is_err());
    }

    #[test]
    fn export_fails_when_not_connected() {
        assert!(export_pairing(&BackupConfig::default()).is_err(), "empty config");

        let mut no_refresh = connected_cfg();
        no_refresh.refresh_token = None;
        assert!(export_pairing(&no_refresh).is_err(), "missing refresh token");

        let mut no_folder = connected_cfg();
        no_folder.folder_id = None;
        assert!(export_pairing(&no_folder).is_err(), "missing folder");
    }
}

#[cfg(test)]
mod export_import_tests {
    use super::*;
    use crate::merge::{SyncBookmark, SyncSnapshot};
    use std::sync::Mutex;

    // Minimal in-memory Drive: at most one backup file in one folder.
    #[derive(Default)]
    struct FakeInner {
        file: Option<(String, String)>, // (id, content)
        clock: u64,
    }
    #[derive(Default)]
    struct FakeDrive {
        inner: Mutex<FakeInner>,
    }
    impl FakeDrive {
        fn seed(&self, content: &str) {
            let mut g = self.inner.lock().unwrap();
            g.file = Some(("file-1".into(), content.into()));
        }
        fn content(&self) -> Option<String> {
            self.inner.lock().unwrap().file.as_ref().map(|(_, c)| c.clone())
        }
    }
    impl DriveStore for FakeDrive {
        async fn find_backup(&self, _folder_id: &str) -> Result<Option<DriveFileMeta>, AppError> {
            let g = self.inner.lock().unwrap();
            Ok(g.file.as_ref().map(|(id, _)| DriveFileMeta {
                id: id.clone(),
                name: BACKUP_FILENAME.to_string(),
                modified_time: Some(format!("2026-01-01T00:00:00.{:06}Z", g.clock)),
            }))
        }
        async fn create_empty(&self, _folder_id: &str) -> Result<DriveFileMeta, AppError> {
            let mut g = self.inner.lock().unwrap();
            g.file = Some(("file-1".into(), String::new()));
            Ok(DriveFileMeta {
                id: "file-1".into(),
                name: BACKUP_FILENAME.to_string(),
                modified_time: None,
            })
        }
        async fn download(&self, file_id: &str) -> Result<String, AppError> {
            let g = self.inner.lock().unwrap();
            match &g.file {
                Some((id, c)) if id == file_id => Ok(c.clone()),
                _ => Err(berr("fake: no such file")),
            }
        }
        async fn update_content(
            &self,
            file_id: &str,
            content: &str,
        ) -> Result<DriveFileMeta, AppError> {
            let mut g = self.inner.lock().unwrap();
            g.clock += 1;
            let mt = format!("2026-01-01T00:00:00.{:06}Z", g.clock);
            match &mut g.file {
                Some((id, c)) if id == file_id => *c = content.to_string(),
                _ => return Err(berr("fake: no such file")),
            }
            Ok(DriveFileMeta {
                id: file_id.to_string(),
                name: BACKUP_FILENAME.to_string(),
                modified_time: Some(mt),
            })
        }
    }

    fn bm(id: &str, title: &str) -> SyncBookmark {
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
            updated_at: 1,
            deleted_at: None,
            purged_at: None,
        }
    }

    #[tokio::test]
    async fn export_creates_file_then_overwrites_it() {
        let drive = FakeDrive::default();
        let snap = SyncSnapshot { bookmarks: vec![bm("A", "a")], ..Default::default() };
        let json = crate::merge::to_json(&snap).unwrap();
        export_once(&drive, "folder", &json).await.unwrap();
        assert_eq!(drive.content().as_deref(), Some(json.as_str()));

        // Second export blindly overwrites — one-way by design.
        let snap2 = SyncSnapshot { bookmarks: vec![bm("B", "b")], ..Default::default() };
        let json2 = crate::merge::to_json(&snap2).unwrap();
        export_once(&drive, "folder", &json2).await.unwrap();
        assert_eq!(drive.content().as_deref(), Some(json2.as_str()));
    }

    #[tokio::test]
    async fn import_parses_v2_snapshot() {
        let drive = FakeDrive::default();
        let snap = SyncSnapshot { bookmarks: vec![bm("A", "a")], ..Default::default() };
        drive.seed(&crate::merge::to_json(&snap).unwrap());

        let (restored, mtime) = import_once(&drive, "folder").await.unwrap();
        assert_eq!(restored, snap);
        assert!(mtime.is_some());
    }

    #[tokio::test]
    async fn import_without_backup_file_errors() {
        let drive = FakeDrive::default();
        assert!(import_once(&drive, "folder").await.is_err());
    }

    #[tokio::test]
    async fn import_refuses_unreadable_backup() {
        let drive = FakeDrive::default();
        drive.seed("{ definitely not a snapshot");
        assert!(import_once(&drive, "folder").await.is_err());
    }

    #[tokio::test]
    async fn import_of_blank_file_restores_empty_dataset() {
        let drive = FakeDrive::default();
        drive.seed("");
        let (restored, _) = import_once(&drive, "folder").await.unwrap();
        assert_eq!(restored, SyncSnapshot::default());
    }
}

