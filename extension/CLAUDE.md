# Ferrico — browser extension (`extension/`)

A Manifest V3 browser extension that lets you save the current tab into Ferrico without
opening the desktop app.

## Files

| File | Purpose |
|---|---|
| `manifest.json` | MV3 manifest (name "Ferrico") |
| `popup.html` / `popup.js` | Toolbar popup — add the current page, show existing bookmarks for the URL/site |
| `options.html` / `options.js` | Settings — host/port + API token for the local server |

## How it talks to the app

The extension calls the **local HTTP server** started by the Rust backend
(`start_http_server` in `src-tauri/src/main.rs`). Requests are authenticated with the
**API token** persisted in the app's `settings.json` and entered on the options page.
The desktop app must be running for the extension to work.

When changing the request/response shape, update both sides together: the handlers in
`main.rs` and the fetch calls in `popup.js` / `options.js`.
