//! The actual egui editor with browser document tabs and local drafts. None of
//! the native folder picker, disk writer or persistence worker is reachable here.
use super::*;
use crate::browser::{MAX_DOCUMENTS, filename};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = velocimdQueueEvent)]
    fn queue_browser_event(name: &str, detail: &str);
}

impl VelocimdApp {
    pub(crate) fn browser_action(&mut self, action: &str, index: usize) -> Result<(), String> {
        let ctx = self.browser.context.clone().ok_or("Editor is not ready")?;
        match action {
            "new" => {
                if self.state.documents.len() >= MAX_DOCUMENTS {
                    return Err("At most 16 browser tabs; download and close a tab first".into());
                }
                self.browser.import_generation = self.browser.import_generation.wrapping_add(1);
                self.state.new_tab_deferred();
                self.reset_scroll_sync();
                self.persist_state();
            }
            "select" => {
                if index >= self.state.documents.len() {
                    return Err("Unknown document tab".into());
                }
                self.browser.import_generation = self.browser.import_generation.wrapping_add(1);
                self.state.active_document = index;
                self.reset_scroll_sync();
                self.persist_state();
            }
            "close" => {
                if self.state.documents.len() <= 1 {
                    return Err("Keep at least one browser document open".into());
                }
                self.browser.import_generation = self.browser.import_generation.wrapping_add(1);
                self.state.close_tab();
                self.reset_scroll_sync();
                self.persist_state();
            }
            "edit" => self.run_command(&ctx, Command::SetMode(EditorMode::Edit)),
            "preview" => self.run_command(&ctx, Command::SetMode(EditorMode::Preview)),
            "split" => self.run_command(&ctx, Command::SetMode(EditorMode::Split)),
            "theme" => self.run_command(&ctx, Command::ToggleTheme),
            "retry" => {
                self.browser.retry(&self.state)?;
                self.autosave_due = None;
                self.state_save_due = None;
            }
            "replace-recovery" => {
                self.browser.replace_recovered(&self.state)?;
            }
            "verify" => {
                self.browser.changed();
                self.browser.save(&self.state)?;
            }
            "flush" => self.flush_browser_drafts()?,
            _ => return Err("Unknown browser action".into()),
        }
        ctx.request_repaint();
        Ok(())
    }

    pub(super) fn request_browser_close(&mut self, index: usize) {
        if self.state.documents.len() <= 1 {
            return;
        }
        queue_browser_event(
            "velocimd-effect",
            &serde_json::json!({
                "kind":"close", "index":index, "generation":self.browser.import_generation,
            })
            .to_string(),
        );
    }

    pub(crate) fn complete_browser_close(
        &mut self,
        generation: u32,
        index: usize,
    ) -> Result<(), String> {
        if generation != self.browser.import_generation {
            return Err("Close cancelled because the selected file or document changed".into());
        }
        if self.state.documents.len() <= 1 {
            return Err("Keep at least one browser document open".into());
        }
        self.browser_action("select", index)?;
        self.browser_action("close", index)
    }

    pub(crate) fn begin_browser_import(&mut self) -> u32 {
        self.browser.import_generation = self.browser.import_generation.wrapping_add(1);
        self.browser.import_generation
    }

    pub(crate) fn complete_browser_import(
        &mut self,
        generation: u32,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), String> {
        if generation != self.browser.import_generation {
            return Err("Import cancelled because the selected file or document changed".into());
        }
        if self.state.documents.len() >= MAX_DOCUMENTS {
            return Err("At most 16 browser tabs; download and close a tab first".into());
        }
        let document = crate::browser::import(name, bytes)?;
        self.state.documents.push(document);
        self.state.active_document = self.state.documents.len() - 1;
        if let Some(ctx) = &self.browser.context {
            ctx.request_repaint();
        }
        // A completion is single-use, even if an external caller replays it.
        self.browser.import_generation = self.browser.import_generation.wrapping_add(1);
        self.reset_scroll_sync();
        self.persist_state();
        Ok(())
    }

    pub(crate) fn browser_snapshot(&self) -> String {
        serde_json::json!({
            "documents": self.state.documents.iter().map(|d| &d.title).collect::<Vec<_>>(),
            "active": self.state.active_document,
            "mode": self.state.mode.label(),
            "light": self.state.theme.is_light(),
            "pending": self.browser.pending,
            "error": self.browser.error,
            "status": self.browser.message,
            "recovery": self.browser.recovery.is_some(),
            "palette_open": self.state.command_palette_open,
            "persistence": self.browser.metrics(),
        })
        .to_string()
    }

    pub(crate) fn browser_active(&self) -> Result<String, String> {
        let document = self.state.active_document().ok_or("No active document")?;
        let title = filename(&document.title)?;
        Ok(serde_json::json!({"title": title, "content": document.content}).to_string())
    }

    pub(crate) fn browser_recovery(&self) -> Option<String> {
        self.browser.recovery.clone()
    }

    pub(crate) fn flush_browser_drafts(&mut self) -> Result<(), String> {
        if !self.browser.pending {
            return Ok(());
        }
        let result = self.browser.save(&self.state);
        if let Err(error) = &result {
            self.browser.error = Some(error.clone());
        }
        result
    }

    pub(super) fn publish_browser_state(&mut self) {
        let snapshot = self.browser_snapshot();
        if snapshot == self.browser.published {
            return;
        }
        self.browser.published = snapshot.clone();
        queue_browser_event("velocimd-state", &snapshot);
    }

    pub(super) fn commit_file_name_edit(&mut self, index: usize, id: u64) {
        if self.state.documents.get(index).is_none_or(|d| d.id != id) {
            return;
        }
        let proposed = self.file_name_editor_text.trim();
        let proposed = if proposed.is_empty() {
            "Untitled"
        } else {
            proposed
        };
        let name = if proposed.to_lowercase().ends_with(".md")
            || proposed.to_lowercase().ends_with(".markdown")
        {
            proposed.to_string()
        } else {
            format!("{proposed}.md")
        };
        match filename(&name) {
            Ok(name) if name != self.state.documents[index].title => {
                self.state.documents[index].title = name;
                self.state.documents[index].mark_identity_changed();
                self.persist_state();
            }
            Ok(_) => {}
            Err(error) => self.browser.error = Some(error),
        }
        self.file_name_editor_text = self.state.documents[index].visible_title();
    }

    pub(super) fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        if self.handle_split_activation(ctx, command) {
            return;
        }
        let result = match command {
            Command::NewTab => self.browser_action("new", 0),
            Command::CloseFolderTab => {
                self.request_browser_close(self.state.active_document);
                Ok(())
            }
            Command::SelectWorkingFolder => {
                self.browser.error = Some("Folder navigation is desktop-only. Browser tabs are documents; Open imports a copy without disk-write permission.".into());
                Ok(())
            }
            Command::OpenFile => {
                let generation = self.begin_browser_import();
                queue_browser_event(
                    "velocimd-effect",
                    &serde_json::json!({"kind":"open", "generation":generation}).to_string(),
                );
                Ok(())
            }
            Command::SaveFile | Command::SaveFileAs => {
                // Capture bytes now; host effects run only after rendering releases app_mut.
                if let Ok(active) = self.browser_active() {
                    queue_browser_event(
                        "velocimd-effect",
                        &serde_json::json!({
                            "kind": if command == Command::SaveFileAs { "save-as" } else { "save" },
                            "document": serde_json::from_str::<serde_json::Value>(&active).unwrap(),
                        })
                        .to_string(),
                    );
                }
                self.browser.message =
                    "Download a copy · imported disk files are never overwritten".into();
                self.flush_browser_drafts()
            }
            Command::ToggleTheme => {
                self.switch_theme(ctx, self.state.theme.toggled());
                Ok(())
            }
            Command::SwitchThemeLight => {
                self.switch_theme(ctx, ThemeConfig::default_light());
                Ok(())
            }
            Command::SwitchThemeDark => {
                self.switch_theme(ctx, ThemeConfig::default_dark());
                Ok(())
            }
            Command::TogglePalette => {
                self.state.command_palette_open = !self.state.command_palette_open;
                self.state.command_query.clear();
                self.palette_selection = 0;
                self.palette_focus_requested = self.state.command_palette_open;
                Ok(())
            }
            command => {
                self.state.execute(command);
                self.persist_state();
                Ok(())
            }
        };
        if let Err(error) = result {
            self.browser.error = Some(error);
        }
        ctx.request_repaint();
    }

    pub(super) fn process_deferred_work(&mut self, ctx: &egui::Context) {
        if self.browser.context.is_none() {
            self.browser.context = Some(ctx.clone());
        }
        if self.browser.poll_completion() {
            let _ = self.flush_browser_drafts();
        }
        let now = Instant::now();
        let due = match (self.autosave_due, self.state_save_due) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
        if let Some(due) = due {
            if now >= due {
                self.autosave_due = None;
                self.state_save_due = None;
                let _ = self.flush_browser_drafts();
            } else {
                ctx.request_repaint_after(due.duration_since(now));
            }
        }
    }

    pub(super) fn persist_state(&mut self) {
        self.browser.changed();
        self.state_save_due = Some(Instant::now() + STATE_SAVE_DEBOUNCE);
        if let Some(ctx) = &self.browser.context {
            ctx.request_repaint_after(STATE_SAVE_DEBOUNCE);
        }
    }

    pub(super) fn shutdown_persistence(&mut self) {
        self.browser.shutdown(&self.state);
    }
}
