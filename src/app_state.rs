use crate::{commands::Command, document::Document, modes::EditorMode, theme::ThemeConfig};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub documents: Vec<Document>,
    pub active_document: usize,
    pub mode: EditorMode,
    pub command_palette_open: bool,
    pub command_query: String,
    pub theme: ThemeConfig,
    pub window_size: Option<(f32, f32)>,
    pub recent_files: Vec<PathBuf>,
}

impl AppState {
    pub fn new() -> Self {
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
            mode: EditorMode::Split,
            command_palette_open: false,
            command_query: String::new(),
            theme,
            window_size: None,
            recent_files: Vec::new(),
        }
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
        let Ok(content) = fs::read_to_string(&path) else {
            return false;
        };

        let document = Document::from_path(path.clone(), content);
        self.documents.push(document);
        self.active_document = self.documents.len() - 1;
        self.remember_recent_file(path);
        true
    }

    pub fn save_file(&mut self) -> Option<PathBuf> {
        let document = self.active_document_mut()?;
        let path = document.path.clone()?;
        fs::write(&path, &document.content).ok()?;
        document.dirty = false;
        Some(path)
    }

    pub fn save_file_as(&mut self, path: PathBuf) -> bool {
        let Some(document) = self.active_document_mut() else {
            return false;
        };

        if fs::write(&path, &document.content).is_err() {
            return false;
        }

        document.path = Some(path.clone());
        document.title = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Unknown")
            .to_string();
        document.dirty = false;
        self.remember_recent_file(path);
        true
    }

    pub fn execute(&mut self, command: Command) {
        match command {
            Command::NewTab => self.new_tab(),
            Command::CloseTab => self.close_tab(),
            Command::TogglePalette => self.command_palette_open = !self.command_palette_open,
            Command::SetMode(mode) => self.mode = mode,
            Command::CycleMode => self.mode = self.mode.next(),
            Command::OpenFile
            | Command::SaveFile
            | Command::SaveFileAs
            | Command::SwitchThemeLight
            | Command::SwitchThemeDark => {}
        }
    }

    fn remember_recent_file(&mut self, path: PathBuf) {
        self.recent_files.retain(|existing| existing != &path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(20);
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
