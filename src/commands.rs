use crate::modes::EditorMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    NewTab,
    SelectWorkingFolder,
    OpenFile,
    SaveFile,
    SaveFileAs,
    CloseFolderTab,
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
            Self::CloseFolderTab => "Close folder tab",
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
            Self::CloseFolderTab => "Ctrl+W",
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
            Self::CloseFolderTab => "x",
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
            Self::CloseFolderTab,
            Self::SetMode(EditorMode::Edit),
            Self::SetMode(EditorMode::Preview),
            Self::SetMode(EditorMode::Split),
            Self::CycleMode,
            Self::TogglePalette,
            Self::SwitchThemeLight,
            Self::SwitchThemeDark,
        ]
    }

    /// Case-insensitive substring-first, then subsequence matching for palette search.
    pub fn matching(query: &str) -> Vec<Command> {
        let query = query.trim().to_lowercase();
        let mut matches: Vec<_> = Self::all()
            .iter()
            .copied()
            .filter(|command| *command != Self::TogglePalette)
            .filter_map(|command| {
                let label = command.label().to_lowercase();
                if label.contains(&query) {
                    return Some((0, command));
                }
                let mut chars = label.chars();
                query
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .all(|needle| chars.any(|c| c == needle))
                    .then_some((1, command))
            })
            .collect();
        matches.sort_by_key(|(rank, _)| *rank);
        matches.into_iter().map(|(_, command)| command).collect()
    }

    pub fn toolbar() -> &'static [Command] {
        &[
            Self::NewTab,
            Self::SelectWorkingFolder,
            Self::OpenFile,
            Self::SaveFile,
            Self::SaveFileAs,
            Self::CloseFolderTab,
            Self::SetMode(EditorMode::Edit),
            Self::SetMode(EditorMode::Preview),
            Self::SetMode(EditorMode::Split),
            Self::CycleMode,
            Self::SwitchThemeLight,
            Self::SwitchThemeDark,
        ]
    }
}
