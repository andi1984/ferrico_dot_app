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
- **cargo**: the rustup toolchain, not the `snap` package. `/snap/bin/cargo` is often an
  older pinned version that can't read a current-format `Cargo.lock` — if plain `cargo`
  errors on the lockfile, prepend the rustup toolchain explicitly:
  `export PATH="$HOME/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin:$PATH"`.
- **iOS**: blocked here — needs macOS 13+ and Xcode 15.3.

### Android setup (one-time)

**macOS** (Android Studio → SDK Manager → install **Android SDK**, and under SDK Tools
the **NDK (Side by side)**), then add to `~/.zshrc`:

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/$(ls $ANDROID_HOME/ndk | tail -1)"
export PATH="$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
```

**Linux** — same Android Studio SDK Manager install, different default path
(`$HOME/Android/Sdk` instead of `~/Library/Android/sdk`). Steps:

1. Android Studio → SDK Manager → install **Android SDK Platform**, **Android SDK
   Command-line Tools**, **Android SDK Platform-Tools**, and under **SDK Tools** the
   **NDK (Side by side)** — pin a stable r26+ release (e.g. `27.3.13750724`); avoid the
   newest rc/beta line unless you've verified it against `rusqlite`'s bundled-SQLite `cc`
   cross-compile, the classic Android toolchain failure mode. Command-line equivalent:
   ```bash
   sdkmanager --install "ndk;27.3.13750724" "platforms;android-35" \
     "system-images;android-35;google_apis;x86_64" "emulator"
   ```
2. Add to `~/.bashrc` (adjust the NDK version to whatever you installed):
   ```bash
   export ANDROID_HOME="$HOME/Android/Sdk"
   export NDK_HOME="$ANDROID_HOME/ndk/27.3.13750724"
   export JAVA_HOME="/usr/lib/jvm/java-21-openjdk-amd64"   # any JDK 17+ works
   export PATH="$ANDROID_HOME/cmdline-tools/latest/bin:$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
   ```
   Open a new shell (or `source ~/.bashrc`) and confirm: `echo $ANDROID_HOME $NDK_HOME`
   resolve to real paths, and `sdkmanager`/`adb`/`emulator` are on `PATH`.
3. `rustup target add aarch64-linux-android armv7-linux-androideabi i686-linux-android x86_64-linux-android`
4. Create an emulator with a **recent API level** — a stale Android System WebView can't
   render Tailwind 4's modern CSS (`oklch`, `@property`; needs Chromium ≥ ~111). API 35
   `google_apis x86_64` is a safe pick; `pixel_5` is the newest built-in device profile in
   the command-line tools (no `pixel_6`+ profile shipped there as of this writing):
   ```bash
   avdmanager create avd -n ferrico_dev -k "system-images;android-35;google_apis;x86_64" -d pixel_5
   emulator -avd ferrico_dev -no-window -no-audio -no-boot-anim &   # headless boot
   adb devices   # wait for "device" (not "offline") — first boot takes a minute or two
   ```
   A real device with USB debugging enabled works just as well — `adb devices` should
   list it once plugged in and authorized.
5. Run `bun run android:init` once, then `bun run android:dev`.

## Tauri config notes

- `tauri.conf.json` uses the Tauri 2 schema: `identifier`, `devUrl`, `productName` at top level.
- `devUrl` → Vite dev server at `http://localhost:5173`.
- `beforeDevCommand` is `bun run dev` (Vite only, not Tauri — avoids an infinite loop).
