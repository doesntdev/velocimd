use serde::{Deserialize, Serialize};
use std::collections::HashSet;
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
    #[serde(default)]
    pub revision: u64,
}

impl Document {
    pub fn scratch(title: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: next_id(),
            title: title.into(),
            path: None,
            content: content.into(),
            dirty: false,
            revision: 0,
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
            revision: 0,
        }
    }

    pub fn set_content(&mut self, content: String) {
        if self.content != content {
            self.content = content;
            self.mark_changed();
        }
    }

    pub fn mark_changed(&mut self) {
        self.dirty = true;
        self.revision = self.revision.saturating_add(1);
    }

    pub fn display_title(&self) -> String {
        let title = self.visible_title();
        if self.dirty {
            format!("{title} •")
        } else {
            title
        }
    }

    pub fn visible_title(&self) -> String {
        strip_markdown_extension(&self.title).to_string()
    }

    pub(crate) fn repair_ids(documents: &mut [Self]) {
        let mut seen = HashSet::new();
        let mut max_id = documents
            .iter()
            .map(|document| document.id)
            .max()
            .unwrap_or(0);

        for document in documents {
            if document.id == 0 || seen.contains(&document.id) {
                max_id = max_id.saturating_add(1);
                document.id = max_id;
            }

            seen.insert(document.id);
            max_id = max_id.max(document.id);
        }

        advance_next_id_to(max_id.saturating_add(1));
    }
}

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

fn advance_next_id_to(next: u64) {
    let mut current = NEXT_ID.load(Ordering::Relaxed);
    while current < next {
        match NEXT_ID.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

pub(crate) fn is_markdown_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
}

pub(crate) fn strip_markdown_extension(title: &str) -> &str {
    match title.rsplit_once('.') {
        Some((stem, extension)) if is_markdown_extension(extension) => stem,
        _ => title,
    }
}
