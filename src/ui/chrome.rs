//! Shared tab chrome. Targets supply labels, menu contents and effects; the
//! button/menu/close/add painter and its spacing remain the native implementation.
use super::*;

struct ChromeTab {
    index: usize,
    title: String,
    hover: String,
    active: bool,
    #[cfg(not(target_arch = "wasm32"))]
    path: PathBuf,
}

impl VelocimdApp {
    fn chrome_tabs(&self) -> Vec<ChromeTab> {
        #[cfg(not(target_arch = "wasm32"))]
        return self
            .state
            .folder_tabs
            .iter()
            .enumerate()
            .map(|(index, tab)| ChromeTab {
                index,
                title: folder_title(&tab.path),
                hover: tab.path.display().to_string(),
                active: index == self.state.active_folder_tab,
                path: tab.path.clone(),
            })
            .collect();
        #[cfg(target_arch = "wasm32")]
        self.state
            .documents
            .iter()
            .enumerate()
            .map(|(index, document)| ChromeTab {
                index,
                title: document.title.clone(),
                hover: "Browser document · local draft, not a mounted folder".into(),
                active: index == self.state.active_document,
            })
            .collect()
    }

    pub(super) fn folder_tabs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tokens = self.tokens();
        let tabs = self.chrome_tabs();
        let can_close = tabs.len() > 1;
        let mut close_requested = None;
        #[cfg(not(target_arch = "wasm32"))]
        let mut selected_file = None;
        #[cfg(not(target_arch = "wasm32"))]
        let mut selected_folder = None;
        let spacing = ui.spacing().item_spacing.x;
        let font = egui::TextStyle::Button.resolve(ui.style());
        let padding = ui.spacing().button_padding.x * 2.0;
        let needed_width = 30.0
            + spacing
            + tabs
                .iter()
                .map(|tab| {
                    let text_width = ui.fonts_mut(|fonts| {
                        fonts
                            .layout_no_wrap(tab.title.clone(), font.clone(), tokens.text)
                            .size()
                            .x
                    });
                    (text_width + padding).max(132.0)
                        + spacing
                        + if can_close { 20.0 + spacing } else { 0.0 }
                })
                .sum::<f32>();
        let scroll_tabs = needed_width > ui.available_width();
        let paint_tabs = |ui: &mut egui::Ui| {
            ui.horizontal_centered(|ui| {
                for tab in tabs {
                    let button =
                        egui::Button::new(egui::RichText::new(&tab.title).color(if tab.active {
                            tokens.text
                        } else {
                            tokens.text_muted
                        }))
                        .fill(if tab.active {
                            tokens.panel_bg_active
                        } else {
                            tokens.panel_bg
                        })
                        .stroke(egui::Stroke::new(
                            1.0_f32,
                            if tab.active {
                                tokens.border_active
                            } else {
                                tokens.border
                            },
                        ))
                        .corner_radius(6)
                        .min_size(egui::vec2(132.0, HEADER_CONTROL_HEIGHT));
                    let (response, _) =
                        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                            #[cfg(not(target_arch = "wasm32"))]
                            folder_tab_menu(
                                ui,
                                &mut self.folder_entry_cache,
                                &tab.path,
                                &mut selected_file,
                                &mut selected_folder,
                            );
                            #[cfg(target_arch = "wasm32")]
                            {
                                ui.label(&tab.title);
                                ui.small("Browser document · local draft only");
                                ui.small("Open imports a copy; Save downloads a copy.");
                            }
                        });
                    self.ui_probe
                        .control(format!("tab:{}", tab.index), &response, tab.active);
                    if response.clicked() {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            self.state.active_folder_tab = tab.index;
                            self.persist_state();
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            let _ = self.browser_action("select", tab.index);
                        }
                    }
                    response.on_hover_text(&tab.hover);
                    if can_close {
                        let close = compact_icon_button(
                            ui,
                            Icon::X,
                            format!(
                                "Close {} tab {}",
                                if cfg!(target_arch = "wasm32") {
                                    "document"
                                } else {
                                    "folder"
                                },
                                tab.hover
                            ),
                        );
                        self.ui_probe
                            .control(format!("tab-close:{}", tab.index), &close, false);
                        if close.clicked() {
                            close_requested = Some(tab.index);
                        }
                    }
                }
                let add = icon_button(
                    ui,
                    Icon::Plus,
                    false,
                    if cfg!(target_arch = "wasm32") {
                        "New browser document".to_string()
                    } else {
                        "New folder tab".to_string()
                    },
                );
                self.ui_probe.control("tab-add", &add, false);
                if add.clicked() {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.select_folder_tab(ctx);
                    #[cfg(target_arch = "wasm32")]
                    self.run_command(ctx, Command::NewTab);
                }
            });
        };
        if scroll_tabs {
            // Bound the navigation allocation, not its content: every tab/close/add
            // control stays reachable by horizontal wheel or touch scrolling.
            let output = egui::ScrollArea::horizontal()
                .id_salt("navigation_tabs")
                .max_width(ui.available_width().max(0.0))
                .max_height(HEADER_CONTROL_HEIGHT)
                .min_scrolled_width(0.0)
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, paint_tabs);
            self.ui_probe.region("tabs_viewport", output.inner_rect);
        } else {
            paint_tabs(ui);
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
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
        #[cfg(target_arch = "wasm32")]
        if let Some(index) = close_requested {
            // Confirmation runs outside the render borrow; completion is
            // generation-bound so it cannot close a newer selected draft.
            self.request_browser_close(index);
        }
    }
}
