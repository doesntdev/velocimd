# Velocimd

Velocimd is a native Markdown editor built in Rust with `egui`. It is designed
around fast local editing, folder-based document switching, live preview, and a
small command surface that stays out of the way.

[![Release Packages](https://github.com/doesntdev/velocimd/actions/workflows/release.yml/badge.svg)](https://github.com/doesntdev/velocimd/actions/workflows/release.yml)

## Highlights

- Native desktop app for Linux, macOS, and Windows.
- Folder tabs: each tab represents a working folder, with Markdown files and
  child folders available from the tab menu.
- Markdown-only workflow with `.md` extensions hidden in the UI where possible.
- Edit–Split–View controls, with a draggable Split divider that remembers its position.
- Debounced autosave with staged file replacement and failed-save draft recovery.
- Searchable command palette with keyboard navigation.
- Line-numbered editor with synchronized preview positioning.
- Native preview rendering with code highlighting, local images, and Mermaid
  flowchart support.
- One light/dark theme toggle, with the existing explicit theme shortcuts retained.
- Icon command bar with hover tooltips and keyboard shortcuts.
- Local app state persistence for open documents, recent files, folders, and
  theme selection.

## Status

Velocimd is early public software. It is usable for local Markdown editing, but
the file model, packaging, and preview behavior are still expected to evolve.
Use normal version-control habits for important notes.

## Quickstart

Download the latest packages from
[GitHub Releases](https://github.com/doesntdev/velocimd/releases/latest).

### macOS

Velocimd is not currently Developer ID signed or notarized. The command-line
installer downloads the latest DMG, verifies the GitHub release checksum,
installs `Velocimd.app` to `~/Applications`, and removes quarantine metadata:

```bash
curl -fsSL https://raw.githubusercontent.com/doesntdev/velocimd/main/scripts/install-macos.sh | bash
```

To install somewhere else:

```bash
curl -fsSL https://raw.githubusercontent.com/doesntdev/velocimd/main/scripts/install-macos.sh | VELOCIMD_INSTALL_DIR=/Applications bash
```

### Linux

Debian and Ubuntu users can download the `.deb` package from the latest release,
then install it with:

```bash
sudo apt install ./velocimd_*_amd64.deb
```

For other distributions, download the AppImage, make it executable, and run it:

```bash
chmod +x velocimd_*_x86_64.AppImage
./velocimd_*_x86_64.AppImage
```

### Windows

Download `velocimd_*_x64-setup.exe` from the latest release and run the
installer. Windows may show a SmartScreen warning while the installer is
unsigned.

### From Source

Install Rust stable, then run:

```bash
git clone https://github.com/doesntdev/velocimd.git
cd velocimd
cargo run --release
```

## Browser build (real Rust/WASM editor)

The standalone browser app runs the same `VelocimdApp` egui editor and preview,
with document tabs instead of native folder tabs. It does not write to the
imported file or access a desktop working folder.

Split adapts to the app's own allocation (including when embedded): below 720
logical points wide, or in portrait, the editor sits above the preview. Wide
landscape retains the desktop left/right layout. Drag the divider along its
active axis; double-click resets to 50/50. Resizing alone preserves the saved
editor share, selected mode and source. Narrow toolbars wrap without removing
commands, and overflowing tabs scroll horizontally.

Click **Split** (or press **Ctrl+3**) to enter automatic Split. Activate it again
while selected to lock the current over/under or side-by-side orientation, then
again to unlock. A padlock badge marks the locked view; the divider remains
adjustable. Unlocking adapts immediately to the current viewport. This view-session
lock clears when leaving Split or restarting the app; it does not change note bytes.

```bash
rustup target add wasm32-unknown-unknown
# Match Cargo.lock exactly (currently 0.2.120):
cargo install wasm-bindgen-cli --version 0.2.120 --locked
bash scripts/build-web.sh
python3 -m http.server 8080 --directory target/web --bind 127.0.0.1
# Open http://127.0.0.1:8080/
```

`WASM_BINDGEN=/absolute/path/to/wasm-bindgen bash scripts/build-web.sh` is
supported when the CLI is not on PATH. The script refuses a mismatched CLI,
uses the existing Cargo target cache, and produces `target/web/index.html`,
`target/web/velocimd.js`, and `target/web/velocimd_bg.wasm`. Copy these assets
together if serving at a subdirectory; no root-relative asset URL is required.
Serve over HTTPS or localhost, not `file://`. WebGL and the Web Locks API are
required. Keep the supplied HTML host together with the generated module: its
lease-provider functions are part of the `WebHandle::start` contract.

- **New**, **Import .md**, document selection, **Close tab**, and **Download .md**
  operate on the actual active egui document. Import accepts `.md`/`.markdown`,
  valid UTF-8, up to 1 MiB; imported bytes (including BOM, CRLF, and missing final
  newline) are preserved until edited. Download exports the exact active content.
- Edit, Split, Preview and light/dark controls share the real editor state.
  Rename the active note using the filename field inside the editor.
- Local browser drafts restore tabs, contents, selection, mode, divider and theme
  from `localStorage` on the same origin. Edits autosave after a short debounce;
  page-hide/unload also attempts a synchronous flush. Limits: 16 open tabs and
  4 MiB serialized drafts. This is **not a backup or filesystem autosave**;
  clearing site data/private browsing or storage eviction can remove drafts.
- One active editor per origin holds an exclusive Web Lock from before App
  construction through teardown, covering every save/recovery/shutdown path. A
  second editor cannot start until the first closes. Reload or Retry afterward.
  Browsers without Web Locks fail closed rather than running an unsafe writer.
- Storage denial, quota failure and outside-storage changes are visible. Notes
  remain editable and downloadable in memory; **Retry draft save** retries
  durability. Download each open note before leaving if a warning remains.
- Malformed/unsupported stored data is left intact, not autosaved over with a
  fresh session. **Download stored recovery** exports its original bytes.
  **Replace stored drafts…** requires confirmation and a verified localStorage
  backup at `velocimd.browser-drafts.v1.recovery` before replacement; it refuses
  if storage is full or unreadable. Download recovery first for an external copy.
- Import reads are token-bound: newer imports, New, Close or a document selection
  invalidate an older asynchronous completion. Cancellation does not change notes.
- Browser native-folder shortcuts explain the browser controls instead of calling
  filesystem APIs. Desktop folder/session/autosave behavior is unchanged. Local
  image-file access remains desktop-only; this browser adapter does not import
  image attachments. Embedded font coverage may omit some emoji glyphs without
  changing their source/download bytes.

`render_markdown(source)` is also exported from the generated WASM module. It
forwards to the existing `markdown::render_to_html` helper unchanged; this is an
HTML generation API, not the egui editor's native preview rendering path.

### Browser regression smoke

After building, install/use Playwright externally (no Node production dependency
is required by the app), then run:

```bash
PLAYWRIGHT_MODULE=/absolute/path/to/node_modules/playwright \
CHROME=/absolute/path/to/chrome \
EVIDENCE_DIR=/absolute/path/to/evidence node scripts/test-web.mjs
```

The harness serves the generated assets locally at `/demo/`, launches Chrome
**with its sandbox enabled**, uses disposable contexts and synthetic notes,
exercises real keyboard editing/import/download/reload and failure recovery,
and saves screenshots plus `results.json`. It closes its browser and HTTP server.
Also run `cargo fmt --check`, `cargo test`, and
`cargo check --locked --lib --target wasm32-unknown-unknown`.

## Usage

On first launch, choose a working folder. New Markdown files are saved there by
default, and folder tabs let you switch between working directories. The `+`
command creates a new Markdown file in the active folder.

Drag the line between editor and preview to resize Split view. The proportion is
saved with the session; double-click the divider to return to 50/50. Both panes
retain a usable width when resizing the window. The toolbar theme toggle preserves
your editor/preview font sizes; the header no longer displays the theme name.

Native file/folder dialogs are owned by the app window. Windows lets the Shell
choose its starting directory rather than passing stored canonical/verbatim paths.
Picker failures are shown explicitly; cancellation leaves the current note and
folder unchanged.

Open an existing Markdown file with `Ctrl+O`, or pass files on startup:

```bash
cargo run --release -- notes.md
```

Mermaid diagrams render directly in preview for fenced flowchart blocks:

````markdown
```mermaid
flowchart LR
  A[Draft] --> B{Review}
  B --> C[Ship]
```
````

## Keyboard Shortcuts

| Shortcut | Action |
| --- | --- |
| `Ctrl+K` or `Ctrl+Shift+P` | Open command palette |
| `Ctrl+N` | Create a new Markdown file |
| `Ctrl+Shift+O` | Select working folder |
| `Ctrl+O` | Open Markdown file |
| `Ctrl+S` | Save current file |
| `Ctrl+Shift+S` | Save current file as |
| `Ctrl+W` | Close active folder tab |
| `Ctrl+1` | Editor mode |
| `Ctrl+2` | Preview mode |
| `Ctrl+3` | Enter Split; repeat to lock/unlock orientation |
| `Ctrl+Tab` | Cycle editor / preview / split |
| `Alt+L` | Switch to Velocilight |
| `Alt+D` | Switch to Velocidark |

On macOS, egui maps command-style shortcuts to the platform command modifier.

The command palette supports case-insensitive/fuzzy search, Up/Down selection,
Enter to run a command, and Escape to dismiss. `Ctrl+W` closes a **folder tab**,
not a document; it leaves loaded documents intact and keeps the last folder open.

Save As refuses destinations already owned by another open document (including
symlink aliases). Failed writes retain dirty content in session recovery. If a
recovered draft differs from its on-disk file, it reopens as a pathless copy rather
than automatically overwriting the disk version. Save or rename that copy to
choose its destination.

## Development

Requirements:

- The pinned Rust toolchain in `rust-toolchain.toml`
- Platform build dependencies required by `eframe`/`wgpu`

Common commands:

```bash
cargo check --locked
cargo test --locked
cargo clippy --all-targets --all-features
cargo run
```

The desktop/browser parity gate uses the same entrypoint locally and in CI:

```bash
python3 scripts/verify-parity.py
```

It verifies both real runtimes, shared control geometry, native reference images,
Chromium/Firefox journeys and the source-bound browser artifact. See
[desktop/browser parity](docs/browser-parity.md) for one-time prerequisites,
intentional platform differences, baseline policy and website promotion.

Linux native smoke (isolated Xvfb display and temporary notes/config):

```bash
cargo build --locked
python3 scripts/native-smoke.py
```

Requires `Xvfb`, `xvfb-run`, `xdotool`, and ImageMagick. Results and screenshots
are written to the ignored `target/native-smoke/` directory. See
[stabilization notes](docs/stabilization.md) for coverage and remaining limitations.

The desktop-controls smoke checks the actual toolbar, divider drag/reset and
saved-layout reopen. With Zenity installed, the opt-in picker case opens and selects
an owned temporary folder, entirely inside the private Xvfb display:

```bash
python3 scripts/desktop-controls-smoke.py
python3 scripts/desktop-controls-smoke.py --native-picker
```

Evidence is written under ignored `target/desktop-controls/`. On Windows, run
`cargo check --locked --all-targets` and `cargo test --locked` for native code and
nonmodal component/state tests. An interactive Windows folder-picker test still
requires a Windows desktop; Linux native QA does not establish Windows behavior.

Performance verification uses the same project toolchain:

```bash
python3 scripts/performance-probe.py --rounds 4
python3 scripts/performance-smoke.py
```

The paired CPU probe requires Python 3.11+ and the Rust toolchain. The Linux native
performance smoke additionally requires Pillow and Tesseract, plus the native
smoke tools above. Evidence is written under ignored `target/performance-execution/`.
See [PERFORMANCE.md](PERFORMANCE.md) for scope, historical measurements, and limits;
CPU pane timings are not GPU presentation or end-to-end typing latency.

The main app modules are:

- `src/ui.rs`: egui application shell and editor/preview layout.
- `src/ui/editor_layout.rs`: shared revision/width/font-aware editor layouts.
- `src/ui/preview.rs`: retained semantic preview blocks and viewport rendering.
- `src/ui/splitter.rs`: bounded draggable Split layout and reset behavior.
- `src/ui/persistence.rs`: UI save scheduling and revision-aware completion.
- `src/persistence.rs`: ordered native document/session writer and WASM fallback.
- `src/app_state.rs`: documents, folder tabs, saving, recent files, and state
  persistence.
- `src/markdown.rs`: Markdown-to-HTML helper used by tests/export-style code.
- `src/mermaid.rs`: native Mermaid flowchart parsing and rendering.
- `src/theme.rs`: design tokens and theme application.

## Packaging

Velocimd uses `cargo-packager` with `Packager.toml`.

```bash
cargo install cargo-packager --locked
cargo packager --release
```

Package outputs are written to `dist/`.

After merging and final verification, the GitHub Actions release workflow builds
packages on Linux, macOS, and Windows from a tag matching the package version:

```bash
git tag v0.1.6
git push origin v0.1.6
```

The workflow can also be run manually from GitHub Actions.

## Privacy And Security

Velocimd is local-first. It does not require an account and does not send
document contents to a service. The app reads and writes files selected by the
user, plus its local config/state files under the operating system config
directory.

Current guardrails include:

- No `unsafe` code in the project.
- Markdown file opens are limited to regular UTF-8 files up to 16 MiB.
- Unreadable restored files are detached instead of overwritten on shutdown.
- The public HTML helper strips raw HTML and neutralizes dangerous URL schemes.
- Theme font sizes are clamped before being applied to the UI.

## License

Velocimd is released under the GNU Affero General Public License v3.0 only. See
`LICENSE` for the full license text.
