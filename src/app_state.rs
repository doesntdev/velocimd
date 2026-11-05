use crate::{
    commands::Command,
    document::Document,
    file_io::atomic_write,
    modes::EditorMode,
    persistence::{DocumentWrite, DocumentWriteResult, SessionWrite},
    theme::ThemeConfig,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_DOCUMENT_BYTES: u64 = 16 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static DOCUMENT_READS: std::cell::Cell<(usize, usize)> = const { std::cell::Cell::new((0, 0)) };
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub documents: Vec<Document>,
    pub active_document: usize,
    #[serde(default)]
    pub folder_tabs: Vec<FolderTab>,
    #[serde(default)]
    pub active_folder_tab: usize,
    pub mode: EditorMode,
    #[serde(default = "default_split_ratio")]
    pub split_ratio: f32,
    pub command_palette_open: bool,
    pub command_query: String,
    pub theme: ThemeConfig,
    pub window_size: Option<(f32, f32)>,
    pub recent_files: Vec<PathBuf>,
    #[serde(default)]
    pub working_folder: Option<PathBuf>,
    #[serde(skip)]
    storage_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FolderTab {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PersistedAppState {
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
    #[serde(default = "default_split_ratio")]
    split_ratio: f32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            split_ratio: normalize_split_ratio(state.split_ratio),
            command_palette_open: false,
            command_query: String::new(),
            theme: state.theme.clone(),
            window_size: state.window_size,
            recent_files: state.recent_files.clone(),
            working_folder: state.working_folder.clone(),
        }
    }

    fn into_state(self) -> AppState {
        let active = self
            .active_document
            .min(self.documents.len().saturating_sub(1));
        AppState {
            documents: self
                .documents
                .into_iter()
                .enumerate()
                .map(|(index, document)| document.restore(index == active))
                .collect(),
            active_document: self.active_document,
            folder_tabs: self.folder_tabs,
            active_folder_tab: self.active_folder_tab,
            mode: self.mode,
            split_ratio: normalize_split_ratio(self.split_ratio),
            command_palette_open: self.command_palette_open,
            command_query: self.command_query,
            theme: self.theme,
            window_size: self.window_size,
            recent_files: self.recent_files,
            working_folder: self.working_folder,
            storage_path: None,
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
            scratch_content: (document.path.is_none() || document.dirty)
                .then(|| document.content.clone()),
            content: None,
        }
    }

    #[cfg(test)]
    fn into_document(self) -> Document {
        self.restore(true)
    }

    fn restore(self, eager: bool) -> Document {
        let snapshot = self.scratch_content.or(self.content);
        if !eager
            && !self.dirty
            && snapshot.is_none()
            && self.path.as_deref().is_some_and(document_metadata_is_valid)
        {
            return Document {
                id: self.id,
                title: self.title,
                path: self.path,
                content: String::new(),
                dirty: false,
                revision: 0,
                loaded: false,
                identity_revision: 0,
            };
        }
        let was_scratch = self.path.is_none();
        let mut path = self.path;
        let disk_content = path.as_deref().and_then(read_document_content);
        let (content, dirty) = if self.dirty && snapshot.is_some() {
            let recovered = snapshot.unwrap_or_default();
            if disk_content.as_deref() == Some(recovered.as_str()) {
                (recovered, false)
            } else {
                // Preserve the draft separately; never autosave it over changed disk content.
                path = None;
                (recovered, true)
            }
        } else if let Some(content) = disk_content {
            (content, false)
        } else {
            path = None;
            (snapshot.unwrap_or_default(), self.dirty && was_scratch)
        };

        Document {
            id: self.id,
            title: self.title,
            path,
            content,
            dirty,
            revision: 0,
            loaded: true,
            identity_revision: 0,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        // Try to load the state from file
        if let Ok(state) = Self::load() {
            return state;
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
            split_ratio: default_split_ratio(),
            command_palette_open: false,
            command_query: String::new(),
            theme,
            window_size: None,
            recent_files: Vec::new(),
            working_folder: None,
            storage_path: None,
        }
    }

    /// Load the app state from the config file.
    pub fn load() -> Result<Self> {
        Self::load_from(Self::state_path()?)
    }

    /// Load a session from an explicit location; subsequent saves stay in that location.
    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;
        let persisted: PersistedAppState = serde_json::from_str(&content)?;
        let mut state = persisted.into_state().normalized();
        state.storage_path = Some(path.to_path_buf());
        Ok(state)
    }

    /// Save the app state to the config file.
    pub fn save(&self) -> Result<()> {
        self.prepare_session_write()?
            .write()
            .map_err(anyhow::Error::msg)
    }

    pub fn save_to(&self, path: impl AsRef<Path>) -> Result<()> {
        self.prepare_session_write_to(path)
            .write()
            .map_err(anyhow::Error::msg)
    }

    pub fn prepare_session_write(&self) -> Result<SessionWrite> {
        let path = match &self.storage_path {
            Some(path) => path.clone(),
            None => Self::state_path()?,
        };
        Ok(self.prepare_session_write_to(path))
    }

    /// Explicit target for isolated fixtures and callers not using default config.
    pub fn prepare_session_write_to(&self, path: impl AsRef<Path>) -> SessionWrite {
        SessionWrite {
            path: path.as_ref().to_path_buf(),
            snapshot: PersistedAppState::from_state(self),
        }
    }

    /// Get the path to the state file.
    fn state_path() -> Result<PathBuf> {
        let mut path =
            dirs::config_dir().ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
        path.push("velocimd");
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
        let path = if let Some(folder) = self.storage_path.as_deref().and_then(Path::parent) {
            fs::create_dir_all(folder)?;
            folder.join("theme.toml")
        } else {
            Self::theme_path()?
        };
        self.theme.save_to(path)
    }

    pub fn active_document(&self) -> Option<&Document> {
        self.documents.get(self.active_document)
    }

    pub fn active_document_mut(&mut self) -> Option<&mut Document> {
        self.ensure_document_loaded(self.active_document);
        self.documents.get_mut(self.active_document)
    }

    /// Hydrate once. On failure detach the unavailable file as in legacy restore;
    /// never expose an unloaded placeholder to any writer.
    pub fn ensure_document_loaded(&mut self, index: usize) -> bool {
        let Some(document) = self.documents.get_mut(index) else {
            return false;
        };
        if document.loaded {
            return true;
        }
        let content = document.path.as_deref().and_then(read_document_content);
        document.loaded = true;
        document.revision = document.revision.saturating_add(1);
        if let Some(content) = content {
            document.content = content;
            true
        } else {
            log::warn!(
                "Could not load document {} ({:?})",
                document.id,
                document.path
            );
            document.path = None;
            document.mark_identity_changed();
            false
        }
    }

    pub fn new_tab(&mut self) {
        self.new_tab_deferred();
        let _ = self.save_document_at(self.active_document);
    }

    /// Create the buffer immediately; the UI submits its first durable write.
    pub fn new_tab_deferred(&mut self) {
        let number = self.documents.len() + 1;
        self.documents.push(Document::scratch(
            format!("Untitled-{number}.md"),
            "# Untitled\n\nStart writing.\n",
        ));
        self.active_document = self.documents.len() - 1;
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
        self.ensure_document_loaded(self.active_document);
    }

    pub fn open_file(&mut self, path: PathBuf) -> bool {
        let path = fs::canonicalize(&path).unwrap_or(path);
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path.as_ref() == Some(&path))
        {
            self.active_document = index;
            if !self.ensure_document_loaded(index) {
                return false;
            }
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

    pub fn path_owned_by_other(&self, index: usize, path: &Path) -> bool {
        let path = normalized_path(path);
        self.documents.iter().enumerate().any(|(owner, document)| {
            owner != index
                && document
                    .path
                    .as_deref()
                    .is_some_and(|existing| existing == path)
        })
    }

    pub fn save_file(&mut self) -> Option<PathBuf> {
        self.save_document_at(self.active_document)
    }

    pub fn save_file_as(&mut self, path: PathBuf) -> bool {
        if self.path_owned_by_other(self.active_document, &path) {
            return false;
        }
        let path = normalized_path(&path);
        if !self.ensure_document_loaded(self.active_document) {
            return false;
        }
        let Some(content) = self
            .active_document()
            .map(|document| document.content.clone())
        else {
            return false;
        };

        if atomic_write(&path, content.as_bytes(), true).is_err() {
            return false;
        }

        let path = fs::canonicalize(&path).unwrap_or(path);
        let saved_parent = path.parent().map(Path::to_path_buf);
        let Some(document) = self.active_document_mut() else {
            return false;
        };
        document.mark_identity_changed();
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

        if !self.ensure_document_loaded(index) {
            return None;
        }
        let file_name = markdown_file_name(visible_name);
        let content = self.documents.get(index)?.content.clone();

        let path = if let Some(current_path) = self.documents.get(index)?.path.clone() {
            let folder = current_path.parent().unwrap_or_else(|| Path::new("."));
            let new_path =
                unique_markdown_path_excluding(folder, &file_name, &self.documents, index);

            crate::file_io::rename_with_content(&current_path, &new_path, content.as_bytes())
                .ok()?;
            new_path
        } else if let Some(folder) = self.active_folder_path() {
            let new_path =
                unique_markdown_path_excluding(folder, &file_name, &self.documents, index);
            atomic_write(&new_path, content.as_bytes(), false).ok()?;
            new_path
        } else {
            let document = self.documents.get_mut(index)?;
            document.title = file_name;
            document.mark_changed();
            document.mark_identity_changed();
            return None;
        };

        let document = self.documents.get_mut(index)?;
        document.mark_identity_changed();
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
            Command::CloseFolderTab => self.close_folder_tab_at(self.active_folder_tab),
            Command::TogglePalette => self.command_palette_open = !self.command_palette_open,
            Command::SetMode(mode) => self.mode = mode,
            Command::CycleMode => self.mode = self.mode.next(),
            Command::ToggleTheme => self.theme = self.theme.toggled(),
            Command::SelectWorkingFolder
            | Command::OpenFile
            | Command::SaveFile
            | Command::SaveFileAs
            | Command::SwitchThemeLight
            | Command::SwitchThemeDark => {}
        }
    }

    pub fn set_working_folder(&mut self, path: PathBuf) -> bool {
        if !self.set_working_folder_deferred(path) {
            return false;
        }
        let _ = self.save_dirty_documents();
        true
    }

    /// Change metadata without running note fsyncs on the UI thread.
    pub fn set_working_folder_deferred(&mut self, path: PathBuf) -> bool {
        if fs::create_dir_all(&path).is_err() {
            return false;
        }

        let path = fs::canonicalize(&path).unwrap_or(path);
        self.working_folder = Some(path.clone());
        let _ = self.add_folder_tab(path);
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
            .filter_map(|(index, document)| {
                (document.dirty || document.path.is_none()).then_some(index)
            })
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
        let document = self.documents.get(index)?;
        if !document.loaded && !document.dirty {
            // A synchronous save-all must not hydrate or truncate clean deferred bodies.
            return document.path.clone();
        }
        let result = self.prepare_document_write(index)?.write();
        let path = result.result.as_ref().ok().cloned();
        self.apply_write_result(&result);
        path
    }

    pub fn prepare_dirty_writes(&self) -> Vec<DocumentWrite> {
        let (writes, errors) = self.prepare_dirty_writes_with_errors();
        for error in errors {
            log::warn!("{error}");
        }
        writes
    }

    /// The controller should surface these named preparation failures alongside
    /// worker failures. No failed preparation is marked clean or dropped from recovery.
    pub fn prepare_dirty_writes_with_errors(&self) -> (Vec<DocumentWrite>, Vec<String>) {
        let mut writes = Vec::new();
        let mut errors = Vec::new();
        let mut reserved = HashSet::new();
        for (index, document) in self.documents.iter().enumerate() {
            if !document.dirty && document.path.is_some() {
                continue;
            }
            match self.prepare_write(index, &reserved) {
                Ok(write) => {
                    reserved.insert(write.path.clone());
                    writes.push(write);
                }
                Err(error) => errors.push(error),
            }
        }
        (writes, errors)
    }

    pub fn prepare_document_write(&self, index: usize) -> Option<DocumentWrite> {
        match self.prepare_write(index, &HashSet::new()) {
            Ok(write) => Some(write),
            Err(error) => {
                log::warn!("{error}");
                None
            }
        }
    }

    fn prepare_write(
        &self,
        index: usize,
        reserved: &HashSet<PathBuf>,
    ) -> Result<DocumentWrite, String> {
        let document = self
            .documents
            .get(index)
            .ok_or_else(|| format!("Could not prepare document at missing index {index}"))?;
        let failure = |reason: &str| {
            format!(
                "Could not prepare document {} ({}): {reason}",
                document.id, document.title
            )
        };
        if !document.loaded {
            return Err(failure("content is not loaded"));
        }
        let path = if let Some(path) = &document.path {
            normalized_path(path)
        } else {
            let folder = self
                .active_folder_path()
                .ok_or_else(|| failure("no working folder"))?;
            let file_name = markdown_file_name(&document.title);
            unique_markdown_path_for_file_name(
                folder,
                &file_name,
                &self.documents,
                None,
                Some(reserved),
            )
        };
        // All model-owned paths were normalized when acquired. Only this incoming
        // target needs normalization; never canonicalize every open owner on save.
        if self
            .documents
            .iter()
            .enumerate()
            .any(|(owner, other)| owner != index && other.path.as_ref() == Some(&path))
        {
            return Err(failure("target belongs to another open document"));
        }
        if reserved.contains(&path) {
            return Err(failure("target already reserved by this batch"));
        }
        Ok(DocumentWrite {
            document_id: document.id,
            revision: document.revision,
            original_path: document.path.clone(),
            path,
            content: Arc::from(document.content.as_str()),
            overwrite: document.path.is_some(),
        })
    }

    /// True only while this request still belongs to the same live document.
    /// A successful first save may attach a path to a newer edited revision, but
    /// cannot clear it, or redirect a later rename/Save As (including path ABA).
    pub fn apply_write_result(&mut self, result: &DocumentWriteResult) -> bool {
        let request = &result.request;
        let Some(index) = self
            .documents
            .iter()
            .position(|document| document.id == request.document_id)
        else {
            return false;
        };
        let document = &self.documents[index];
        if document.path != request.original_path
            || request.revision < document.identity_revision
            || request.revision > document.revision
        {
            return false;
        }
        let path = match &result.result {
            Ok(path) => path.clone(),
            Err(error) => {
                log::warn!("{error}");
                return true;
            }
        };
        if self
            .documents
            .iter()
            .enumerate()
            .any(|(owner, other)| owner != index && other.path.as_ref() == Some(&path))
        {
            log::warn!(
                "Saved document {} could not acquire target owned by another document: {}",
                request.document_id,
                path.display()
            );
            return false;
        }
        let document = &mut self.documents[index];
        document.path = Some(path.clone());
        document.title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string();
        if document.revision == request.revision
            && document.content.as_str() == request.content.as_ref()
        {
            document.dirty = false;
        }
        self.remember_recent_file(path);
        true
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
        let mut owned_paths = std::collections::HashSet::new();
        for document in &mut self.documents {
            if let Some(path) = document.path.take() {
                let path = normalized_path(&path);
                if owned_paths.insert(path.clone()) {
                    document.path = Some(path);
                } else {
                    // A duplicate must become an eager recovery buffer BEFORE
                    // losing its only disk reference, even if it was deferred.
                    if !document.loaded {
                        document.content = read_document_content(&path).unwrap_or_default();
                        document.loaded = true;
                    }
                    document.dirty = true;
                }
            }
        }
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

fn default_split_ratio() -> f32 {
    0.5
}

pub(crate) fn normalize_split_ratio(ratio: f32) -> f32 {
    if ratio.is_finite() {
        ratio.clamp(0.15, 0.85)
    } else {
        default_split_ratio()
    }
}

fn unique_markdown_path_excluding(
    folder: &Path,
    file_name: &str,
    documents: &[Document],
    excluded_index: usize,
) -> PathBuf {
    unique_markdown_path_for_file_name(folder, file_name, documents, Some(excluded_index), None)
}

fn unique_markdown_path_for_file_name(
    folder: &Path,
    file_name: &str,
    documents: &[Document],
    excluded_index: Option<usize>,
    reserved: Option<&HashSet<PathBuf>>,
) -> PathBuf {
    let folder = normalized_path(folder);
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

        let is_reserved = reserved.is_some_and(|paths| paths.contains(&candidate));
        if !is_reserved && (is_current_document || (!candidate.exists() && !is_open)) {
            return candidate;
        }

        suffix += 1;
    }
}

fn normalized_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        match (fs::canonicalize(parent), path.file_name()) {
            (Ok(parent), Some(name)) => parent.join(name),
            _ => path.to_path_buf(),
        }
    })
}

fn document_metadata_is_valid(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.len() <= MAX_DOCUMENT_BYTES)
}

fn read_document_content(path: &Path) -> Option<String> {
    if !document_metadata_is_valid(path) {
        return None;
    }
    // Bound the read as well as the preliminary metadata check: an external
    // writer can grow a document between those operations.
    use std::io::Read;
    let file = fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DOCUMENT_BYTES {
        return None;
    }
    // Include the oversize sentinel without geometrically growing a large body.
    let mut content = String::with_capacity(metadata.len() as usize + 1);
    #[cfg(test)]
    DOCUMENT_READS.with(|reads| {
        let (count, bytes) = reads.get();
        reads.set((count + 1, bytes));
    });
    file.take(MAX_DOCUMENT_BYTES + 1)
        .read_to_string(&mut content)
        .ok()?;
    #[cfg(test)]
    DOCUMENT_READS.with(|reads| {
        let (count, bytes) = reads.get();
        reads.set((count, bytes + content.len()));
    });
    (content.len() as u64 <= MAX_DOCUMENT_BYTES).then_some(content)
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

    if crate::document::strip_markdown_extension(&file_name) == file_name {
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
    fn restore_55_mebibyte_fixture_opens_only_active_body_and_hydrates_once() {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("state.json");
        let body = "x".repeat(1024 * 1024);
        let mut state = AppState::fresh();
        state.documents.clear();
        for index in 0..55 {
            let path = temp.path().join(format!("Note-{index}.md"));
            fs::write(&path, &body).unwrap();
            state
                .documents
                .push(Document::from_path(path, body.clone()));
        }
        state.active_document = 27;
        state.save_to(&session).unwrap();
        DOCUMENT_READS.with(|reads| reads.set((0, 0)));
        let mut restored = AppState::load_from(&session).unwrap();
        assert_eq!(DOCUMENT_READS.with(|reads| reads.get()), (1, 1024 * 1024));
        assert_eq!(
            restored.documents.iter().filter(|doc| doc.loaded).count(),
            1
        );
        let retained_bytes: usize = restored
            .documents
            .iter()
            .map(|doc| doc.content.capacity())
            .sum();
        assert!(retained_bytes < 2 * 1024 * 1024);
        assert_eq!(restored.documents[27].content, body);
        assert!(restored.ensure_document_loaded(10));
        assert!(restored.ensure_document_loaded(10));
        assert_eq!(
            DOCUMENT_READS.with(|reads| reads.get()),
            (2, 2 * 1024 * 1024)
        );
        println!(
            "lazy fixture: documents=55 fixture_body_bytes={} restore_read_calls=1 restore_read_bytes={} retained_body_capacity={} after_activation_read_calls=2",
            55 * body.len(),
            body.len(),
            retained_bytes
        );
    }

    #[test]
    fn failed_first_save_preserves_scratch_path_and_recovery() {
        let folder =
            std::env::temp_dir().join(format!("velocimd-first-save-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let mut state = AppState::fresh();
        assert!(state.add_folder_tab(folder.clone()));
        fs::remove_dir_all(&folder).unwrap();
        state
            .active_document_mut()
            .unwrap()
            .set_content("irreplaceable draft".into());
        assert!(state.save_file().is_none());
        assert!(state.active_document().unwrap().path.is_none());
        let restored = round_trip(&state);
        assert_eq!(
            restored.active_document().unwrap().content,
            "irreplaceable draft"
        );
        assert!(restored.active_document().unwrap().dirty);
    }

    #[test]
    fn failed_file_backed_save_recovers_draft_without_overwriting_disk() {
        let folder = std::env::temp_dir().join(format!("velocimd-recovery-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("Note.md");
        fs::write(&path, "disk original").unwrap();
        let mut state = AppState::fresh();
        assert!(state.open_file(path.clone()));
        state
            .active_document_mut()
            .unwrap()
            .set_content("unsaved draft".into());
        // A directory at the destination deterministically makes a save fail even as root.
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(state.save_file().is_none());
        fs::remove_dir(&path).unwrap();
        fs::write(&path, "external edit").unwrap();
        let restored = round_trip(&state);
        let recovered = restored.active_document().unwrap();
        assert_eq!(recovered.content, "unsaved draft");
        assert!(
            recovered.path.is_none(),
            "recovery must not overwrite external changes"
        );
        assert!(recovered.dirty);
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn restored_duplicate_paths_keep_only_one_owner_and_retain_both_buffers() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("Note.md");
        fs::write(&path, "disk").unwrap();
        let mut state = AppState::fresh();
        assert!(state.open_file(path.clone()));
        let mut duplicate = state.active_document().unwrap().clone();
        duplicate.id += 1;
        state.documents.push(duplicate);
        let restored = round_trip(&state);
        let canonical = fs::canonicalize(path).unwrap();
        assert_eq!(
            restored
                .documents
                .iter()
                .filter(|doc| doc.path.as_ref() == Some(&canonical))
                .count(),
            1
        );
        assert_eq!(restored.documents.last().unwrap().content, "disk");
        assert!(restored.documents.last().unwrap().path.is_none());
    }

    fn round_trip(state: &AppState) -> AppState {
        let json = serde_json::to_string(&PersistedAppState::from_state(state)).unwrap();
        serde_json::from_str::<PersistedAppState>(&json)
            .unwrap()
            .into_state()
            .normalized()
    }

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
