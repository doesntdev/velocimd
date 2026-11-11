use crate::{
    app_state::AppState,
    commands::Command,
    icons::{Icon, paint_icon, paint_logo},
    modes::EditorMode,
    theme::{DesignTokens, ThemeConfig},
};
#[cfg(target_arch = "wasm32")]
mod browser;
mod chrome;
#[cfg(test)]
mod chrome_tests;
#[cfg(test)]
mod desktop_controls_tests;
mod edit;
mod editor_layout;
use edit::{LineMetadataCache, RecordingBuffer};
#[cfg(not(target_arch = "wasm32"))]
mod persistence;
mod probe;
#[cfg(test)]
mod responsive_tests;
mod splitter;
use editor_layout::EditorLayoutCache;

use eframe::{App, CreationContext, Frame, egui};
use std::{path::PathBuf, sync::Arc, time::Duration};

use crate::icons::{compact_icon_button, icon_button};
#[cfg(not(target_arch = "wasm32"))]
use native_dialog::FileDialogBuilder;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use std::{collections::HashMap, fs, path::Path};
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

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
#[cfg(not(target_arch = "wasm32"))]
const FOLDER_ENTRY_CACHE_TTL: Duration = Duration::from_secs(2);
#[cfg(test)]
thread_local! { static MENU_ROWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(Clone, Copy, PartialEq)]
struct PreviewSyncTarget {
    line: usize,
    anchor: PreviewSyncAnchor,
}

#[derive(Clone, Copy, PartialEq)]
enum PreviewSyncAnchor {
    Top,
    ScreenY(f32),
}

mod preview;
pub use preview::PreviewRenderer;
#[cfg(test)]
use preview::{PREVIEW_BLOCK_WORK, PreviewSegmentKind, split_preview_segments};

#[derive(Default)]
struct EditorOutcome {
    changed: bool,
    clicked_line: Option<usize>,
    clicked_y: Option<f32>,
    line_count: usize,
    galley_top: f32,
    text_clip_top: f32,
    galley: Option<std::sync::Arc<egui::Galley>>,
    visible_response_rect: Option<egui::Rect>,
}

#[derive(Default)]
#[cfg(not(target_arch = "wasm32"))]
struct FolderEntryCache {
    entries: HashMap<PathBuf, CachedFolderEntries>,
}

#[cfg(not(target_arch = "wasm32"))]
struct CachedFolderEntries {
    loaded_at: Instant,
    entries: Arc<[FolderEntry]>,
}

#[cfg(not(target_arch = "wasm32"))]
impl FolderEntryCache {
    fn entries(&mut self, folder: &Path) -> Arc<[FolderEntry]> {
        let now = Instant::now();
        if let Some(cached) = self.entries.get(folder)
            && now.duration_since(cached.loaded_at) < FOLDER_ENTRY_CACHE_TTL
        {
            return cached.entries.clone();
        }

        let entries: Arc<[FolderEntry]> = folder_entries(folder).into();
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
    editor_layout: EditorLayoutCache,
    #[cfg(not(target_arch = "wasm32"))]
    persistence: persistence::UiPersistence,
    #[cfg(target_arch = "wasm32")]
    browser: crate::browser::BrowserState,
    #[cfg(not(target_arch = "wasm32"))]
    folder_entry_cache: FolderEntryCache,
    theme_applied: bool,
    #[cfg(not(target_arch = "wasm32"))]
    working_folder_prompted: bool,
    synced_scroll_y: f32,
    editor_sync_geometry: Option<(u64, egui::Rect, Arc<egui::Galley>)>,
    preview_sync_line: usize,
    preview_sync_anchor: PreviewSyncAnchor,
    autosave_due: Option<Instant>,
    state_save_due: Option<Instant>,
    file_name_editor_document_id: Option<u64>,
    file_name_editor_text: String,
    status_message: Option<String>,
    dialog_error: Option<String>,
    #[cfg(not(target_arch = "wasm32"))]
    file_dialog_builder: FileDialogBuilder,
    palette_selection: usize,
    palette_focus_requested: bool,
    // A view-session choice, not document content or storage-format metadata.
    split_axis_lock: Option<splitter::SplitAxis>,
    last_split_axis: splitter::SplitAxis,
    ui_probe: probe::UiProbe,
}

impl VelocimdApp {
    pub fn new(cc: &CreationContext<'_>) -> Self {
        Self::new_with_files(cc, Vec::new())
    }

    pub fn new_with_files(cc: &CreationContext<'_>, files: Vec<PathBuf>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        #[cfg(not(target_arch = "wasm32"))]
        let mut state = AppState::new();
        #[cfg(target_arch = "wasm32")]
        let (state, browser) = (AppState::fresh(), crate::browser::BrowserState::default());
        #[cfg(not(target_arch = "wasm32"))]
        for path in files {
            let _ = state.open_file(path);
        }
        #[cfg(target_arch = "wasm32")]
        cc.egui_ctx.set_theme(if state.theme.is_light() {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
        state.theme.apply_to(&cc.egui_ctx);
        let app = Self::from_state(state);
        #[cfg(target_arch = "wasm32")]
        let app = {
            let _ = files;
            let mut app = app;
            app.browser = browser;
            app.browser.context = Some(cc.egui_ctx.clone());
            app
        };
        app
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn new_browser(
        cc: &CreationContext<'_>,
        state: AppState,
        mut browser: crate::browser::BrowserState,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        cc.egui_ctx.set_theme(if state.theme.is_light() {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
        state.theme.apply_to(&cc.egui_ctx);
        browser.context = Some(cc.egui_ctx.clone());
        let mut app = Self::from_state(state);
        app.browser = browser;
        app
    }

    fn from_state(state: AppState) -> Self {
        Self {
            state,
            preview_renderer: PreviewRenderer::default(),
            line_metadata: LineMetadataCache::default(),
            editor_layout: EditorLayoutCache::default(),
            #[cfg(not(target_arch = "wasm32"))]
            persistence: persistence::UiPersistence::default(),
            #[cfg(target_arch = "wasm32")]
            browser: crate::browser::BrowserState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            folder_entry_cache: FolderEntryCache::default(),
            theme_applied: true,
            #[cfg(not(target_arch = "wasm32"))]
            working_folder_prompted: false,
            synced_scroll_y: 0.0,
            editor_sync_geometry: None,
            preview_sync_line: 0,
            preview_sync_anchor: PreviewSyncAnchor::Top,
            autosave_due: None,
            state_save_due: None,
            file_name_editor_document_id: None,
            file_name_editor_text: String::new(),
            status_message: None,
            dialog_error: None,
            #[cfg(not(target_arch = "wasm32"))]
            file_dialog_builder: FileDialogBuilder::default(),
            palette_selection: 0,
            palette_focus_requested: false,
            split_axis_lock: None,
            last_split_axis: splitter::SplitAxis::Horizontal,
            ui_probe: probe::UiProbe::default(),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn prompt_for_working_folder_once(&mut self, ctx: &egui::Context) {
        if self.state.working_folder.is_none() && !self.working_folder_prompted {
            self.working_folder_prompted = true;
            self.select_working_folder(ctx);
        }
    }

    fn command_label(&self, command: Command) -> &'static str {
        if command == Command::SetMode(EditorMode::Split) && self.state.mode == EditorMode::Split {
            return if self.split_axis_lock.is_some() {
                "Unlock split layout (automatic)"
            } else {
                match self.last_split_axis {
                    splitter::SplitAxis::Horizontal => "Lock side-by-side layout",
                    splitter::SplitAxis::Vertical => "Lock over/under layout",
                }
            };
        }
        #[cfg(target_arch = "wasm32")]
        match command {
            Command::NewTab => return "New browser document",
            Command::SelectWorkingFolder => {
                return "Folder navigation requires desktop (no browser filesystem mount)";
            }
            Command::OpenFile => return "Import Markdown copy (1 MiB per file; 16 document tabs)",
            Command::SaveFile => {
                return "Save local draft and download a copy (never overwrites disk)";
            }
            Command::SaveFileAs => return "Save local draft and download as…",
            Command::CloseFolderTab => return "Close browser document (download a backup first)",
            _ => {}
        }
        command.label()
    }

    fn tokens(&self) -> DesignTokens {
        DesignTokens::from_theme(&self.state.theme)
    }

    #[cfg(not(target_arch = "wasm32"))]
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
        if self.dialog_error.is_some() {
            return;
        }
        let command = ctx.input_mut(|input| {
            let shortcut = |modifiers, key| egui::KeyboardShortcut::new(modifiers, key);
            let command = egui::Modifiers::COMMAND;
            let command_shift = egui::Modifiers::COMMAND | egui::Modifiers::SHIFT;

            // Sanitize theme Key/Text pairs even when another command wins below.
            // An already-open palette still owns its input, including Alt text.
            let theme_command = if self.state.command_palette_open {
                None
            } else {
                consume_theme_shortcuts(input)
            };

            let split_repeat = input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Num3,
                        pressed: true,
                        repeat: true,
                        ..
                    }
                )
            });
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
                // A held key is one activation, not a stream of lock toggles.
                if split_repeat {
                    return None;
                }
                Some(Command::SetMode(EditorMode::Split))
            } else if input.consume_shortcut(&shortcut(command, egui::Key::Tab)) {
                Some(Command::CycleMode)
            } else {
                theme_command
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
            #[cfg(target_arch = "wasm32")]
            let commands: Vec<_> = commands
                .into_iter()
                .filter(|c| *c != Command::SelectWorkingFolder)
                .collect();
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
                            format!("{}    {}", self.command_label(*command), command.shortcut()),
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
        let narrow = ui.available_width() < 720.0;
        let status_height = if narrow && self.status_message.is_some() {
            18.0 + ui.spacing().item_spacing.y
        } else {
            0.0
        };
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), HEADER_BAR_HEIGHT + status_height),
            egui::Sense::hover(),
        );

        if ui.is_rect_visible(rect) {
            ui.painter().rect_filled(rect, 0, tokens.chrome_bg);
        }

        self.ui_probe.region("top_bar", response.rect);
        let content_rect = rect.shrink2(egui::vec2(
            HEADER_HORIZONTAL_PADDING,
            HEADER_VERTICAL_PADDING,
        ));

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(if narrow {
                    egui::Layout::top_down(egui::Align::Min)
                } else {
                    egui::Layout::left_to_right(egui::Align::Center)
                }),
            |ui| {
                ui.set_clip_rect(content_rect);
                ui.set_height(content_rect.height());
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
                        if !narrow && let Some(status) = &self.status_message {
                            ui.horizontal(|ui| {
                                ui.set_height(HEADER_CONTROL_HEIGHT);
                                let rect = ui
                                    .allocate_exact_size(
                                        egui::vec2(16.0, 16.0),
                                        egui::Sense::hover(),
                                    )
                                    .0;
                                paint_icon(ui, Icon::Check, rect.shrink(2.0), tokens.success);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(status)
                                            .small()
                                            .color(tokens.text_muted),
                                    )
                                    .truncate(),
                                );
                            });
                        }
                    });
                });
                if narrow && let Some(status) = &self.status_message {
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(status).small().color(tokens.text_muted),
                        )
                        .truncate(),
                    )
                    .on_hover_text(status);
                }
            },
        );
    }

    fn command_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tokens = self.tokens();
        let narrow = ui.available_width() < 720.0;
        let spacing = ui.spacing().item_spacing;
        let content_width = (ui.available_width() - COMMAND_BAR_HORIZONTAL_PADDING * 2.0).max(0.0);
        let columns = ((content_width + spacing.x) / (30.0 + spacing.x))
            .floor()
            .max(1.0) as usize;
        let rows = if narrow {
            Command::toolbar().len().div_ceil(columns) + 1
        } else {
            1
        };
        let height =
            COMMAND_BAR_HEIGHT + (rows - 1) as f32 * (COMMAND_BAR_CONTROL_HEIGHT + spacing.y);
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), height),
            egui::Sense::hover(),
        );

        if ui.is_rect_visible(rect) {
            ui.painter().rect_filled(rect, 0, tokens.chrome_bg);
        }

        self.ui_probe.region("command_bar", response.rect);
        let content_rect = rect.shrink2(egui::vec2(
            COMMAND_BAR_HORIZONTAL_PADDING,
            COMMAND_BAR_VERTICAL_PADDING,
        ));

        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(content_rect)
                .layout(egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(narrow)),
            |ui| {
                ui.set_clip_rect(content_rect);
                if narrow {
                    ui.set_row_height(COMMAND_BAR_CONTROL_HEIGHT);
                } else {
                    ui.set_height(COMMAND_BAR_CONTROL_HEIGHT);
                }

                for (index, command) in Command::toolbar().iter().enumerate() {
                    if narrow && index > 0 && index % columns == 0 {
                        ui.end_row();
                    }
                    let active =
                        matches!(command, Command::SetMode(mode) if *mode == self.state.mode);
                    let tooltip_command = if *command == Command::ToggleTheme {
                        if self.state.theme.is_light() {
                            Command::SwitchThemeDark
                        } else {
                            Command::SwitchThemeLight
                        }
                    } else {
                        *command
                    };
                    let tooltip = format!(
                        "{} ({})",
                        self.command_label(tooltip_command),
                        tooltip_command.shortcut()
                    );
                    // Native retains its original widget allocation/ID path.
                    #[cfg(not(target_arch = "wasm32"))]
                    let response = icon_button(ui, Icon::for_command(*command), active, tooltip);
                    #[cfg(target_arch = "wasm32")]
                    let response = ui
                        .add_enabled_ui(
                            *command != Command::SelectWorkingFolder
                                && (*command != Command::CloseFolderTab
                                    || self.state.documents.len() > 1),
                            |ui| icon_button(ui, Icon::for_command(*command), active, tooltip),
                        )
                        .inner;
                    self.ui_probe
                        .control(probe::toolbar_id(*command), &response, active);
                    if *command == Command::SetMode(EditorMode::Split)
                        && self.split_axis_lock.is_some()
                    {
                        // A small padlock badge distinguishes locked Split from
                        // the ordinary selected state without adding a control.
                        let badge = egui::Rect::from_min_size(
                            response.rect.right_bottom() - egui::vec2(12.0, 12.0),
                            egui::vec2(11.0, 11.0),
                        );
                        let body = egui::Rect::from_min_size(
                            badge.min + egui::vec2(2.0, 5.0),
                            egui::vec2(7.0, 5.0),
                        );
                        ui.painter().rect_filled(badge, 2, tokens.chrome_bg);
                        ui.painter().rect_filled(body, 1, tokens.accent);
                        ui.painter().rect_stroke(
                            egui::Rect::from_min_size(
                                badge.min + egui::vec2(3.0, 1.0),
                                egui::vec2(5.0, 6.0),
                            ),
                            2,
                            egui::Stroke::new(1.3, tokens.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    if response.clicked() {
                        self.run_command(ctx, *command);
                    }
                }

                if narrow {
                    ui.end_row();
                } else {
                    ui.add_space(10.0);
                }
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

        let filename_width = 220.0_f32.min((ui.available_width() - 16.0).max(24.0));
        let response = ui
            .add(
                egui::TextEdit::singleline(&mut self.file_name_editor_text)
                    .id(egui::Id::new(("file_name_editor", document_id)))
                    .font(egui::TextStyle::Button)
                    .desired_width(filename_width)
                    .margin(egui::Margin::symmetric(8, 3))
                    .hint_text("Untitled")
                    .clip_text(true),
            )
            .on_hover_text("Rename Markdown file");

        self.ui_probe.control("filename", &response, false);
        if response.lost_focus() {
            self.commit_file_name_edit(document_index, document_id);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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

        let rename_title = rename_title.to_owned();
        self.finish_background_writes();
        let old_path = self
            .state
            .documents
            .get(document_index)
            .and_then(|document| document.path.clone());
        let renamed_path = self.state.rename_document(document_index, &rename_title);
        if let Some(document) = self.state.documents.get(document_index) {
            self.file_name_editor_text = document.visible_title();
        }

        match renamed_path {
            Some(path) => {
                if old_path.as_ref() != Some(&path) {
                    if let Some(old) = &old_path {
                        self.invalidate_folder_for_path(old);
                    }
                    self.invalidate_folder_for_path(&path);
                }
                self.status_message = Some(format!("Renamed {}", short_path(&path)));
            }
            None => self.status_message = Some("Rename failed".to_string()),
        }
        self.persist_state();
    }

    fn editor_content(&mut self, ui: &mut egui::Ui, min_height: f32) -> EditorOutcome {
        let tokens = self.tokens();
        let editor_font = egui::TextStyle::Monospace.resolve(ui.style());
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
                let epoch = self
                    .editor_layout
                    .begin_tracked((document.id, document.revision), line_count);
                let galley = self.editor_layout.layout_known(
                    ui,
                    (document.id, document.revision),
                    &document.content,
                    &epoch,
                    &editor_font,
                    wrap_width,
                );
                let visible_rows = galley.rows.len().max(line_count).max(1);
                let text_height = min_height.max(galley.size().y + editor_margin * 2.0);

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
        let (outer_rect, outer_response) = ui.allocate_exact_size(outer_size, egui::Sense::hover());
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
        let epoch = self
            .editor_layout
            .begin_tracked((document.id, document.revision), line_count);
        let layout = &mut self.editor_layout;
        let identity = (document.id, document.revision);
        let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, width: f32| {
            layout.layout_recording(ui, identity, text, &epoch, &editor_font, width)
        };
        let mut buffer = RecordingBuffer::tracked(
            &mut document.content,
            &mut self.line_metadata.metadata,
            &epoch,
        );
        let text_output = ui
            .scope_builder(
                egui::UiBuilder::new()
                    .max_rect(text_rect)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    egui::TextEdit::multiline(&mut buffer)
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
                        .layouter(&mut layouter)
                        .min_size(text_rect.size())
                        .show(ui)
                },
            )
            .inner;
        let edits = buffer.finish();
        let changed = text_output.response.changed() || !edits.is_empty();
        // TextEdit can perform several deletes/inserts (or an undo replacement)
        // before returning. Publish the index and preview as one revision only
        // after every mutation has been captured in its original order.
        if changed {
            let before = document.revision;
            document.mark_changed();
            self.editor_layout.finish_tracked(
                identity,
                document.revision,
                epoch.value(),
                &text_output.galley,
            );
            self.line_metadata
                .finish_edit(document.id, before, document.revision);
            self.preview_renderer
                .apply_edits(document.id, before, document.revision, &edits);
        }
        let line_count =
            self.line_metadata
                .line_count(document.id, document.revision, &document.content);

        let gutter_inner = egui::Rect::from_min_max(
            egui::pos2(gutter_rect.left() + 8.0, gutter_rect.top()),
            egui::pos2(gutter_rect.right() - 8.0, gutter_rect.bottom()),
        );
        let gutter_clip = gutter_inner.intersect(ui.clip_rect());
        let gutter_painter = painter.with_clip_rect(gutter_clip);
        let numbered = self.editor_layout.numbered_rows(&text_output.galley);
        let local_top = gutter_clip.top() - text_output.galley_pos.y;
        let first = numbered.partition_point(|&(index, _)| {
            let row = &text_output.galley.rows[index];
            row.pos.y + row.size.y < local_top
        });
        for &(index, line_number) in &numbered[first..] {
            let row = &text_output.galley.rows[index];
            let y = text_output.galley_pos.y + row.pos.y;
            if y > gutter_clip.bottom() {
                break;
            }
            if line_number <= line_count {
                gutter_painter.text(
                    egui::pos2(gutter_inner.right(), y),
                    egui::Align2::RIGHT_TOP,
                    format!("{line_number:>digit_count$}"),
                    editor_font.clone(),
                    tokens.text_muted,
                );
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
            changed,
            clicked_line: clicked_line.map(|line| line.min(line_count.saturating_sub(1))),
            clicked_y,
            line_count,
            galley_top: text_output.galley_pos.y,
            text_clip_top: text_output.text_clip_rect.top(),
            galley: Some(text_output.galley.clone()),
            visible_response_rect: Some(outer_response.rect.intersect(ui.clip_rect())),
        };

        if outcome.changed {
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

        if let Some(rect) = output.inner.visible_response_rect {
            self.ui_probe
                .region("editor", rect.intersect(output.inner_rect));
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
            .animated(false)
            .max_width(viewport_size.x)
            .max_height(viewport_size.y)
            .auto_shrink([false, false]);
        let output = scroll_area
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
                )
            });
        self.ui_probe
            .region("preview", output.inner.rect.intersect(output.inner_rect));
    }

    fn sync_preview_from_editor(
        &mut self,
        inner_rect: egui::Rect,
        output_offset_y: f32,
        applied_scroll: f32,
        outcome: &EditorOutcome,
        ctx: &egui::Context,
    ) {
        let offset = output_offset_y.max(0.0);
        let moved = (offset - applied_scroll).abs() > 0.5;
        let document_id = self
            .state
            .active_document()
            .map_or(0, |document| document.id);
        let geometry_changed = outcome.galley.as_ref().is_some_and(|galley| {
            let changed = self
                .editor_sync_geometry
                .as_ref()
                .is_none_or(|(id, rect, old)| {
                    *id != document_id || *rect != inner_rect || !Arc::ptr_eq(old, galley)
                });
            self.editor_sync_geometry = Some((document_id, inner_rect, Arc::clone(galley)));
            changed
        });
        self.synced_scroll_y = offset;
        if let Some(clicked_line) = outcome.clicked_line {
            self.preview_sync_line = clicked_line;
            self.preview_sync_anchor =
                PreviewSyncAnchor::ScreenY(outcome.clicked_y.unwrap_or(inner_rect.center().y));
            ctx.request_repaint();
        } else if moved || geometry_changed || outcome.changed {
            // ScrollArea constructs the content using applied_scroll, then
            // returns its post-input/clamp offset. Reconcile those coordinate
            // spaces before asking which logical source line is visible.
            let line = top_visible_editor_line(inner_rect.top() + offset - applied_scroll, outcome);
            let changed = self.preview_sync_line != line
                || self.preview_sync_anchor != PreviewSyncAnchor::Top;
            self.preview_sync_line = line;
            self.preview_sync_anchor = PreviewSyncAnchor::Top;
            if changed {
                ctx.request_repaint();
            }
        }
    }

    fn workspace(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        #[cfg(not(target_arch = "wasm32"))]
        self.state
            .ensure_document_loaded(self.state.active_document);
        self.ui_probe.region("app", ui.max_rect());
        let automatic_axis = splitter::SplitAxis::for_size(ui.max_rect().size());
        self.last_split_axis = self.split_axis_lock.unwrap_or(automatic_axis);
        self.command_bar(ui, ctx);
        // The toolbar may have just unlocked a layout; apply the current
        // viewport immediately, not the previous frame's locked orientation.
        let split_axis = self.split_axis_lock.unwrap_or(automatic_axis);
        self.last_split_axis = split_axis;

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
                    let layout =
                        splitter::show(ui, self.state.split_ratio, self.tokens(), split_axis);
                    self.ui_probe.region("editor_pane", layout.editor);
                    self.ui_probe.region("preview_pane", layout.preview);
                    self.ui_probe.region("divider", layout.divider);
                    if let Some(rect) = layout.response_rect {
                        self.ui_probe.region("divider_hit", rect);
                    }
                    if let Some(ratio) = layout.changed_ratio {
                        self.state.split_ratio = ratio;
                        self.persist_state();
                    }
                    let mut editor = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("split_editor")
                            .max_rect(layout.editor)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    editor.set_clip_rect(layout.editor.intersect(ui.clip_rect()));
                    let mut preview = ui.new_child(
                        egui::UiBuilder::new()
                            .id_salt("split_preview")
                            .max_rect(layout.preview)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    preview.set_clip_rect(layout.preview.intersect(ui.clip_rect()));
                    if preview.is_enabled()
                        && preview.rect_contains_pointer(preview.available_rect_before_wrap())
                        && ctx.dragged_id().is_none()
                    {
                        // The editor owns Split's source position. Feed wheel input from
                        // the preview into that ScrollArea instead of letting it snap back.
                        // Preserve egui's smoothing and consume the vertical delta once.
                        let delta = preview
                            .input_mut(|input| std::mem::take(&mut input.smooth_scroll_delta.y));
                        if delta != 0.0 {
                            editor.scroll_with_delta_animation(
                                egui::vec2(0.0, delta),
                                egui::style::ScrollAnimation::none(),
                            );
                        }
                    }
                    let outcome = self.editor_pane(&mut editor, ctx, true);
                    if split_axis == splitter::SplitAxis::Vertical
                        && outcome.clicked_line.is_some()
                        && let PreviewSyncAnchor::ScreenY(y) = self.preview_sync_anchor
                    {
                        // A clicked source line has the same pane-local y, not the
                        // same screen y when the preview is below the editor.
                        self.preview_sync_anchor = PreviewSyncAnchor::ScreenY(
                            y + layout.preview.top() - layout.editor.top(),
                        );
                    }
                    self.preview_pane(&mut preview, true);
                    splitter::raise_hit_target(ui, &layout);
                }
            },
        );
    }

    // Both target adapters invoke this before their platform-specific effects.
    fn handle_split_activation(&mut self, ctx: &egui::Context, command: Command) -> bool {
        match command {
            Command::SetMode(EditorMode::Split) if self.state.mode == EditorMode::Split => {
                self.split_axis_lock = if self.split_axis_lock.is_some() {
                    None
                } else {
                    Some(self.last_split_axis)
                };
                ctx.request_repaint();
                true
            }
            Command::SetMode(_) | Command::CycleMode => {
                self.split_axis_lock = None;
                false
            }
            _ => false,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        if self.handle_split_activation(ctx, command) {
            return;
        }
        self.ensure_persistence_worker(ctx);
        match command {
            Command::NewTab => self.create_markdown_file(ctx),
            Command::SelectWorkingFolder => self.select_working_folder(ctx),
            Command::OpenFile => self.open_file(ctx),
            Command::SaveFile => {
                self.request_document_save(ctx, true);
                self.persist_state();
            }
            Command::SaveFileAs => self.save_file_as(ctx),
            Command::ToggleTheme => self.switch_theme(ctx, self.state.theme.toggled()),
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

    #[cfg(not(target_arch = "wasm32"))]
    fn create_markdown_file(&mut self, ctx: &egui::Context) {
        if self.state.active_folder_path().is_none() {
            self.status_message = Some("Choose a folder first".to_string());
            return;
        }
        self.state.new_tab_deferred();
        self.reset_scroll_sync();
        self.request_document_save(ctx, true);
        self.persist_state();
        ctx.request_repaint();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn file_dialog(&self) -> FileDialogBuilder {
        let builder = self.file_dialog_builder.clone();
        // Shell folder initialization can reject stale or verbatim Windows paths
        // before Show. Keep canonical model paths intact and let Shell use history.
        #[cfg(target_os = "windows")]
        {
            builder
        }
        #[cfg(not(target_os = "windows"))]
        {
            let location = dialog_location(&self.state);
            if location.is_dir() {
                builder.set_location(&location)
            } else {
                builder
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn dialog_selection(
        &mut self,
        ctx: &egui::Context,
        operation: &str,
        result: native_dialog::Result<Option<PathBuf>>,
    ) -> Option<PathBuf> {
        match result {
            Ok(Some(path)) => {
                self.dialog_error = None;
                Some(path)
            }
            Ok(None) => None,
            Err(error) => {
                let message = format!("Could not {operation}: {error}");
                log::error!("{message}; {error:?}");
                self.dialog_error = Some(message);
                ctx.request_repaint();
                None
            }
        }
    }

    fn show_dialog_error(&mut self, ctx: &egui::Context) {
        let Some(message) = self.dialog_error.clone() else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("file_dialog_error")).show(ctx, |ui| {
            ui.set_max_width(400.0);
            ui.heading("File dialog failed");
            ui.colored_label(self.tokens().danger, message);
            ui.button("OK").clicked()
        });
        if response.inner || response.should_close() {
            self.dialog_error = None;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn select_folder_tab(&mut self, ctx: &egui::Context) {
        let result = self
            .file_dialog()
            .set_title("Open Folder Tab")
            .open_single_dir()
            .show();
        let selected = self.dialog_selection(ctx, "open the folder picker", result);

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
        let result = self
            .file_dialog()
            .set_title("Select Working Folder")
            .open_single_dir()
            .show();
        let selected = self.dialog_selection(ctx, "select the working folder", result);

        if let Some(path) = selected {
            self.finish_background_writes();
            if self.state.set_working_folder_deferred(path.clone()) {
                self.request_document_save(ctx, false);
                self.folder_entry_cache.invalidate(&path);
                self.status_message = Some(format!("Working folder {}", short_path(&path)));
                self.persist_state();
                ctx.request_repaint();
            } else {
                self.status_message = Some("Could not use working folder".to_string());
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
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
        let result = self
            .file_dialog()
            .set_title("Open Markdown File")
            .add_filter("Markdown", ["md", "markdown"])
            .open_single_file()
            .show();
        let selected = self.dialog_selection(ctx, "open the Markdown picker", result);

        if let Some(path) = selected {
            self.open_markdown_file(ctx, path);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_file_as(&mut self, ctx: &egui::Context) {
        let filename = self
            .state
            .active_document()
            .map(|document| document.title.clone())
            .unwrap_or_else(|| "Untitled.md".to_string());
        let result = self
            .file_dialog()
            .set_title("Save Markdown File")
            .set_filename(filename)
            .add_filter("Markdown", ["md", "markdown"])
            .save_single_file()
            .show();
        let selected = self.dialog_selection(ctx, "open the Save As dialog", result);
        if let Some(path) = selected {
            self.save_to_selected_path(ctx, path);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn save_to_selected_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        let path = with_markdown_extension(path);
        self.finish_background_writes();
        let existed = path.exists();
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
            if !existed {
                self.invalidate_folder_for_path(&path);
            }
            self.file_name_editor_document_id = None;
            self.status_message = Some(format!("Saved {}", short_path(&path)));
            self.persist_state();
            ctx.request_repaint();
        } else {
            self.status_message = Some("Save failed".to_string());
        }
    }

    fn switch_theme(&mut self, ctx: &egui::Context, theme: ThemeConfig) {
        self.state.theme = theme;
        #[cfg(target_arch = "wasm32")]
        ctx.set_theme(if self.state.theme.is_light() {
            egui::Theme::Light
        } else {
            egui::Theme::Dark
        });
        self.state.theme.apply_to(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = self.state.save_theme();
        self.persist_state();
    }

    fn reset_scroll_sync(&mut self) {
        self.synced_scroll_y = 0.0;
        self.editor_sync_geometry = None;
        self.preview_sync_line = 0;
        self.preview_sync_anchor = PreviewSyncAnchor::Top;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn invalidate_folder_for_path(&mut self, path: &Path) {
        if let Some(folder) = path.parent() {
            self.folder_entry_cache.invalidate(folder);
        }
    }

    fn schedule_autosave(&mut self, ctx: &egui::Context) {
        #[cfg(target_arch = "wasm32")]
        self.persist_state();
        let due = Instant::now() + AUTOSAVE_DEBOUNCE;
        self.autosave_due = Some(due);
        ctx.request_repaint_after(AUTOSAVE_DEBOUNCE);
    }
}

impl App for VelocimdApp {
    #[cfg(target_arch = "wasm32")]
    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut Frame) {
        let ctx = ui.ctx().clone();
        self.ui_probe.begin();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.file_dialog_builder = FileDialogBuilder::default().set_owner(frame);
        }
        #[cfg(target_arch = "wasm32")]
        let _ = frame;
        self.process_deferred_work(&ctx);
        if !self.theme_applied {
            self.state.theme.apply_to(&ctx);
            self.theme_applied = true;
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.prompt_for_working_folder_once(&ctx);
        self.handle_shortcuts(&ctx);

        #[cfg(target_arch = "wasm32")]
        ui.set_style(ctx.global_style());
        let tokens = self.tokens();
        egui::Frame::central_panel(ui.style())
            .fill(tokens.app_bg)
            .inner_margin(egui::Margin::same(APP_PADDING))
            .show(ui, |ui| {
                #[cfg(not(target_arch = "wasm32"))]
                if !self.working_folder_gate(ui, &ctx) {
                    return;
                }

                self.top_bar(ui, &ctx);
                self.workspace(ui, &ctx);
            });
        self.command_palette(&ctx);
        self.show_dialog_error(&ctx);
        self.finish_ui_probe(&ctx);
        #[cfg(target_arch = "wasm32")]
        self.publish_browser_state();
    }
}

impl Drop for VelocimdApp {
    fn drop(&mut self) {
        self.shutdown_persistence();
    }
}

fn consume_theme_shortcuts(input: &mut egui::InputState) -> Option<Command> {
    let mut command = None;
    let mut consumed = Vec::new();
    for (index, event) in input.events.iter().enumerate() {
        let egui::Event::Key {
            key: key @ (egui::Key::L | egui::Key::D),
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            continue;
        };
        if !modifiers.matches_logically(egui::Modifiers::ALT) {
            continue;
        }
        // Preserve Light's existing precedence if both shortcuts arrive together,
        // but consume every recognized pair so neither can type into a text field.
        if *key == egui::Key::L || command.is_none() {
            command = Some(if *key == egui::Key::L {
                Command::SwitchThemeLight
            } else {
                Command::SwitchThemeDark
            });
        }
        consumed.push(index);
        // egui-winit emits Key then Text; eframe's web keydown path emits Text then Key.
        // Remove only the paired Text, never all same-letter input from this frame.
        let text_index = if cfg!(target_arch = "wasm32") {
            index.checked_sub(1)
        } else {
            index.checked_add(1)
        };
        if let Some(text_index) = text_index
            && matches!(input.events.get(text_index), Some(egui::Event::Text(_)))
        {
            consumed.push(text_index);
        }
    }
    let command = command?;
    let mut index = 0;
    input.events.retain(|_| {
        let keep = !consumed.contains(&index);
        index += 1;
        keep
    });
    Some(command)
}

#[cfg(test)]
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

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "windows")))]
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

#[cfg(not(target_arch = "wasm32"))]
fn short_path(path: &Path) -> String {
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        name.to_string()
    } else {
        path.display().to_string()
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn folder_title(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(not(target_arch = "wasm32"))]
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
        .show_rows(
            ui,
            ui.spacing().interact_size.y.max(
                ui.text_style_height(&egui::TextStyle::Button)
                    + 2.0 * ui.spacing().button_padding.y,
            ),
            entries.len(),
            |ui, range| {
                for index in range {
                    let entry = &entries[index];
                    #[cfg(test)]
                    MENU_ROWS.with(|count| count.set(count.get() + 1));
                    let label = match entry.kind {
                        FolderEntryKind::Folder => format!("[folder] {}", entry.name),
                        FolderEntryKind::Markdown => format!("[md] {}", entry.name),
                    };

                    if ui.button(label).clicked() {
                        match entry.kind {
                            FolderEntryKind::Folder => *selected_folder = Some(entry.path.clone()),
                            FolderEntryKind::Markdown => *selected_file = Some(entry.path.clone()),
                        }
                        ui.close();
                    }
                }
            },
        );
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg(not(target_arch = "wasm32"))]
enum FolderEntryKind {
    Folder,
    Markdown,
}

#[derive(Clone)]
#[cfg(not(target_arch = "wasm32"))]
struct FolderEntry {
    name: String,
    path: PathBuf,
    kind: FolderEntryKind,
}

#[cfg(not(target_arch = "wasm32"))]
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

    entries.sort_by_cached_key(|entry| (entry.kind, entry.name.to_lowercase()));
    entries
}

#[cfg(not(target_arch = "wasm32"))]
fn is_markdown_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(crate::document::is_markdown_extension)
}

#[cfg(not(target_arch = "wasm32"))]
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

    #[test]
    fn preview_semantic_blocks_keep_loose_nested_lists_and_literal_mermaid() {
        let list = "- first\n\n  continuation\n\n  - nested\n\n- second\n";
        assert_eq!(
            split_preview_segments(list).len(),
            1,
            "a loose list is one semantic block"
        );
        let literal = "````text\n```mermaid\ngraph TD\nA --> B\n```\n````";
        assert!(
            split_preview_segments(literal)
                .iter()
                .all(|block| matches!(block.kind, PreviewSegmentKind::Markdown(_)))
        );
    }

    #[test]
    fn preview_warm_frame_bounds_actual_block_work() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let markdown = "plain paragraph\n\n".repeat(10_000);
        for _ in 0..2 {
            PREVIEW_BLOCK_WORK.with(|work| work.set(0));
            let _ = ctx.run_ui(key_input(egui::Key::F1, egui::Modifiers::NONE), |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    renderer.show_document(ui, 7, 1, &markdown, DesignTokens::zed_dark(), None);
                });
            });
        }
        let work = PREVIEW_BLOCK_WORK.with(|work| work.get());
        assert!(
            work < 100,
            "warm frame constructed {work} blocks, not just clipped shapes"
        );
    }

    pub(super) fn test_app(content: &str) -> (tempfile::TempDir, VelocimdApp) {
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("state.json");
        let mut state = AppState::fresh();
        state.documents = vec![crate::document::Document::scratch("Test.md", content)];
        state.save_to(&session).unwrap();
        let state = AppState::load_from(session).unwrap();
        (temp, VelocimdApp::from_state(state))
    }

    fn scroll_frame(
        app: &mut VelocimdApp,
        ctx: &egui::Context,
        frame_number: &mut usize,
        pointer: egui::Pos2,
        wheel: Option<(egui::MouseWheelUnit, f32)>,
    ) -> egui::FullOutput {
        let mut events = vec![egui::Event::PointerMoved(pointer)];
        if let Some((unit, delta)) = wheel {
            events.push(egui::Event::MouseWheel {
                unit,
                delta: egui::vec2(0.0, delta),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            time: Some(*frame_number as f64 / 60.0),
            events,
            ..Default::default()
        };
        *frame_number += 1;
        ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show_inside(root, |ui| app.workspace(ui, ctx));
        })
    }

    fn first_visible_preview_section(output: &egui::FullOutput) -> usize {
        output
            .shapes
            .iter()
            .filter_map(|clipped| {
                let egui::Shape::Text(text) = &clipped.shape else {
                    return None;
                };
                if !clipped.clip_rect.contains(text.pos) {
                    return None;
                }
                text.galley
                    .text()
                    .strip_prefix("SECTION ")?
                    .parse::<usize>()
                    .ok()
            })
            .min()
            .expect("a visible rendered Markdown heading")
    }

    #[test]
    fn wheel_scrolls_both_split_panes_without_clicking_or_snapping_back() {
        let content = (0..200)
            .map(|index| format!("## SECTION {index:04}\n\nPlain paragraph {index:04}.\n\n"))
            .collect::<String>();
        for (unit, delta) in [
            (egui::MouseWheelUnit::Point, 320.0),
            (egui::MouseWheelUnit::Line, 8.0),
        ] {
            let mut offsets = Vec::new();
            for x in [250.0, 750.0] {
                let (_temp, mut app) = test_app(&content);
                let ctx = egui::Context::default();
                egui_extras::install_image_loaders(&ctx);
                app.state.theme.apply_to(&ctx);
                let revision = app.state.active_document().unwrap().revision;
                let pointer = egui::pos2(x, 350.0);
                let mut number = 0;
                for _ in 0..12 {
                    scroll_frame(&mut app, &ctx, &mut number, pointer, None);
                }
                let before = scroll_frame(&mut app, &ctx, &mut number, pointer, None);
                let first = first_visible_preview_section(&before);
                scroll_frame(&mut app, &ctx, &mut number, pointer, Some((unit, -delta)));
                for _ in 0..30 {
                    scroll_frame(&mut app, &ctx, &mut number, pointer, None);
                }
                let after = scroll_frame(&mut app, &ctx, &mut number, pointer, None);
                assert!(
                    app.synced_scroll_y > 100.0 && app.preview_sync_line > 0,
                    "wheel over x={x} ({unit:?}) must scroll the shared document; offset={}, line={}",
                    app.synced_scroll_y,
                    app.preview_sync_line
                );
                assert!(
                    first_visible_preview_section(&after) > first,
                    "the preview must visibly move and stay scrolled"
                );
                offsets.push(app.synced_scroll_y);
                scroll_frame(&mut app, &ctx, &mut number, pointer, Some((unit, delta)));
                for _ in 0..30 {
                    scroll_frame(&mut app, &ctx, &mut number, pointer, None);
                }
                let returned = scroll_frame(&mut app, &ctx, &mut number, pointer, None);
                assert!(app.synced_scroll_y < 1.0, "upward wheel must return to top");
                assert_eq!(first_visible_preview_section(&returned), first);
                let document = app.state.active_document().unwrap();
                assert_eq!(document.content, content);
                assert_eq!(
                    document.revision, revision,
                    "scrolling must not edit the note"
                );
            }
            assert!(
                (offsets[0] - offsets[1]).abs() < 1.0,
                "the same wheel input must apply once from either pane: {offsets:?}"
            );
        }
    }

    #[test]
    fn split_header_wheel_does_not_scroll_and_preview_only_wheel_still_works() {
        let content = (0..100)
            .map(|index| format!("## SECTION {index:04}\n\nPlain paragraph {index:04}.\n\n"))
            .collect::<String>();
        let (_temp, mut app) = test_app(&content);
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        app.state.theme.apply_to(&ctx);
        let mut number = 0;
        let header = egui::pos2(750.0, 25.0);
        for _ in 0..12 {
            scroll_frame(&mut app, &ctx, &mut number, header, None);
        }
        scroll_frame(
            &mut app,
            &ctx,
            &mut number,
            header,
            Some((egui::MouseWheelUnit::Point, -320.0)),
        );
        for _ in 0..30 {
            scroll_frame(&mut app, &ctx, &mut number, header, None);
        }
        assert_eq!(app.synced_scroll_y, 0.0, "toolbar input is not pane input");
        app.state.mode = EditorMode::Preview;
        let pointer = egui::pos2(750.0, 350.0);
        for _ in 0..12 {
            scroll_frame(&mut app, &ctx, &mut number, pointer, None);
        }
        let before = scroll_frame(&mut app, &ctx, &mut number, pointer, None);
        scroll_frame(
            &mut app,
            &ctx,
            &mut number,
            pointer,
            Some((egui::MouseWheelUnit::Point, -320.0)),
        );
        for _ in 0..30 {
            scroll_frame(&mut app, &ctx, &mut number, pointer, None);
        }
        let after = scroll_frame(&mut app, &ctx, &mut number, pointer, None);
        assert!(first_visible_preview_section(&after) > first_visible_preview_section(&before));
        assert_eq!(
            app.synced_scroll_y, 0.0,
            "Preview mode scrolls independently"
        );
    }

    #[test]
    fn gutter_builds_only_visible_labels_for_ten_thousand_lines() {
        let (_temp, mut app) = test_app(&"one line\n".repeat(10_000));
        let ctx = egui::Context::default();
        let output = ctx.run_ui(key_input(egui::Key::F1, egui::Modifiers::NONE), |ui| {
            let _ = app.editor_content(ui, 500.0);
        });
        let labels = output
            .shapes
            .iter()
            .filter(|shape| matches!(&shape.shape, egui::Shape::Text(_)))
            .count();
        assert!(
            labels < 100,
            "viewport must bound label construction, got {labels}"
        );
    }

    #[test]
    fn folder_menu_builds_only_viewport_rows() {
        let mut cache = FolderEntryCache::default();
        let folder = Path::new("/fixture-only-folder");
        let entries = (0..10_000)
            .map(|index| FolderEntry {
                name: format!("Note {index}"),
                path: folder.join(format!("{index}.md")),
                kind: FolderEntryKind::Markdown,
            })
            .collect::<Vec<_>>();
        cache.entries.insert(
            folder.to_path_buf(),
            CachedFolderEntries {
                loaded_at: Instant::now(),
                entries: entries.into(),
            },
        );
        let ctx = egui::Context::default();
        MENU_ROWS.with(|count| count.set(0));
        let output = ctx.run_ui(key_input(egui::Key::F1, egui::Modifiers::NONE), |ui| {
            folder_tab_menu(ui, &mut cache, folder, &mut None, &mut None);
        });
        let labels = output
            .shapes
            .iter()
            .filter(|shape| matches!(&shape.shape, egui::Shape::Text(_)))
            .count();
        assert!(labels < 100, "menu paint must be clipped, got {labels}");
        let rows = MENU_ROWS.with(|count| count.get());
        assert!(
            rows < 100,
            "menu must not construct all 10k buttons, got {rows}"
        );
    }

    #[test]
    fn ordinary_mode_changes_coalesce_without_immediate_disk_writes() {
        let (temp, mut app) = test_app("draft");
        let ctx = egui::Context::default();
        app.run_command(&ctx, Command::SetMode(EditorMode::Preview));
        let disk = AppState::load_from(temp.path().join("state.json")).unwrap();
        assert_eq!(
            disk.mode,
            EditorMode::Split,
            "mode changes should wait for the coalesced session deadline"
        );
        assert!(app.state_save_due.is_some());
    }

    #[test]
    fn editor_widget_shares_the_measured_galley_on_warm_frames() {
        let (_temp, mut app) = test_app(&"wrapped line words ".repeat(200));
        let ctx = egui::Context::default();
        let mut previous = None;
        for _ in 0..3 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.set_max_width(300.0);
                let output = app.editor_content(ui, 500.0);
                if let Some(old) = &previous {
                    assert!(Arc::ptr_eq(old, output.galley.as_ref().unwrap()));
                }
                previous = output.galley;
            });
        }
        assert_eq!(
            app.editor_layout.layouts, 1,
            "height measurement and TextEdit must share one cached layout"
        );
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
        app.state_save_due = Some(Instant::now());
        app.process_deferred_work(&egui::Context::default());
        app.finish_background_writes();
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

    fn theme_shortcut_frame(app: &mut VelocimdApp, ctx: &egui::Context, input: egui::RawInput) {
        let _ = ctx.run_ui(input, |root| {
            app.handle_shortcuts(root.ctx());
            egui::CentralPanel::default().show_inside(root, |ui| app.workspace(ui, ctx));
        });
    }

    #[test]
    fn theme_shortcuts_do_not_type_into_focused_editor_or_schedule_a_document_save() {
        for mode in [EditorMode::Edit, EditorMode::Split] {
            let content = "Original Markdown λ界.\n".repeat(20);
            let (_temp, mut app) = test_app(&content);
            app.state.mode = mode;
            let ctx = egui::Context::default();
            egui_extras::install_image_loaders(&ctx);
            app.state.theme.apply_to(&ctx);
            theme_shortcut_frame(
                &mut app,
                &ctx,
                key_input(egui::Key::F1, egui::Modifiers::NONE),
            );
            let position = egui::pos2(250.0, 180.0);
            for pressed in [true, false] {
                let mut input = key_input(egui::Key::F1, egui::Modifiers::NONE);
                input.events = vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ];
                theme_shortcut_frame(&mut app, &ctx, input);
            }
            assert!(ctx.memory(|memory| memory.focused().is_some()));
            let revision = app.state.active_document().unwrap().revision;
            for (key, text, modifiers, theme) in [
                (egui::Key::L, Some("l"), egui::Modifiers::ALT, "Velocilight"),
                (egui::Key::D, Some("d"), egui::Modifiers::ALT, "Velocidark"),
                (egui::Key::D, Some("d"), egui::Modifiers::ALT, "Velocidark"),
                (
                    egui::Key::L,
                    Some("L"),
                    egui::Modifiers::ALT | egui::Modifiers::SHIFT,
                    "Velocilight",
                ),
                (egui::Key::D, None, egui::Modifiers::ALT, "Velocidark"),
            ] {
                let mut input = key_input(key, modifiers);
                // Native egui-winit emits Key followed immediately by Text.
                if let Some(text) = text {
                    input.events.push(egui::Event::Text(text.into()));
                }
                // A modifier may already be released by the time a frame starts.
                input.modifiers = egui::Modifiers::NONE;
                theme_shortcut_frame(&mut app, &ctx, input);
                assert_eq!(app.state.theme.name, theme);
                let document = app.state.active_document().unwrap();
                assert_eq!(
                    document.content, content,
                    "{mode:?}: {key:?} must not type {text:?}"
                );
                assert_eq!(document.revision, revision);
                assert!(
                    app.autosave_due.is_none(),
                    "theme changes are not document edits"
                );
            }

            // A frame can also contain an earlier-priority mode or Save command.
            // Keep the real TextEdit focused, with all source selected, so stray
            // shortcut text would replace the note rather than merely append.
            app.state.working_folder = Some(_temp.path().to_path_buf());
            let mode_key = if mode == EditorMode::Edit {
                egui::Key::Num1
            } else {
                egui::Key::Num3
            };
            for command_key in [mode_key, egui::Key::S] {
                for (theme_key, text) in [(egui::Key::L, "l"), (egui::Key::D, "d")] {
                    for command_first in [false, true] {
                        theme_shortcut_frame(
                            &mut app,
                            &ctx,
                            key_input(egui::Key::A, egui::Modifiers::COMMAND),
                        );
                        let old_theme = app.state.theme.name.clone();
                        let mut input = key_input(theme_key, egui::Modifiers::ALT);
                        input.events.push(egui::Event::Text(text.into()));
                        let command_event = key_input(command_key, egui::Modifiers::COMMAND)
                            .events
                            .remove(0);
                        if command_first {
                            input.events.insert(0, command_event);
                        } else {
                            input.events.push(command_event);
                        }
                        input.modifiers = egui::Modifiers::NONE;
                        theme_shortcut_frame(&mut app, &ctx, input);
                        assert_eq!(app.state.mode, mode, "mode command retains priority");
                        assert_eq!(app.state.theme.name, old_theme, "theme must not win");
                        let document = app.state.active_document().unwrap();
                        assert_eq!(
                            document.content, content,
                            "{mode:?}: {theme_key:?} with {command_key:?} must not replace selected text"
                        );
                        assert_eq!(document.revision, revision);
                        assert!(app.autosave_due.is_none(), "no synthetic document edit");
                        if command_key == egui::Key::S {
                            app.finish_background_writes();
                            let path = app.state.active_document().unwrap().path.as_ref().unwrap();
                            assert_eq!(fs::read_to_string(path).unwrap(), content);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn theme_shortcut_preserves_other_typing_paste_and_ime_in_the_same_frame() {
        let (_temp, mut app) = test_app("draft");
        let ctx = egui::Context::default();
        let mut input = key_input(egui::Key::L, egui::Modifiers::ALT);
        let shortcut_key = input.events.remove(0);
        let ordinary_key = key_input(egui::Key::L, egui::Modifiers::NONE)
            .events
            .remove(0);
        let other_shortcut = key_input(egui::Key::D, egui::Modifiers::ALT)
            .events
            .remove(0);
        input.events = vec![
            egui::Event::Text("l".into()),
            shortcut_key,
            egui::Event::Text("l".into()),
            ordinary_key,
            egui::Event::Text("l".into()),
            other_shortcut,
            egui::Event::Text("d".into()),
            egui::Event::Paste("literal ld".into()),
            egui::Event::Ime(egui::ImeEvent::Commit("λ界".into())),
        ];
        let mut texts = Vec::new();
        let mut paste = None;
        let mut ime = None;
        let _ = ctx.run_ui(input, |ui| {
            app.handle_shortcuts(ui.ctx());
            ui.input(|input| {
                for event in &input.events {
                    match event {
                        egui::Event::Text(text) => texts.push(text.clone()),
                        egui::Event::Paste(text) => paste = Some(text.clone()),
                        egui::Event::Ime(egui::ImeEvent::Commit(text)) => ime = Some(text.clone()),
                        _ => {}
                    }
                }
            });
        });
        assert_eq!(app.state.theme.name, "Velocilight");
        assert_eq!(texts, ["l", "l"], "consume only the shortcut's paired Text");
        assert_eq!(paste.as_deref(), Some("literal ld"));
        assert_eq!(ime.as_deref(), Some("λ界"));
    }

    #[test]
    fn theme_shortcut_does_not_consume_altgr_or_palette_text() {
        for palette_open in [false, true] {
            let (_temp, mut app) = test_app("draft");
            app.state.theme = ThemeConfig::default_dark();
            app.state.command_palette_open = palette_open;
            let ctx = egui::Context::default();
            let modifiers = if palette_open {
                egui::Modifiers::ALT
            } else {
                egui::Modifiers::ALT | egui::Modifiers::CTRL
            };
            let mut input = key_input(egui::Key::L, modifiers);
            input.events.push(egui::Event::Text("λ".into()));
            let mut texts = Vec::new();
            let _ = ctx.run_ui(input, |ui| {
                app.handle_shortcuts(ui.ctx());
                texts = ui.input(|input| {
                    input
                        .events
                        .iter()
                        .filter_map(|event| {
                            if let egui::Event::Text(text) = event {
                                Some(text.clone())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                });
            });
            assert_eq!(app.state.theme.name, "Velocidark");
            assert_eq!(texts, ["λ"]);
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
        app.state_save_due = Some(Instant::now());
        app.process_deferred_work(&ctx);
        app.finish_background_writes();
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
