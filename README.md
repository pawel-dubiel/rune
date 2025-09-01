# Rune

[![CI](https://github.com/pawel-dubiel/rune/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/pawel-dubiel/rune/actions/workflows/ci.yml)
[![Clippy](https://img.shields.io/github/actions/workflow/status/pawel-dubiel/rune/ci.yml?branch=main&label=clippy)](https://github.com/pawel-dubiel/rune/actions/workflows/ci.yml)
[![rustfmt](https://img.shields.io/github/actions/workflow/status/pawel-dubiel/rune/ci.yml?branch=main&label=rustfmt)](https://github.com/pawel-dubiel/rune/actions/workflows/ci.yml)
![Platforms](https://img.shields.io/badge/platforms-macOS%20|%20Linux%20|%20Windows-4c1)

A tiny, fast terminal text editor written in Rust. It focuses on instant startup, responsive navigation, and a minimal feature set. Now with Vim-like modal editing and configurable key bindings.

## Features
- Fast startup and smooth cursor movement
- Vim-like modes (Normal/Insert)
- Open existing files; save changes
- Viewport scrolling with resize handling
- Status bar with file name, line count, position, and mode
- Cross-platform terminal support via `crossterm`

## Install Rust
- macOS (Homebrew):
  - `brew install rustup`
  - `rustup-init -y`
  - `source "$HOME/.cargo/env"`
- macOS/Linux (official installer):
  - `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y`
  - `source "$HOME/.cargo/env"`
- Verify: `cargo --version`

## Build
- Release build: `cargo build --release`
- With Ropey buffer: `cargo build --features ropey`
- Binary path: `./target/release/rune`

## Install (CLI)
- System-wide (may need sudo): `make install`
- Or with Cargo: `cargo install --path .`
- Run from anywhere: `rune [file]`

## Run
- Open a file: `cargo run --release -- path/to/file.txt`
- Start empty (no file): `cargo run --release`
- With Ropey: `cargo run --features ropey -- -- path/to/file.txt`

## Keys
Default bindings use Vim conventions. The editor is modeless to launch, but starts in Normal mode.

- Insert mode: `i` to enter (also `a`/`o`/`O`), `Esc` to leave. In Insert, `Ctrl-g` then `u` starts a new undo step (like Vim’s Ctrl-g u).
- Movement: `h` `j` `k` `l`, `0` (line start), `$` (line end), `gg` (top), `G` (bottom). Arrows/Home/End/Page keys also work.
- Edit: `x` (delete char under cursor), `dd` (delete line), `o` (open below), `O` (open above), operators with motions: `d{motion}`, `c{motion}`, `y{motion}` (e.g., `dw`, `cw`, `y$`). Counts apply: `3dd`, `2dw`, etc.
- Commands: `:` opens a prompt; supported: `w`, `q`, `wq`/`x`.
- Undo/Redo: Normal `u` undo, `Ctrl-R` redo. In Insert, `Ctrl-Z` also triggers undo for convenience.
- System: `Ctrl-S` save (prompts for filename if unset), `Ctrl-Q` quit (with modification guard).

## Notes
- File format: UTF-8 text with `\n` newlines. `\r` are stripped on open.
- Status bar: shows file name, modified flag, line count, mode, and current line.
- Without a filename, pressing Ctrl-S opens a Save As prompt on the status line. Press Esc to cancel.
 - Undo semantics mirror Vim:
   - One Normal-mode command (even with a count) is a single undo step (e.g., `3dd` undoes all 3 lines at once).
   - One Insert session is a single undo step; use `Ctrl-g u` to break the undo group while staying in Insert.

## Configurable Key Bindings
You can override Normal-mode bindings and general options with a simple config file. Search order:

1. `./vedit.conf`
2. `$XDG_CONFIG_HOME/vedit/config.conf`
3. `~/.config/vedit/config.conf`

Format is a minimal INI-like file. Supported sections: `[general]`, `[normal]`.

Example `vedit.conf`:

```
[general]
start_in_insert = true

[normal]
h = move_left
j = move_down
k = move_up
l = move_right
0 = line_start
$ = line_end
gg = goto_top
G = goto_bottom
i = insert
a = append
o = open_below
O = open_above
x = delete_char
dd = delete_line
: = command
```

Recognized actions: `move_left`, `move_down`, `move_up`, `move_right`, `line_start`, `line_end`, `goto_top`, `goto_bottom`, `insert`, `append`, `open_below`, `open_above`, `delete_char`, `delete_line`, `command`.

General options:
- `start_in_insert` (bool): start the editor in Insert mode. Values: `true/false` (also `on/off`, `1/0`).

## Performance
- Renders only the visible viewport
- Minimal allocations during navigation and editing
- Release profile tuned: `lto=fat`, `opt-level=3`, `codegen-units=1`, `panic=abort`

## Troubleshooting
- Terminal looks odd after a crash: run `reset` or `stty sane`.
- Resize issues or odd characters: ensure you are in a UTF-8 locale and using a modern terminal emulator.

## Development
- Format: `cargo fmt`
- Lint: `cargo clippy` (Ropey: `cargo clippy --features ropey`)
- Tests: `cargo test` (Ropey: `cargo test --features ropey`)
- Toolchain pinned in `rust-toolchain.toml` to stable.

### Git Hooks
- Enable pre-commit hook:
  - `make hooks-install`
  - This runs on each commit: `cargo fmt --check`, `cargo clippy -D warnings`, and `cargo test`.
- You can temporarily skip checks by setting env vars:
  - `SKIP_HOOKS=1 git commit -m "..."` to skip everything
  - `SKIP_TESTS=1 git commit -m "..."` to skip tests only

## macOS App Bundle
If you want a double-clickable app that opens Terminal and runs Rune:

- Build the app bundle: `make app-macos`
- Output: `dist/rune.app`
- Drag `dist/vedit.app` into Applications if you’d like.

Notes:
- This bundle opens Terminal and runs the bundled `rune` binary. It does not handle drag-and-drop files onto the app icon; open files from the CLI or via `:w`/Save As.
- To remove the bundle: `make clean-app`
