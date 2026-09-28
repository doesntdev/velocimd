# Velocimd v0.1.5 — a quieter toolbar

## Refreshed

- A cohesive family of 15 minimalist SVG icons replaces the hand-drawn toolbar symbols, with consistent rounded strokes, balanced proportions, and clearer file, save, edit, preview, and theme controls.
- The same icons adapt to Velocidark, Velocilight, and custom themes: muted at rest, brighter on hover or keyboard focus, and accent-colored for the selected mode.
- Square, centered artwork stays sharp at normal and high-DPI scales without stretching inside buttons.
- Keyboard focus is more visible, and icon buttons expose their labels and selected state to accessibility tooling.

## Kept familiar

- The application logo, commands, keyboard shortcuts, tooltips, and button hit targets are unchanged.
- Icons are embedded in the application and use the existing SVG loader/cache. No added dependency, icon font, runtime asset download, or network request.
- This release is limited to the icon refresh. The separate navigation/sidebar redesign is not included.

## Verification

- Added regression coverage for icon source coverage, clipping, tinting, high-DPI raster sizes, texture reuse, and widget sizing/selection.
- Added an isolated native toolbar smoke test for dark/light themes, normal and double-DPI rendering, real mode/theme clicks, save preservation, and graceful exit.
- Retained native editing, autosave, command-palette, and reopen smoke coverage.

## Downloads

- **Linux:** Debian/Ubuntu `.deb`, `.AppImage`, or Arch package archive/PKGBUILD.
- **macOS:** universal `.dmg` for Apple Silicon and Intel.
- **Windows:** x64 NSIS setup installer.
- **Integrity:** `SHA256SUMS` covers all six package assets.

The app remains unsigned/not notarized; macOS Gatekeeper and Windows SmartScreen may warn. Native GUI interaction is exercised on Linux; successful Windows/macOS package builds alone are not a claim of GUI certification.

**Full changelog:** https://github.com/doesntdev/velocimd/compare/v0.1.4...v0.1.5
