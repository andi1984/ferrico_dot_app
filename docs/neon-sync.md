# Neon sync

Ferrico syncs bookmarks across devices through **your own Postgres database** —
[Neon](https://neon.tech)'s free tier is the recommended host, but any
TLS-reachable Postgres works. This is the primary sync mechanism; Google Drive
remains as a [manual backup fallback](./google-drive-backup.md).

> **Privacy model.** There is no Ferrico-operated server. The app speaks the
> plain Postgres wire protocol (TLS required) straight to a database *you*
> create and own. Credentials stay on your devices: the password goes into the
> OS keychain on desktop (Keychain / Credential Manager / Secret Service, with
> a `settings.json` fallback if none is available) and into the app-private
> `settings.json` on Android. Nothing is sent anywhere except your database.

---

## Setup

1. Create a free project at [neon.tech](https://neon.tech) (one project per
   person — every user runs their own).
2. From the project's connection details, note the **host**
   (`ep-…-….<region>.aws.neon.tech`), the **role name** and its **password**,
   and the **database name** (default `neondb`).
3. In Ferrico: **Settings → Sync & Backup → Neon Sync**, paste host / user /
   database / password, **Save connection**, then enable
   **Sync automatically**.
4. Ferrico creates and migrates its tables itself on first contact — you never
   run SQL.
5. Additional desktops: repeat step 3 with the same credentials. Phones: use
   **Pair a mobile device** (QR or paste code) instead of typing anything.

Self-builders can bake defaults into the binary (the settings fields then come
prefilled): set `FERRICO_NEON_HOST`, `FERRICO_NEON_DB`, and/or
`FERRICO_NEON_USER` at compile time.

---

## How it works

| Aspect | Behaviour |
|---|---|
| **Remote storage** | Real rows in three Postgres tables (`folders`, `tags`, `bookmarks` — tag links ride in a `tag_ids` JSONB column). Tombstones travel; the schema is app-managed and idempotent. |
| **Change counter** | One global sequence assigns a `seq` to every inserted/updated row via a trigger. Each device stores the highest `seq` it has reconciled (its *cursor*). |
| **Incremental transfer** | A sync pulls only rows with `seq > cursor` and pushes only rows marked locally dirty (SQLite triggers track every local write), plus any repairs/corrections the merge produces. Steady state transfers nothing. |
| **Conflict resolution** | Byte-for-byte the same per-record merge as the old Drive sync (`merge.rs`): UUID identity, `updated_at` ranking, delete-beats-edit ties, structural normalization (duplicate-folder collapse, cycle breaking, re-homing). Commutative — all devices converge. |
| **Serialization** | Every cycle runs inside one Postgres transaction holding `pg_advisory_xact_lock`, so concurrent devices queue instead of interleaving; the cursor is read inside the lock. |
| **Push timing** | Desktop pushes within ~5 seconds of a local change (change-driven loop), plus on open, before close (window held briefly), on the optional pull interval, and manually. |
| **Android** | Full two-way sync (since v0.16 — pull-only before that). Pulls on launch, on foreground resume, on the optional interval, and via the refresh button; local edits push through the change loop (~15 s tick) and a best-effort flush when the app is backgrounded (Android has no close event). |
| **Empty-local safety** | A fresh install or wiped local DB always pulls the remote in full (stale cursors are ignored when local is empty) — absence never wins. |
| **Drive restore interplay** | Restoring a Drive backup replaces the *local* device only; if Neon sync is enabled, the next cycle merges the synced state back in. To make a restore authoritative you'd currently need a fresh Neon database (or branch). |

### Trade-offs to know

- Same as before on same-record conflicts: two devices editing the same
  bookmark resolve by `updated_at` (later write wins that record); edits to
  different records always both survive.
- The remote schema carries no foreign keys on purpose — cross-row invariants
  are repaired at merge time by `merge::normalize`, exactly as on Drive.
- Neon's free tier suspends compute after idle; the first sync after a pause
  takes a moment longer (cold start) but nothing is lost.

---

## Troubleshooting

- **"password is required when changing host or user"** — entering a new
  target invalidates the stored password; re-enter it.
- **"no stored Neon password"** — the OS keyring rejected or lost the entry;
  open Settings → Sync & Backup, edit the connection, re-enter the password.
- **Connection timeouts** — Neon requires TLS and the host must be the
  *endpoint* host from the dashboard (starts with `ep-`). Corporate networks
  that block outbound 5432 will block sync.
- **Two devices show different data** — trigger **Sync now** on both; the
  merge is commutative, so they converge after each has completed a cycle.

---

## Implementation notes (contributors)

| Piece | Where |
|---|---|
| Sync engine, store trait, Postgres transport, credential storage | `src-tauri/src/pgsync.rs` |
| Per-record merge + normalize (shared with Drive-era format) | `src-tauri/src/merge.rs` |
| Local dirty tracking (SQLite triggers, `sync_dirty`, `sync_meta` gate) | `src-tauri/src/db.rs` (`init_schema`, `db_get_dirty`, …) |
| Pairing codes (v2: Neon + optional Drive block; v1 still imports) | `src-tauri/src/pairing.rs` |
| Commands & lifecycle wiring (`neon_*`, open-pull, change loop, close-sync) | `src-tauri/src/lib.rs` |
| Settings UI | `src/components/BackupSettingsPage.tsx` |
| Design decisions & sync algorithm walkthrough | `docs/neon-sync-plan.md` |

The sync core (`pgsync::sync_once`) is pure orchestration over a `SyncStore`
trait — tests run it against an in-memory fake (`pgsync::tests`), and a future
web build swaps the wire-protocol transport for an HTTP one without touching
the algorithm.
