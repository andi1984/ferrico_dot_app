# Changelog

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

