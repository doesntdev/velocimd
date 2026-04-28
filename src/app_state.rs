use crate::{commands::Command, document::Document, modes::EditorMode, theme::ThemeConfig};

#[derive(Debug, Clone)]
pub struct AppState {
    pub documents: Vec<Document>,
    pub active_document: usize,
    pub mode: EditorMode,
    pub command_palette_open: bool,
    pub command_query: String,
    pub theme: ThemeConfig,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            documents: vec![Document::scratch(
                "Welcome.md",
                "# Velocimd\n\nA fast Markdown reader/editor in Rust.\n\n- Tabs\n- Command palette\n- Edit / preview / split modes\n- TOML themes\n",
            )],
            active_document: 0,
            mode: EditorMode::Split,
            command_palette_open: false,
            command_query: String::new(),
            theme: ThemeConfig::default(),
        }
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

    pub fn execute(&mut self, command: Command) {
        match command {
            Command::NewTab => self.new_tab(),
            Command::TogglePalette => self.command_palette_open = !self.command_palette_open,
            Command::SetMode(mode) => self.mode = mode,
            Command::CycleMode => self.mode = self.mode.next(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
