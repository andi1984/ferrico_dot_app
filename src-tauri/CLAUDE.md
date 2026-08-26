# Ferrico — Rust backend (`src-tauri/`)

Tauri 2 backend. App state is `AppState { db: Arc<Mutex<Connection>> }` (SQLite via
rusqlite). The DB file lives in the OS data dir (see root `CLAUDE.md` → Platform notes).

## Architecture

- **`db.rs`** holds all DB logic as **pure functions** taking `&Connection` (`db_*`).
  No locking, no Tauri types — easy to unit-test against in-memory SQLite.
- **`lib.rs`** Tauri commands are **thin wrappers**: lock the mutex with the `lock_db!`
  macro → call the `db::*` function → return. No business logic in the command layer.
- Commands are registered in `tauri::generate_handler![ ... ]` inside `run()` (`lib.rs`).
  `main.rs` is a thin shim calling `ferrico_lib::run()`; `run()` doubles as the
  Tauri 2 mobile entry point via `#[cfg_attr(mobile, tauri::mobile_entry_point)]`.
- `setup()` wires lifecycle: open DB, load/create API token, start the HTTP server and
  background cover scanner, and the Neon sync engine (open-pull, change-driven push
  loop, final sync on `CloseRequested`). The Drive engine is managed for manual
  export/import commands only.

## Module map

| File | Responsibility |
|---|---|
| `db.rs` | All SQLite CRUD, search, cascade deletes — pure `db_*` fns + tests |
| `error.rs` | `AppError` type shared with the frontend |
| `io.rs` | Import/export: JSON, Netscape HTML, OPML (+ legacy snapshot) |
| `io_validate.rs` | Input validation/sanitization for imports (URLs, sizes, tags, BOM) |
| `merge.rs` | Per-record merge for multi-machine sync (`SyncSnapshot`, `merge()`) |
| `pgsync.rs` | Neon/Postgres sync engine — the primary sync backend (incremental, `SyncStore` seam) |
| `pairing.rs` | Device pairing codes (v2: Neon + Drive blocks; legacy v1 imports) |
| `gdrive.rs` | Google Drive manual backup (OAuth2 PKCE, Drive v3 REST, one-way export/restore) |
| `og_image.rs` | Fetch Open Graph cover images for bookmarks |
| `health_check.rs` | Async URL liveness checks (dead-link detection) |
| `lib.rs` | Tauri commands, `lock_db!`, HTTP server, scanners, `setup()` |

## Error type

`AppError` (`error.rs`) is a discriminated union, `#[serde(tag = "name")]`, serializing to
`{ name, message }` for the frontend. Variants: `Db`, `Lock`, `NotFound`, `Validation`,
`Backup`. See the `rust-errors` skill for the Rust↔TS error-handling pattern.

## Adding a command

1. Add a pure `db_*` function in `db.rs` taking `&Connection`, and write its tests there.
2. Add a thin Tauri command wrapper in `lib.rs` using the `lock_db!` macro.
3. Register it in `tauri::generate_handler![ ... ]` in `run()` (`lib.rs`).

## HTTP server & background work

- `start_http_server(db, token, app_handle)` exposes a small local HTTP API (token-guarded)
  that the **browser extension** uses to add/query bookmarks. The token is persisted in
  `settings.json` (`load_or_create_token`).
- `background_cover_scanner(db, app)` backfills missing cover images via `og_image.rs`,
  emitting `cover-updated` events to the frontend.

## Neon sync (`pgsync.rs`)

- The primary sync backend: the user's own Postgres (Neon recommended), raw wire
  protocol via `tokio-postgres` + rustls/ring. Remote schema is app-managed idempotent
  DDL: row tables with a `seq` column bumped from one global sequence by trigger.
- **Incremental**: each device pulls `WHERE seq > cursor` and pushes only rows marked
  in SQLite's `sync_dirty` (maintained by triggers in `db.rs`), inside one transaction
  holding `pg_advisory_xact_lock`. Conflict resolution reuses `merge.rs` unchanged.
- Mobile is pull-only at compile time (`SyncMode::PullOnly` via `cfg!(mobile)`).
- Config under `"neon"` in `settings.json`; password in the OS keyring on desktop
  (settings fallback), settings.json on mobile. Build-time env prefill:
  `FERRICO_NEON_HOST` / `FERRICO_NEON_DB` / `FERRICO_NEON_USER`.
- `neon_*` commands in `lib.rs` wrap the engine. UI: `src/components/BackupSettingsPage.tsx`.
- Docs: `docs/neon-sync.md`, design: `docs/neon-sync-plan.md`.

## Google Drive backup (`gdrive.rs`) — manual fallback

- OAuth2 PKCE + loopback flow, Drive v3 REST, `drive.file` scope. No automatic sync:
  one-way `export_to_drive` (overwrite remote file with local snapshot) and
  `import_from_drive` (restore local from remote, after a frontend confirm; writes a
  `pre-restore-backup-<ts>.json` safety export beside the DB first).
- Backup file = versioned `SyncSnapshot` JSON (`merge::to_json`); legacy v1 files parse.
- Config (client id/secret, refresh token, folder, `last_sync`) is persisted under the
  `"backup"` key in `settings.json`, merged so the HTTP `api_token` is preserved.
- Docs: `docs/google-drive-backup.md`.

## Testing

All Rust tests use **in-memory SQLite** — no fixtures, no disk state. They live in
`#[cfg(test)]` modules next to the code they cover (`db.rs`, `io.rs`, `io_validate.rs`,
`merge.rs`, `gdrive.rs`, `health_check.rs`), ~320 tests total.

```bash
cargo test                 # cargo is the rustup default on this machine
```

Coverage with llvm-cov:

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --html
```
