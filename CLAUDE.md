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

- State lives in `AppState` with `Arc<Mutex<Connection>>` (SQLite via rusqlite)
- Data persisted to `~/.local/share/ferrico/ferrico.db` (via `dirs::data_dir()`)
- All DB logic lives in `src-tauri/src/db.rs` — pure functions taking `&Connection`
- Tauri commands in `main.rs` are thin wrappers: lock mutex → call `db::*` function → return
- Error type: `AppError` in `src-tauri/src/error.rs` — discriminated union `#[serde(tag = "name")]`
  serializes as `{ name: "Db" | "Lock" | "NotFound" | "Validation", message: "..." }` to frontend

## Testing

The Rust test suite lives in `src-tauri/src/db.rs` (`#[cfg(test)]` module). All tests use
in-memory SQLite — no fixtures, no disk state.

```bash
# System `cargo` may be old (snap). Use the rustup one:
~/.rustup/toolchains/stable-x86_64-unknown-linux-gnu/bin/cargo test
```

Current coverage: 36 tests, all DB operations covered (CRUD, cascade deletes, validation,
search, OPML export, error types).

To measure coverage with llvm-cov:
```bash
cargo install cargo-llvm-cov
cargo llvm-cov --html   # opens htmlcov/index.html
```

### Adding new commands

1. Add a pure `db_*` function in `db.rs` taking `&Connection` + write tests there
2. Add a thin Tauri command wrapper in `main.rs` using the `lock_db!` macro
3. Register it in `invoke_handler!` in `main()`
