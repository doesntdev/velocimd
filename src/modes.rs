use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditorMode {
    Edit,
    Preview,
    #[default]
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
