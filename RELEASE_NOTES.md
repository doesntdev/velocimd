# Velocimd v0.1.4 — editor reliability and command palette

## Fixed

- Failed saves no longer assign a path before the first successful write. Dirty drafts are retained in session recovery, including file-backed drafts whose save failed.
- Note and session writes use staged replacement instead of truncating files in place. Rename stages new content before removing the original.
- Save As refuses a destination already owned by another open document, including symlink aliases. Legacy duplicate owners are recovered as separate scratch copies.
- Wrapped text has correctly sized editor bounds and padding. Preview synchronization now distinguishes logical source lines from wrapped visual rows.
- The Close command consistently closes a **folder tab** without discarding loaded documents.
- Markdown extensions are handled case-insensitively; `Notes.MD` no longer becomes `Notes.MD.md`.

## Added

- A working command palette: **Ctrl+K** or **Ctrl+Shift+P** (Cmd on macOS), searchable commands, Up/Down navigation, Enter to execute, and Escape to dismiss.
- Regression tests for failed saves, recovery, path collisions, editor layout, and real UI command routing.
- An isolated Linux native smoke test covering launch, palette, editing, autosave, graceful close, and reopen.
- A release gate that assembles packages only after Linux, macOS, and Windows build jobs all succeed, plus `SHA256SUMS` for downloads.

## Downloads

- **Linux:** `.deb` for Debian/Ubuntu, `.AppImage`, or the Arch package archive/PKGBUILD.
- **macOS:** universal `.dmg` for Apple Silicon and Intel.
- **Windows:** x64 NSIS setup installer.

## Notes

Back up important notes before upgrading. Recovery drafts that differ from the on-disk file reopen as separate scratch copies rather than automatically overwriting the disk version.

The app remains unsigned/not notarized; macOS Gatekeeper and Windows SmartScreen may warn. Native Linux GUI behavior is exercised; Windows/macOS GUI interaction is not certified by package builds alone. This release does not add cross-process file locking or guarantee recovery when both the note and session storage are unwritable. Preview alignment within complex rendered Markdown blocks remains approximate.

**Full changelog:** https://github.com/doesntdev/velocimd/compare/v0.1.3...v0.1.4
