use crate::{commands::Command, document::Document, modes::EditorMode, theme::ThemeConfig};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub documents: Vec<Document>,
    pub active_document: usize,
    #[serde(default)]
    pub folder_tabs: Vec<FolderTab>,
    #[serde(default)]
    pub active_folder_tab: usize,
    pub mode: EditorMode,
    pub command_palette_open: bool,
    pub command_query: String,
    pub theme: ThemeConfig,
    pub window_size: Option<(f32, f32)>,
    pub recent_files: Vec<PathBuf>,
    #[serde(default)]
    pub working_folder: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderTab {
    pub path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedAppState {
    #[serde(default)]
    documents: Vec<PersistedDocument>,
    #[serde(default)]
    active_document: usize,
    #[serde(default)]
    folder_tabs: Vec<FolderTab>,
    #[serde(default)]
    active_folder_tab: usize,
    #[serde(default)]
    mode: EditorMode,
    #[serde(default)]
    command_palette_open: bool,
    #[serde(default)]
    command_query: String,
    #[serde(default)]
    theme: ThemeConfig,
    #[serde(default)]
    window_size: Option<(f32, f32)>,
    #[serde(default)]
    recent_files: Vec<PathBuf>,
    #[serde(default)]
    working_folder: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedDocument {
    id: u64,
    title: String,
    path: Option<PathBuf>,
    #[serde(default)]
    dirty: bool,
    #[serde(default)]
    scratch_content: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

impl PersistedAppState {
    fn from_state(state: &AppState) -> Self {
        Self {
            documents: state
                .documents
                .iter()
                .map(PersistedDocument::from_document)
                .collect(),
            active_document: state.active_document,
            folder_tabs: state.folder_tabs.clone(),
            active_folder_tab: state.active_folder_tab,
            mode: state.mode,
            command_palette_open: state.command_palette_open,
            command_query: state.command_query.clone(),
            theme: state.theme.clone(),
            window_size: state.window_size,
            recent_files: state.recent_files.clone(),
            working_folder: state.working_folder.clone(),
        }
    }

    fn into_state(self) -> AppState {
        AppState {
            documents: self
                .documents
                .into_iter()
                .map(PersistedDocument::into_document)
                .collect(),
            active_document: self.active_document,
            folder_tabs: self.folder_tabs,
            active_folder_tab: self.active_folder_tab,
            mode: self.mode,
            command_palette_open: self.command_palette_open,
            command_query: self.command_query,
            theme: self.theme,
            window_size: self.window_size,
            recent_files: self.recent_files,
            working_folder: self.working_folder,
        }
    }
}

impl PersistedDocument {
    fn from_document(document: &Document) -> Self {
        Self {
            id: document.id,
            title: document.title.clone(),
            path: document.path.clone(),
            dirty: document.dirty,
            scratch_content: document.path.is_none().then(|| document.content.clone()),
            content: None,
        }
    }

    fn into_document(self) -> Document {
        let fallback_content = self.scratch_content.or(self.content).unwrap_or_default();
        let mut path = self.path;
        let (content, dirty) = if let Some(document_path) = path.as_ref() {
            match read_document_content(document_path) {
                Some(content) => (content, self.dirty),
                None => {
                    path = None;
                    (fallback_content, false)
                }
            }
        } else {
            (fallback_content, self.dirty)
        };

        Document {
            id: self.id,
            title: self.title,
            path,
            content,
            dirty,
            revision: 0,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        // Try to load the state from file
        if let Ok(state) = Self::load() {
            return state.normalized();
        }

        Self::fresh()
    }

    pub fn fresh() -> Self {
        let theme = Self::theme_path()
            .ok()
            .and_then(|path| ThemeConfig::load_from(path).ok())
            .unwrap_or_else(ThemeConfig::default_dark);

        Self {
            documents: vec![Document::scratch(
                "Welcome.md",
                "# Velocimd\n\nA fast Markdown reader/editor in Rust.\n\n- Tabs\n- Command palette\n- Edit / preview / split modes\n- TOML themes\n",
            )],
            active_document: 0,
            folder_tabs: Vec::new(),
            active_folder_tab: 0,
            mode: EditorMode::Split,
            command_palette_open: false,
            command_query: String::new(),
            theme,
            window_size: None,
            recent_files: Vec::new(),
            working_folder: None,
        }
    }

    /// Load the app state from the config file.
    pub fn load() -> Result<Self> {
        let path = Self::state_path()?;
        let content = fs::read_to_string(path)?;
        let state: PersistedAppState = serde_json::from_str(&content)?;
        Ok(state.into_state())
    }

    /// Save the app state to the config file.
    pub fn save(&self) -> Result<()> {
        let path = Self::state_path()?;
        let persisted = PersistedAppState::from_state(self);
        let json = serde_json::to_string(&persisted)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Get the path to the state file.
    fn state_path() -> Result<PathBuf> {
        let mut path =
            dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
        path.push("velocimd");
        fs::create_dir_all(&path)?;
        path.push("state.json");
        Ok(path)
    }

    fn theme_path() -> Result<PathBuf> {
        let mut path =
            dirs::config_dir().ok_or_else(|| anyhow::anyhow!("could not find config directory"))?;
        path.push("velocimd");
        fs::create_dir_all(&path)?;
        path.push("theme.toml");
        Ok(path)
    }

    pub fn save_theme(&self) -> Result<()> {
        let path = Self::theme_path()?;
        self.theme.save_to(path)
    }

    pub fn active_document(&self) -> Option<&Document> {
        self.documents.get(self.active_document)
    }

    pub fn active_document_mut(&mut self) -> Option<&mut Document> {
        self.documents.get_mut(self.active_document)
    }

    pub fn new_tab(&mut self) {
        let number = self.documents.len() + 1;
        self.documents.push(Document::scratch(
            format!("Untitled-{number}.md"),
            "# Untitled\n\nStart writing.\n",
        ));
        self.active_document = self.documents.len() - 1;
        let _ = self.save_document_at(self.active_document);
    }

    pub fn close_tab(&mut self) {
        self.close_tab_at(self.active_document);
    }

    pub fn close_tab_at(&mut self, index: usize) {
        if self.documents.len() <= 1 || index >= self.documents.len() {
            return;
        }

        self.documents.remove(index);
        if self.active_document > index {
            self.active_document -= 1;
        } else if self.active_document >= self.documents.len() {
            self.active_document = self.documents.len().saturating_sub(1);
        }
    }

    pub fn open_file(&mut self, path: PathBuf) -> bool {
        let path = fs::canonicalize(&path).unwrap_or(path);
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path.as_ref() == Some(&path))
        {
            self.active_document = index;
            self.remember_recent_file(path);
            return true;
        }

        let Some(content) = read_document_content(&path) else {
            return false;
        };

        let document = Document::from_path(path.clone(), content);
        self.documents.push(document);
        self.active_document = self.documents.len() - 1;
        if let Some(parent) = path.parent() {
            let _ = self.add_folder_tab(parent.to_path_buf());
        }
        self.remember_recent_file(path);
        true
    }

    pub fn save_file(&mut self) -> Option<PathBuf> {
        self.save_document_at(self.active_document)
    }

    pub fn save_file_as(&mut self, path: PathBuf) -> bool {
        let Some(content) = self
            .active_document()
            .map(|document| document.content.clone())
        else {
            return false;
        };

        if fs::write(&path, content).is_err() {
            return false;
        }

        let path = fs::canonicalize(&path).unwrap_or(path);
        let saved_parent = path.parent().map(Path::to_path_buf);
        let Some(document) = self.active_document_mut() else {
            return false;
        };
        document.path = Some(path.clone());
        document.title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string();
        document.dirty = false;
        if let Some(parent) = saved_parent {
            let _ = self.add_folder_tab(parent);
        }
        self.remember_recent_file(path);
        true
    }

    pub fn rename_document(&mut self, index: usize, visible_name: &str) -> Option<PathBuf> {
        if index >= self.documents.len() {
            return None;
        }

        let file_name = markdown_file_name(visible_name);
        let content = self.documents.get(index)?.content.clone();

        let path = if let Some(current_path) = self.documents.get(index)?.path.clone() {
            let folder = current_path.parent().unwrap_or_else(|| Path::new("."));
            let new_path =
                unique_markdown_path_excluding(folder, &file_name, &self.documents, index);

            if new_path != current_path && current_path.exists() {
                fs::rename(&current_path, &new_path).ok()?;
            }

            fs::write(&new_path, content).ok()?;
            new_path
        } else if let Some(folder) = self.active_folder_path() {
            let new_path =
                unique_markdown_path_excluding(folder, &file_name, &self.documents, index);
            fs::write(&new_path, content).ok()?;
            new_path
        } else {
            let document = self.documents.get_mut(index)?;
            document.title = file_name;
            document.dirty = true;
            return None;
        };

        let document = self.documents.get_mut(index)?;
        document.path = Some(path.clone());
        document.title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string();
        document.dirty = false;
        self.remember_recent_file(path.clone());
        Some(path)
    }

    pub fn execute(&mut self, command: Command) {
        match command {
            Command::NewTab => self.new_tab(),
            Command::CloseTab => self.close_tab(),
            Command::TogglePalette => self.command_palette_open = !self.command_palette_open,
            Command::SetMode(mode) => self.mode = mode,
            Command::CycleMode => self.mode = self.mode.next(),
            Command::SelectWorkingFolder
            | Command::OpenFile
            | Command::SaveFile
            | Command::SaveFileAs
            | Command::SwitchThemeLight
            | Command::SwitchThemeDark => {}
        }
    }

    pub fn set_working_folder(&mut self, path: PathBuf) -> bool {
        if fs::create_dir_all(&path).is_err() {
            return false;
        }

        let path = fs::canonicalize(&path).unwrap_or(path);
        self.working_folder = Some(path.clone());
        let _ = self.add_folder_tab(path);
        self.save_all_documents();
        true
    }

    pub fn add_folder_tab(&mut self, path: PathBuf) -> bool {
        if fs::create_dir_all(&path).is_err() {
            return false;
        }

        let path = fs::canonicalize(&path).unwrap_or(path);
        if let Some(index) = self
            .folder_tabs
            .iter()
            .position(|folder_tab| folder_tab.path == path)
        {
            self.active_folder_tab = index;
        } else {
            self.folder_tabs.push(FolderTab { path: path.clone() });
            self.active_folder_tab = self.folder_tabs.len() - 1;
        }

        if self.working_folder.is_none() {
            self.working_folder = Some(path);
        }

        true
    }

    pub fn close_folder_tab_at(&mut self, index: usize) {
        if self.folder_tabs.len() <= 1 || index >= self.folder_tabs.len() {
            return;
        }

        self.folder_tabs.remove(index);
        if self.active_folder_tab > index {
            self.active_folder_tab -= 1;
        } else if self.active_folder_tab >= self.folder_tabs.len() {
            self.active_folder_tab = self.folder_tabs.len().saturating_sub(1);
        }
    }

    pub fn active_folder_path(&self) -> Option<&Path> {
        self.folder_tabs
            .get(self.active_folder_tab)
            .map(|folder_tab| folder_tab.path.as_path())
            .or(self.working_folder.as_deref())
    }

    pub fn save_all_documents(&mut self) {
        for index in 0..self.documents.len() {
            let _ = self.save_document_at(index);
        }
    }

    pub fn save_dirty_documents(&mut self) -> SaveDirtyDocumentsResult {
        let dirty_indices = self
            .documents
            .iter()
            .enumerate()
            .filter_map(|(index, document)| document.dirty.then_some(index))
            .collect::<Vec<_>>();
        let mut result = SaveDirtyDocumentsResult::default();

        for index in dirty_indices {
            match self.save_document_at(index) {
                Some(path) => result.saved.push(path),
                None => result.failed += 1,
            }
        }

        result
    }

    pub fn stream_active_document(&mut self) -> Option<PathBuf> {
        self.save_document_at(self.active_document)
    }

    pub fn save_document_at(&mut self, index: usize) -> Option<PathBuf> {
        let path = self.ensure_document_path(index)?;
        let content = self.documents.get(index)?.content.clone();
        fs::write(&path, content).ok()?;

        let document = self.documents.get_mut(index)?;
        document.dirty = false;
        self.remember_recent_file(path.clone());
        Some(path)
    }

    fn ensure_document_path(&mut self, index: usize) -> Option<PathBuf> {
        if let Some(path) = self.documents.get(index)?.path.clone() {
            return Some(path);
        }

        let title = self.documents.get(index)?.title.clone();
        let path = self.default_document_path(&title)?;
        let document = self.documents.get_mut(index)?;
        document.path = Some(path.clone());
        document.title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string();
        Some(path)
    }

    fn default_document_path(&self, title: &str) -> Option<PathBuf> {
        let folder = self.active_folder_path()?;
        Some(unique_markdown_path(folder, title, &self.documents))
    }

    fn remember_recent_file(&mut self, path: PathBuf) {
        self.recent_files.retain(|existing| existing != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(20);
    }

    fn normalized(mut self) -> Self {
        if self.documents.is_empty() {
            self.documents = Self::fresh().documents;
        }

        Document::repair_ids(&mut self.documents);
        self.normalize_folder_tabs();

        if self.active_document >= self.documents.len() {
            self.active_document = self.documents.len().saturating_sub(1);
        }

        self
    }

    fn normalize_folder_tabs(&mut self) {
        for folder_tab in &mut self.folder_tabs {
            folder_tab.path =
                fs::canonicalize(&folder_tab.path).unwrap_or_else(|_| folder_tab.path.clone());
        }

        let mut unique_tabs: Vec<FolderTab> = Vec::new();
        for folder_tab in self.folder_tabs.drain(..) {
            if !unique_tabs
                .iter()
                .any(|existing| existing.path == folder_tab.path)
            {
                unique_tabs.push(folder_tab);
            }
        }
        self.folder_tabs = unique_tabs;

        if self.folder_tabs.is_empty()
            && let Some(working_folder) = self.working_folder.clone()
        {
            self.folder_tabs.push(FolderTab {
                path: fs::canonicalize(&working_folder).unwrap_or(working_folder),
            });
        }

        if self.working_folder.is_none()
            && let Some(folder_tab) = self.folder_tabs.first()
        {
            self.working_folder = Some(folder_tab.path.clone());
        }

        if self.active_folder_tab >= self.folder_tabs.len() {
            self.active_folder_tab = self.folder_tabs.len().saturating_sub(1);
        }
    }
}

#[derive(Debug, Default)]
pub struct SaveDirtyDocumentsResult {
    pub saved: Vec<PathBuf>,
    pub failed: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

fn unique_markdown_path(folder: &Path, title: &str, documents: &[Document]) -> PathBuf {
    let file_name = markdown_file_name(title);
    unique_markdown_path_for_file_name(folder, &file_name, documents, None)
}

fn unique_markdown_path_excluding(
    folder: &Path,
    file_name: &str,
    documents: &[Document],
    excluded_index: usize,
) -> PathBuf {
    unique_markdown_path_for_file_name(folder, file_name, documents, Some(excluded_index))
}

fn unique_markdown_path_for_file_name(
    folder: &Path,
    file_name: &str,
    documents: &[Document],
    excluded_index: Option<usize>,
) -> PathBuf {
    let (stem, extension) = split_stem_extension(file_name);

    let mut suffix = 1;
    loop {
        let candidate_name = if suffix == 1 {
            format!("{stem}.{extension}")
        } else {
            format!("{stem}-{suffix}.{extension}")
        };
        let candidate = folder.join(candidate_name);
        let is_open = documents.iter().enumerate().any(|(index, document)| {
            excluded_index != Some(index) && document.path.as_ref() == Some(&candidate)
        });
        let is_current_document = excluded_index
            .and_then(|index| documents.get(index))
            .and_then(|document| document.path.as_ref())
            == Some(&candidate);

        if is_current_document || (!candidate.exists() && !is_open) {
            return candidate;
        }

        suffix += 1;
    }
}

fn read_document_content(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        return None;
    }

    fs::read_to_string(path).ok()
}

fn markdown_file_name(title: &str) -> String {
    let mut sanitized = String::with_capacity(title.len());
    for character in title.trim().chars() {
        if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_' | '.') {
            sanitized.push(character);
        } else {
            sanitized.push('_');
        }
    }

    let sanitized = sanitized.trim_matches([' ', '.']).trim();
    let mut file_name = if sanitized.is_empty() {
        "Untitled".to_string()
    } else {
        sanitized.to_string()
    };

    if !file_name.ends_with(".md") && !file_name.ends_with(".markdown") {
        file_name.push_str(".md");
    }

    file_name
}

fn split_stem_extension(file_name: &str) -> (String, String) {
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Untitled")
        .to_string();
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("md")
        .to_string();
    (stem, extension)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn normalized_repairs_duplicate_document_ids() {
        let mut state = AppState::fresh();
        let duplicate_id = state.documents[0].id;
        let mut duplicate = Document::scratch("Duplicate.md", "");
        duplicate.id = duplicate_id;
        state.documents.push(duplicate);

        let normalized = state.normalized();

        assert_unique_document_ids(&normalized.documents);
    }

    #[test]
    fn normalized_advances_document_id_generator_past_loaded_ids() {
        let mut state = AppState::fresh();
        let next_id_probe = Document::scratch("Probe.md", "").id;
        let would_be_reused_without_normalization = next_id_probe + 1;
        state.documents[0].id = would_be_reused_without_normalization;

        let mut normalized = state.normalized();
        normalized.new_tab();

        assert_unique_document_ids(&normalized.documents);
        assert_ne!(
            normalized.documents[0].id,
            normalized
                .active_document()
                .expect("new tab should exist")
                .id
        );
    }

    #[test]
    fn persisted_state_omits_file_backed_document_content() {
        let mut state = AppState::fresh();
        state.documents[0].path = Some(PathBuf::from("/tmp/welcome.md"));
        state.documents[0].content = "# Large file-backed body".to_string();
        state
            .documents
            .push(Document::scratch("Scratch.md", "# Scratch body"));

        let persisted = PersistedAppState::from_state(&state);

        assert!(persisted.documents[0].content.is_none());
        assert!(persisted.documents[0].scratch_content.is_none());
        assert_eq!(
            persisted.documents[1].scratch_content.as_deref(),
            Some("# Scratch body")
        );
    }

    #[test]
    fn persisted_state_drops_unreadable_file_path_without_dirtying_document() {
        let missing_path = std::env::temp_dir().join(format!(
            "velocimd-missing-{}-{}.md",
            std::process::id(),
            "persisted"
        ));
        let _ = fs::remove_file(&missing_path);
        let persisted = PersistedDocument {
            id: 1,
            title: "Missing.md".to_string(),
            path: Some(missing_path),
            dirty: true,
            scratch_content: None,
            content: None,
        };

        let document = persisted.into_document();

        assert!(document.path.is_none());
        assert!(!document.dirty);
        assert!(document.content.is_empty());
    }

    #[test]
    fn open_file_rejects_directories() {
        let folder = std::env::temp_dir().join(format!(
            "velocimd-directory-{}-{}",
            std::process::id(),
            "open"
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("temp directory should be created");
        let mut state = AppState::fresh();

        assert!(!state.open_file(folder.clone()));

        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn open_file_rejects_oversized_documents() {
        let path = std::env::temp_dir().join(format!(
            "velocimd-large-{}-{}.md",
            std::process::id(),
            "open"
        ));
        let file = fs::File::create(&path).expect("large fixture should be created");
        file.set_len(MAX_DOCUMENT_BYTES + 1)
            .expect("large fixture should be sized");
        let mut state = AppState::fresh();

        assert!(!state.open_file(path.clone()));

        let _ = fs::remove_file(path);
    }

    fn assert_unique_document_ids(documents: &[Document]) {
        let ids = documents
            .iter()
            .map(|document| document.id)
            .collect::<HashSet<_>>();

        assert_eq!(ids.len(), documents.len());
    }
}
