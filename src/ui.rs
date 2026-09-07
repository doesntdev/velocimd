use crate::{
    app_state::AppState,
    commands::Command,
    icons::{Icon, compact_icon_button, icon_button, paint_icon, paint_logo},
    mermaid::MermaidRenderCache,
    modes::EditorMode,
    theme::{DesignTokens, ThemeConfig},
};
use eframe::{App, CreationContext, Frame, egui};
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[cfg(not(target_arch = "wasm32"))]
use native_dialog::FileDialogBuilder;

const APP_PADDING: i8 = 10;
const PANE_SCROLL_MARGIN: i8 = 8;
const PREVIEW_FRAME_MARGIN: i8 = 14;
const EDITOR_TEXT_MARGIN: i8 = 12;
const HEADER_HORIZONTAL_PADDING: f32 = 10.0;
const HEADER_VERTICAL_PADDING: f32 = 6.0;
const HEADER_CONTROL_HEIGHT: f32 = 30.0;
const HEADER_BAR_HEIGHT: f32 = HEADER_CONTROL_HEIGHT + HEADER_VERTICAL_PADDING * 2.0;
const COMMAND_BAR_HORIZONTAL_PADDING: f32 = 8.0;
const COMMAND_BAR_VERTICAL_PADDING: f32 = 5.0;
const COMMAND_BAR_CONTROL_HEIGHT: f32 = 28.0;
const COMMAND_BAR_HEIGHT: f32 = COMMAND_BAR_CONTROL_HEIGHT + COMMAND_BAR_VERTICAL_PADDING * 2.0;
const AUTOSAVE_DEBOUNCE: Duration = Duration::from_millis(450);
const STATE_SAVE_DEBOUNCE: Duration = Duration::from_millis(900);
const FOLDER_ENTRY_CACHE_TTL: Duration = Duration::from_secs(2);

#[derive(Clone, Copy)]
struct PreviewSyncTarget {
    line: usize,
    anchor: PreviewSyncAnchor,
}

#[derive(Clone, Copy)]
enum PreviewSyncAnchor {
    Top,
    ScreenY(f32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreviewSegment {
    start_line: usize,
    line_count: usize,
    kind: PreviewSegmentKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PreviewSegmentKind {
    Markdown(String),
    Mermaid(String),
}

#[derive(Default)]
pub struct PreviewRenderer {
    cache: CommonMarkCache,
    segments: PreviewSegmentCache,
    mermaid: MermaidRenderCache,
}

#[derive(Default)]
struct PreviewSegmentCache {
    document_id: Option<u64>,
    revision: u64,
    segments: Vec<PreviewSegment>,
}

impl PreviewRenderer {
    pub fn name(&self) -> &'static str {
        "egui-commonmark"
    }

    pub fn supports_code_highlighting(&self) -> bool {
        true
    }

    pub fn supports_local_images(&self) -> bool {
        true
    }

    pub fn show(&mut self, ui: &mut egui::Ui, markdown: &str, tokens: DesignTokens) {
        let segments = split_preview_segments(markdown);
        show_preview_segments(
            ui,
            &segments,
            &mut self.cache,
            &mut self.mermaid,
            tokens,
            None,
        );
    }

    fn show_document(
        &mut self,
        ui: &mut egui::Ui,
        document_id: u64,
        revision: u64,
        markdown: &str,
        tokens: DesignTokens,
        sync_target: Option<PreviewSyncTarget>,
    ) {
        if self.segments.document_id != Some(document_id) || self.segments.revision != revision {
            self.segments.document_id = Some(document_id);
            self.segments.revision = revision;
            self.segments.segments = split_preview_segments(markdown);
        }

        show_preview_segments(
            ui,
            &self.segments.segments,
            &mut self.cache,
            &mut self.mermaid,
            tokens,
            sync_target,
        );
    }
}

fn show_preview_segments(
    ui: &mut egui::Ui,
    segments: &[PreviewSegment],
    commonmark_cache: &mut CommonMarkCache,
    mermaid_cache: &mut MermaidRenderCache,
    tokens: DesignTokens,
    sync_target: Option<PreviewSyncTarget>,
) {
    ui.style_mut().url_in_tooltip = true;
    let margin = egui::Margin::same(PREVIEW_FRAME_MARGIN);
    let available_size = ui.available_size();
    let margin_size = egui::vec2(
        f32::from(PREVIEW_FRAME_MARGIN) * 2.0,
        f32::from(PREVIEW_FRAME_MARGIN) * 2.0,
    );
    let min_inner_size = egui::vec2(
        (available_size.x - margin_size.x).max(0.0),
        (available_size.y - margin_size.y).max(0.0),
    );

    egui::Frame::new()
        .fill(ui.visuals().widgets.noninteractive.bg_fill)
        .stroke(egui::Stroke::new(1.0_f32, tokens.border))
        .corner_radius(6)
        .inner_margin(margin)
        .show(ui, |ui| {
            ui.set_min_size(min_inner_size);
            let mut pending_sync = sync_target;

            for segment in segments {
                let PreviewSegment {
                    start_line,
                    line_count,
                    kind,
                } = segment;
                let start_y = ui.cursor().top();

                ui.push_id(
                    ("preview_segment", start_line, line_count),
                    |ui| match kind {
                        PreviewSegmentKind::Markdown(markdown) => {
                            if !markdown.trim().is_empty() {
                                CommonMarkViewer::new().show(ui, commonmark_cache, markdown);
                            }
                        }
                        PreviewSegmentKind::Mermaid(source) => {
                            mermaid_cache.render(ui, source, tokens);
                        }
                    },
                );

                if let Some(target) = pending_sync
                    && line_in_segment(target.line, *start_line, *line_count)
                {
                    let end_y = ui.cursor().top();
                    scroll_preview_line(ui, target, *start_line, *line_count, start_y, end_y);
                    pending_sync = None;
                }
            }

            if let Some(target) = pending_sync {
                let y = ui.cursor().top();
                scroll_preview_rect_at(
                    ui,
                    target.anchor,
                    y,
                    ui.text_style_height(&egui::TextStyle::Body),
                );
            }
        });
}

#[derive(Default)]
struct EditorOutcome {
    changed: bool,
    focused: bool,
    clicked_line: Option<usize>,
    clicked_y: Option<f32>,
    line_count: usize,
    galley_top: f32,
    text_clip_top: f32,
    galley: Option<std::sync::Arc<egui::Galley>>,
}

#[derive(Default)]
struct LineMetadataCache {
    document_id: Option<u64>,
    revision: u64,
    metadata: LineMetadata,
}

#[derive(Clone)]
struct LineMetadata {
    line_starts: Vec<usize>,
}

impl Default for LineMetadata {
    fn default() -> Self {
        Self {
            line_starts: vec![0],
        }
    }
}

impl LineMetadataCache {
    fn line_count(&mut self, document_id: u64, revision: u64, content: &str) -> usize {
        self.metadata(document_id, revision, content)
            .line_starts
            .len()
            .max(1)
    }

    fn metadata(&mut self, document_id: u64, revision: u64, content: &str) -> &LineMetadata {
        if self.document_id != Some(document_id) || self.revision != revision {
            self.document_id = Some(document_id);
            self.revision = revision;
            self.metadata = LineMetadata::from_content(content);
        }

        &self.metadata
    }
}

impl LineMetadata {
    fn from_content(content: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, character) in content.chars().enumerate() {
            if character == '\n' {
                line_starts.push(index + 1);
            }
        }

        Self { line_starts }
    }
}

#[derive(Default)]
struct FolderEntryCache {
    entries: HashMap<PathBuf, CachedFolderEntries>,
}

struct CachedFolderEntries {
    loaded_at: Instant,
    entries: Vec<FolderEntry>,
}

impl FolderEntryCache {
    fn entries(&mut self, folder: &Path) -> Vec<FolderEntry> {
        let now = Instant::now();
        if let Some(cached) = self.entries.get(folder)
            && now.duration_since(cached.loaded_at) < FOLDER_ENTRY_CACHE_TTL
        {
            return cached.entries.clone();
        }

        let entries = folder_entries(folder);
        self.entries.insert(
            folder.to_path_buf(),
            CachedFolderEntries {
                loaded_at: now,
                entries: entries.clone(),
            },
        );
        entries
    }

    fn invalidate(&mut self, folder: &Path) {
        self.entries.remove(folder);
    }
}

pub struct VelocimdApp {
    state: AppState,
    preview_renderer: PreviewRenderer,
    line_metadata: LineMetadataCache,
    folder_entry_cache: FolderEntryCache,
    theme_applied: bool,
    working_folder_prompted: bool,
    synced_scroll_y: f32,
    preview_sync_line: usize,
    preview_sync_anchor: PreviewSyncAnchor,
    autosave_due: Option<Instant>,
    state_save_due: Option<Instant>,
    file_name_editor_document_id: Option<u64>,
    file_name_editor_text: String,
    status_message: Option<String>,
    palette_selection: usize,
    palette_focus_requested: bool,
}

impl VelocimdApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        Self::new_with_files(cc, Vec::new())
    }

    pub fn new_with_files(cc: &CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        let mut state = AppState::new();
        for path in files {
            let _ = state.open_file(path);
        }
        state.theme.apply_to(&cc.egui_ctx);
        Self::from_state(state)
    }

    fn from_state(state: AppState) -> Self {
        Self {
            state,
            preview_renderer: PreviewRenderer::default(),
            line_metadata: LineMetadataCache::default(),
            folder_entry_cache: FolderEntryCache::default(),
            theme_applied: true,
            working_folder_prompted: false,
            synced_scroll_y: 0.0,
            preview_sync_line: 0,
            preview_sync_anchor: PreviewSyncAnchor::Top,
            autosave_due: None,
            state_save_due: None,
            file_name_editor_document_id: None,
            file_name_editor_text: String::new(),
            status_message: None,
            palette_selection: 0,
            palette_focus_requested: false,
        }
    }

    fn prompt_for_working_folder_once(&mut self, ctx: &egui::Context) {
        if self.state.working_folder.is_none() && !self.working_folder_prompted {
            self.working_folder_prompted = true;
            self.select_working_folder(ctx);
        }
    }

    fn tokens(&self) -> DesignTokens {
        DesignTokens::from_theme(&self.state.theme)
    }

    fn working_folder_gate(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> bool {
        if self.state.active_folder_path().is_some() {
            return true;
        }

        let tokens = self.tokens();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.35);
            egui::Frame::new()
                .fill(tokens.panel_bg)
                .stroke(egui::Stroke::new(1.0_f32, tokens.border))
                .corner_radius(8)
                .inner_margin(egui::Margin::same(18))
                .show(ui, |ui| {
                    ui.heading(egui::RichText::new("Select a working folder").color(tokens.text));
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(
                            "New and edited Markdown files stream into this folder.",
                        )
                        .color(tokens.text_muted),
                    );
                    ui.add_space(12.0);
                    if ui.button("Choose folder").clicked() {
                        self.select_working_folder(ctx);
                    }
                });
        });

        false
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let command = ctx.input_mut(|input| {
            let shortcut = |modifiers, key| egui::KeyboardShortcut::new(modifiers, key);
            let command = egui::Modifiers::COMMAND;
            let command_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;
            let alt = egui::Modifiers::ALT;

            if input.consume_shortcut(&shortcut(command, egui::Key::K))
                || input.consume_shortcut(&shortcut(command_shift, egui::Key::P))
            {
                Some(Command::TogglePalette)
            } else if self.state.command_palette_open {
                None
            } else if input.consume_shortcut(&shortcut(command_shift, egui::Key::S)) {
                Some(Command::SaveFileAs)
            } else if input.consume_shortcut(&shortcut(command_shift, egui::Key::O)) {
                Some(Command::SelectWorkingFolder)
            } else if input.consume_shortcut(&shortcut(command, egui::Key::S)) {
                Some(Command::SaveFile)
            } else if input.consume_shortcut(&shortcut(command, egui::Key::O)) {
                Some(Command::OpenFile)
            } else if input.consume_shortcut(&shortcut(command, egui::Key::N)) {
                Some(Command::NewTab)
            } else if input.consume_shortcut(&shortcut(command, egui::Key::W)) {
                Some(Command::CloseFolderTab)
            } else if input.consume_shortcut(&shortcut(command, egui::Key::Num1)) {
                Some(Command::SetMode(EditorMode::Edit))
            } else if input.consume_shortcut(&shortcut(command, egui::Key::Num2)) {
                Some(Command::SetMode(EditorMode::Preview))
            } else if input.consume_shortcut(&shortcut(command, egui::Key::Num3)) {
                Some(Command::SetMode(EditorMode::Split))
            } else if input.consume_shortcut(&shortcut(command, egui::Key::Tab)) {
                Some(Command::CycleMode)
            } else if input.consume_shortcut(&shortcut(alt, egui::Key::L)) {
                Some(Command::SwitchThemeLight)
            } else if input.consume_shortcut(&shortcut(alt, egui::Key::D)) {
                Some(Command::SwitchThemeDark)
            } else {
                None
            }
        });

        if let Some(command) = command {
            self.run_command(ctx, command);
        }
    }

    fn command_palette(&mut self, ctx: &egui::Context) {
        if !self.state.command_palette_open {
            return;
        }
        let (up, down, enter) = ctx.input_mut(|input| {
            (
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                input.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        });
        let mut selected_command = None;
        let response = egui::Modal::new(egui::Id::new("command_palette")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 48.0).clamp(180.0, 480.0));
            ui.heading("Command palette");
            let search = ui.add(
                egui::TextEdit::singleline(&mut self.state.command_query)
                    .id_salt("command_search")
                    .desired_width(f32::INFINITY)
                    .hint_text("Search commands…"),
            );
            if self.palette_focus_requested {
                search.request_focus();
                self.palette_focus_requested = false;
            }
            if search.changed() {
                self.palette_selection = 0;
            }
            let commands = Command::matching(&self.state.command_query);
            self.palette_selection = self.palette_selection.min(commands.len().saturating_sub(1));
            if up {
                self.palette_selection = self.palette_selection.saturating_sub(1);
            }
            if down && !commands.is_empty() {
                self.palette_selection = (self.palette_selection + 1).min(commands.len() - 1);
            }
            ui.separator();
            if commands.is_empty() {
                ui.label("No matching commands");
            }
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for (index, command) in commands.iter().enumerate() {
                        let response = ui.selectable_label(
                            index == self.palette_selection,
                            format!("{}    {}", command.label(), command.shortcut()),
                        );
                        if response.clicked() {
                            selected_command = Some(*command);
                        }
                        if index == self.palette_selection && (up || down || search.changed()) {
                            response.scroll_to_me(None);
                        }
                    }
                });
            if enter {
                selected_command = commands.get(self.palette_selection).copied();
            }
            ui.separator();
            ui.small("Up/Down select · Enter run · Esc close");
        });
        if response.should_close() || selected_command.is_some() {
            self.state.command_palette_open = false;
        }
        if let Some(command) = selected_command {
            self.run_command(ctx, command);
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tokens = self.tokens();
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), HEADER_BAR_HEIGHT),
            egui::Sense::hover(),
        );

        if ui.is_rect_visible(rect) {
            ui.painter().rect_filled(rect, 0, tokens.chrome_bg);
        }

        let content_rect = rect.shrink2(egui::vec2(
            HEADER_HORIZONTAL_PADDING,
            HEADER_VERTICAL_PADDING,
        ));

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_clip_rect(content_rect);
                ui.set_height(HEADER_CONTROL_HEIGHT);
                ui.horizontal_centered(|ui| {
                    ui.set_height(HEADER_CONTROL_HEIGHT);
                    let logo_rect = ui
                        .allocate_exact_size(
                            egui::vec2(44.0, HEADER_CONTROL_HEIGHT),
                            egui::Sense::hover(),
                        )
                        .0;
                    paint_logo(
                        ui.painter(),
                        logo_rect.translate(egui::vec2(0.0, 1.0)),
                        tokens.accent,
                        tokens.text,
                    );
                    ui.add_space(8.0);
                    self.folder_tabs(ui, ctx);

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if let Some(status) = &self.status_message {
                            ui.horizontal(|ui| {
                                ui.set_height(HEADER_CONTROL_HEIGHT);
                                let rect = ui
                                    .allocate_exact_size(
                                        egui::vec2(16.0, 16.0),
                                        egui::Sense::hover(),
                                    )
                                    .0;
                                paint_icon(
                                    ui.painter(),
                                    Icon::Check,
                                    rect.shrink(2.0),
                                    tokens.success,
                                );
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(status)
                                            .small()
                                            .color(tokens.text_muted),
                                    )
                                    .truncate(),
                                );
                            });
                        } else {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(format!(
                                        "Theme: {}",
                                        self.state.theme.name
                                    ))
                                    .small()
                                    .color(tokens.text_muted),
                                )
                                .truncate(),
                            );
                        }
                    });
                });
            },
        );
    }

    fn folder_tabs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tokens = self.tokens();
        let tabs: Vec<(usize, PathBuf, bool)> = self
            .state
            .folder_tabs
            .iter()
            .enumerate()
            .map(|(index, folder_tab)| {
                (
                    index,
                    folder_tab.path.clone(),
                    index == self.state.active_folder_tab,
                )
            })
            .collect();
        let mut close_requested = None;
        let mut selected_file = None;
        let mut selected_folder = None;

        ui.horizontal_centered(|ui| {
            for (index, path, is_active) in tabs {
                let title = folder_title(&path);
                let fill = if is_active {
                    tokens.panel_bg_active
                } else {
                    tokens.panel_bg
                };
                let stroke = if is_active {
                    tokens.border_active
                } else {
                    tokens.border
                };
                let button = egui::Button::new(egui::RichText::new(title).color(if is_active {
                    tokens.text
                } else {
                    tokens.text_muted
                }))
                .fill(fill)
                .stroke(egui::Stroke::new(1.0_f32, stroke))
                .corner_radius(6)
                .min_size(egui::vec2(132.0, HEADER_CONTROL_HEIGHT));

                let (response, _) =
                    egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                        folder_tab_menu(
                            ui,
                            &mut self.folder_entry_cache,
                            &path,
                            &mut selected_file,
                            &mut selected_folder,
                        );
                    });
                if response.clicked() {
                    self.state.active_folder_tab = index;
                    self.persist_state();
                }
                response.on_hover_text(path.display().to_string());

                if self.state.folder_tabs.len() > 1 {
                    let close = compact_icon_button(
                        ui,
                        Icon::X,
                        format!("Close folder tab {}", path.display()),
                    );
                    if close.clicked() {
                        close_requested = Some(index);
                    }
                }
            }

            let add = icon_button(ui, Icon::Plus, false, "New folder tab".to_string());
            if add.clicked() {
                self.select_folder_tab(ctx);
            }
        });

        if let Some(path) = selected_folder {
            if self.state.add_folder_tab(path.clone()) {
                self.status_message = Some(format!("Folder tab {}", short_path(&path)));
                self.persist_state();
            } else {
                self.status_message = Some("Could not open folder tab".to_string());
            }
        }

        if let Some(path) = selected_file {
            self.open_markdown_file(ctx, path);
        }

        if let Some(index) = close_requested {
            self.state.close_folder_tab_at(index);
            self.persist_state();
        }
    }

    fn command_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tokens = self.tokens();
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), COMMAND_BAR_HEIGHT),
            egui::Sense::hover(),
        );

        if ui.is_rect_visible(rect) {
            ui.painter().rect_filled(rect, 0, tokens.chrome_bg);
        }

        let content_rect = rect.shrink2(egui::vec2(
            COMMAND_BAR_HORIZONTAL_PADDING,
            COMMAND_BAR_VERTICAL_PADDING,
        ));

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.set_clip_rect(content_rect);
                ui.set_height(COMMAND_BAR_CONTROL_HEIGHT);

                for command in Command::toolbar() {
                    let active =
                        matches!(command, Command::SetMode(mode) if *mode == self.state.mode);
                    let response = icon_button(
                        ui,
                        Icon::for_command(*command),
                        active,
                        format!("{} ({})", command.label(), command.shortcut()),
                    );
                    if response.clicked() {
                        self.run_command(ctx, *command);
                    }
                }

                ui.add_space(10.0);
                self.file_name_editor(ui);
            },
        );
    }

    fn file_name_editor(&mut self, ui: &mut egui::Ui) {
        let Some((document_id, document_index, visible_title)) =
            self.state.active_document().map(|document| {
                (
                    document.id,
                    self.state.active_document,
                    document.visible_title(),
                )
            })
        else {
            return;
        };

        if self.file_name_editor_document_id != Some(document_id) {
            self.file_name_editor_document_id = Some(document_id);
            self.file_name_editor_text = visible_title;
        }

        let response = ui
            .add(
                egui::TextEdit::singleline(&mut self.file_name_editor_text)
                    .font(egui::TextStyle::Button)
                    .desired_width(220.0)
                    .margin(egui::Margin::symmetric(8, 3))
                    .hint_text("Untitled")
                    .clip_text(true),
            )
            .on_hover_text("Rename Markdown file");

        if response.lost_focus() {
            self.commit_file_name_edit(document_index, document_id);
        }
    }

    fn commit_file_name_edit(&mut self, document_index: usize, document_id: u64) {
        if self.file_name_editor_document_id != Some(document_id) {
            return;
        }

        let Some(current_title) = self
            .state
            .documents
            .get(document_index)
            .filter(|document| document.id == document_id)
            .map(|document| document.visible_title())
        else {
            return;
        };

        let proposed_title = self.file_name_editor_text.trim();
        if proposed_title == current_title {
            self.file_name_editor_text = current_title;
            return;
        }

        let rename_title = if proposed_title.is_empty() {
            "Untitled"
        } else {
            proposed_title
        };

        let renamed_path = self.state.rename_document(document_index, rename_title);
        if let Some(document) = self.state.documents.get(document_index) {
            self.file_name_editor_text = document.visible_title();
        }

        match renamed_path {
            Some(path) => {
                self.invalidate_folder_for_path(&path);
                self.status_message = Some(format!("Renamed {}", short_path(&path)));
            }
            None => self.status_message = Some("Rename failed".to_string()),
        }
        self.persist_state();
    }

    fn editor_content(&mut self, ui: &mut egui::Ui, min_height: f32) -> EditorOutcome {
        let tokens = self.tokens();
        let editor_font = egui::TextStyle::Monospace.resolve(ui.style());
        let row_height = ui.fonts_mut(|fonts| fonts.row_height(&editor_font));
        let editor_margin = f32::from(EDITOR_TEXT_MARGIN);
        let Some((line_count, text_height, visible_rows, gutter_width)) =
            self.state.active_document().map(|document| {
                let line_count = self.line_metadata.line_count(
                    document.id,
                    document.revision,
                    &document.content,
                );
                let digit_count = line_count.to_string().len().max(2);
                let gutter_width = editor_gutter_width(digit_count);
                let wrap_width =
                    (ui.available_width() - gutter_width - 1.0 - editor_margin * 2.0).max(1.0);
                let visible_rows =
                    editor_visual_rows(ui, &document.content, &editor_font, wrap_width, row_height)
                        .max(line_count)
                        .max(1);
                let text_height =
                    min_height.max(row_height * visible_rows as f32 + editor_margin * 2.0);

                (line_count, text_height, visible_rows, gutter_width)
            })
        else {
            ui.label("No document open.");
            return EditorOutcome::default();
        };
        let Some(document) = self.state.active_document_mut() else {
            ui.label("No document open.");
            return EditorOutcome::default();
        };

        let digit_count = line_count.to_string().len().max(2);
        let available_width = ui.available_width();
        let outer_size = egui::vec2(available_width, text_height);
        let (outer_rect, _) = ui.allocate_exact_size(outer_size, egui::Sense::hover());
        let painter = ui.painter_at(outer_rect);
        painter.rect_filled(outer_rect, 6, tokens.panel_bg);
        painter.rect_stroke(
            outer_rect,
            6,
            egui::Stroke::new(1.0_f32, tokens.border),
            egui::StrokeKind::Inside,
        );

        let gutter_rect = egui::Rect::from_min_size(
            outer_rect.min,
            egui::vec2(gutter_width, outer_rect.height()),
        );
        painter.rect_filled(gutter_rect, 6, tokens.chrome_bg);
        let separator_x = gutter_rect.right();
        painter.vline(
            separator_x,
            outer_rect.y_range(),
            egui::Stroke::new(1.0_f32, tokens.border),
        );

        let text_rect = egui::Rect::from_min_max(
            egui::pos2(separator_x + 1.0, outer_rect.top()),
            outer_rect.right_bottom(),
        );
        let text_output = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(text_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    egui::TextEdit::multiline(&mut document.content)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(visible_rows)
                        .id_salt(("document_editor", document.id))
                        .frame(
                            egui::Frame::new()
                                .fill(egui::Color32::TRANSPARENT)
                                .inner_margin(egui::Margin::same(EDITOR_TEXT_MARGIN)),
                        )
                        .lock_focus(true)
                        .min_size(text_rect.size())
                        .show(ui)
                },
            )
            .inner;

        let gutter_inner = egui::Rect::from_min_max(
            egui::pos2(gutter_rect.left() + 8.0, gutter_rect.top()),
            egui::pos2(gutter_rect.right() - 8.0, gutter_rect.bottom()),
        );
        let gutter_painter = painter.with_clip_rect(gutter_inner);
        let mut line_number = 1;
        let mut number_next_row = true;
        for row in &text_output.galley.rows {
            if number_next_row && line_number <= line_count {
                let y = text_output.galley_pos.y + row.pos.y;
                gutter_painter.text(
                    egui::pos2(gutter_inner.right(), y),
                    egui::Align2::RIGHT_TOP,
                    format!("{line_number:>digit_count$}"),
                    editor_font.clone(),
                    tokens.text_muted,
                );
            }

            if row.ends_with_newline {
                line_number += 1;
                number_next_row = true;
            } else {
                number_next_row = false;
            }
        }

        if text_output.galley.rows.is_empty() {
            gutter_painter.text(
                egui::pos2(gutter_inner.right(), text_output.galley_pos.y),
                egui::Align2::RIGHT_TOP,
                format!("{:>digit_count$}", 1),
                editor_font,
                tokens.text_muted,
            );
        }

        let clicked_position = if text_output.response.clicked() {
            text_output.response.interact_pointer_pos()
        } else {
            None
        };
        let clicked_line = if text_output.response.clicked() {
            text_output
                .cursor_range
                .map(|range| line_for_char_index(&document.content, range.primary.index))
                .or_else(|| {
                    clicked_position.map(|position| {
                        line_for_editor_y(
                            position.y - text_output.galley_pos.y,
                            &text_output.galley,
                        )
                    })
                })
        } else {
            None
        };
        let clicked_y = clicked_position.map(|position| position.y);

        let outcome = EditorOutcome {
            changed: text_output.response.changed(),
            focused: text_output.response.has_focus(),
            clicked_line: clicked_line.map(|line| line.min(line_count.saturating_sub(1))),
            clicked_y,
            line_count,
            galley_top: text_output.galley_pos.y,
            text_clip_top: text_output.text_clip_rect.top(),
            galley: Some(text_output.galley.clone()),
        };

        if outcome.changed {
            document.mark_changed();
            // Newlines/wrapping can change height during this frame's input handling.
            ui.ctx()
                .request_discard("editor content changed; recompute bounds and gutter");
        }

        outcome
    }

    fn editor_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        sync_scroll: bool,
    ) -> EditorOutcome {
        let doc_id = self
            .state
            .active_document()
            .map(|document| document.id)
            .unwrap_or_default();
        let viewport_size = ui.available_size();
        let min_height = (viewport_size.y - f32::from(PANE_SCROLL_MARGIN) * 2.0).max(160.0);
        let applied_scroll = if sync_scroll {
            self.synced_scroll_y
        } else {
            0.0
        };

        let scroll_area = egui::ScrollArea::vertical()
            .id_salt(("editor", doc_id))
            .max_width(viewport_size.x)
            .max_height(viewport_size.y)
            .auto_shrink([false, false]);
        #[cfg(not(target_arch = "wasm32"))]
        let scroll_area = scroll_area.vertical_scroll_offset(applied_scroll);
        let output = scroll_area
            .content_margin(egui::Margin::same(PANE_SCROLL_MARGIN))
            .show(ui, |ui| self.editor_content(ui, min_height));

        if sync_scroll {
            self.sync_preview_from_editor(
                output.inner_rect,
                output.state.offset.y,
                applied_scroll,
                &output.inner,
                ctx,
            );
        }

        if output.inner.changed {
            self.schedule_autosave(ctx);
        }

        output.inner
    }

    fn preview_pane(&mut self, ui: &mut egui::Ui, sync_scroll: bool) {
        let tokens = self.tokens();
        let Some(document) = self.state.active_document() else {
            ui.label("No document open.");
            return;
        };
        let doc_id = document.id;
        let revision = document.revision;
        let viewport_size = ui.available_size();
        let min_height = (viewport_size.y - f32::from(PANE_SCROLL_MARGIN) * 2.0).max(160.0);
        let sync_target = sync_scroll.then_some(PreviewSyncTarget {
            line: self.preview_sync_line,
            anchor: self.preview_sync_anchor,
        });

        let scroll_area = egui::ScrollArea::vertical()
            .id_salt(("preview", doc_id))
            .max_width(viewport_size.x)
            .max_height(viewport_size.y)
            .auto_shrink([false, false]);
        scroll_area
            .content_margin(egui::Margin::same(PANE_SCROLL_MARGIN))
            .show(ui, |ui| {
                ui.set_min_height(min_height);
                self.preview_renderer.show_document(
                    ui,
                    doc_id,
                    revision,
                    &document.content,
                    tokens,
                    sync_target,
                );
            });
    }

    fn sync_preview_from_editor(
        &mut self,
        inner_rect: egui::Rect,
        output_offset_y: f32,
        applied_scroll: f32,
        outcome: &EditorOutcome,
        ctx: &egui::Context,
    ) {
        let pointer_over = ctx
            .pointer_hover_pos()
            .is_some_and(|position| inner_rect.contains(position));
        let offset = output_offset_y.max(0.0);
        let moved = (offset - applied_scroll).abs() > 0.5;

        if let Some(clicked_line) = outcome.clicked_line {
            self.synced_scroll_y = offset;
            self.preview_sync_line = clicked_line;
            self.preview_sync_anchor =
                PreviewSyncAnchor::ScreenY(outcome.clicked_y.unwrap_or(inner_rect.center().y));
            ctx.request_repaint();
        } else if moved && (pointer_over || outcome.focused) {
            self.synced_scroll_y = offset;
            self.preview_sync_line = top_visible_editor_line(inner_rect.top(), outcome);
            self.preview_sync_anchor = PreviewSyncAnchor::Top;
            ctx.request_repaint();
        } else if moved {
            self.synced_scroll_y = offset;
        }
    }

    fn workspace(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.command_bar(ui, ctx);

        ui.allocate_ui_with_layout(
            ui.available_size(),
            egui::Layout::top_down(egui::Align::Min),
            |ui| match self.state.mode {
                EditorMode::Edit => {
                    self.editor_pane(ui, ctx, true);
                }
                EditorMode::Preview => {
                    self.preview_pane(ui, false);
                }
                EditorMode::Split => {
                    ui.columns(2, |columns| {
                        self.editor_pane(&mut columns[0], ctx, true);

                        self.preview_pane(&mut columns[1], true);
                    });
                }
            },
        );
    }

    fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        match command {
            Command::NewTab => self.create_markdown_file(ctx),
            Command::SelectWorkingFolder => self.select_working_folder(ctx),
            Command::OpenFile => self.open_file(ctx),
            Command::SaveFile => {
                match self.state.save_file() {
                    Some(path) => {
                        self.invalidate_folder_for_path(&path);
                        self.status_message = Some(format!("Saved {}", short_path(&path)))
                    }
                    None => self.status_message = Some("Save failed".to_string()),
                }
                self.persist_state();
            }
            Command::SaveFileAs => self.save_file_as(ctx),
            Command::SwitchThemeLight => self.switch_theme(ctx, ThemeConfig::default_light()),
            Command::SwitchThemeDark => self.switch_theme(ctx, ThemeConfig::default_dark()),
            Command::TogglePalette => {
                self.state.execute(command);
                self.state.command_query.clear();
                self.palette_selection = 0;
                self.palette_focus_requested = self.state.command_palette_open;
                ctx.request_repaint();
            }
            command => {
                self.state.execute(command);
                self.persist_state();
            }
        }
    }

    fn create_markdown_file(&mut self, ctx: &egui::Context) {
        if self.state.active_folder_path().is_none() {
            self.status_message = Some("Choose a folder first".to_string());
            return;
        }

        self.state.new_tab();
        self.reset_scroll_sync();
        let created_path = self
            .state
            .active_document()
            .and_then(|document| document.path.as_ref())
            .cloned();
        if let Some(path) = &created_path {
            self.invalidate_folder_for_path(path);
        }
        self.status_message = Some(
            created_path
                .as_ref()
                .map(|path| format!("Created {}", short_path(path)))
                .unwrap_or_else(|| {
                    "File creation failed; draft retained. Choose another folder or use Save As."
                        .to_string()
                }),
        );
        self.persist_state();
        ctx.request_repaint();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn select_folder_tab(&mut self, ctx: &egui::Context) {
        let location = dialog_location(&self.state);
        let selected = FileDialogBuilder::default()
            .set_title("Open Folder Tab")
            .set_location(&location)
            .open_single_dir()
            .show()
            .unwrap_or(None);

        if let Some(path) = selected {
            if self.state.add_folder_tab(path.clone()) {
                self.folder_entry_cache.invalidate(&path);
                self.status_message = Some(format!("Folder tab {}", short_path(&path)));
                self.persist_state();
                ctx.request_repaint();
            } else {
                self.status_message = Some("Could not open folder tab".to_string());
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn select_working_folder(&mut self, ctx: &egui::Context) {
        let location = dialog_location(&self.state);
        let selected = FileDialogBuilder::default()
            .set_title("Select Working Folder")
            .set_location(&location)
            .open_single_dir()
            .show()
            .unwrap_or(None);

        if let Some(path) = selected {
            if self.state.set_working_folder(path.clone()) {
                self.folder_entry_cache.invalidate(&path);
                self.status_message = Some(format!("Working folder {}", short_path(&path)));
                self.persist_state();
                ctx.request_repaint();
            } else {
                self.status_message = Some("Could not use working folder".to_string());
            }
        }
    }

    fn open_markdown_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        if self.state.open_file(path.clone()) {
            self.reset_scroll_sync();
            self.status_message = Some(format!("Opened {}", short_path(&path)));
            self.persist_state();
            ctx.request_repaint();
        } else {
            self.status_message = Some("Open failed".to_string());
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn open_file(&mut self, ctx: &egui::Context) {
        let location = dialog_location(&self.state);
        let selected = FileDialogBuilder::default()
            .set_title("Open Markdown File")
            .set_location(&location)
            .add_filter("Markdown", ["md", "markdown"])
            .open_single_file()
            .show()
            .unwrap_or(None);

        if let Some(path) = selected {
            self.open_markdown_file(ctx, path);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_file_as(&mut self, ctx: &egui::Context) {
        let location = dialog_location(&self.state);
        let filename = self
            .state
            .active_document()
            .map(|document| document.title.clone())
            .unwrap_or_else(|| "Untitled.md".to_string());
        let selected = FileDialogBuilder::default()
            .set_title("Save Markdown File")
            .set_location(&location)
            .set_filename(filename)
            .add_filter("Markdown", ["md", "markdown"])
            .save_single_file()
            .show()
            .unwrap_or(None);

        if let Some(path) = selected {
            let path = with_markdown_extension(path);
            if self
                .state
                .path_owned_by_other(self.state.active_document, &path)
            {
                self.status_message = Some(
                    "Save As blocked: that file is already open. Choose another name.".to_string(),
                );
                return;
            }
            if self.state.save_file_as(path.clone()) {
                self.invalidate_folder_for_path(&path);
                self.file_name_editor_document_id = None;
                self.status_message = Some(format!("Saved {}", short_path(&path)));
                self.persist_state();
                ctx.request_repaint();
            } else {
                self.status_message = Some("Save failed".to_string());
            }
        }
    }

    fn switch_theme(&mut self, ctx: &egui::Context, theme: ThemeConfig) {
        self.state.theme = theme;
        self.state.theme.apply_to(ctx);
        let _ = self.state.save_theme();
        self.persist_state();
    }

    fn reset_scroll_sync(&mut self) {
        self.synced_scroll_y = 0.0;
        self.preview_sync_line = 0;
        self.preview_sync_anchor = PreviewSyncAnchor::Top;
    }

    fn invalidate_folder_for_path(&mut self, path: &Path) {
        if let Some(folder) = path.parent() {
            self.folder_entry_cache.invalidate(folder);
        }
    }

    fn schedule_autosave(&mut self, ctx: &egui::Context) {
        let due = Instant::now() + AUTOSAVE_DEBOUNCE;
        self.autosave_due = Some(due);
        ctx.request_repaint_after(AUTOSAVE_DEBOUNCE);
    }

    fn schedule_state_save(&mut self, ctx: &egui::Context) {
        let due = Instant::now() + STATE_SAVE_DEBOUNCE;
        self.state_save_due = Some(due);
        ctx.request_repaint_after(STATE_SAVE_DEBOUNCE);
    }

    fn process_deferred_work(&mut self, ctx: &egui::Context) {
        let now = Instant::now();

        if let Some(due) = self.autosave_due {
            if now >= due {
                self.autosave_due = None;
                if self.flush_dirty_documents() {
                    self.schedule_state_save(ctx);
                }
            } else {
                ctx.request_repaint_after(due.duration_since(now));
            }
        }

        if let Some(due) = self.state_save_due {
            if now >= due {
                self.state_save_due = None;
                self.persist_state();
            } else {
                ctx.request_repaint_after(due.duration_since(now));
            }
        }
    }

    fn flush_dirty_documents(&mut self) -> bool {
        let result = self.state.save_dirty_documents();
        for path in &result.saved {
            self.invalidate_folder_for_path(path);
        }
        let saved_count = result.saved.len();
        if result.failed > 0 {
            self.status_message = Some(if saved_count > 0 {
                format!("Saved {saved_count}, failed {}", result.failed)
            } else {
                "Save failed".to_string()
            });
            return true;
        }

        if let Some(path) = result.saved.last() {
            self.status_message = Some(format!("Saved {}", short_path(path)));
            return true;
        }

        false
    }

    fn persist_state(&mut self) {
        if let Err(error) = self.state.save() {
            self.status_message = Some(format!("State save failed: {error}"));
        }
    }
}

impl App for VelocimdApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut Frame) {
        let ctx = ui.ctx().clone();
        self.process_deferred_work(&ctx);
        if !self.theme_applied {
            self.state.theme.apply_to(&ctx);
            self.theme_applied = true;
        }

        self.prompt_for_working_folder_once(&ctx);
        self.handle_shortcuts(&ctx);

        let tokens = self.tokens();
        egui::Frame::central_panel(ui.style())
            .fill(tokens.app_bg)
            .inner_margin(egui::Margin::same(APP_PADDING))
            .show(ui, |ui| {
                if !self.working_folder_gate(ui, &ctx) {
                    return;
                }

                self.top_bar(ui, &ctx);
                self.workspace(ui, &ctx);
            });
        self.command_palette(&ctx);
    }
}

impl Drop for VelocimdApp {
    fn drop(&mut self) {
        let result = self.state.save_dirty_documents();
        if result.failed > 0 {
            eprintln!(
                "Velocimd: {} document saves failed; preserving drafts in session recovery",
                result.failed
            );
        }
        if let Err(error) = self.state.save() {
            eprintln!("Velocimd: session recovery save failed: {error}");
        }
    }
}

fn split_preview_segments(markdown: &str) -> Vec<PreviewSegment> {
    let lines = markdown.split('\n').collect::<Vec<_>>();
    let mut segments = Vec::new();
    let mut markdown_start = 0;
    let mut line_index = 0;

    while line_index < lines.len() {
        let trimmed = lines[line_index].trim_start();

        if let Some(fence) = mermaid_fence_marker(trimmed)
            && let Some(closing_line) = find_closing_fence(&lines, line_index + 1, fence)
        {
            push_markdown_segments(&mut segments, &lines, markdown_start, line_index);

            let source = lines[line_index + 1..closing_line].join("\n");
            segments.push(PreviewSegment {
                start_line: line_index,
                line_count: closing_line - line_index + 1,
                kind: PreviewSegmentKind::Mermaid(source.trim().to_string()),
            });

            line_index = closing_line + 1;
            markdown_start = line_index;
            continue;
        }

        line_index += 1;
    }

    push_markdown_segments(&mut segments, &lines, markdown_start, lines.len());

    if segments.is_empty() {
        segments.push(PreviewSegment {
            start_line: 0,
            line_count: 1,
            kind: PreviewSegmentKind::Markdown(String::new()),
        });
    }

    segments
}

fn push_markdown_segments(
    segments: &mut Vec<PreviewSegment>,
    lines: &[&str],
    start: usize,
    end: usize,
) {
    if start >= end {
        return;
    }

    let mut block_start = start;
    let mut open_fence = None;

    for line_index in start..end {
        let trimmed = lines[line_index].trim_start();

        if let Some(fence) = open_fence {
            if trimmed.starts_with(fence) {
                open_fence = None;
            }
        } else if let Some(fence) = markdown_fence_marker(trimmed) {
            open_fence = Some(fence);
        } else if trimmed.trim().is_empty() {
            push_markdown_segment(segments, lines, block_start, line_index + 1);
            block_start = line_index + 1;
        }
    }

    push_markdown_segment(segments, lines, block_start, end);
}

fn push_markdown_segment(
    segments: &mut Vec<PreviewSegment>,
    lines: &[&str],
    start: usize,
    end: usize,
) {
    if start >= end {
        return;
    }

    segments.push(PreviewSegment {
        start_line: start,
        line_count: end - start,
        kind: PreviewSegmentKind::Markdown(lines[start..end].join("\n")),
    });
}

fn mermaid_fence_marker(trimmed: &str) -> Option<&'static str> {
    let fence = markdown_fence_marker(trimmed)?;
    let language = trimmed.trim_start_matches(fence).trim();

    if language.eq_ignore_ascii_case("mermaid") {
        Some(fence)
    } else {
        None
    }
}

fn markdown_fence_marker(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with("```") {
        Some("```")
    } else if trimmed.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

fn find_closing_fence(lines: &[&str], start: usize, fence: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, line)| line.trim_start().starts_with(fence).then_some(index))
}

fn line_in_segment(line: usize, start_line: usize, line_count: usize) -> bool {
    line >= start_line && line < start_line.saturating_add(line_count.max(1))
}

fn scroll_preview_line(
    ui: &mut egui::Ui,
    target: PreviewSyncTarget,
    start_line: usize,
    line_count: usize,
    start_y: f32,
    end_y: f32,
) {
    let line_count = line_count.max(1);
    let line_offset = target
        .line
        .saturating_sub(start_line)
        .min(line_count.saturating_sub(1));
    let block_height = (end_y - start_y)
        .max(ui.text_style_height(&egui::TextStyle::Body))
        .max(1.0);
    let line_height = (block_height / line_count as f32).max(1.0);
    let target_y = start_y + line_height * line_offset as f32;

    scroll_preview_rect_at(ui, target.anchor, target_y, line_height);
}

fn scroll_preview_rect_at(ui: &mut egui::Ui, anchor: PreviewSyncAnchor, y: f32, height: f32) {
    let clip_rect = ui.clip_rect();
    let (target_y, target_height, align) = match anchor {
        PreviewSyncAnchor::Top => (y, height.max(1.0), egui::Align::Min),
        PreviewSyncAnchor::ScreenY(screen_y) => {
            let anchor = viewport_anchor_for_y(screen_y, clip_rect);
            let shifted_y = y + (0.5 - anchor) * clip_rect.height();
            (shifted_y, 1.0, egui::Align::Center)
        }
    };
    let rect = egui::Rect::from_min_max(
        egui::pos2(clip_rect.left(), target_y),
        egui::pos2(clip_rect.right(), target_y + target_height),
    );
    ui.scroll_to_rect_animation(rect, Some(align), egui::style::ScrollAnimation::none());
}

fn top_visible_editor_line(visible_top: f32, outcome: &EditorOutcome) -> usize {
    let Some(galley) = &outcome.galley else {
        return 0;
    };
    let local_y = visible_top.max(outcome.text_clip_top) - outcome.galley_top;
    line_for_editor_y(local_y, galley).min(outcome.line_count.saturating_sub(1))
}

fn line_for_editor_y(local_y: f32, galley: &egui::Galley) -> usize {
    let row_index = galley
        .rows
        .partition_point(|row| row.pos.y <= local_y.max(0.0))
        .saturating_sub(1);
    galley
        .rows
        .iter()
        .take(row_index)
        .filter(|row| row.ends_with_newline)
        .count()
}

fn editor_gutter_width(digit_count: usize) -> f32 {
    digit_count as f32 * 9.0 + 18.0
}

fn editor_visual_rows(
    ui: &mut egui::Ui,
    content: &str,
    font_id: &egui::FontId,
    wrap_width: f32,
    row_height: f32,
) -> usize {
    if row_height <= 0.0 {
        return 1;
    }

    let text_color = ui.visuals().widgets.inactive.text_color();
    let galley_height = ui.fonts_mut(|fonts| {
        let job = egui::text::LayoutJob::simple(
            content.to_owned(),
            font_id.clone(),
            text_color,
            wrap_width.max(1.0),
        );
        fonts.layout_job(job).size().y
    });

    (galley_height / row_height).ceil().max(1.0) as usize
}

fn viewport_anchor_for_y(y: f32, rect: egui::Rect) -> f32 {
    if rect.height() <= 0.0 {
        return 0.5;
    }

    ((y - rect.top()) / rect.height()).clamp(0.0, 1.0)
}

fn line_for_char_index(text: &str, char_index: usize) -> usize {
    let mut line = 0;

    for (index, character) in text.chars().enumerate() {
        if index >= char_index {
            break;
        }

        if character == '\n' {
            line += 1;
        }
    }

    line
}

#[cfg(not(target_arch = "wasm32"))]
fn dialog_location(state: &AppState) -> PathBuf {
    state
        .working_folder
        .clone()
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(not(target_arch = "wasm32"))]
fn with_markdown_extension(mut path: PathBuf) -> PathBuf {
    if path.extension().is_none() {
        path.set_extension("md");
    }
    path
}

fn short_path(path: &Path) -> String {
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        name.to_string()
    } else {
        path.display().to_string()
    }
}

fn folder_title(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

fn folder_tab_menu(
    ui: &mut egui::Ui,
    folder_entry_cache: &mut FolderEntryCache,
    folder: &Path,
    selected_file: &mut Option<PathBuf>,
    selected_folder: &mut Option<PathBuf>,
) {
    ui.set_min_width(260.0);
    ui.label(
        egui::RichText::new(folder.display().to_string())
            .small()
            .color(ui.visuals().weak_text_color()),
    );
    ui.separator();

    let entries = folder_entry_cache.entries(folder);
    if entries.is_empty() {
        ui.label(egui::RichText::new("No Markdown files or folders").small());
        return;
    }

    egui::ScrollArea::vertical()
        .max_height(320.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for entry in entries {
                let label = match entry.kind {
                    FolderEntryKind::Folder => format!("[folder] {}", entry.name),
                    FolderEntryKind::Markdown => format!("[md] {}", entry.name),
                };

                if ui.button(label).clicked() {
                    match entry.kind {
                        FolderEntryKind::Folder => *selected_folder = Some(entry.path),
                        FolderEntryKind::Markdown => *selected_file = Some(entry.path),
                    }
                    ui.close();
                }
            }
        });
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FolderEntryKind {
    Folder,
    Markdown,
}

#[derive(Clone)]
struct FolderEntry {
    name: String,
    path: PathBuf,
    kind: FolderEntryKind,
}

fn folder_entries(folder: &Path) -> Vec<FolderEntry> {
    let Ok(read_dir) = fs::read_dir(folder) else {
        return Vec::new();
    };

    let mut entries = read_dir
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let file_type = entry.file_type().ok()?;
            let name = entry.file_name().to_string_lossy().to_string();

            if file_type.is_dir() {
                Some(FolderEntry {
                    name,
                    path,
                    kind: FolderEntryKind::Folder,
                })
            } else if file_type.is_file() && is_markdown_path(&path) {
                Some(FolderEntry {
                    name: strip_markdown_extension(&name).to_string(),
                    path,
                    kind: FolderEntryKind::Markdown,
                })
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    entries
}

fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(crate::document::is_markdown_extension)
}

use crate::document::strip_markdown_extension;

#[cfg(test)]
mod preview_sync_tests {
    use super::*;

    #[test]
    fn preview_segments_keep_mermaid_source_line_spans() {
        let segments = split_preview_segments("# A\n\n```mermaid\ngraph TD\nA --> B\n```\n\nAfter");
        let mermaid = segments
            .iter()
            .find(|segment| {
                matches!(&segment.kind, PreviewSegmentKind::Mermaid(source) if source.contains("A --> B"))
            })
            .expect("mermaid segment should be detected");

        assert_eq!(mermaid.start_line, 2);
        assert_eq!(mermaid.line_count, 4);
        assert!(line_in_segment(4, mermaid.start_line, mermaid.line_count));
        assert!(!line_in_segment(6, mermaid.start_line, mermaid.line_count));
    }

    fn test_app(content: &str) -> (tempfile::TempDir, VelocimdApp) {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("state.json");
        let mut state = AppState::fresh();
        state.documents = vec![crate::document::Document::scratch("Test.md", content)];
        state.save_to(&session).unwrap();
        let state = AppState::load_from(session).unwrap();
        (temp, VelocimdApp::from_state(state))
    }

    #[test]
    fn wrapped_first_line_maps_all_visual_rows_to_logical_zero() {
        let (_temp, mut app) = test_app(&format!("{}\nsecond", "W".repeat(200)));
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.set_max_width(180.0);
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
            let outcome = app.editor_content(ui, 160.0);
            let line = top_visible_editor_line(outcome.galley_top + row_height * 1.2, &outcome);
            assert_eq!(line, 0, "a soft wrap is not a new source line");
        });
    }

    #[test]
    fn editor_height_covers_wrapped_text_across_widths_and_themes() {
        for theme in [ThemeConfig::default_light(), ThemeConfig::default_dark()] {
            for width in [180.0, 320.0, 640.0, 960.0] {
                let (_temp, mut app) = test_app(&format!("{}\nsecond", "words λ界 ".repeat(80)));
                let ctx = egui::Context::default();
                theme.apply_to(&ctx);
                let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                    ui.set_max_width(width);
                    let top = ui.cursor().top();
                    let outcome = app.editor_content(ui, 160.0);
                    let galley = outcome.galley.as_ref().unwrap();
                    assert!(galley.rows.len() > 2);
                    assert!(outcome.galley_top + galley.size().y <= ui.min_rect().bottom() + 0.5);
                    assert!(outcome.galley_top >= top + f32::from(EDITOR_TEXT_MARGIN));
                    for row in &galley.rows[..galley.rows.len() - 1] {
                        assert_eq!(line_for_editor_y(row.pos.y + 0.1, galley), 0);
                    }
                    assert_eq!(
                        line_for_editor_y(galley.rows.last().unwrap().pos.y + 0.1, galley),
                        1
                    );
                });
            }
        }
    }

    #[test]
    fn close_command_has_same_folder_semantics_in_core_and_ui() {
        let (temp, mut app) = test_app("draft");
        assert!(app.state.add_folder_tab(temp.path().join("one")));
        assert!(app.state.add_folder_tab(temp.path().join("two")));
        let mut core = app.state.clone();
        core.execute(Command::CloseFolderTab);
        app.run_command(&egui::Context::default(), Command::CloseFolderTab);
        assert_eq!(Command::CloseFolderTab.label(), "Close folder tab");
        assert_eq!(core.folder_tabs, app.state.folder_tabs);
        assert_eq!(core.folder_tabs.len(), 1);
        assert_eq!(core.documents, app.state.documents);
        let restored = AppState::load_from(temp.path().join("state.json")).unwrap();
        assert_eq!(restored.folder_tabs, core.folder_tabs);
    }

    fn key_input(key: egui::Key, modifiers: egui::Modifiers) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            modifiers,
            events: vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn palette_opens_from_both_advertised_shortcuts() {
        for (key, modifiers) in [
            (egui::Key::K, egui::Modifiers::COMMAND),
            (
                egui::Key::P,
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            ),
        ] {
            let (_temp, mut app) = test_app("draft");
            let ctx = egui::Context::default();
            let _ = ctx.run_ui(key_input(key, modifiers), |ui| {
                app.handle_shortcuts(ui.ctx())
            });
            assert!(app.state.command_palette_open);
        }
    }

    fn palette_frame(app: &mut VelocimdApp, ctx: &egui::Context, input: egui::RawInput) {
        let _ = ctx.run_ui(input, |ui| {
            app.handle_shortcuts(ui.ctx());
            app.command_palette(ui.ctx());
        });
    }

    #[test]
    fn palette_search_arrow_enter_executes_real_command_and_persists() {
        let (temp, mut app) = test_app("draft");
        let ctx = egui::Context::default();
        palette_frame(
            &mut app,
            &ctx,
            key_input(egui::Key::K, egui::Modifiers::COMMAND),
        );
        palette_frame(&mut app, &ctx, egui::RawInput::default());
        let query = egui::RawInput {
            events: vec![egui::Event::Text("mode".into())],
            ..Default::default()
        };
        palette_frame(&mut app, &ctx, query);
        assert_eq!(app.state.command_query, "mode");
        palette_frame(
            &mut app,
            &ctx,
            key_input(egui::Key::ArrowDown, egui::Modifiers::NONE),
        );
        palette_frame(
            &mut app,
            &ctx,
            key_input(egui::Key::Enter, egui::Modifiers::NONE),
        );
        assert!(!app.state.command_palette_open);
        assert_eq!(app.state.mode, EditorMode::Preview);
        assert_eq!(
            AppState::load_from(temp.path().join("state.json"))
                .unwrap()
                .mode,
            EditorMode::Preview
        );
        assert_eq!(app.state.active_document().unwrap().content, "draft");
    }

    #[test]
    fn palette_escape_and_unmatched_enter_do_not_execute_commands() {
        let (_temp, mut app) = test_app("draft");
        let ctx = egui::Context::default();
        palette_frame(
            &mut app,
            &ctx,
            key_input(egui::Key::K, egui::Modifiers::COMMAND),
        );
        palette_frame(&mut app, &ctx, egui::RawInput::default());
        app.state.command_query = "no-such-command-xyz".into();
        palette_frame(
            &mut app,
            &ctx,
            key_input(egui::Key::Enter, egui::Modifiers::NONE),
        );
        assert!(app.state.command_palette_open);
        palette_frame(
            &mut app,
            &ctx,
            key_input(egui::Key::Escape, egui::Modifiers::NONE),
        );
        assert!(!app.state.command_palette_open);
        assert_eq!(app.state.mode, EditorMode::Split);
        assert_eq!(app.state.documents.len(), 1);
        assert_eq!(Command::matching("svfls"), vec![Command::SaveFileAs]);
    }

    #[test]
    fn close_folder_shortcut_preserves_dirty_document_and_last_folder() {
        let (temp, mut app) = test_app("draft");
        app.state.add_folder_tab(temp.path().join("one"));
        app.state.add_folder_tab(temp.path().join("two"));
        app.state.active_document_mut().unwrap().mark_changed();
        let docs = app.state.documents.clone();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = ctx.run_ui(key_input(egui::Key::W, egui::Modifiers::COMMAND), |ui| {
                app.handle_shortcuts(ui.ctx())
            });
        }
        assert_eq!(app.state.folder_tabs.len(), 1);
        assert_eq!(app.state.documents, docs);
    }

    #[test]
    fn char_indices_map_to_editor_lines() {
        assert_eq!(line_for_char_index("a\nb\nc", 0), 0);
        assert_eq!(line_for_char_index("a\nb\nc", 2), 1);
        assert_eq!(line_for_char_index("a\nb\nc", 4), 2);
    }

    #[test]
    fn viewport_anchor_tracks_click_position() {
        let rect = egui::Rect::from_min_max(egui::pos2(0.0, 100.0), egui::pos2(200.0, 500.0));

        assert_eq!(viewport_anchor_for_y(100.0, rect), 0.0);
        assert_eq!(viewport_anchor_for_y(300.0, rect), 0.5);
        assert_eq!(viewport_anchor_for_y(500.0, rect), 1.0);
    }
}

#[cfg(target_arch = "wasm32")]
impl VelocimdApp {
    fn select_folder_tab(&mut self, _ctx: &egui::Context) {
        // File dialogs not supported on WASM
    }

    fn select_working_folder(&mut self, _ctx: &egui::Context) {
        // File dialogs not supported on WASM
    }

    fn open_file(&mut self, _ctx: &egui::Context) {
        // File dialogs not supported on WASM
    }

    fn save_file_as(&mut self, _ctx: &egui::Context) {
        // File dialogs not supported on WASM
    }
}
