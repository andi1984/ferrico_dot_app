# Google Drive backup & sync

Ferrico can mirror your library through **your own Google Drive** so multiple
machines stay in sync. This document covers what it does, how to set it up, and
how to troubleshoot it.

> **Privacy model.** There is no Ferrico-operated server. Your bookmarks are
> written to a folder *in your own Drive* by an OAuth app *you* create. Ferrico
> requests the [`drive.file`](https://developers.google.com/drive/api/guides/api-specific-auth)
> scope, which grants access **only to files the app itself creates** — it can
> never see, list, or read the rest of your Drive. Your OAuth client ID, secret,
> and refresh token live locally in `settings.json` (next to the database) and
> are sent to nobody except Google's token endpoint.

---

## How it works

| Aspect | Behaviour |
|---|---|
| **Backup file** | A single `ferrico-backup.json` — the same lossless JSON as **Settings → Export → JSON** — stored in a Drive folder you choose. |
| **On app open** | Pulls the latest snapshot. If the remote is newer than the last reconciliation point, the local database is replaced with it. |
| **Before app close** | Pushes the current database, overwriting the remote snapshot. The window is held open until the upload finishes. |
| **Periodic autosave** | Optional. Pushes every *N* minutes while the app runs (`0` disables it). |
| **Manual** | **Sync now** pulls, then pushes the resulting state. |
| **Conflict resolution** | Full-snapshot **last-write-wins**, keyed on Drive's server-side `modifiedTime` (immune to clock skew between machines). |

### Trade-offs to know

- **Last-write-wins is not a merge.** If two machines edit while *both* are
  offline and then sync, the one that syncs last overwrites the other's changes.
  This is fine for using one machine at a time; it is **not** a CRDT.
- **The Trash (bin) is not synced.** The JSON export excludes soft-deleted
  bookmarks, so items in the bin stay local to each machine.
- **`drive.file` cannot browse pre-existing folders.** The folder picker only
  lists folders Ferrico created. To target a folder, create it from within
  Ferrico (it appears in your Drive immediately).

---

## One-time Google Cloud setup

You need a free Google OAuth client. **Tip:** create a *dedicated project* for
Ferrico so the consent screen is branded "Ferrico" rather than reusing another
project's name (the consent dialog shows the *project's* configured app name,
not the credential's).

1. **Create / select a project** — [Google Cloud Console](https://console.cloud.google.com/)
   → project picker → **New Project** (e.g. `ferrico`).
2. **Enable the Drive API** — **APIs & Services → Library** → search
   **"Google Drive API"** → **Enable**. *(New projects start with it off; skipping
   this is the most common cause of a `403`.)*
3. **Configure the OAuth consent screen** — **APIs & Services → OAuth consent screen**:
   - User type **External** is fine for personal use.
   - Set **App name** to `Ferrico` (this is what the sign-in dialog displays).
   - Under **Test users**, add your own Google account. *(The scopes Ferrico uses
     — `drive.file`, `openid`, `email` — are all **non-sensitive**, so no Google
     verification is required and refresh tokens do not expire after 7 days, even
     while the app is in "Testing".)*
4. **Create the OAuth client** — **APIs & Services → Credentials → Create
   credentials → OAuth client ID** → **Application type: Desktop app** →
   **Create**. Copy the **Client ID** and **Client secret**.
   - *Desktop app* type is required: it permits the loopback redirect
     (`http://127.0.0.1:<random-port>`) Ferrico uses, with no redirect URI to
     register.

---

## Configure it in Ferrico

**Settings → Cloud Backup → Configure Google Drive…**

1. Paste the **Client ID** and **Client secret**, then **Save credentials**.
2. **Connect Google Drive** — a browser window opens for Google sign-in and
   consent. Approve it; the tab confirms and you return to the app.
3. **Pick a backup folder** — choose an existing Ferrico-created folder, or type
   a name and **Create** one.
4. **Enable automatic sync** and, if you like, set an autosave interval.
5. **Sync now** to push the first snapshot.

### Adding a second machine

Install Ferrico, open **Settings → Cloud Backup**, paste the **same** client
ID/secret, **Connect** with the **same Google account**, and select the **same
folder**. On the next launch (or **Sync now**) it pulls the shared snapshot.

---

## Troubleshooting

| Symptom | Cause & fix |
|---|---|
| `Google API 403 Forbidden: Google Drive API has not been used in project … or it is disabled` | The Drive API is not enabled for the project that owns your OAuth client. Enable it (setup step 2) and wait ~1 min. |
| Consent dialog shows the **wrong app name** (e.g. another product) | The dialog shows the **project's** OAuth-consent **App name**, which is shared by every credential in that project. Either rename it, or create a dedicated project for Ferrico. |
| `… 400 invalid_grant` / "reconnect Google Drive" | The stored refresh token was revoked or expired. **Disconnect**, then **Connect** again. |
| "Google did not return a refresh token" | Google withheld a refresh token because consent was already granted. Revoke Ferrico's access at [myaccount.google.com/permissions](https://myaccount.google.com/permissions), then reconnect. |
| Folder list is empty | Expected on first use with `drive.file` — the picker only shows folders Ferrico created. Create one from the **New folder** field. |

---

## Implementation notes (for contributors)

- **`src-tauri/src/gdrive.rs`** — the whole feature: OAuth2 PKCE + loopback flow,
  the Drive v3 REST calls, the `BackupEngine` (pull / push / sync orchestration),
  and config persistence into `settings.json`.
- **`src-tauri/src/main.rs`** — the `backup_*` Tauri commands (thin wrappers over
  `BackupEngine`) and the lifecycle wiring in `setup()`: open-pull, periodic
  autosave, and the `CloseRequested` handler that pushes before the window closes.
- **`src/components/BackupSettingsModal.tsx`** — the settings UI.
- **`src/events.ts`** — `backup-syncing` / `backup-synced` / `backup-error`
  events drive the in-app sync indicator and the post-pull refresh.

The LWW clock is the remote file's `modifiedTime`: `last_sync` records the value
last reconciled with, a pull happens only when the remote's `modifiedTime` is
greater, and each push advances `last_sync` to the value the upload returns.
