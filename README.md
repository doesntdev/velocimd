# Velocimd

Velocimd is a lightning-fast Markdown reader and editor written in Rust.

Initial product direction:

- Simple native desktop app.
- Tabbed Markdown documents.
- Robust Markdown editor surface.
- Built-in command palette.
- Rapid edit, preview, and split-view mode switching.
- Easy app theme customization through TOML theme files.

## Current status

Fresh scaffold. The first implementation establishes the core state model, Markdown rendering, theme parsing, and a native `egui` shell.

## Development

```bash
cargo test
cargo run
```

## Packaging

```bash
cargo install cargo-packager --locked
cargo packager --release
```

Release builds are also available through the GitHub Actions workflow in
`.github/workflows/release.yml`. Push a `v*` tag or run the workflow manually to
build Linux, macOS, and Windows packages.
