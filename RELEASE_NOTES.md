# Velocimd v0.1.7 — responsive Split and safer browser drafts

## Responsive editor and Split locking

- Narrow app viewports below 720 logical pixels, or portrait workspaces, stack the editor above the preview. Wide landscape workspaces retain side-by-side panes. This uses the embedded app's own available size, not the outer website or a device guess.
- Narrow controls wrap without removing commands; overflowing tabs scroll horizontally and the divider has a touch-sized target.
- Activate Split, then activate it again to lock the current orientation. Activate it once more to return to automatic orientation. The toolbar and `Ctrl+3` share this cycle; a padlock badge indicates the lock.
- Locking preserves orientation during window resizing, while the divider remains adjustable/resettable. It is view-session state and clears when leaving Split or restarting; note bytes and persisted draft schemas are unchanged by locking.

## Editing and preview work

- Capture actual editor mutations and incrementally update line metadata and eligible plain-paragraph preview regions, with complete-parser fallbacks for structural/context-sensitive Markdown, missed revisions and whole-buffer replacements.
- Reuse guarded, bounded large-document layouts to avoid repeated whole-source copy/hash requests on eligible warm redraws and scrolling, while retaining the existing TextEdit input, selection, undo and canonical layout geometry.
- Fix an inherited browser hidden-input history conflict that could echo Unicode insertions during redo. Ordinary text/composition and separate HTML filename controls retain their own behavior.

## Transactional browser drafts

- Replace synchronous whole-session draft writes with asynchronous, per-document IndexedDB transactions and coherent session metadata. Acknowledgments apply to captured revisions only; newer edits remain pending.
- Preserve and verify legacy drafts before migration, retain exact-byte recovery, keep the same exclusive-writer identity across versions, and prevent older clients from overwriting newer storage.
- Preserve explicit Retry intent behind an outstanding failed save and keep uncertain writes distinguishable from acknowledged content. Quota, denied storage, corruption and outside changes remain recoverable without silently overwriting notes.
- Browser limits remain 16 tabs, 1 MiB imports and a 4 MiB serialized logical session. Imports/downloads are copies, not filesystem overwrite or cloud backup. Only acknowledged transactions are promised after real page termination.

## Release and browser delivery

- Qualify native and WASM policies separately: native uses thin LTO and one codegen unit; browser code keeps its validated compiler policy and strips only optional name/debug custom sections.
- Native executable size decreased 19.39% in the same-source policy comparison. The selected WASM size also decreased; fresh-process browser startup did not show a reliable gain. These are not general typing-latency claims.
- The controlled browser persistence comparison reduced median synchronous flush-action blocking from 28.3 ms to 0.1 ms; total commit latency and keystroke latency are different measurements. See [PERFORMANCE.md](https://github.com/doesntdev/velocimd/blob/main/PERFORMANCE.md) for scope and limits.
- The browser distribution includes the required storage runtime, exact corresponding source and a source/asset-bound verification receipt. The website imports an explicit SHA-256-pinned artifact rather than rebuilding an arbitrary sibling checkout.

## Verification and known limits

- The implementation checkpoint passed 191 local Rust tests, native/WASM strict checks, real Chrome/Firefox workflows and focused lock/resize tests. Release-source results and package identities are verified before publication.
- A rapid zero-delay `Ctrl+3` event batch during a Firefox text-input/canvas focus handoff intermittently missed activation. Its cause remains unresolved. Passing frame-synchronized chords do not establish that this edge case is fixed; the on-canvas Split control remains available.
- This release does not include the paused rich-block caching/virtualization candidate, a complete viewport-paged editor backend, or new navigation/sidebar/headless-render work. Large-document typing timings are mixed and giant-paragraph reflow remains.
- Linux native GUI and Chromium/Firefox checks are exercised. Native-platform build/tests and packaging are not interactive Windows/macOS desktop acceptance or physical-phone keyboard/IME qualification.
- Packages are unsigned/not notarized; Windows SmartScreen and macOS Gatekeeper may warn.

**Full changelog:** https://github.com/doesntdev/velocimd/compare/v0.1.6...v0.1.7
