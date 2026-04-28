# Velocimd Foundation Implementation Plan

> **For Hermes:** Use subagent-driven-development skill to implement this plan task-by-task.

**Goal:** Build the initial Velocimd Rust project foundation for a fast, simple, tabbed Markdown reader/editor.

**Architecture:** Use a native Rust desktop app built on `eframe/egui` for quick startup, immediate-mode UI, and low ceremony. Keep core behavior in testable library modules: Markdown rendering, editor mode transitions, theme parsing, document tabs, and command definitions. The GUI shell should be thin and defer behavior to those modules.

**Tech Stack:** Rust 2024, Cargo, `eframe/egui`, `pulldown-cmark`, `serde`, `toml`.

---

### Task 1: Establish tested core behavior

**Objective:** Add tests for mode switching, Markdown rendering, and theme parsing before implementation.

**Files:**
- Create: `tests/core_behaviors.rs`
- Modify: `Cargo.toml`

**Verification:**
- Run `cargo test` and confirm failure because the library modules do not exist yet.

### Task 2: Implement library modules

**Objective:** Create testable core modules for editor modes, Markdown rendering, theme parsing, tabs, commands, and app state.

**Files:**
- Create: `src/lib.rs`
- Create: `src/modes.rs`
- Create: `src/markdown.rs`
- Create: `src/theme.rs`
- Create: `src/document.rs`
- Create: `src/commands.rs`
- Create: `src/app_state.rs`

**Verification:**
- Run `cargo test` and confirm tests pass.

### Task 3: Build the native GUI shell

**Objective:** Replace the generated hello-world binary with an `eframe` application that exposes tabs, command palette, editor/preview/split switching, and basic theme application.

**Files:**
- Modify: `src/main.rs`
- Create: `src/ui.rs`

**Verification:**
- Run `cargo check`.
- Run `cargo run` locally to launch the shell.

### Task 4: Document the project

**Objective:** Capture the product direction and development commands.

**Files:**
- Create: `README.md`
- Create: `docs/architecture.md`

**Verification:**
- Confirm docs mention tabbed editing, command palette, preview modes, and theme customization.

### Task 5: Format and final verification

**Objective:** Ensure the initial scaffold is clean.

**Commands:**
- `cargo fmt --check`
- `cargo test`
- `cargo check`

**Commit:**
```bash
git add .
git commit -m "feat: scaffold velocimd foundation"
```
