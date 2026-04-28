use crate::{
    app_state::AppState, commands::Command, markdown, modes::EditorMode, theme::ThemeConfig,
};
use eframe::{App, CreationContext, Frame, egui};
use native_dialog::FileDialogBuilder;

pub struct VelocimdApp {
    state: AppState,
    theme_applied: bool,
}

impl VelocimdApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        let state = AppState::new();
        state.theme.apply_to(&cc.egui_ctx);
        Self {
            state,
            theme_applied: true,
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|input| {
            if input.modifiers.ctrl && input.key_pressed(egui::Key::K) {
                self.state.execute(Command::TogglePalette);
            }
            if input.modifiers.ctrl && input.key_pressed(egui::Key::N) {
                self.state.execute(Command::NewTab);
            }
            if input.modifiers.ctrl && input.key_pressed(egui::Key::Num1) {
                self.state.execute(Command::SetMode(EditorMode::Edit));
            }
            if input.modifiers.ctrl && input.key_pressed(egui::Key::Num2) {
                self.state.execute(Command::SetMode(EditorMode::Preview));
            }
            if input.modifiers.ctrl && input.key_pressed(egui::Key::Num3) {
                self.state.execute(Command::SetMode(EditorMode::Split));
            }
            if input.modifiers.ctrl && input.key_pressed(egui::Key::Tab) {
                self.state.execute(Command::CycleMode);
            }
        });
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Velocimd");
                ui.separator();
                if ui.button("New tab").clicked() {
                    self.state.execute(Command::NewTab);
                }
                if ui.button("Command palette").clicked() {
                    self.state.execute(Command::TogglePalette);
                }
                ui.separator();
                for mode in [EditorMode::Edit, EditorMode::Preview, EditorMode::Split] {
                    if ui
                        .selectable_label(self.state.mode == mode, mode.label())
                        .clicked()
                    {
                        self.state.execute(Command::SetMode(mode));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(format!("Theme: {}", self.state.theme.name));
                    if ui.button("🌓").clicked() {
                        self.state.execute(Command::TogglePalette);
                        self.state.command_query = "theme".to_string();
                    }
                });
            });
        });
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        let mut close_requested = None;

        ui.horizontal_wrapped(|ui| {
            for index in 0..self.state.documents.len() {
                let title = self.state.documents[index].display_title();
                let is_active = self.state.active_document == index;

                ui.horizontal(|ui| {
                    if ui.selectable_label(is_active, title).clicked() {
                        self.state.active_document = index;
                    }

                    if ui.button("✕").clicked() {
                        close_requested = Some(index);
                    }
                });
            }
        });

        if let Some(index) = close_requested {
            self.state.close_tab_at(index);
        }

        ui.separator();
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        if let Some(document) = self.state.active_document_mut() {
            let response = ui.add(
                egui::TextEdit::multiline(&mut document.content)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY)
                    .desired_rows(32)
                    .lock_focus(true),
            );
            if response.changed() {
                document.dirty = true;
            }
        }
    }

    fn preview(&self, ui: &mut egui::Ui) {
        let Some(document) = self.state.active_document() else {
            ui.label("No document open.");
            return;
        };

        let rendered = markdown::render_to_html(&document.content);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.label(
                egui::RichText::new(rendered)
                    .text_style(egui::TextStyle::Monospace)
                    .color(egui::Color32::from_rgb(230, 234, 244)),
            );
        });
    }

    fn workspace(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default().show(ctx, |ui| {
            self.tabs(ui);
            match self.state.mode {
                EditorMode::Edit => self.editor(ui),
                EditorMode::Preview => self.preview(ui),
                EditorMode::Split => {
                    ui.columns(2, |columns| {
                        columns[0].heading("Editor");
                        self.editor(&mut columns[0]);
                        columns[1].heading("Preview");
                        self.preview(&mut columns[1]);
                    });
                }
            }
        });
    }

    fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.state.command_palette_open {
            return;
        }

        let mut open = self.state.command_palette_open;
        let mut selected: Option<Command> = None;
        egui::Window::new("Command palette")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.state.command_query)
                        .hint_text("Type a command..."),
                );
                ui.separator();

                let query = self.state.command_query.to_lowercase();
                for command in Command::all()
                    .iter()
                    .copied()
                    .filter(|command| command.label().to_lowercase().contains(&query))
                {
                    if ui
                        .button(format!("{}    {}", command.label(), command.shortcut()))
                        .clicked()
                    {
                        selected = Some(command);
                    }
                }
            });

        self.state.command_palette_open = open;
        if let Some(command) = selected {
            match command {
                Command::OpenFile => {
                    let path = FileDialogBuilder::default()
                        .set_location("~/Documents")
                        .add_filter("Markdown", ["md", "markdown"])
                        .open_single_file()
                        .show()
                        .unwrap_or(None);
                    if let Some(path) = path {
                        self.state.open_file(path);
                    }
                }
                Command::SaveFile => {
                    let _path = self.state.save_file();
                    // Successfully saved
                }
                Command::SaveFileAs => {
                    let path = FileDialogBuilder::default()
                        .set_location("~/Documents")
                        .add_filter("Markdown", ["md", "markdown"])
                        .save_single_file()
                        .show()
                        .unwrap_or(None);
                    if let Some(path) = path {
                        self.state.save_file_as(path);
                    }
                }
                Command::SwitchThemeLight => {
                    self.state.theme = ThemeConfig::default_light();
                    self.state.theme.apply_to(ctx);
                }
                Command::SwitchThemeDark => {
                    self.state.theme = ThemeConfig::default_dark();
                    self.state.theme.apply_to(ctx);
                }
                _ => {
                    self.state.execute(command);
                }
            }
            self.state.command_palette_open = false;
            self.state.command_query.clear();
        }
    }
}

impl App for VelocimdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        if !self.theme_applied {
            self.state.theme.apply_to(ctx);
            self.theme_applied = true;
        }
        self.handle_shortcuts(ctx);
        self.top_bar(ctx);
        self.workspace(ctx);
        self.command_palette(ctx);
    }
}
