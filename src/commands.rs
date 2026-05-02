use crate::modes::EditorMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewTab,
    SelectWorkingFolder,
    OpenFile,
    SaveFile,
    SaveFileAs,
    CloseTab,
    TogglePalette,
    SetMode(EditorMode),
    CycleMode,
    SwitchThemeLight,
    SwitchThemeDark,
}

impl Command {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewTab => "New Markdown file",
            Self::SelectWorkingFolder => "Select working folder...",
            Self::OpenFile => "Open file...",
            Self::SaveFile => "Save file",
            Self::SaveFileAs => "Save file as...",
            Self::CloseTab => "Close tab",
            Self::TogglePalette => "Toggle command palette",
            Self::SetMode(EditorMode::Edit) => "Switch to edit mode",
            Self::SetMode(EditorMode::Preview) => "Switch to preview mode",
            Self::SetMode(EditorMode::Split) => "Switch to split view",
            Self::CycleMode => "Cycle edit / preview / split",
            Self::SwitchThemeLight => "Switch to light theme",
            Self::SwitchThemeDark => "Switch to dark theme",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Self::NewTab => "Ctrl+N",
            Self::SelectWorkingFolder => "Ctrl+Shift+O",
            Self::OpenFile => "Ctrl+O",
            Self::SaveFile => "Ctrl+S",
            Self::SaveFileAs => "Ctrl+Shift+S",
            Self::CloseTab => "Ctrl+W",
            Self::TogglePalette => "Ctrl+K",
            Self::SetMode(EditorMode::Edit) => "Ctrl+1",
            Self::SetMode(EditorMode::Preview) => "Ctrl+2",
            Self::SetMode(EditorMode::Split) => "Ctrl+3",
            Self::CycleMode => "Ctrl+Tab",
            Self::SwitchThemeLight => "Alt+L",
            Self::SwitchThemeDark => "Alt+D",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::NewTab => "+",
            Self::SelectWorkingFolder => "[F]",
            Self::OpenFile => "[O]",
            Self::SaveFile => "[S]",
            Self::SaveFileAs => "[S+]",
            Self::CloseTab => "x",
            Self::TogglePalette => "[?]",
            Self::SetMode(EditorMode::Edit) => "[E]",
            Self::SetMode(EditorMode::Preview) => "[P]",
            Self::SetMode(EditorMode::Split) => "[/]",
            Self::CycleMode => "[>]",
            Self::SwitchThemeLight => "[L]",
            Self::SwitchThemeDark => "[D]",
        }
    }

    pub fn all() -> &'static [Command] {
        &[
            Self::NewTab,
            Self::SelectWorkingFolder,
            Self::OpenFile,
            Self::SaveFile,
            Self::SaveFileAs,
            Self::CloseTab,
            Self::SetMode(EditorMode::Edit),
            Self::SetMode(EditorMode::Preview),
            Self::SetMode(EditorMode::Split),
            Self::CycleMode,
            Self::TogglePalette,
            Self::SwitchThemeLight,
            Self::SwitchThemeDark,
        ]
    }

    pub fn toolbar() -> &'static [Command] {
        &[
            Self::NewTab,
            Self::SelectWorkingFolder,
            Self::OpenFile,
            Self::SaveFile,
            Self::SaveFileAs,
            Self::CloseTab,
            Self::SetMode(EditorMode::Edit),
            Self::SetMode(EditorMode::Preview),
            Self::SetMode(EditorMode::Split),
            Self::CycleMode,
            Self::SwitchThemeLight,
            Self::SwitchThemeDark,
        ]
    }
}
