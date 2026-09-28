# Velocimd toolbar icons

Original, monochrome line art for Velocimd, licensed with the repository.
These are toolbar assets, not replacements for the application logo.

## Design contract

- A shared 24 × 24 viewBox and 1.75-unit stroke, with round caps and joins.
- No gradients, shadows, fonts, embedded images, or network dependencies.
- Transparent backgrounds and white strokes; egui tints the exact same texture
  using the current theme. The white source is intentional, not a light-theme bug.
- Centered 18-point square icons in the existing 30 × 28 toolbar buttons;
  14-point icons in compact 20 × 20 tab controls. No nonuniform stretching.
- Muted idle state, brighter hover/focus, and theme-accent selected state.
- New note is a page with a plus, while add-folder-tab remains a plain plus.
  Save As adds a pencil; Edit has a closed pencil silhouette; Preview and Moon
  use smooth curves instead of small jagged line segments.
- Preserve command routing, shortcuts, tooltip text, hit targets, and the app logo.

## Implementation and verification

`src/icons.rs` embeds these sources at compile time. Existing egui SVG loaders
rasterize and cache at display scale; no new crate or runtime file lookup is needed.
`tests/toolbar_icons.rs` checks source coverage, raster bounds, scale and tinting,
texture-cache reuse, and real widget rendering.

Run `cargo test --locked`, `cargo clippy --all-targets --locked -- -D warnings`,
`cargo build --locked`, and `python3 scripts/native-smoke.py`.
The dedicated `python3 scripts/icon-smoke.py` captures the native app in both
Velocidark and Velocilight and exercises toolbar clicks in an isolated Xvfb
session with disposable notes/settings. It writes review images under
`target/icon-review/`. No user session is opened or changed.
