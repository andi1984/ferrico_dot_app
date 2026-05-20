# Ferrico

Tauri 2 + React 19 + TypeScript + Tailwind CSS 4 app targeting desktop and Android.

## Stack

- **Frontend**: React 19, TypeScript, Vite 5, Tailwind CSS 4
- **Backend**: Tauri 2 (Rust), state managed via `Mutex<Vec<Todo>>`
- **Package manager**: Bun

## Running the app

Node 24+ is required — the Tauri CLI uses optional chaining which breaks on older Node.

```bash
nvm use           # switches to Node 24 (see .nvmrc)
source ~/.cargo/env  # if cargo isn't in PATH
bun tauri dev     # starts Vite + Tauri desktop app
```

## Scripts

| Command | Description |
|---|---|
| `bun run dev` | Vite dev server only (used internally by Tauri) |
| `bun run build` | Vite production build |
| `bun tauri dev` | Full desktop dev (starts Vite + Tauri) |
| `bun tauri build` | Production desktop build |
| `bun run android:init` | One-time Android project init |
| `bun run android:dev` | Android dev build |
| `bun run android:build` | Android production build |

## Android setup (one-time)

1. Open Android Studio → SDK Manager → install **Android SDK**
2. Under SDK Tools, install **NDK (Side by side)**
3. Add to `~/.zshrc`:

```bash
export ANDROID_HOME="$HOME/Library/Android/sdk"
export NDK_HOME="$ANDROID_HOME/ndk/$(ls $ANDROID_HOME/ndk | tail -1)"
export PATH="$ANDROID_HOME/emulator:$ANDROID_HOME/platform-tools:$PATH"
```

4. Run `bun run android:init` once, then `bun run android:dev`

## iOS

Blocked on this machine — requires macOS 13+ and Xcode 15.3. Current OS is macOS 12.

## Tauri config notes

- `tauri.conf.json` uses Tauri 2 schema: `identifier`, `devUrl`, `productName` at top level
- `devUrl` points to Vite dev server at `http://localhost:5173`
- `beforeDevCommand` is `bun run dev` (runs Vite only, not Tauri — avoids infinite loop)

## Rust backend

- State lives in `AppState` with a `Mutex<Vec<Todo>>` and a `data_dir: PathBuf`
- `save_todos` takes `(data_dir, todos)` directly — do NOT pass `&State` to avoid mutex deadlock
- Data persisted to `~/Library/Application Support/ferrico/todos.json` (via `dirs::data_dir()`)
