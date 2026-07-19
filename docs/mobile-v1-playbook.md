# Mobile v1 Playbook — execution guide for the remaining work

Audience: the AI assistant (or human) doing day-to-day work on the **Mobile v1 —
read-only Android** milestone. This doc assumes you have the repo's `CLAUDE.md`
files in context but no memory of the sessions that produced Phases 1–3.

**Source of truth for scope**: epic issue **#76** and the per-ticket GitHub issues.
Always run `gh issue view <N>` and read the full body **before starting a ticket** —
the issues contain task checklists, acceptance criteria, and file lists that this
doc does not duplicate. This doc adds the process rules, the current state of the
code, and gotchas the issues don't mention.

---

## 1. Where the project stands

Done and merged to `main` (all issues closed):

| Phase | Issues | PRs | What landed |
|---|---|---|---|
| P1 Rust restructure | #54–#57 | #77–#81 | `main.rs` → `lib.rs` split (`ferrico_lib`, `pub fn run()` + `#[cfg_attr(mobile, tauri::mobile_entry_point)]`); DB/AppState init inside `setup()`; `resolve_data_dir()` platform split; `open` crate → `tauri-plugin-opener`; `#[cfg(desktop)]` gates on HTTP server, cover scanner, close-push, autosave, `rfd` pickers |
| P2 Sync backend | #58, #59 | #82, #83 | `SyncMode { Full, PullOnly }` on `sync_once` (mode picked via `cfg!(mobile)` — compile-time read-only); `PairingPayload` v1 + `export_pairing`/`import_pairing` + `backup_export_pairing`/`backup_import_pairing` commands |
| P3 Desktop pairing UI | #60 | #84 | "Pair a mobile device" section in `BackupSettingsPage.tsx` (QR + copy string), `qrcode` dependency |
| P4.1 Android toolchain | #61 | #87 | SDK/NDK/rustup targets installed, AVD `ferrico_dev` created (Linux docs) |
| P4.2 Android scaffold | #62 | #88 | `tauri android init` output committed, vite `host: true`, CSP `img-src` fix |
| P4.3 Platform detection | #63 | #86 | `src/platform.ts` UA sniff, `src/main.tsx` mobile/desktop split |
| P4.4 Mobile shell | #64 | #89 | `src/mobile/MobileApp.tsx` — state, data loading, theme, events |
| P4.5 Mobile header | #65 | #91 | `src/mobile/MobileHeader.tsx` — search, view toggle, refresh + sync status |
| P4.6 FilterDrawer | #66 | #92 | `src/mobile/FilterDrawer.tsx` — folders/tags bottom sheet |
| P4.7 Mobile list view | #67 | #93 | `MobileBookmarkListItem`/`MobileBookmarkList` — virtualized read-only row |
| P4.8 readOnly grid | #68 | #94 | `readOnly` prop on `BookmarkGrid`/`BookmarkCard`, wired into `MobileApp` |
| P5.1 Pairing import UI | #69 | #95 | `src/mobile/MobileSettings.tsx` — paste pairing code, sync now, unpair |
| P5.2 Pull lifecycle | #70 | #96 | Foreground-resume pull via `visibilitychange` + cooldown |

**All Phase 1–5 core tickets (#54–#70) are merged and shipped in v0.14.0.** The app runs
on a real device: pairing, pull sync, browsing, and tap-to-open are all confirmed
working. Post-merge fixes from that first device session landed in #99 (inotify watch
limit), #101 (touch behavior) and #102 (the `OpenerExt` bug — see §7). Android CI landed
in #103.

Remaining:

- **#71** P5.3 End-to-end device verification pass — needs the user with a physical
  device or the emulator; checklist ready at `docs/mobile-v1-device-verification.md`.
  Parts of it are now informally confirmed, but the full pass — especially the
  **pull-only proof** (Drive `modifiedTime` must never change from phone activity) —
  has not been walked end to end.
- **#72–#75** Stretch (QR scanning, pull-to-refresh, release signing, iOS) — only on
  explicit user request. Note #74 (release signing) is what would let CI ship a
  properly-signed APK instead of the current debug-signed one.

**Lesson from #67–#70**: they were built in parallel worktrees off the same
`origin/main` point (all independent per the epic's dependency notes), which meant
each PR's branch predated the others' edits to the shared `MobileApp.tsx`. Every PR
after the first to merge hit a real conflict (not just a stale-branch warning) and
needed `git rebase origin/main` + manual resolution — twice for #95, since #94 and
#96 both merged while it was still open. Watch for two things when resolving: (1)
dead imports/helpers left behind once a sibling PR's placeholder code is gone
(e.g. `openBookmark`, `domainOf`, `IconArrowLeft` all went dead across these merges
as the real components replaced the placeholders they'd been serving) — delete them,
don't just resolve the textual conflict; (2) `cargo test` + `bun run typecheck` +
`bun run test` green in the worktree before force-pushing the rebase, same as any
other push. Same-parallel-batch tickets touching a shared shell file next time:
consider landing them serially instead, or expect a rebase pass per PR.

## 2. Settled architecture decisions — do not re-litigate

From the epic (#76). Re-opening these wastes a review cycle:

1. **One Tauri app**, mobile target in the same crate. The single ~59-command
   `generate_handler!` list stays identical on both platforms; desktop-only behavior
   lives in `#[cfg]`'d function *bodies*, never inside the macro.
2. **Data dir is platform-split** (`resolve_data_dir` in `src-tauri/src/lib.rs`).
   Desktop stays on `dirs::data_dir()/ferrico` forever — moving to Tauri's
   `app_data_dir()` would orphan existing user databases.
3. **Pull-only sync is compile-time**: `SyncMode` selected via `cfg!(mobile)` in
   `run_sync` (`src-tauri/src/gdrive.rs`). Never add a runtime toggle for this.
4. **Pairing, not OAuth, on the phone**: the phone imports a
   `ferrico-pair:v1:<base64 json>` code exported by a connected desktop.
   `refresh_access_token()` works on any platform; the loopback OAuth flow is
   desktop-only.
5. **Dedicated mobile shell** at `src/mobile/MobileApp.tsx`, selected at startup via
   userAgent detection; desktop `App.tsx` stays untouched. Reuse: `SearchBox`,
   `Favicon`, `EmptyState`, icons, theme CSS vars, `SettingsLayout`,
   `BookmarkGrid`/`BookmarkCard` (add a `readOnly` prop). Do **not** reuse `Sidebar`
   or `BookmarkRow` (hover/drag/touch-action problems).
6. **Zero DB mutations from the mobile UI** — not even `purge_expired_bin`.
   Snapshots are applied only by the sync engine via `db_apply_sync_snapshot`.

## 3. Process rules (hard requirements)

### Branch/PR workflow

1. **Always work in a git worktree**: `git worktree add .claude/worktrees/<branch> -b <branch> origin/main`
   (run `git fetch origin` first). Remove the worktree after the PR is open:
   `git worktree remove .claude/worktrees/<branch>`.
2. **Flat PRs only, always based on `origin/main`.** Do not stack PRs on feature
   branches. Phase 1 was stacked and the bottom-up merge stranded three PRs' commits
   on feature branches (recovered by catch-up PR #81). One ticket = one branch = one
   PR against `main`.
3. Branch naming: `mobile/p<phase>-<n>-<slug>` (e.g. `mobile/p4-3-platform-detect`).
4. **PR body must contain `Closes #<issue>`** (GitHub only auto-closes issues from
   PRs that merge into the default branch — another reason PRs must target `main`).
5. Commit messages: Conventional Commits, end with the trailer
   `Co-Authored-By: Claude <noreply@anthropic.com>`.
6. The user merges PRs; never merge or delete branches yourself. **After the user
   says they merged**, verify before continuing:
   `git fetch origin && git merge-base --is-ancestor <branch-tip-sha> origin/main`
   and check the issue actually closed. Do not assume.

### Verification before every push

Run all of these in the worktree; every ticket must leave desktop fully working:

```bash
# Rust (see toolchain note below for the PATH line)
cd src-tauri && cargo test         # all green, no NEW warnings
cd .. && bun run typecheck         # clean
bun run test                       # all green
```

Baselines as of #70 merged (2026-07-19): **328 Rust tests**, **200 frontend tests /
24 files** (grows as tickets add tests).

State in the PR body that the `bun tauri dev` manual sanity check is pending — the
user runs it before merging (working agreement from #76). Don't run `bun tauri dev`
yourself unattended: the dev build points at the user's real DB and can trigger a
real Drive sync.

### Machine/toolchain gotchas

- **`/snap/bin/cargo` is broken** (cargo 1.72, cannot read the v4 `Cargo.lock`).
  Use the rustup toolchain explicitly:
  `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`
  (The root `CLAUDE.md` claim that plain `cargo` works is stale — `~/.cargo/bin` is empty.)
- Node 24 required (`nvm use`); package manager is **Bun** (`bun add`, `bun run …`).
- **Every fresh worktree needs `bun install` before `bun run typecheck`/`bun run test`**
  — `node_modules` isn't shared across worktrees (hit repeatedly across #63's session
  and the #67–#70 session; `tsc` fails with `Cannot find module 'qrcode'` or similar
  until you run it).
- First `cargo test` in a fresh worktree recompiles everything (~minutes); later runs are fast.
- Rust tests must stay in-memory SQLite, no disk fixtures (`src-tauri/CLAUDE.md`).

## 4. Code map for the remaining tickets

Post-restructure layout (differs from what older docs may imply):

- `src-tauri/src/lib.rs` — all Tauri commands, `AppState`, `lock_db!`,
  `resolve_data_dir` (two `#[cfg]` variants), `run()` with builder + `setup()`.
  The `setup()` closure resolves the data dir, opens the DB, manages `AppState`,
  spawns `#[cfg(desktop)]` tasks (HTTP server :59432, cover scanner, autosave),
  wires the open-pull (both platforms — this becomes the mobile launch pull in #70)
  and the `#[cfg(desktop)]` close-push handler.
- `src-tauri/src/gdrive.rs` — `BackupConfig`, `BackupEngine`, `SyncMode`,
  `sync_once` (pure, tested against `FakeDrive` in `sync_tests`), pairing:
  `PairingPayload`, `export_pairing`, `import_pairing`, engine methods
  `export_pairing_code`/`apply_pairing`, tests in `pairing_tests`.
  `apply_pairing` sets `enabled = true`, `interval_min = 0`, clears
  `last_sync`/`last_pushed_digest` → next sync cycle pulls.
- `src/components/BackupSettingsPage.tsx` — Drive settings page incl. the pairing
  export section. Its `BackupStatus` TS interface mirrors `gdrive::BackupStatus`.
- `src/components/SettingsLayout.tsx` — full-page settings pattern (breadcrumb + back).
- Frontend conventions: `src/CLAUDE.md` (CSS variables, button patterns, icons,
  testing rules). Component tests mock `invoke` via
  `vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))` — see
  `BackupSettingsPage.test.tsx` and `SettingsPage.test.tsx` for the pattern.

## 5. Per-ticket notes beyond the issue bodies

**#61 (toolchain)** — user must be present (system installs). This is a Linux
machine; the epic's `~/.zshrc`/`~/Library` snippets in root `CLAUDE.md` are
macOS-flavored — adapt paths for Linux (`~/Android/Sdk`, shell rc accordingly) and
write the Linux steps into `docs/` as part of the ticket. Pin NDK r26+ (rusqlite
`cc` cross-compile is the classic failure). Add rustup targets:
`aarch64-linux-android` (+ others `tauri android init` expects).

**#62 (scaffold)** — `tauri android init` generates `src-tauri/gen/android/`.
Config fixes called out by the epic: Vite dev server must listen on the network for
device testing (`host: true` or `TAURI_DEV_HOST`), CSP `img-src` must allow
arbitrary `https:` origins for cover images (latent desktop bug — fix applies to
both platforms), window title check. Read the issue body for the full list.

**#63 (platform detection)** — keep it dead simple: a `src/platform.ts` helper
(userAgent-based, e.g. `/android|iphone|ipad/i`) and a split in `src/main.tsx`
choosing `MobileApp` vs desktop `App`. Desktop bundle must not regress; `App.tsx`
untouched.

**#64–#68 (mobile shell)** — build inside `src/mobile/`. Read-only: bookmark tap →
`invoke('open_url', …)` (works on Android via `tauri-plugin-opener` since #56).
Reuse the components listed in §2.5. For `readOnly` on `BookmarkGrid`/`BookmarkCard`
(#68): additive prop defaulting to `false`, no visual/behavior change on desktop —
existing tests must stay green. List view (#67): virtualize with
`@tanstack/react-virtual` (already a dependency). Every new component gets a
`*.test.tsx` next to it (happy-dom; avoid layout/measurement assertions —
virtualizers need their measure functions mocked or the test asserts on the
virtualizer's props, see existing `BookmarkList.test.tsx`).

**#69 (pairing import UI)** — mobile settings page that takes a pasted
`ferrico-pair:v1:…` string and calls `backup_import_pairing`, then triggers a pull
and shows `backup_status`. The command exists and works on desktop too — you can
test the full import flow in a desktop build/tests without a device.

**#70 (pull lifecycle)** — the launch pull already runs on both platforms (the
delayed open-pull spawn in `setup()`). This ticket adds foreground-resume pull
(Tauri `Resumed`/visibility event on Android) and a manual refresh command path.
Keep every path funneling through `run_sync` — mode selection stays compile-time.

**#71 (device E2E)** — needs the user with a physical device. Checklist prepared:
`docs/mobile-v1-device-verification.md` — a step-by-step expansion of the issue's
checklist mapped to this repo's actual UI/commands (pairing flow, `adb` snippets for
airplane mode and WebView version, where to check the Drive file's `modifiedTime` for
the pull-only proof). Run it on the device, don't attempt to automate the device parts.

## 6. Definition of done (every ticket)

1. Issue body's task checklist satisfied; acceptance criteria met.
2. `cargo test` + `bun run typecheck` + `bun run test` green in the worktree; no new
   Rust warnings.
3. Desktop behavior unchanged unless the issue says otherwise.
4. New logic has tests next to the code it covers (Rust: `#[cfg(test)]` module;
   frontend: `*.test.tsx`).
5. PR opened against `main` with `Closes #<N>`, verification results, and the
   pending `bun tauri dev` manual-check note. Worktree removed after opening.
6. After the user merges: `git fetch`, ancestor-check the branch tip against
   `origin/main`, confirm the issue closed, then move to the next ticket.

## 7. On-device debugging (learned the hard way)

From the first real-device session (Pixel, Android 16). These cost several rebuild
cycles to work out; they're cheap to re-apply.

**Instrument before theorizing.** Every mobile `invoke` call site swallows errors with
`.catch(() => {})`, so a failing command looks identical to a dead click handler. Don't
guess — temporarily log the rejection and read it:

```tsx
invoke('open_url', { url })
  .then(() => console.log('[ferrico] OK'))
  .catch((e) => console.log('[ferrico] FAILED:', JSON.stringify(e)))
```

Frontend `console.log` surfaces in logcat under the `Tauri/Console` tag:

```bash
adb logcat -c                                   # clear, then reproduce on device
adb logcat | grep -iE "Tauri/Console|ferrico"
```

One instrumented run gives the exact error string. Three rounds of plausible-looking
blind fixes gave nothing.

**Check the dev server is actually connected first.** The device can silently sit on
stale code showing `[vite] server connection lost. Polling for restart...`, which makes
a correct fix look broken. Confirm before drawing conclusions, and force a reconnect
with:

```bash
adb logcat -d | grep '\[vite\]'                 # look for "connected" vs "connection lost"
adb shell am force-stop com.ferrico.app         # then relaunch from the launcher
```

Frontend edits hot-reload over HMR (no rebuild); **Rust or capability changes always
need a full `bun run android:dev` rebuild.**

**Prefer a plugin's `…Ext` trait method over its free function.** Several Tauri plugins
expose both, and the free function is often desktop-only. `tauri_plugin_opener::open_url`
has no `#[cfg(mobile)]` variant — it shells out to `xdg-open`/`gio`/`kde-open` and fails
with `ENOENT (os error 2)` on Android. Only `OpenerExt::open_url` (via `app.opener()`)
routes to the Android `Intent.ACTION_VIEW` handler. See
[plugins-workspace#2913](https://github.com/tauri-apps/plugins-workspace/issues/2913).

**Reusing a desktop component on touch? Audit its CSS for `touch-action`.** `.bm-card`
carried `touch-action: none` for drag-and-drop, which made the read-only grid completely
unscrollable under touch. Same trap the epic already called out for `BookmarkRow`.
Read-only mobile surfaces want `touch-action: manipulation`, and should
`preventDefault()` the `contextmenu` event so Chromium's native long-press menu doesn't
eat the tap.

**Getting the pairing code onto a device:** focus the pairing textarea, then
`adb shell input text '<code>'`. The base64 alphabet is shell-safe in single quotes.
⚠️ That code contains live Google Drive credentials — never paste it into a chat,
screenshot, or issue.

**Don't run `android:build` while `android:dev` is live** — concurrent Gradle
invocations on the same project directory contend and can disrupt the dev session.
