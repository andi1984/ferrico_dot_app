<div align="center">

# Ferrico

**A fast, local-first desktop bookmark manager.**

Organize, search, and reclaim your bookmarks — all stored locally, no account, no cloud.

[![CI](https://github.com/andi1984/ferrico_dot_app/actions/workflows/ci.yml/badge.svg)](https://github.com/andi1984/ferrico_dot_app/actions/workflows/ci.yml)
[![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-blue.svg)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-24C8DB.svg)](https://tauri.app)

</div>

> **Status:** early but functional (v0.x). The desktop app works on macOS, Linux, and Windows; Android is a work in progress.

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
- ⌨️ **Keyboard-friendly** — shortcuts, context menus, and accessible controls throughout.

## Tech stack

| Layer | Technology |
|---|---|
| Frontend | React 19, TypeScript, Vite 5, Tailwind CSS 4 |
| Backend | Tauri 2 (Rust), SQLite via `rusqlite` |
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

A companion extension lives in [`extension/`](extension/). It saves the current tab to your Ferrico library by talking to the local HTTP server the app exposes on `127.0.0.1:59432` (loopback only — never exposed to the network).

**Load it unpacked:**

- **Chrome / Edge:** open `chrome://extensions`, enable *Developer mode*, click *Load unpacked*, and select the `extension/` folder.
- **Firefox:** open `about:debugging` → *This Firefox* → *Load Temporary Add-on*, and select `extension/manifest.json`.

Then open the extension's **Options** page and paste the API token shown in Ferrico's settings. The desktop app must be running for the extension to save bookmarks.

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
- Conflict resolution is **full-snapshot last-write-wins**, using Drive's server-side `modifiedTime` as the clock (so it survives clock skew between machines). If two machines edit while *both* are offline, whichever syncs last wins — this is intended for one-machine-at-a-time use, not a CRDT-style merge.
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

### Running tests

```bash
# Frontend (Vitest)
bun run test

# Rust backend (in-memory SQLite, no fixtures)
cd src-tauri && cargo test
```

The Rust test suite lives in `src-tauri/src/db.rs` and covers all database operations (CRUD, cascade deletes, validation, search, OPML export, error types). CI runs both suites plus a type-check on every push and pull request.

### Project structure

```
ferrico/
├─ src/                  # React + TypeScript frontend
│  ├─ components/        # UI components (+ colocated *.test.tsx)
│  ├─ App.tsx            # Root component / layout
│  └─ types.ts           # Shared TS types
├─ src-tauri/            # Rust backend (Tauri 2)
│  ├─ src/
│  │  ├─ main.rs         # Tauri commands + local HTTP server
│  │  ├─ db.rs           # Pure DB functions + test suite
│  │  ├─ error.rs        # Typed AppError (serialized to the frontend)
│  │  └─ io.rs           # Import/export helpers
│  └─ tauri.conf.json    # Tauri 2 config
├─ extension/            # Browser extension (Manifest V3)
└─ .github/workflows/    # CI, release, and PR-report pipelines
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
