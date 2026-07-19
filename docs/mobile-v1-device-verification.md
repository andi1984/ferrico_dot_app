# Mobile v1 — device verification checklist (#71)

Operator checklist for the final end-to-end pass on a real device/emulator,
expanding issue #71's checklist into concrete steps for this repo. Needs a
human with the device — nothing here is automatable, so run it yourself
(or hand it to whoever has the phone).

Prerequisites: Android toolchain set up (`docs/mobile-v1-playbook.md` §"Android
setup"), an AVD or physical device with USB debugging, and a desktop build
already connected to Google Drive with a backup folder selected
(`docs/google-drive-backup.md`).

```bash
adb devices                 # confirm the target is listed as "device"
bun run android:dev         # or android:build + sideload the APK
```

## 1. Pairing (fresh phone install)

- [ ] Desktop: **Settings → Cloud Backup → Pair a mobile device → Show pairing
      code**. Confirm the QR renders and the copy-paste string starts with
      `ferrico-pair:v1:`.
- [ ] Phone: open the app → **Settings** (gear icon) → paste the string into
      the pairing textarea → **Pair this device**.
- [ ] Confirm the dashboard shows the correct `account_email` and
      `folder_name`, and that a first pull happens automatically (empty local
      DB → populated) without touching **Sync now**.
- [ ] Browse: folder/tag filter (drawer), search, list view, grid view
      (covers load), tap a bookmark → opens in the system browser.

## 2. Propagation (desktop → phone)

- [ ] Desktop: add one bookmark, edit one existing bookmark, delete one
      existing bookmark. **Sync now** on desktop.
- [ ] Phone: pull via the header refresh button (don't wait for the 10-minute
      foreground-resume cooldown, see `docs/mobile-v1-playbook.md` §5 #70).
      Confirm all three changes appear, including the delete (tombstone
      merge — the deleted bookmark disappears, not just goes unlisted).

## 3. Pull-only proof

The core safety property: nothing the phone does may ever write to Drive.

- [ ] Note the backup file's `modifiedTime` before touching the phone. Drive
      web UI: right-click the backup file (named per your chosen folder,
      typically `ferrico-backup.json`) → **File information → Details** →
      hover the "Modified" timestamp for the exact value. (`drive.file` scope
      means the file won't show up via a generic Drive API browse call from
      outside the app — the web UI is the simplest check.)
- [ ] On the phone: tap-refresh several times, background/foreground the app
      (triggers the visibility-resume sync once the cooldown elapses), force-
      close and relaunch the app 2-3 times.
- [ ] Re-check `modifiedTime` in the Drive web UI — **must be unchanged**
      from the pre-phone-activity value. Any change is a P0 bug (means a
      mobile build somehow pushed) — stop and file it immediately, don't
      continue the rest of the checklist until it's understood.

## 4. Negative paths

- [ ] Garbage pairing string (e.g. `not-a-real-code`) → clean error message
      in the settings pairing textarea, no crash, no partial state (still
      shows the pairing form, not a half-connected dashboard).
- [ ] Airplane mode, then try a manual refresh:
      ```bash
      adb shell settings put global airplane_mode_on 1
      adb shell am broadcast -a android.intent.action.AIRPLANE_MODE
      ```
      Confirm the sync fails gracefully (error surface, no crash) and
      previously-synced bookmarks are still browsable (reads from local
      SQLite, no network needed). Restore networking after:
      ```bash
      adb shell settings put global airplane_mode_on 0
      adb shell am broadcast -a android.intent.action.AIRPLANE_MODE
      ```

## 5. Platform checks

- [ ] Theme toggle (Settings → Theme): switch, force-close, relaunch →
      persisted.
- [ ] Touch scroll is smooth (no jank/stutter) in both list and grid view —
      this is the regression the read-only row/card rewrite (#67/#68) was
      specifically built to avoid versus the desktop drag-enabled components.
- [ ] Note the Android System WebView version in use — a stale one can't
      render Tailwind 4's modern CSS (see playbook risk notes):
      ```bash
      adb shell dumpsys package com.google.android.webview | grep versionName
      ```
      Needs to resolve to a Chromium ≥ ~111-equivalent release.

## 6. Automated baseline

Confirm green on `main` before or alongside the device pass (this doesn't
need the device, just recorded here so the report in #71 has one place to
point to):

```bash
cd src-tauri && cargo test   # expect: all passing, no new warnings
cd .. && bun run test        # expect: all passing
```

## Output

- File a follow-up issue for anything found (use `gh issue create`,
  reference #71 and the specific checklist bullet).
- Close #71 with a short report: device/emulator model, Android version,
  WebView version (from §5), and a per-section pass/fail summary.
