//! Device pairing: desktop exports its sync connection(s) as a QR/paste code,
//! the phone imports it — no OAuth and no password typing on mobile.
//!
//! v2 (`ferrico-pair:v2:` + base64(json)) carries an optional block per
//! backend: `neon` (host/db/user/password — the primary sync) and `drive`
//! (the legacy manual-backup credentials). v1 codes (`ferrico-pair:v1:`,
//! Drive-only) still import, so an old desktop can pair a new phone.

use base64::Engine;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::gdrive::{self, BackupConfig};
use crate::pgsync::{self, NeonConfig};

const PREFIX_V2: &str = "ferrico-pair:v2:";

fn berr(msg: impl std::fmt::Display) -> AppError {
    AppError::Backup { message: msg.to_string() }
}

#[derive(Serialize, Deserialize)]
pub struct DrivePairing {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
    pub account_email: Option<String>,
    pub folder_id: String,
    pub folder_name: Option<String>,
    pub file_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct NeonPairing {
    pub host: String,
    pub dbname: Option<String>,
    pub user: String,
    pub password: String,
}

#[derive(Serialize, Deserialize)]
struct PairingV2 {
    /// Payload format version; always 2 here.
    v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    neon: Option<NeonPairing>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    drive: Option<DrivePairing>,
}

/// What an import found, already validated. Either block may be absent.
pub struct ImportedPairing {
    pub neon: Option<NeonPairing>,
    pub drive: Option<DrivePairing>,
}

fn drive_block(cfg: &BackupConfig) -> Option<DrivePairing> {
    match (&cfg.client_id, &cfg.client_secret, &cfg.refresh_token, &cfg.folder_id) {
        (Some(id), Some(secret), Some(refresh), Some(folder)) => Some(DrivePairing {
            client_id: id.clone(),
            client_secret: secret.clone(),
            refresh_token: refresh.clone(),
            account_email: cfg.account_email.clone(),
            folder_id: folder.clone(),
            folder_name: cfg.folder_name.clone(),
            file_id: cfg.file_id.clone(),
        }),
        _ => None,
    }
}

// The database password travels in the code in plaintext — unavoidable for a
// hands-off phone setup, and the same trust model as the v1 payload (which
// carried the Drive refresh token + client secret). The pairing channel
// (QR shown on the local screen / manual paste) is assumed trusted; the UI
// warns the user to keep the code out of chats, screenshots and backups.
fn neon_block(cfg: &NeonConfig) -> Option<NeonPairing> {
    let host = cfg.effective_host()?;
    let user = cfg.effective_user()?;
    let password = pgsync::load_password(cfg)?;
    Some(NeonPairing { host, dbname: Some(cfg.effective_dbname()), user, password })
}

/// Serialize every configured backend as a v2 code. Errors when neither
/// backend is set up — there is nothing to pair.
pub fn export_pairing(drive_cfg: &BackupConfig, neon_cfg: &NeonConfig) -> Result<String, AppError> {
    let payload =
        PairingV2 { v: 2, neon: neon_block(neon_cfg), drive: drive_block(drive_cfg) };
    if payload.neon.is_none() && payload.drive.is_none() {
        return Err(berr(
            "nothing to pair — connect Neon sync (or Google Drive) on this desktop first",
        ));
    }
    let json = serde_json::to_string(&payload).map_err(berr)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(json);
    Ok(format!("{PREFIX_V2}{b64}"))
}

/// Parse a v2 or legacy v1 pairing code.
pub fn import_pairing(s: &str) -> Result<ImportedPairing, AppError> {
    let s = s.trim();
    if let Some(b64) = s.strip_prefix(PREFIX_V2) {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .map_err(|e| berr(format!("invalid pairing code: {e}")))?;
        let p: PairingV2 = serde_json::from_slice(&bytes)
            .map_err(|e| berr(format!("invalid pairing payload: {e}")))?;
        if p.v != 2 {
            return Err(berr(format!("unsupported pairing version {}", p.v)));
        }
        if let Some(n) = &p.neon {
            if n.host.trim().is_empty() || n.user.trim().is_empty() || n.password.is_empty() {
                return Err(berr("pairing payload's Neon block is missing required fields"));
            }
        }
        if let Some(d) = &p.drive {
            if d.client_id.trim().is_empty()
                || d.client_secret.trim().is_empty()
                || d.refresh_token.trim().is_empty()
                || d.folder_id.trim().is_empty()
            {
                return Err(berr("pairing payload's Drive block is missing required fields"));
            }
        }
        if p.neon.is_none() && p.drive.is_none() {
            return Err(berr("pairing payload contains no connection"));
        }
        return Ok(ImportedPairing { neon: p.neon, drive: p.drive });
    }

    // Legacy v1: Drive-only. `gdrive::import_pairing` validates it fully.
    let v1 = gdrive::import_pairing(s)?;
    Ok(ImportedPairing {
        neon: None,
        drive: Some(DrivePairing {
            client_id: v1.client_id,
            client_secret: v1.client_secret,
            refresh_token: v1.refresh_token,
            account_email: v1.account_email,
            folder_id: v1.folder_id,
            folder_name: v1.folder_name,
            file_id: v1.file_id,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn neon_cfg() -> NeonConfig {
        NeonConfig {
            host: Some("ep-test.eu-central-1.aws.neon.tech".into()),
            dbname: Some("neondb".into()),
            user: Some("ferrico".into()),
            password: Some("pw-123".into()),
            enabled: true,
            interval_min: 0,
            last_seq: 42,
            last_sync: None,
        }
    }

    fn drive_cfg() -> BackupConfig {
        BackupConfig {
            client_id: Some("id-123".into()),
            client_secret: Some("secret-456".into()),
            refresh_token: Some("refresh-789".into()),
            account_email: Some("user@example.com".into()),
            folder_id: Some("folder-abc".into()),
            folder_name: Some("Ferrico Backup".into()),
            file_id: Some("file-def".into()),
            last_sync: None,
            interval_min: 0,
            enabled: false,
            last_pushed_digest: None,
        }
    }

    #[test]
    fn v2_round_trip_carries_both_blocks() {
        let code = export_pairing(&drive_cfg(), &neon_cfg()).unwrap();
        assert!(code.starts_with("ferrico-pair:v2:"));

        let p = import_pairing(&code).unwrap();
        let n = p.neon.expect("neon block");
        assert_eq!(n.host, "ep-test.eu-central-1.aws.neon.tech");
        assert_eq!(n.user, "ferrico");
        assert_eq!(n.password, "pw-123");
        assert_eq!(n.dbname.as_deref(), Some("neondb"));
        let d = p.drive.expect("drive block");
        assert_eq!(d.folder_id, "folder-abc");
    }

    #[test]
    fn neon_only_exports_without_drive() {
        let code = export_pairing(&BackupConfig::default(), &neon_cfg()).unwrap();
        let p = import_pairing(&code).unwrap();
        assert!(p.neon.is_some());
        assert!(p.drive.is_none());
    }

    #[test]
    fn nothing_configured_refuses_to_export() {
        assert!(export_pairing(&BackupConfig::default(), &NeonConfig::default()).is_err());
    }

    #[test]
    fn legacy_v1_code_still_imports_as_drive_block() {
        let v1 = gdrive::export_pairing(&drive_cfg()).unwrap();
        assert!(v1.starts_with("ferrico-pair:v1:"));
        let p = import_pairing(&v1).unwrap();
        assert!(p.neon.is_none());
        assert_eq!(p.drive.unwrap().refresh_token, "refresh-789");
    }

    #[test]
    fn import_rejects_garbage_and_empty_blocks() {
        assert!(import_pairing("hello").is_err());
        let empty = serde_json::json!({ "v": 2 }).to_string();
        let b64 = base64::engine::general_purpose::STANDARD.encode(empty);
        assert!(import_pairing(&format!("ferrico-pair:v2:{b64}")).is_err(), "no blocks");

        let blank_neon = serde_json::json!({
            "v": 2,
            "neon": { "host": " ", "dbname": null, "user": "u", "password": "p" }
        })
        .to_string();
        let b64 = base64::engine::general_purpose::STANDARD.encode(blank_neon);
        assert!(import_pairing(&format!("ferrico-pair:v2:{b64}")).is_err(), "blank host");
    }

    #[test]
    fn import_tolerates_surrounding_whitespace() {
        let code = export_pairing(&drive_cfg(), &neon_cfg()).unwrap();
        assert!(import_pairing(&format!("  {code}\n")).is_ok());
    }
}
