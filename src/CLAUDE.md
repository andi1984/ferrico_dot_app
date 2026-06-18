# Ferrico — React frontend (`src/`)

React 19 + TypeScript + Tailwind 4. Talks to the Rust backend via Tauri `invoke` and
listens to backend events through `events.ts`.

## Layout

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

Long lists use `@tanstack/react-virtual` for virtualization.

## CSS variables (defined in `src/index.css`)

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

## Button patterns

All header buttons share `height: 32, fontSize: 12, fontWeight: 500, rounded-lg`.

- **Default**: `background: var(--input-bg)`, `border: 1px solid var(--border-soft)`, `color: var(--text-1)`
  - Hover: `background: var(--btn-hover-bg)`
- **Accent outline** (AI Sort, active toggles): `border: 1px solid var(--accent)`, `color: var(--accent)`
  - Hover: `background: var(--accent-dim)`
- **Filled accent** (Add button): `.btn-accent` CSS class
- **Danger**: `color: var(--red)`, hover changes `borderColor` to `var(--red)`

Never use `color: var(--text-2)` on a button — it reads as disabled.

## Icons

All icons live in `src/components/icons.tsx`, accept `size?: number`, set `aria-hidden="true"`.
Common sizes: 13px (header buttons), 14px (sidebar), 16px (default).

## Skeleton / loading

`<LoadingSkeleton>` (14 `<RowSkeleton>` rows) shows while `bookmarks === null` (first load).
After first load, stale cached rows paint instantly; fresh data reconciles silently.

## Backend events (`events.ts`)

`events.ts` centralizes Tauri event subscriptions; each `subscribeTo*` returns an
`UnlistenFn`. Current channels: bookmark-added, health-check progress, cover-updated, and
backup sync (`start`/`done`/`error`). Add new event wiring here, not inline in components.

## AI features

- `run_claude(prompt)` — calls the local `claude` CLI via stdin, default model.
- `run_claude_model(prompt, model)` — same but passes `--model`; use
  `claude-haiku-4-5-20251001` for cost-sensitive tasks.
- Prompt format: compact pipe-delimited lines, minimal system instructions, JSON-only response.
- Always extract JSON with the `extract_json()` helper (strips markdown fences).
- AI search panel: `src/components/AiChatPanel.tsx` mounts right of the main column and sets
  an `aiFilter: Set<string>` overlay on `sortedBookmarks`.

## Testing

Vitest + `@testing-library/react` on **happy-dom** (not jsdom). Test files sit next to
components (`*.test.tsx`), ~16 files. Setup in `test-setup.ts`, helpers in `test-utils.ts`.

```bash
bun run test          # once
bun run test:watch    # watch mode
bun run test:coverage # v8 coverage
```

happy-dom has known quirks vs. a real browser — prefer `@testing-library` queries and
`user-event` over manual DOM poking, and avoid asserting on layout/measurement APIs.
