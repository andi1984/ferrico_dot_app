# Ferrico

A local-first bookmark manager. **Tauri 2 + React 19 + TypeScript + Tailwind CSS 4**,
targeting desktop (macOS/Linux/Windows) and Android. Bookmarks live in local SQLite;
optional Google Drive sync; an AI panel powered by the local `claude` CLI; and a
companion browser extension.

## Stack

- **Frontend**: React 19, TypeScript, Vite 5, Tailwind CSS 4, Vitest + happy-dom
- **Backend**: Tauri 2 (Rust), SQLite via rusqlite, state = `Arc<Mutex<Connection>>`
- **Package manager**: Bun

## Repo map

| Path | What | Detail doc |
|---|---|---|
| `src/` | React frontend (components, events, hooks) | `src/CLAUDE.md` |
| `src-tauri/` | Rust backend (DB, commands, sync, HTTP server) | `src-tauri/CLAUDE.md` |
| `extension/` | MV3 browser extension (popup + options) | `extension/CLAUDE.md` |
| `site/` | Landing page deployed to GitHub Pages | — |
| `docs/` | Contributor docs (e.g. `google-drive-backup.md`) | — |

> **Nested `CLAUDE.md` files** — this root file is always in context. The per-directory
> files above are loaded automatically when you read or edit files in that directory, so
> keep deep backend/frontend detail there and keep this file a lean overview.

## Running the app

Node 24+ is required — the Tauri CLI uses optional chaining that breaks on older Node.

```bash
nvm use              # switches to Node 24 (see .nvmrc)
source ~/.cargo/env  # if cargo isn't already on PATH
bun tauri dev        # starts Vite + the Tauri desktop app
```

## Scripts

| Command | Description |
|---|---|
| `bun run dev` | Vite dev server only (used internally by Tauri) |
| `bun run build` | Vite production build |
| `bun run typecheck` | `tsc --noEmit` |
| `bun run test` | Frontend tests once (Vitest) |
| `bun run test:watch` | Frontend tests in watch mode |
| `bun tauri dev` | Full desktop dev (Vite + Tauri) |
| `bun tauri build` | Production desktop build |
| `bun run android:dev` / `android:build` | Android dev / production build |

(One-time Android setup lives in `docs/`-style notes at the bottom of this file.)

## Development workflow

- **Always work in a git worktree** under `.claude/worktrees/<branch-name>` — never as a
  direct child of `~/dev` or a sibling of the project. Remove the worktree once the PR is
  open and pushed; the branch then lives normally on `origin`.

  ```bash
  git worktree add .claude/worktrees/<branch> -b <branch>   # start
  git worktree remove .claude/worktrees/<branch>            # once PR is open
  ```
- Commit/push only when asked. End commit messages with the `Co-Authored-By` trailer.

## Platform notes

- **Data dir** (`dirs::data_dir()`): macOS `~/Library/Application Support/ferrico/ferrico.db`,
  Linux `~/.local/share/ferrico/ferrico.db`. `settings.json` sits beside the DB.
- **cargo** on this machine is the rustup default (`~/.cargo/bin/cargo`); just run `cargo`.
- **iOS**: blocked here — needs macOS 13+ and Xcode 15.3.

### Android setup (one-time)

1. Android Studio → SDK Manager → install **Android SDK**, and under SDK Tools the **NDK (Side by side)**.
2. Add to `~/.zshrc`:

   ```bash
   export ANDROID_HOME="$HOME/Library/Android/sdk"
   export NDK_HOME="$ANDROID_HOME/ndk/$(ls $ANDROID_HOME/ndk | tail -1)"
   export PATH="$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
   ```

3. Run `bun run android:init` once, then `bun run android:dev`.

## Tauri config notes

- `tauri.conf.json` uses the Tauri 2 schema: `identifier`, `devUrl`, `productName` at top level.
- `devUrl` → Vite dev server at `http://localhost:5173`.
- `beforeDevCommand` is `bun run dev` (Vite only, not Tauri — avoids an infinite loop).
