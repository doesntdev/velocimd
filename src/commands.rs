use crate::modes::EditorMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewTab,
    TogglePalette,
    SetMode(EditorMode),
    CycleMode,
}

impl Command {
    pub fn label(self) -> &'static str {
        match self {
            Self::NewTab => "New tab",
            Self::TogglePalette => "Toggle command palette",
            Self::SetMode(EditorMode::Edit) => "Switch to edit mode",
            Self::SetMode(EditorMode::Preview) => "Switch to preview mode",
            Self::SetMode(EditorMode::Split) => "Switch to split view",
            Self::CycleMode => "Cycle edit / preview / split",
        }
    }

    pub fn shortcut(self) -> &'static str {
        match self {
            Self::NewTab => "Ctrl+N",
            Self::TogglePalette => "Ctrl+K",
            Self::SetMode(EditorMode::Edit) => "Ctrl+1",
            Self::SetMode(EditorMode::Preview) => "Ctrl+2",
            Self::SetMode(EditorMode::Split) => "Ctrl+3",
            Self::CycleMode => "Ctrl+Tab",
        }
    }

    pub fn all() -> &'static [Command] {
        &[
            Self::NewTab,
            Self::SetMode(EditorMode::Edit),
            Self::SetMode(EditorMode::Preview),
            Self::SetMode(EditorMode::Split),
            Self::CycleMode,
            Self::TogglePalette,
        ]
    }
}
