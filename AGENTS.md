# AGENTS.md

Rust workspace for a GUI terminal emulator (winit + wgpu). Development is spec-driven: specs and plans live in Spanish under `specs/`, code and identifiers are in English.

## Commands

Verification order (matches CI): **fmt → clippy → test**.

- `cargo fmt --check` — format gate
- `cargo clippy --workspace -- -D warnings` — lint gate (CI). Add `--all-targets` to also lint tests and examples (CI's command does not).
- `cargo test --workspace` — full suite (Linux/Windows)
- `cargo test -p terminal_core parser` — run one crate / one test by name filter
- `cargo run` — build and run the GUI binary (requires a display; no headless mode)
- `cargo run -p terminal --example offscreen_render` — render a grid to a headless texture and report non-background pixels (GPU smoke test)

## Workspace layout

Dependency direction: `terminal_core` ← `terminal_render` ← `terminal_gui` ← `terminal` (binary). `terminal_config` and `terminal_pty` are standalone.

- `terminal_core` — GUI-free engine: grid, ANSI/ECMA-48 parser, state, scrollback, session, input. Must stay free of GUI dependencies (constitution).
- `terminal_config` — TOML config load/validate + clap CLI (CLI overrides file values).
- `terminal_pty` — PTY trait; `nix` on unix, ConPTY on Windows.
- `terminal_render` — wgpu pipeline: fontdb + swash glyphs, atlas, renderer.
- `terminal_gui` — winit window, tabs, splits, keymap, clipboard, mouse selection.
- `terminal` — binary wiring everything; entry point `terminal/src/main.rs`.

## Workflow: spec-first (SDD)

- Nothing is implemented unless it is in `specs/XXX-YYY/spec.md`. Changes go to the spec first, then plan, then tasks (constitution principle 2).
- Current spec: `specs/001-terminal-emulator/{spec,plan,tasks}.md` — RF-1…RF-43 in EARS notation, 45 tasks. `docs/constitution.md` holds the binding principles.
- SDD skills are available via the `skill` tool and enforce the workflow: `sdd-generator`, `ssd-planning`, `ssd-tasks`, `ssd-implementation`, `ssd-change`, `ssd-clarification`, `ssd-validation`, `ssd-constitution`.
- `ssd-implementation` is strict TDD: write tests first, implement only the requested task Tn, run tests, mark it done in `tasks.md`, then stop.

## Gotchas

- Root `Cargo.toml` sets `warnings = "deny"` and clippy `pedantic` for the whole workspace: any warning fails build/CI, and all public items need doc comments.
- Integration tests (`terminal/tests/integration.rs`) spawn real PTYs (`/bin/bash`, `/bin/true`, `/bin/false`) with `sleep`-based polling — Unix-only and slightly slow by design. This is why macOS CI runs `cargo test --workspace --lib`.
- The `terminal` binary picks its shell from `$SHELL` (Unix) / `%COMSPEC%` (Windows) in `main.rs::default_shell()`, falling back to `/bin/bash` / `cmd.exe`.
- GPU base (wgpu instance/device) is intentionally initialized before the winit event loop (fixes invisible window on Wayland). Do not refactor this away.
- Config lives at `$XDG_CONFIG_HOME/terminal/config.toml` (Linux) / `%APPDATA%\terminal\config.toml` (Windows); a missing file silently falls back to defaults.
- Logging goes through `tracing`; set `RUST_LOG` to change the filter (default: `terminal=debug,info`).
- `terminal_render` colors are RGBA-packed (`r << 24 | g << 16 | b << 8 | a`) via `pack_rgba`/`unpack_rgba`, matching `TerminalPalette` entries. Note `color::rgb`/`rgba`/`parse_hex_color` use the *opposite* (ARGB) order — don't mix them.
