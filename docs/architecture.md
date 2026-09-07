# Velocimd Architecture

> Historical foundation design. The current folder-tab behavior, native preview,
> save/recovery invariants, and command palette are described in README.md and
> [stabilization.md](stabilization.md). The “Next real work” list below predates those changes.

Velocimd starts as a native Rust desktop application using `eframe/egui`. That keeps the app simple, fast to launch, and portable without immediately taking on a webview stack.

## Design priorities

1. **Fast first paint:** small native shell, no browser runtime required.
2. **Tabbed documents:** document state lives in `AppState`, with each tab represented by `Document`.
3. **Mode switching:** `EditorMode` supports `Edit`, `Preview`, and `Split`, with explicit commands and cycling.
4. **Command palette:** commands are data-backed in `commands.rs`, so keyboard shortcuts and palette entries share one source.
5. **Theme customization:** themes are TOML-backed `ThemeConfig` values. The initial shell applies background, accent, and font sizes through egui styles.
6. **Thin UI, tested core:** parsing, rendering, modes, commands, document state, and theme loading live outside the GUI for straightforward tests.

## Module map

- `src/main.rs`: native app launcher.
- `src/ui.rs`: egui application shell.
- `src/app_state.rs`: active documents, current mode, command palette state, active theme.
- `src/document.rs`: document/tab model.
- `src/modes.rs`: edit/preview/split mode model.
- `src/commands.rs`: command palette command definitions.
- `src/markdown.rs`: Markdown-to-HTML rendering.
- `src/theme.rs`: TOML theme parsing and egui theme application.

## Next real work

- File open/save with dirty-state handling.
- Incremental Markdown preview rather than raw rendered HTML display.
- Proper Markdown editor features: line numbers, syntax highlighting, bracket pairing, search, and Vim-like optional bindings.
- User theme directory loading from `~/.config/velocimd/themes`.
- Benchmarks for large Markdown files.
