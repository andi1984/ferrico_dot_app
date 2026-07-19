<div align="center">

# Ferrico

**A fast, local-first bookmark manager for desktop — with a read-only Android companion.**

Organize, search, and reclaim your bookmarks — all stored locally, no account, no cloud.

[![CI](https://github.com/andi1984/ferrico_dot_app/actions/workflows/ci.yml/badge.svg)](https://github.com/andi1984/ferrico_dot_app/actions/workflows/ci.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB.svg)](https://tauri.app)

</div>

> **Status:** early but functional (v0.x). The desktop app works on macOS, Linux, and Windows. The **Android app is read-only** — browse and open your library on a phone; adding and editing stay on the desktop. It's new and has had less real-world use than the desktop app.

---

## Why Ferrico?

Most bookmark managers either lock your data in a browser or sync it to someone else's server. Ferrico keeps everything in a local SQLite database on your machine. It's a small, native desktop app built with [Tauri](https://tauri.app) — fast to launch, light on memory, and yours to keep.

## Features

- 📚 **Local-first** — all data lives in a local SQLite file. No account, no telemetry, and no cloud unless you opt into Drive backup.
- 🗂️ **Organize** — hierarchical folders, tags with counts, and an Inbox tray for triage.
- 🔍 **Fast fuzzy search** — search across title, URL, and page body, even in large libraries (virtualized list/grid).
- ♻️ **Trash with retention** — deleted bookmarks move to a bin and are kept for 30 days before purging.
- 🔁 **Import / export** — symmetric support for JSON, Netscape HTML, OPML, and CSV.
- ☁️ **Optional cloud sync** — mirror your library through *your own* Google Drive folder to keep multiple machines in sync. No Ferrico server; `drive.file` scope only (Ferrico sees only the files it creates).
- 🧹 **Duplicate detection** — find and merge duplicate bookmarks.
- 🔗 **Broken-link detection** — scan your library and surface dead links.
- 🤖 **Optional AI features** — natural-language search, automatic Inbox sorting, CSV column mapping, and duplicate-resolution suggestions, powered by your local [Claude CLI](#ai-features-optional).
- 🧩 **Browser extension** — save the current page straight to your library (Chrome/Firefox).
- 📱 **Android companion** — browse, filter, and search your library on a phone, and tap through to the system browser. Read-only, and paired from the desktop so your phone never needs a Google sign-in. See [Android app](#android-app).
- ⌨️ **Keyboard-friendly** — shortcuts, context menus, and accessible controls throughout.

## Tech stack

| Layer | Technology |
|---|---|
| Frontend | React 19, TypeScript, Vite 5, Tailwind CSS 4 |
| Backend | Tauri 2 (Rust), SQLite via `rusqlite` |
| Targets | macOS, Linux, Windows — plus Android (read-only) from the same codebase |
| Extension bridge | Local `axum` HTTP server bound to `127.0.0.1:59432` |
| Package manager | [Bun](https://bun.sh) |

## Getting started

### Prerequisites

- [**Bun**](https://bun.sh) — package manager and task runner
- **Node.js 24+** — the Tauri CLI relies on syntax that breaks on older Node (see `.nvmrc`)
- **Rust** (stable toolchain) + Cargo — [rustup.rs](https://rustup.rs)
- **Tauri system dependencies** for your OS — follow the [Tauri prerequisites guide](https://tauri.app/start/prerequisites/)
- *(Optional)* the **`claude` CLI** on your `PATH` for [AI features](#ai-features-optional)

### Run the app

```bash
bun install        # install JS dependencies
bun tauri dev      # start Vite + the Tauri desktop app
```

On first run, Cargo will compile the Rust backend — this takes a few minutes. Subsequent runs are fast.

### Build a release binary

```bash
bun tauri build    # produces a native installer/bundle for your platform
```

## AI features (optional)

Ferrico's AI features shell out to a locally installed [`claude` CLI](https://docs.claude.com/en/docs/claude-code). They are **entirely optional** — every core feature (adding, organizing, searching, importing) works without it.

- Install and authenticate the `claude` CLI, and make sure it's on your `PATH`.
- AI features then become available: AI search, Inbox auto-sort, CSV mapping, and duplicate resolution.

**Privacy note:** core bookmark management never leaves your machine. When you explicitly invoke an AI feature, the relevant bookmark metadata (e.g. titles and URLs) is passed to your local `claude` CLI, which sends it to Anthropic to generate a response. Nothing is sent unless you trigger an AI action.

## Browser extension

A companion extension lives in [`extension/`](extension/). It saves the current tab to your Ferrico library by talking to the local HTTP server the app exposes on `127.0.0.1:59432` (loopback only — never exposed to the network). When you open it, it also tells you at a glance whether the page is **already in your library** and surfaces any other pages you've saved from the same site.

**Load it unpacked:**

- **Chrome / Edge:** open `chrome://extensions`, enable *Developer mode*, click *Load unpacked*, and select the `extension/` folder.
- **Firefox:** open `about:debugging` → *This Firefox* → *Load Temporary Add-on*, and select `extension/manifest.json`.

Then open the extension's **Options** page and paste the API token shown in Ferrico's settings. The desktop app must be running for the extension to save bookmarks.

## Android app

Ferrico ships the same Tauri app to Android as a **read-only** client: browse your whole library, filter by folder or tag, fuzzy-search, switch between list and grid (with cover images), and tap a bookmark to open it in your system browser. Adding, editing, and deleting stay on the desktop.

Read-only isn't just a UI choice — **the mobile build physically cannot write to your backup.** Sync mode is selected at compile time (`cfg!(mobile)`), so no code path in an Android binary can push to Drive, and the mobile UI performs zero database mutations of its own.

**Pairing, not sign-in.** Rather than running a Google OAuth flow on your phone, a connected desktop exports a pairing code (QR or copy-paste string) under **Settings → Cloud Backup → Pair a mobile device**. Paste it into the phone's settings and it adopts the same Drive folder, then pulls. Your phone never signs in to Google.

> ⚠️ That pairing code contains your Drive credentials. Transfer it over a channel you trust and keep it out of chats, screenshots, and backups.

The phone pulls on launch, on returning to the foreground (rate-limited), and on manual refresh.

**Install:** grab the APK from a [release](https://github.com/andi1984/ferrico_dot_app/releases) and sideload it, or build it yourself (see below). Release APKs are currently **debug-signed** — fine for sideloading, not Play-Store ready.

**Build it yourself:** one-time Android SDK/NDK setup is documented in [`CLAUDE.md`](CLAUDE.md#android-setup-one-time), then:

```bash
bun run android:init   # once, if src-tauri/gen/android is missing
bun run android:dev    # run on an emulator or USB-connected device
bun run android:build  # produce an APK
```

iOS is not supported yet.

## Where your data lives

Ferrico stores everything in a single SQLite database under your platform's data directory:

| Platform | Location |
|---|---|
| Linux | `~/.local/share/ferrico/ferrico.db` |
| macOS | `~/Library/Application Support/ferrico/ferrico.db` |
| Windows | `%APPDATA%\ferrico\ferrico.db` |

To start fresh, quit Ferrico and delete that file (or use **Clear all data** in the app's danger zone).

## Cloud backup & sync (optional)

Ferrico is local-first, but you can optionally mirror your library through **your own Google Drive** so several machines stay in sync. There is no Ferrico-operated server: backups live in a folder *in your Drive*, written by an OAuth app *you* create, and only that app can read them.

**How it works**

- The lossless JSON export is stored as a single `ferrico-backup.json` in a Drive folder you pick.
- **On launch**, Ferrico pulls the latest snapshot; **before close** (and optionally on a timer) it pushes the current state.
- Conflict resolution is a **per-record merge**, not blind last-write-wins. Snapshots are unioned by row UUID and reconciled individually (`updated_at`, with deletes as tombstones), so edits to *different* bookmarks from two machines both survive. The merge is commutative — both machines converge on the same result regardless of who syncs first — and the result is normalized afterwards, collapsing duplicate folders/tags and re-homing anything pointing at a deleted container.
- Your **phone is pull-only**, enforced at compile time, so pairing a device can never overwrite your library. See [Android app](#android-app).
- The OAuth scope is [`drive.file`](https://developers.google.com/drive/api/guides/api-specific-auth): Ferrico can only ever see files it created, never the rest of your Drive. Your client ID, secret, and refresh token are stored locally in `settings.json`, never transmitted to anyone but Google.

Setup needs a one-time, free Google OAuth client. Full walkthrough (Google Cloud setup, in-app config, multi-machine setup, and troubleshooting) is in **[docs/google-drive-backup.md](docs/google-drive-backup.md)**. Once configured, manage it under **Settings → Cloud Backup**.

## Development

| Command | Description |
|---|---|
| `bun tauri dev` | Full desktop dev (Vite + Tauri) |
| `bun run dev` | Vite dev server only |
| `bun run build` | Vite production build |
| `bun run typecheck` | TypeScript type-check (no emit) |
| `bun run test` | Frontend tests (Vitest) |
| `bun run test:watch` | Frontend tests in watch mode |
| `bun run test:coverage` | Frontend tests with coverage |
| `bun tauri build` | Production desktop build |
| `bun run android:dev` | Android dev build (one-time `bun run android:init` first) |
| `bun run android:build` | Production Android build (APK) |

### Running tests

```bash
# Frontend (Vitest)
bun run test

# Rust backend (in-memory SQLite, no fixtures)
cd src-tauri && cargo test
```

The Rust test suite lives alongside the code it covers (`db.rs`, `io.rs`, `merge.rs`, `gdrive.rs`, …) and runs against in-memory SQLite — CRUD, cascade deletes, validation, search, sync merge, and error types. CI runs both suites plus a type-check on every push and pull request.

The Android build is **not** part of per-commit CI — a cross-compile plus a Gradle run costs ~12 minutes, which is too much per commit. It runs on release tags (so every release gets an APK) and on demand via the **Android build** workflow, which can target any branch.

### Project structure

```
ferrico/
├─ src/                  # React + TypeScript frontend
│  ├─ components/        # UI components (+ colocated *.test.tsx)
│  ├─ mobile/            # Read-only Android shell (MobileApp + friends)
│  ├─ platform.ts        # Picks the mobile vs desktop root at startup
│  ├─ App.tsx            # Desktop root component / layout
│  └─ types.ts           # Shared TS types
├─ src-tauri/            # Rust backend (Tauri 2) — desktop + Android
│  ├─ src/
│  │  ├─ lib.rs          # Tauri commands, HTTP server, mobile entry point
│  │  ├─ main.rs         # Thin desktop shim over lib.rs
│  │  ├─ db.rs           # Pure DB functions + test suite
│  │  ├─ gdrive.rs       # Drive sync engine + device pairing
│  │  ├─ merge.rs        # Per-record sync merge
│  │  ├─ error.rs        # Typed AppError (serialized to the frontend)
│  │  └─ io.rs           # Import/export helpers
│  ├─ gen/android/       # Generated Android/Gradle project (committed)
│  └─ tauri.conf.json    # Tauri 2 config
├─ extension/            # Browser extension (Manifest V3)
└─ .github/workflows/    # CI, Android, release, and PR-report pipelines
```

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) to get started, and note that this project follows a [Code of Conduct](CODE_OF_CONDUCT.md).

## Security

Found a vulnerability? Please report it privately — see [SECURITY.md](SECURITY.md). Don't open a public issue for security problems.

## License

Ferrico is free software, licensed under the **GNU General Public License v3.0 or later** (`GPL-3.0-or-later`). You may use, study, share, and modify it; if you distribute a modified version, you must release your source under the same license. See the full text in [LICENSE](LICENSE).

```
Copyright (C) 2026 Andreas Sander

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.
```

## Acknowledgements

Built with [Tauri](https://tauri.app), [React](https://react.dev), [Vite](https://vitejs.dev), [Tailwind CSS](https://tailwindcss.com), and [rusqlite](https://github.com/rusqlite/rusqlite). Fuzzy search is powered by [nucleo](https://github.com/helix-editor/nucleo).
