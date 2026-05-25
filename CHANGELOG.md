# Changelog

All notable changes documented here. Format based on [Keep a Changelog](https://keepachangelog.com).

## [Unreleased]

### Features

- (dedup) batch AI processing with cancellation and compact prompt
- (dedup) duplicate bookmark detection and removal
- (import) unified Import button supporting CSV, JSON, Netscape HTML, and OPML
- (io) symmetric import/export in JSON, Netscape HTML, and OPML
- (extension) tag autocomplete with counts + hierarchical folder picker
- (bin) deleted bookmarks move to bin with 30-day retention
- (inbox) Inbox as default tray with AI-powered sorting
- (ui) grid/list view toggle and sort controls
- (ui) CSV import with AI field mapping and virtual scrolling
- (ext) folders and tags in browser extension popup
- (ui) live reload when browser extension adds a bookmark
- (ux) accessibility, keyboard shortcuts, context menus, link opening
- drag-and-drop bookmark repositioning
- CI/CD with GitHub Actions and PR test report
- Rust DB layer with typed errors and 36-test suite
- Initial bookmark manager with editorial dark aesthetic

### Bug Fixes

- (io) char-boundary panic when importing HTML with multi-byte chars
- (claude) pipe prompt via stdin to avoid E2BIG on large prompts
- (dedup) surface real Claude error — check exit code, show stderr
- (import) CSV drag-drop — pass path to wizard, pre-load content
- (import) use Tauri native onDragDropEvent for cross-platform drops
- (import) drag-and-drop on Linux/WebKitGTK
- (db) deleted_at index creation order after migration
- (io) clippy warnings, CSV export, full import/export UI wiring
- (modal) solid panel background and theme switcher
- (inbox) exclude binned/foldered bookmarks from inbox count
- (ci) Tauri Linux system deps, rust-cache param, TS error check
- (tauri) core:default capability for listen() registration
- (ui) debounce search, error handling, total count, favicon, export URL

### Performance

- (rust) indexes, SQL search, unified query builder, full error propagation
- (drag) rAF-throttle pointermove state updates
- (grid) virtualize bookmark grid, move hover state to CSS
