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

## Deploy

It's a static file, so any static host works. For **GitHub Pages**, point Pages
at this `site/` directory (or copy `index.html` to the Pages root). The page
also runs fine from a CDN or object store — no server-side anything.

## Editing notes

- **Theme tokens** mirror the app's own palette (warm charcoal `#1c1a18` +
  copper `#cc9268`), defined once in `:root`.
- The hero "screenshot" is a **hand-built HTML/CSS mock** of the app window —
  swap in a real screenshot later by replacing the `.window` block.
- Content (version, platforms, feature list) is hard-coded; update it here when
  the app changes.
