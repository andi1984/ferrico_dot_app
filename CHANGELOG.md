# Changelog

## [0.14.2] - 2026-07-19




## [0.14.1] - 2026-07-19




## [0.14.0] - 2026-07-19

### Features

- MobileSettings — pairing import UI (P5.1)
- readOnly mode for BookmarkGrid/BookmarkCard (P4.8)
- foreground-resume pull (P5.2)
- virtualized read-only list view (P4.7)
- FilterDrawer — folders/tags navigation (P4.6)
- MobileHeader — search, view toggle, refresh with sync status
- MobileApp shell — state, data loading, theme, events
- tauri android init scaffold + config fixes
- platform detection + mobile entry split
- desktop pairing UI (QR + copy string) in Drive settings
- pairing payload export/import + backup commands
- add SyncMode { Full, PullOnly } to the sync engine

### Bug Fixes

- restore touch scrolling over readOnly grid cards
- open_url via OpenerExt so links work on Android
- suppress native long-press context menu blocking taps
- stop watching src-tauri, avoid inotify watch-limit crash


## [0.13.0] - 2026-07-05

### Features

- redesign settings as dedicated scrollable pages

### Bug Fixes

- redesign bookmark delete hover affordance
- harden snapshot merge against cross-machine duplication


## [0.12.1] - 2026-06-21

### Bug Fixes

- prevent FOREIGN KEY failure when applying Drive snapshot


## [0.12.0] - 2026-06-20

### Features

- accept v2 Drive sync snapshot in JSON import


## [0.11.1] - 2026-06-20

### Bug Fixes

- never erase Drive backup from an empty or unreadable remote


## [0.11.0] - 2026-06-18

### Features

- show existing bookmarks for the current URL/site
- tag autocomplete combobox with suggestions


## [0.10.0] - 2026-06-15

### Features

- tombstones for folders/tags + sync snapshot bridge
- add pure per-record merge engine

### Bug Fixes

- merge-reconcile Drive sync instead of blind overwrite


## [0.9.0] - 2026-06-14

### Features

- Google Drive cloud backup & multi-machine sync


## [0.8.0] - 2026-06-13

### Features

- auto-fetch OG/Twitter cover images for bookmarks
- add ISO date to export filenames

### Bug Fixes

- add cover_url to makeBookmark test factory


## [0.7.0] - 2026-06-05

### Features

- sync download links + version from latest GitHub release

### Bug Fixes

- remove duplicate macOS window chrome, merge into header
- replace placeholder green square with copper bookmark mark
- show real app version in sidebar instead of hardcoded v0.1


## [0.6.0] - 2026-06-05

### Features

- add single-page landing site for Ferrico

### Bug Fixes

- match app branding — text wordmark, no invented icon
- stop two more sources of false-positive broken links
- use browser-shaped User-Agent to avoid false broken links

### Performance

- batch health write-back, chunk tag IN-clause, drop id clones
- isolate search box + faster sort to cut typing/navigation lag


## [0.5.0] - 2026-06-04

### Features

- add AI chat panel for natural language bookmark search
- subfolders — nested folder tree, drag-to-reparent, path imports

### Bug Fixes

- remove unused IconMessageCircle import
- use rfd native file picker on Linux/WebKitGTK
- remove setPointerCapture that broke drag in WKWebView
- add mouse-event fallback for WebKitGTK drag


## [0.4.0] - 2026-05-31

### Features

- clickable tags open the tag detail view
- context-aware tag suggestions and page preview

### Performance

- cut click latency with split loads, single sidebar query, optimistic UI


## [0.3.0] - 2026-05-28

### Features

- fuzzy search for bookmarks (title, URL, body)
- detect and surface broken bookmark links

### Bug Fixes

- 5xx responses are reachable, not broken links
- review findings + caching + false-positive protection


## [0.2.6] - 2026-05-26

### Bug Fixes

- enable bundling in tauri.conf.json


## [0.2.5] - 2026-05-26

### Bug Fixes

- build Tauri directly with bun, drop tauri-action wrapper


## [0.2.4] - 2026-05-26

### Bug Fixes

- upload Tauri bundle artifacts via tauri-action output paths


## [0.2.3] - 2026-05-25

### Bug Fixes

- glob bundle dir directly instead of relying on tauri-action artifactPaths (empty without signing keys)


## [0.2.2] - 2026-05-25

### Bug Fixes

- guard upload-artifact when tauri-action produces no artifactPaths


## [0.2.1] - 2026-05-25

### Bug Fixes

- generate all platform icons from source PNG; fix fromJSON empty-output crash in release workflow


## [0.2.0] - 2026-05-25

### Features

- add automated release workflow and CHANGELOG
- batch AI processing with cancellation and compact prompt
- add duplicate bookmark detection and removal
- unified Import button supporting all formats
- symmetric import/export in JSON, Netscape HTML, and OPML
- tag autocomplete with counts + hierarchical folder picker
- implement drag-and-drop bookmark repositioning
- move deleted bookmarks to bin with 30-day retention
- add Inbox as default tray with AI-powered sorting
- grid/list view toggle and sort controls for bookmarks
- CSV import with AI mapping, danger zone, and virtual scrolling
- add folders and tags to browser extension popup
- reload bookmarks when browser extension adds a new one
- emit bookmark-added event when extension adds a bookmark
- accessibility, keyboard shortcuts, context menus, link opening
- add GitHub Actions CI and PR test report
- extract db layer, typed errors, 36-test suite
- redesign bookmark manager with editorial dark aesthetic
- bootstrap bookmark manager (local-first)
- bootstrap Tauri 2 + React 19 + TypeScript app

### Bug Fixes

- move fail-fast under strategy in release build matrix
- fix char-boundary panic when importing HTML with multi-byte chars
- add missing onDeduplicate prop to SettingsModal test renders
- pipe prompt via stdin to avoid E2BIG on large prompts
- surface real error from Claude — check exit code, show message
- fix settings button — drop redundant onClose() call
- fix CSV drag-drop — pass path to wizard, pre-load content
- use Tauri native onDragDropEvent for cross-platform drops
- fix drag-and-drop on Linux/WebKitGTK
- move deleted_at index creation after migration
- resolve all clippy warnings, wire validators, add CSV export + full import/export UI
- solid panel background + theme switcher
- rewrite bookmark drag-drop with pointer events
- make drag-and-drop actually work in Tauri WKWebView
- correct invoke param name and dragLeave child flicker
- add missing Sidebar drag-and-drop implementation
- exclude binned/foldered bookmarks from inbox count and sort
- resolve conflicts with inbox and bin features
- add deleted_at and binCount to test fixtures
- correct three false-positive failures in pr-report
- correct rust-cache workspaces param and TS error check
- make inboxCount optional in SidebarProps, add Inbox tests
- grant core:default capability so listen() can register
- skip saving ping requests from browser extension
- install Tauri Linux system deps before cargo test
- resolve borrow-after-drop in query_map block tail expressions
- debounce search, error handling, total count, favicon, export URL leak
- error handling, security, data correctness, OPML nesting

### Performance

- indexes, SQL search, unified query builder, full error propagation
- rAF-throttle pointermove state updates
- virtualize bookmark grid and move hover to CSS


All notable changes documented here. Format based on [Keep a Changelog](https://keepachangelog.com).

