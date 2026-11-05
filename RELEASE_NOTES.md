# Velocimd v0.1.6 — editor performance and desktop controls

## Desktop controls

- Combine the light/dark controls into one theme toggle, preserving editor/preview font sizes and the existing `Alt+L` / `Alt+D` shortcuts.
- Remove the theme name from the header and arrange the direct mode buttons as Edit–Split–View.
- Drag the Split divider to resize the panes. The proportion is saved across restart; double-click resets to 50/50, with usable minimum pane widths.
- Own native pickers with the app window, avoid Windows Shell-incompatible initial folder hints, and display picker errors rather than silently treating them as cancellation. Open File and Save As use the same handling.

## Faster editing and preview

- Reuse editor layouts and draw only visible line numbers to reduce repeated work on large documents.
- Retain semantic Markdown preview blocks and render the visible region, preserving lists, tables, code, references, images, Mermaid diagrams, and source-based Split synchronization.
- Share folder-menu snapshots, cache sorting keys, and build only visible rows for large folders.
- Restore inactive clean notes from metadata and load their bodies when selected, rather than reading every note during startup. Active notes and recovery drafts remain available immediately.

## Safer, quieter persistence

- Run native document and session writes through one ordered background worker, with revision-aware completion and coalesced session updates.
- Preserve atomic file replacement, recovery drafts, save/rename ordering, path-collision checks, and graceful-shutdown draining.
- Stop rewriting clean documents when switching working folders and avoid redundant ownership-path normalization.

## Fixed

- Scrolling over the right-hand preview in Split view now scrolls the shared document and keeps both panes synchronized, without clicking the preview first.
- `Alt+L` and `Alt+D` switch themes without inserting their letters into a focused editor, including when another command arrives in the same frame. Command priority, ordinary typing, paste, IME input, AltGr, and command-palette behavior are preserved.
- Decoded newline entities in large Markdown paragraphs no longer stall cold preview layout; UTF-8 progress and newline geometry have regression coverage.

## Verification and limits

- The current development candidate passes 138 local tests, formatting, strict all-target/all-feature Clippy, native compilation, and wasm32 compilation.
- Isolated Linux native checks cover large-session restore, scrolling, mode/theme changes, exact saves, inactive-note activation, graceful close/reopen, and unchanged notes. Theme-shortcut checks include ordinary typing afterward as a positive control.
- Reproducible paired CPU probes and their measurement limits are documented in [PERFORMANCE.md](https://github.com/doesntdev/velocimd/blob/main/PERFORMANCE.md). CPU pane timings are not GPU presentation or end-to-end typing-latency measurements.
- No new application dependencies, services, database, account, or network requirement. The ordinary Markdown files-and-folders model and minimalist interface remain unchanged.
- The separate navigation/sidebar work and headless-render CLI PR are not included in this release candidate.
- Windows folder-picker handling has source and nonmodal regression coverage. The release workflow builds and tests on native platform runners; this is not a substitute for testing the interactive Windows picker on a Windows desktop.

## Packages

The existing release workflow produces Linux `.deb`, `.AppImage`, and Arch package/PKGBUILD assets, a universal macOS `.dmg`, and a Windows x64 NSIS installer, plus `SHA256SUMS`.

Package checksums are supplied in `SHA256SUMS`. The app remains unsigned/not notarized; macOS Gatekeeper and Windows SmartScreen may warn. Native GUI interaction is exercised on Linux; compilation or package generation alone does not certify Windows/macOS GUI behavior. WASM compilation does not imply browser filesystem/dialog support.

**Full changelog:** https://github.com/doesntdev/velocimd/compare/v0.1.5...v0.1.6
