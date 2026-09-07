# Editor stabilization

## Scope and decisions

This pass addresses the six recommendations from the local bug audit, retaining
and completing the existing `fix/editor-line-boundary` work.

1. **Save/recovery:** document and session writes stage and sync a same-directory
   temporary file before replacement. First saves assign paths only after success;
   new paths are committed without clobbering existing files. Dirty bodies remain
   in session snapshots. Restoration prefers those bodies and detaches conflicting
   recovery copies instead of autosaving over changed on-disk contents. Clean
   file-backed bodies are still omitted from session metadata.
2. **Path ownership:** Save As rejects another open buffer's canonical path before
   touching disk or memory; symlink aliases are covered. Legacy duplicate owners
   are detached as recoverable scratch copies. Saving to the current file remains
   supported.
3. **Editor geometry:** height estimation accounts for actual text-frame padding
   and wrapped rows. Scroll synchronization counts real newline boundaries in the
   galley, not visual row numbers. Document editor IDs are distinct. A changed
   frame requests layout recalculation for newlines and wrapping.
4. **Close command:** `Command::CloseFolderTab` explicitly closes a folder, through
   the same core dispatch used by the UI. Labels, shortcut routing, and tests agree.
   Closing a folder retains documents and the final folder cannot be closed.
5. **Extensions:** one case-insensitive helper handles Markdown extension checks
   and display stripping. `Notes.MD` no longer becomes `Notes.MD.md`.
6. **Palette:** Ctrl+K / Cmd+K and Ctrl+Shift+P / Cmd+Shift+P open a real modal
   palette. Case-insensitive substring/subsequence search, arrow selection, Enter,
   Escape, pointer selection, and no-results behavior are implemented. Selection
   executes the existing UI command route. Palette visibility/search are transient.

Related failure-path fix: renaming stages new content before deleting the original,
refuses an occupied destination, preserves ordinary file permissions, and leaves
memory unchanged on failure. If old-file removal fails, destination cleanup is
attempted; no original content is truncated.

## Verification

Commands exercised successfully:

```bash
cargo fmt --check
cargo check --locked
cargo test --locked
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo check --locked --target wasm32-unknown-unknown
cargo build --locked
python3 scripts/native-smoke.py
git diff --check
```

Regression coverage includes failed first saves, file-backed draft recovery after
failed writes, real session serialization/load, duplicate restored path ownership,
Save As collisions and symlink aliases, same-file Save As, failed rename,
replacement failure cleanup, Unix permissions, uppercase/Unicode display names,
wrapped lines across widths and themes, real UI close routing, and palette keyboard
search/selection/execution/persistence/dismissal.

The Linux native smoke uses a software-rendered Xvfb display and temporary notes
and config, never the user's visible desktop or normal session. It exercises native
startup, palette search/Enter, mode persistence, keyboard input, debounced autosave,
graceful close, and reopen. Both app runs must exit zero. Logs, screenshots and
`result.json` live in ignored `target/native-smoke/`.

The native smoke sends an explicit Return key for newlines rather than relying on
multiline synthetic typing. `VELOCIMD_SMOKE_BINARY=/absolute/path/to/velocimd`
selects a release or installed executable; the result records its SHA-256 and
verifies the launched process runs that exact binary.

## Limits (not release claims)

- Windows cross-compilation on this Linux host is blocked by missing `llvm-rc`.
  Windows/macOS runtime behavior was not exercised. WASM compilation does not mean
  browser filesystem/dialog functionality is implemented.
- This is not a cross-process file-locking or external-edit-conflict system. Two
  separate app processes can still race. Atomic replacement does not preserve inode
  identity/hard-link sharing, ownership changes, or all filesystem-specific ACLs.
- Staged file contents are synced; directory-entry power-loss durability and
  transactional multi-file rename across crashes are not guaranteed.
- If both the target and the session recovery location cannot be written, shutdown
  logs the failure but cannot guarantee draft recovery. Forced termination before
  autosave/recovery persistence can lose the latest changes.
- Preview scroll mapping inside a rendered Markdown block remains approximate;
  logical-versus-visual source line mapping is fixed, not full block source maps.
- The original spec's full editor command catalog, plugin system and very-large-file
  performance goals are not part of this repair pass.
