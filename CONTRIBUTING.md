# Contributing to Ferrico

Thanks for your interest in improving Ferrico! This document explains how to set up your environment, the conventions we follow, and how to get a change merged.

By participating, you agree to abide by our [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to contribute

- 🐛 **Report bugs** — open a [bug report](https://github.com/andi1984/ferrico_dot_app/issues/new/choose) with clear reproduction steps.
- 💡 **Suggest features** — open a [feature request](https://github.com/andi1984/ferrico_dot_app/issues/new/choose) describing the problem you're trying to solve.
- 📝 **Improve docs** — typo fixes and clarifications are very welcome.
- 🔧 **Submit code** — fix a bug or build a feature (see below).

If you're planning a larger change, please open an issue first so we can discuss the approach before you invest time.

## Development setup

You'll need **Bun**, **Node.js 24+**, a stable **Rust** toolchain, and the [Tauri system dependencies](https://tauri.app/start/prerequisites/) for your OS. See the [README](README.md#getting-started) for full prerequisites.

```bash
git clone https://github.com/andi1984/ferrico_dot_app.git
cd ferrico_dot_app
bun install
bun tauri dev
```

## Project layout

```
src/         React + TypeScript frontend (components colocate *.test.tsx)
src-tauri/   Rust backend — Tauri commands in main.rs, DB logic in db.rs
extension/   Browser extension (Manifest V3)
```

The backend follows a clean split: pure database functions live in `src-tauri/src/db.rs`, and Tauri commands in `main.rs` are thin wrappers (lock the connection → call a `db::*` function → return). When adding a backend command:

1. Add a pure `db_*` function in `db.rs` **and write tests for it there**.
2. Add a thin Tauri command wrapper in `main.rs`.
3. Register it in the `generate_handler!` list in `main()`.

## Running the checks

Please make sure all of these pass before opening a pull request — CI runs the same checks:

```bash
bun run test          # frontend tests (Vitest)
bun run typecheck     # TypeScript type-check
cd src-tauri && cargo test   # Rust backend tests
```

We also recommend running `cargo fmt` and `cargo clippy` on Rust changes:

```bash
cd src-tauri
cargo fmt
cargo clippy --all-targets
```

## Coding conventions

- **TypeScript / React** — match the surrounding code's style. UI conventions (CSS variables, button patterns, icon sizing) are documented in [`CLAUDE.md`](CLAUDE.md#ui-conventions).
- **Rust** — keep `db.rs` functions pure and testable; keep commands thin. Resolve all `clippy` warnings.
- **Tests** — new behavior should come with tests. Backend tests use in-memory SQLite (no fixtures); frontend tests use Vitest + Testing Library.

## Commit messages

We follow [Conventional Commits](https://www.conventionalcommits.org/). The type prefix drives the changelog:

```
feat(search): add fuzzy matching across page body
fix(import): handle multi-byte chars in Netscape HTML
perf(grid): virtualize the bookmark grid
docs(readme): document the browser extension
```

Common types: `feat`, `fix`, `perf`, `docs`, `refactor`, `test`, `chore`.

## Pull request process

1. **Fork** the repo and create a branch from `main` (e.g. `feat/tag-colors` or `fix/import-crash`).
2. Make your change, with tests, keeping the checks above green.
3. Update documentation (README/CLAUDE.md) if behavior or setup changes.
4. Open a pull request against `main` and fill in the template. Link any related issue.
5. Ensure CI passes. A maintainer will review and may request changes.

## License of contributions

Ferrico is licensed under **GPL-3.0-or-later**. By submitting a contribution, you agree that your work will be licensed under the same terms. Don't submit code you don't have the right to license this way.

Thank you for contributing! 💜
