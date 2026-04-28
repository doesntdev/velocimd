# Velocimd MVP Specification

## Overview
Velocimd is a cross‑platform markdown editor focused on simplicity and speed. The MVP delivers a tabbed interface with a powerful markdown editing surface, a command palette, and seamless switching between edit, preview, and split views. Themes are easily customizable, and the app targets Windows, macOS, and Linux.

## Core Features

### 1. Tabbed Interface
- Multiple markdown documents can be opened in tabs.
- Tab bar supports drag‑and‑drop reordering, closing, and new‑tab button.
- Each tab holds its own editor state (content, cursor position, view mode).

### 2. Markdown Editor Surface
- Built on a performant, extensible textarea‑based or CodeMirror‑like component.
- Supports CommonMark syntax with extensions (tables, task lists, footnotes).
- Inline formatting shortcuts (Ctrl/Bold, Ctrl/Italic, etc.).
- Syntax highlighting for code fences.
- Live character/word count (optional).

### 3. Command Palette
- Invoked via Ctrl+Shift+P (or Cmd+Shift+P on macOS).
- Fuzzy search over available commands.
- Commands include:
  - File operations: New, Open, Save, Save As, Close, Close All.
  - Edit operations: Undo, Redo, Cut, Copy, Paste, Select All.
  - View operations: Toggle Edit Mode, Toggle Preview Mode, Toggle Split View.
  - Theme operations: List themes, Switch theme.
  - Settings: Open preferences.
- Extensible architecture for plugins to add commands.

### 4. View Modes
Each tab can be in one of three modes, togglable via command palette or UI buttons:
- **Edit**: Full‑screen editor.
- **Preview**: Rendered markdown (using a safe markdown‑to‑HTML renderer like pulldown‑cmark + sanitizer).
- **Split**: Editor on left/right, preview on the opposite side, synchronized scrolling.

### 5. Theme Customization
- Themes are defined via simple JSON or CSS files.
- Built‑in light and dark themes.
- Theme selector in command palette and/or settings menu.
- Users can create and load custom themes by placing files in a `themes/` directory.

### 6. File System Integration
- Open/save files to local disk.
- Recent files list (persisted across launches).
- Basic file‑explorer sidebar (optional for MVP) showing opened folder and enabling file creation/deletion.

### 7. Cross‑Platform Build Targets
- Written in Rust with a GUI framework that supports Windows, macOS, and Linux (e.g., Druid, Iced, egui, or Tauri + webview).
- Single binary per platform, no external runtime beyond OS‑provided libraries.
- Packaging scripts for MSI/AppInstaller (Windows), DMG/PKG (macOS), AppImage/deb/rpm (Linux).

### 8. Performance & Reliability
- Startup time < 2 seconds on typical hardware.
- Memory footprint < 150 MB idle.
- Graceful handling of large markdown files (up to ~100 MB) with virtual scrolling if needed.
- Crash‑safe: periodic autosave to temporary location.

## Non‑Goals for MVP
- Real‑time collaboration.
- Advanced plugin system beyond command palette.
- Built‑in file navigator/tree view (may be added later).
- Export to PDF/HTML (can be added via command palette later).
- Mobile/tablet support.

## Success Criteria
- Application launches and presents an empty tab.
- User can create, edit, save, and close markdown files.
- Command palette is functional and includes core commands.
- Switching between edit, preview, and split view works correctly.
- At least two themes (light/dark) are selectable and persist across restarts.
- Binary builds and runs on Windows 10+, macOS 11+, and a recent Linux distribution (Ubuntu 22.04+).
- Basic automated tests cover file load/save and mode switching.

## Next Steps
1. Choose GUI framework and set up project structure.
2. Implement tab container and basic file‑tab widget.
3. Integrate markdown editor component with syntax highlighting.
4. Add markdown‑to‑HTML preview renderer.
5. Wire up command palette with command registry.
6. Implement theme loading and switching mechanism.
7. Add file‑open/save dialogs and recent‑file list.
8. Create build scripts for each target platform.
9. Write MVP‑level unit and integration tests.
10. Package and distribute installerers.
