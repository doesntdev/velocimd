# Desktop/browser parity

Velocimd has one Rust/egui editor and two platform adapters. The browser is not a second JavaScript editor. Shared command-bar and tab-widget painters own the icons, fonts, themes, spacing, filename field, editor, preview and divider. Native folder-menu data and browser document data differ; browser effects run after rendering releases its mutable app borrow.

## Run the gate

Use the pinned Rust toolchain (`rust-toolchain.toml`), Node 24.15.0 and pnpm 10.34.7. Initial setup:

```sh
pnpm install --frozen-lockfile --ignore-scripts
pnpm exec playwright install --with-deps chromium firefox
# Install once into this repository's ignored tool cache, not globally.
version=$(python3 -c 'import tomllib; print(next(p["version"] for p in tomllib.load(open("Cargo.lock","rb"))["package"] if p["name"]=="wasm-bindgen"))')
cargo install wasm-bindgen-cli --locked --version "$version" --root target/tools/wasm-bindgen-cli
python3 scripts/verify-parity.py
```

Linux GUI prerequisites are Xvfb, xauth, xdotool, ImageMagick's `import`, zenity, software Mesa/OpenGL and the normal egui/X11 development libraries. The GitHub workflow lists the exact Ubuntu packages. Python 3.11+ is required. `WASM_BINDGEN` may select an already installed exact-lock-version CLI. The verifier refuses a mismatched CLI or toolchain instead of silently using it.

A run creates a **new** `target/parity/runs/<run-id>/` directory. `--output NEW_PATH` chooses an explicit output; `--require-clean` enforces a committed candidate. Reports and partial captures survive failure. The process exits nonzero on a missing dependency, failed producer, incomplete browser matrix, console/startup error, source mutation, geometry drift, unacceptable raster difference or invalid distribution. An old successful run is never reused as the new verdict.

The entrypoint runs architecture tests, fmt, locked Rust tests/Clippy, optimized native and WASM builds, verifier unit tests, an isolated native GUI journey, headed Chromium and Firefox in private software-GL displays, the browser safety regressions, and matched-state layout/raster comparisons. It creates a source-bound web archive only after those gates pass. Regular verification does not update baselines.

## What is compared

`tests/parity/fixture.md` and `contract.json` define the same UTF-8 content, equivalent navigation labels, a 1280×800 **application-content** viewport at scale 1, all three modes and both themes. A browser-only legal/source footer, if present, is outside that viewport; a second editor toolbar is forbidden. These fixtures do not certify every viewport, display scale, browser or native OS.

The opt-in read-only probe records actual egui `Response` rectangles and shared state after a real frame. It does not add widgets, set expected rectangles, inject commands or change the renderer. Native output uses `VELOCIMD_UI_PROBE_PATH`: the file must not exist, is created exclusively once and retains its owned handle. Browser `WebHandle.set_ui_probe(true)` and `ui_snapshot()` expose the same measurements. Normal operation leaves probing disabled.

Real pointer/keyboard journeys cover mode and theme controls, new/rename/edit/save, native Open/Save As pickers, browser file import/download/Save As, draft reload, close cancellation and command palette execution. Browser file import/download checks preserve UTF-8, BOM, CRLF and final-newline state exactly. Split resizing uses actual pointer movement. The separate safety suite retains import generation/replay, storage denial/quota/corruption/recovery and origin-writer ownership controls.

`tests/parity/baseline/` comes from the previously verified **published native v0.1.6** Linux package, binary SHA-256 `c7500f48dcdb47fa264b0390b4e326199fca8e16567f64b640e14c9603cc834b`, with capture metadata. Current native captures must remain compatible with that reference; current browser captures must match current native captures. Legitimate desktop design changes need an explicit reviewed baseline/contract update, not an automatically generated browser approval.

Geometry comparisons are strict. Raster comparison has named color/antialias allowances, per-region/control limits and overlapping local tiles so a local loss cannot disappear in a page-wide average. Exceptions are limited to navigation data and declared capability availability; the editor, preview and whole toolbar are never blanket-masked. Intentional private mutations must prove that missing toolbar content and a shifted control fail the normal oracle. Verifier unit tests also reject missing/stale/failed receipts and corrupt/unsafe archives.

## Honest platform differences

- Native navigation tabs contain filesystem folders and directory menus. Browser tabs contain origin-local documents, not mounted folders. The working-folder command is explicitly unavailable in the browser.
- Native Save writes the owned disk path atomically. Browser Save persists a local draft and downloads a copy; it does **not** overwrite an imported file. Browser Save As asks for a copy filename without falsely renaming the original disk file.
- Browser drafts retain existing limits: 16 documents, 1 MiB per imported document, 4 MiB serialized session. Storage failure is not reported as a successful durable save. Recovery bytes and explicit replacement protections remain.
- A browser holds an origin-exclusive Web Locks lease before constructing the app and through flush/teardown. Unsupported locking or another writer rejects startup; there is no unsafe multi-writer fallback.
- Browser-reserved/OS shortcuts and native filesystem attachments are not universal browser capabilities. Tests of delivered page keyboard events do not prove interception of browser chrome shortcuts. The shared on-canvas controls remain the reliable command surface.
- Native GUI/raster qualification here is Linux/X11. Existing Linux/macOS/Windows Rust tests and packaging remain in the release matrix; this is not a claim that a real Windows/macOS picker was exercised locally.

## Artifact and delivery contract

A successful run emits `distribution/velocimd-web-VERSION.zip`, `SHA256SUMS`, `artifact.json` and an unpacked `web/` directory. The ZIP contains the HTML host, generated JS/WASM, exact corresponding-source ZIP and `build.json`. Its manifest binds the actual source-file inventory, base commit/dirty state, Cargo/Node locks and tool versions, tested runtime-asset hashes, complete gate/matrix receipt and AGPL license. A locally verified dirty candidate is clearly marked; CI/release requires clean committed source.

The existing `Check repair branch` CI job now calls the **same** entrypoint and therefore cannot pass while browser parity is absent. A future tagged release assembles only after both parity and the three-platform native package matrix succeed. It verifies matching source commit/version, adds the web ZIP to the complete package set and keeps the existing draft-release promotion step. Nothing here rewrites or republishes v0.1.6.

The website consumes an explicitly hash-approved archive using its `demo:import` command. It validates the complete manifest, source archive, runtime API and receipt before replacing the accepted bundle, reads the installed target back, and records an exact pin. Extraction consumes the same byte snapshot that passed the approved archive hash, never a reopened caller path. The receipt must contain both browser safety-regression gates and every contract-declared native/browser input journey, not only six still-image cases. Post-package controls exercise the real importer with omitted safety gates/journeys and a valid archive rewritten after approval; rejection preserves the predecessor, and the rewrite can install only the originally approved bytes. Normal website builds check the vendored/pinned bytes without reading or compiling a sibling Rust checkout. App changes require a deliberate new artifact promotion; they do not silently rebuild a different demo during a site build.

Local source edits, a locally passed pipeline, uploaded CI artifacts, required branch-protection settings and a published/deployed release are separate states. Workflow files alone do not prove that hosted CI or deployment has run.
