#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMode {
    Edit,
    Preview,
    Split,
}

impl EditorMode {
    pub fn next(self) -> Self {
        match self {
            Self::Edit => Self::Preview,
            Self::Preview => Self::Split,
            Self::Split => Self::Edit,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Edit => "Edit",
            Self::Preview => "Preview",
            Self::Split => "Split",
        }
    }
}

impl Default for EditorMode {
    fn default() -> Self {
        Self::Split
    }
}
