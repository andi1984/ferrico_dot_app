# Ferrico landing page

A single, self-contained marketing page for Ferrico. Everything lives in
[`index.html`](./index.html) — no build step, no dependencies. Fonts load from
Google Fonts; everything else (styles, the app "product shot", interactions) is
inline.

## Preview locally

Just open the file:

```bash
open site/index.html        # macOS
xdg-open site/index.html    # Linux
```

Or serve it (so relative links behave like production):

```bash
python3 -m http.server -d site 8000   # → http://localhost:8000
```

## Deploy (GitHub Pages)

Deployment is automated by [`.github/workflows/pages.yml`](../.github/workflows/pages.yml),
which publishes this `site/` folder to Pages on every push to `main` that touches
it (and can be run manually from the **Actions** tab).

**One-time setup:** in the repo's **Settings → Pages**, set **Source** to
**GitHub Actions**. After the first run, the site is live at
`https://andi1984.github.io/ferrico_dot_app/`.

The page is fully self-contained (inline CSS/JS, fonts from Google Fonts, only
anchor links), so it works unchanged under that project subpath — and on any
other static host or CDN too.

## Editing notes

- **Theme tokens** mirror the app's own palette (warm charcoal `#1c1a18` +
  copper `#cc9268`), defined once in `:root`.
- The hero "screenshot" is a **hand-built HTML/CSS mock** of the app window —
  swap in a real screenshot later by replacing the `.window` block.
- **Downloads + version auto-sync** with GitHub Releases: a small client-side
  `fetch` of `/releases/latest` points each platform card at the matching asset
  (`.dmg` / `.AppImage` / `-setup.exe`) and stamps the live version tag. **No
  version is hard-coded** — the badges read `Latest` in the source and are
  filled in at runtime. No edits needed per release. If the request fails
  (offline / API rate limit), the cards keep the `Latest` label and fall back to
  the releases page. To re-map assets, edit the `PICK` matchers in the inline
  script.
- The feature list and copy are hard-coded; update them here when the app changes.
