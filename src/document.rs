use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: u64,
    pub title: String,
    pub path: Option<PathBuf>,
    pub content: String,
    pub dirty: bool,
}

impl Document {
    pub fn scratch(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: next_id(),
            title: title.into(),
            path: None,
            content: content.into(),
            dirty: false,
        }
    }

    pub fn from_path(path: PathBuf, content: impl Into<String>) -> Self {
        let title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string();

        Self {
            id: next_id(),
            title,
            path: Some(path),
            content: content.into(),
            dirty: false,
        }
    }

    pub fn set_content(&mut self, content: String) {
        if self.content != content {
            self.content = content;
            self.dirty = true;
        }
    }

    pub fn display_title(&self) -> String {
        if self.dirty {
            format!("{} •", self.title)
        } else {
            self.title.clone()
        }
    }
}

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}
