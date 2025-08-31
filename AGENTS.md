# Repository Guidelines

## Project Structure & Module Organization
- `src/buffer.rs`: Unicode-aware text buffer (graphemes, widths, insert/delete, merge).
- `src/keymap.rs`: Vim-like modes and actions, default key bindings, config loader (`rune.conf`).
- `src/editor.rs`: Editor state (cursor, mode, file, dirty flag) and editing operations.
- `src/ui.rs`: TUI rendering, viewport scrolling, prompts; partial/diff rendering for speed.
- `src/app.rs`: Event loop and terminal setup/teardown.
- `packaging/`: macOS app bundle assets.
- `.githooks/`: local Git hooks (pre-commit).
- `.github/workflows/ci.yml`: GitHub Actions CI (fmt, clippy, build, test).

## Build, Test, and Development Commands
- Build (release): `cargo build --release`
- Run: `cargo run --release -- [file]`
- Tests: `cargo test`
- Lint (deny warnings): `cargo clippy --workspace --all-targets -- -D warnings`
- Format: `cargo fmt --all`
- Format (check mode): `cargo fmt --all -- --check` (CI uses this)
- Install CLI: `make install` (installs `rune` to `/usr/local/bin`)
- macOS app bundle: `make app-macos` → `dist/rune.app`
- Enable hooks: `make hooks-install`

## Coding Style & Naming Conventions
- Rust style is enforced by `rustfmt` (default settings). Run `cargo fmt --all`.
- Lint with `clippy`; CI uses `-D warnings` (keep code warning-free).
- Naming: snake_case for functions/vars, CamelCase for types, SCREAMING_SNAKE_CASE for consts.
- Prefer explicit error handling over `.unwrap()` in app/UI layers.

## Testing Guidelines
- Framework: Rust `#[test]` in `#[cfg(test)]` modules.
- Existing examples: see `src/buffer.rs` tests (graphemes, widths, deletes).
- Add new tests alongside modules (e.g., `src/editor.rs`) with descriptive snake_case names.
- Run locally: `cargo test`; CI runs tests on Linux/macOS/Windows.

## Commit & Pull Request Guidelines
- Commits: concise imperative subject (e.g., "Add diff-based rendering"), scope small and focused.
- PRs: include a summary, rationale, and testing notes. Link issues when relevant.
- Ensure `cargo fmt`, `clippy -D warnings`, and `cargo test` pass locally (pre-commit hook runs these).
  - Formatting is mandatory; run `cargo fmt --all` after changes to avoid CI failures.

## Configuration & Security Notes
- User config: `rune.conf` (also supports legacy `vedit.conf`); search paths include `./`, `$XDG_CONFIG_HOME/rune/`, and `~/.config/rune/`.
- Do not commit secrets or env files (`.env*` ignored). Avoid adding transient build outputs (`target/`, `dist/`).
