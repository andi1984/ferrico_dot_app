# Neon sync — implementation plan

Status: **implemented** on branch `neon-sync` (pending on-device verification).
User/contributor doc: [`neon-sync.md`](./neon-sync.md).

Ferrico gains a second sync backend: a user-owned [Neon](https://neon.tech) Postgres
project, spoken to over the raw Postgres wire protocol. Neon becomes the *primary*
sync mechanism; Google Drive is demoted to a manual one-way export/import fallback.

## Decisions (from design session, 2026-08-26)

| # | Decision |
|---|---|
| D1 | **Local-first stays.** SQLite remains the per-device source of truth; Neon is a sync backend, never a remote primary. |
| D2 | **BYO Neon project.** Each user creates their own Neon project. No Ferrico-operated infrastructure. Free tier (0.5 GB / 100 CU-hours per project) is ample. |
| D3 | **No Neon Auth, no Data API.** Both are beta; the native-client auth flow is undocumented (HTTP-only session cookie, 15-min JWTs). Plain Postgres role credentials (user/password) + `tokio-postgres` with rustls instead. No in-app login screen — just settings fields. |
| D4 | **Config = host field + user + password.** A build-time env var may *prefill* the host (todo-app pattern) but the field is editable, so prebuilt release binaries work too. |
| D5 | **Row-level storage, incremental sync.** Real rows in Postgres (not a JSON blob), pulled/pushed incrementally via a server-assigned sequence cursor. Merge semantics unchanged: the existing `merge.rs` per-record rank/normalize is reused as-is. |
| D6 | **Near-realtime push** (debounced a few seconds after each local change). Pull on app open + interval + manual. Faster pull polling / realtime channel: later. |
| D7 | **Remote schema is app-managed** — idempotent `CREATE TABLE IF NOT EXISTS` + additive migrations, mirroring the `db.rs` style. User runs zero SQL. |
| D8 | **Android stays pull-only**, enforced at compile time (same `cfg!(mobile)` pattern as Drive). *Superseded: Android is a full read-write sync peer since the mobile-write change (v0.16).* |
| D9 | **QR pairing payload v2** carries Neon host/dbname/user/password; manual paste remains as fallback. v1 (Drive) payloads still parse. |
| D10 | **Drive demoted**: all automatic triggers removed. Two manual buttons remain — "Export to Drive" (upload snapshot) and "Import from Drive" (wipe-and-restore behind an explicit confirm dialog, with an automatic local safety-export first). |
| D11 | **Exactly one auto-sync backend** (Neon). Drive and Neon never both sync automatically. |
| D12 | **Credential storage**: desktop → OS keyring (`keyring` crate); Android → app-private `settings.json` (OS-sandboxed). Non-secret Neon config lives in `settings.json` beside the existing `backup` block. |
| D13 | **One release, big bang** — built in safe increments on this branch, shipped together. |
| D14 | Deferred / out of scope: Neon Auth accounts (revisit at GA with a documented native flow), web version, realtime pull channel, ElectricSQL integration, full Drive removal. |
| D15 | **Transport seam**: sync logic is written against a store trait so a future web-era transport (Data API / thin server) swaps in without touching sync logic. |

## Remote schema

Three tables — `bookmark_tags` is *not* mirrored; tag links ride inside
`bookmarks.tag_ids` (JSONB), exactly matching the existing wire format
(`SyncBookmark.tag_ids`). `is_broken` / `last_checked_at` stay machine-local and are
never synced (unchanged from Drive sync).

```sql
CREATE TABLE IF NOT EXISTS folders (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  parent_id  TEXT,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL DEFAULT 0,
  deleted_at BIGINT,
  seq        BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS tags (
  id         TEXT PRIMARY KEY,
  name       TEXT NOT NULL,
  color      TEXT NOT NULL,
  created_at BIGINT NOT NULL,
  updated_at BIGINT NOT NULL DEFAULT 0,
  deleted_at BIGINT,
  seq        BIGINT NOT NULL
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
  seq         BIGINT NOT NULL
);
```

Notes:

- **No foreign keys remotely.** Tombstones travel and `merge.rs::normalize` already
  repairs cross-row invariants (orphan re-homing, folder cycles, tag collisions);
  remote FKs would only fight upsert ordering.
- **No `UNIQUE(name)` on remote tags** — SQLite needs the `name = id` tombstone
  workaround only because of its local UNIQUE constraint; the remote stores rows
  verbatim and normalize handles collisions at merge time.
- `seq`: one global sequence `ferrico_seq`; a `BEFORE INSERT OR UPDATE` trigger on
  each table sets `NEW.seq := nextval('ferrico_seq')`. Index on `seq` per table.
- A `schema_meta(key TEXT PRIMARY KEY, value TEXT)` table records the remote schema
  version for future additive migrations.

## Sync algorithm

Per-device cursor `last_seq` (stored locally in `settings.json`; each device has its
own). Local dirty tracking: a SQLite table `sync_dirty(kind TEXT, id TEXT, PRIMARY
KEY (kind, id))` written by every mutating `db.rs` operation.

One sync run (`sync_once`, pure orchestration over the store trait):

1. Open a transaction, take `pg_advisory_xact_lock(<fixed key>)` — serializes
   concurrent device syncs; no partial interleaving possible.
2. **Pull**: `SELECT … WHERE seq > $last_seq` from each table.
3. **Merge**: overlay pulled records onto the full local snapshot, run the existing
   `merge.rs` rank per record + `normalize` over the whole set (local state is full
   state, so normalize's cross-row repairs work exactly as before).
4. **Apply** the merged snapshot locally via `db_apply_sync_snapshot` (already
   preserves machine-local fields).
5. **Push**: upsert every record that is dirty locally or whose local version beat
   the pulled remote version. The trigger assigns fresh `seq` values.
6. **Advance cursor**: `SELECT last_value FROM ferrico_seq` inside the lock (no
   concurrent writers), store as new `last_seq`. Re-pulling own writes is harmless
   (merge is idempotent) but this avoids it entirely.
7. Commit; delete the `sync_dirty` rows captured at step 1 (not ones added since).

Empty-remote bootstrap: first sync from a device with data simply pushes everything
(cursor 0, nothing pulled). Empty-local bootstrap: pulls everything. Both are just
the normal algorithm, no special cases.

Mobile: `SyncMode::PullOnly` forced under `cfg!(mobile)` — steps 5 is skipped, the
advisory lock still taken (cheap, correct).

### Triggers

| Trigger | Platform | Behavior |
|---|---|---|
| App open (2s delay) | both | pull(+push on desktop) |
| Local mutation, debounced ~3 s | desktop | push-only fast path (full `sync_once`) |
| Interval (`interval_min`) | desktop | full sync |
| Window close | desktop | final full sync (reuses the existing `CloseRequested` hold) |
| Foreground resume | mobile | pull (existing `visibilitychange` + cooldown path) |
| Manual "Sync now" | both | full sync |

The existing `backup-syncing` / `backup-synced` / `backup-error` events are reused so
the frontend sync indicator works unchanged.

## Work breakdown (increments, in order)

1. **`src-tauri/src/pgstore.rs` + deps** — `tokio-postgres` + `tokio-postgres-rustls`
   (same rustls/ring stack reqwest already uses, so Android cross-compile is proven);
   `NeonConfig`; connection factory; app-managed remote schema init/migrations (D7).
2. **Store trait + engine** — extract the sync orchestration seam (D15): trait with
   `pull_since(seq)`, `push(records)`, `lock()`, `current_seq()`; `SyncEngine`
   struct modeled on `BackupEngine` (status, config persistence, events, autosave
   loop, debounced push). In-memory fake store for tests, like `FakeDrive`.
3. **Local dirty log** — `sync_dirty` table + writes in every `db.rs` mutation;
   `db_export_dirty` helper.
4. **Sync algorithm** — `sync_once` as above, reusing `merge.rs` untouched; unit
   tests against the fake store (two-device scenarios, tombstones, bootstrap,
   pull-only mode, cursor advancement).
5. **Credential storage** — `keyring` crate behind `#[cfg(desktop)]`; Android falls
   back to `settings.json`; config load/save with env prefill
   (`option_env!("FERRICO_NEON_HOST")` etc.) (D4, D12).
6. **Tauri commands + lifecycle** — `neon_*` commands (status, set config, test
   connection, sync now, disconnect); wire triggers into `setup()`; mobile pull-only.
7. **Frontend** — Neon section in `BackupSettingsPage.tsx` (host/user/password
   fields, test-connection, status, sync-now) and `MobileSettings.tsx`; indicator
   reuses existing events.
8. **QR pairing v2** — `ferrico-pair:v2` payload adds the Neon block; v1 still
   accepted; mobile import writes Neon config (D9).
9. **Drive demotion** — remove Drive auto triggers from `setup()`; Drive UI becomes
   "Export to Drive" / "Import from Drive (restore)" with confirm dialog + automatic
   local safety-export before restore (D10).
10. **Docs** — user/contributor doc `docs/neon-sync.md` (setup: create Neon project,
    create role, copy host), update `docs/google-drive-backup.md`, `CLAUDE.md`s.

Each increment compiles and passes tests on its own; the branch ships as one release
(D13).

## Risks / notes

- **Clock skew** is unchanged from Drive sync: `updated_at` is client-minted whole
  seconds; `merge.rs` tie rules already make merge commutative. The `seq` cursor is
  server-assigned, so *transport* correctness never depends on client clocks.
- **tags.name UNIQUE** remains a local-only constraint; remote is verbatim.
- **Web version later** talks HTTP (Data API or thin server) — only a new store
  trait impl + JWT plumbing; sync algorithm and schema already row-shaped for it.
- Password in QR payload: same trust model as the existing v1 payload (which already
  carries the Drive refresh token + client secret).
