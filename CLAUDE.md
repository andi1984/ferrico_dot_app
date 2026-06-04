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

## UI conventions

### Layout

```
App (flex col, full height)
  └─ flex row (flex-1)
       ├─ Sidebar (fixed width, collapsible)
       ├─ Main column (flex-1 flex-col)
       │    ├─ error bar (conditional)
       │    ├─ header (flex row, search + controls)
       │    └─ main content (flex-1, scrollable list/grid)
       └─ AiChatPanel (320px, conditional, right side)
```

### CSS variables (defined in `src/index.css`)

| Variable | Usage |
|---|---|
| `--bg` | Main background |
| `--bg-elevated` | Card / elevated surface |
| `--bg-elev-strong` | Dropdown / popover backgrounds |
| `--header-bg` | Header/sidebar background |
| `--input-bg` | Input + button resting state |
| `--btn-hover-bg` | Button hover state |
| `--border` | Strong border |
| `--border-soft` | Subtle border (buttons, inputs) |
| `--border-dim` | List row separators |
| `--text-1` | Primary text |
| `--text-2` | Secondary text (descriptions, labels) |
| `--text-3` | Tertiary / placeholder |
| `--text-muted` | Disabled / empty state text |
| `--accent` | Brand accent (purple) — buttons, active states, links |
| `--accent-dim` | Accent background tint |
| `--accent-glow` | Focus ring shadow |
| `--red` | Destructive actions |
| `--font-display` | Display font (headings) |

### Button patterns

All header buttons share `height: 32, fontSize: 12, fontWeight: 500, rounded-lg`.

- **Default**: `background: var(--input-bg)`, `border: 1px solid var(--border-soft)`, `color: var(--text-1)`
  - Hover: `background: var(--btn-hover-bg)`
- **Accent outline** (AI Sort, active toggles): `border: 1px solid var(--accent)`, `color: var(--accent)`
  - Hover: `background: var(--accent-dim)`
- **Filled accent** (Add button): `.btn-accent` CSS class
- **Danger**: `color: var(--red)`, hover changes `borderColor` to `var(--red)`

Never use `color: var(--text-2)` on a button — it reads as disabled.

### Icons

All icons live in `src/components/icons.tsx`, accept `size?: number`, set `aria-hidden="true"`.  
Common sizes: 13px (header buttons), 14px (sidebar), 16px (default).

### Skeleton / loading

`<LoadingSkeleton>` (14 `<RowSkeleton>` rows) shown while `bookmarks === null` (first load).  
After first load, stale cached rows paint instantly; fresh data reconciles silently.

### AI features

- `run_claude(prompt)` — calls local `claude` CLI via stdin, default model
- `run_claude_model(prompt, model)` — same but passes `--model` flag; use `claude-haiku-4-5-20251001` for cost-sensitive tasks
- Prompt format: compact pipe-delimited lines, minimal system instructions, JSON-only response
- Always extract JSON with `extract_json()` helper (strips markdown fences)
- AI search panel: `src/components/AiChatPanel.tsx`, mounts right of main column, sets `aiFilter: Set<string>` overlay on `sortedBookmarks`
