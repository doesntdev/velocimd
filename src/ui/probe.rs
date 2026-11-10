//! Opt-in observation of existing widget Responses. No widgets, input, timers or
//! commands are introduced by the probe; disabled snapshots contain no document.
use super::*;
use serde::Serialize;
use std::collections::BTreeMap;

#[cfg(not(target_arch = "wasm32"))]
type Observer = Box<dyn FnMut(&str)>;

#[derive(Default)]
pub(super) struct UiProbe {
    enabled: bool,
    controls: Vec<Control>,
    regions: BTreeMap<&'static str, [f32; 4]>,
    snapshot: String,
    #[cfg(not(target_arch = "wasm32"))]
    observer: Option<Observer>,
}

#[derive(Serialize)]
struct Control {
    id: String,
    rect: [f32; 4],
    enabled: bool,
    selected: bool,
}

fn rect(rect: egui::Rect) -> [f32; 4] {
    [rect.min.x, rect.min.y, rect.width(), rect.height()]
}

impl UiProbe {
    pub(super) fn begin(&mut self) {
        self.controls.clear();
        self.regions.clear();
    }

    pub(super) fn control(
        &mut self,
        id: impl Into<String>,
        response: &egui::Response,
        selected: bool,
    ) {
        if self.enabled {
            self.controls.push(Control {
                id: id.into(),
                rect: rect(response.rect),
                enabled: response.enabled(),
                selected,
            });
        }
    }

    // Callers supply bounds from existing real Responses, clipped to their
    // actual scroll viewport for document-sized editor/preview frames.
    pub(super) fn region(&mut self, id: &'static str, bounds: egui::Rect) {
        if self.enabled {
            self.regions.insert(id, rect(bounds));
        }
    }
}

impl VelocimdApp {
    /// Enables read-only UI measurements after the next real render. Off by default.
    pub fn set_ui_probe(&mut self, enabled: bool) {
        self.ui_probe.enabled = enabled;
        #[cfg(target_arch = "wasm32")]
        if let Some(ctx) = &self.browser.context {
            ctx.request_repaint();
        }
        if !enabled {
            self.ui_probe.snapshot.clear();
            self.ui_probe.begin();
        }
    }

    /// Last rendered measurement, or `{}` before an enabled frame / while disabled.
    pub fn ui_snapshot(&self) -> String {
        if self.ui_probe.snapshot.is_empty() {
            "{}".into()
        } else {
            self.ui_probe.snapshot.clone()
        }
    }

    /// Observes changed rendered snapshots only. Does not itself enable probing.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_ui_probe_observer(&mut self, observer: Box<dyn FnMut(&str)>) {
        self.ui_probe.observer = Some(observer);
    }

    pub(super) fn finish_ui_probe(&mut self, ctx: &egui::Context) {
        if !self.ui_probe.enabled || ctx.will_discard() {
            return;
        }
        let viewport = ctx.content_rect().size();
        let active = self.state.active_document();
        let snapshot = serde_json::json!({
            "schema": 1,
            "viewport": [viewport.x, viewport.y],
            "pixels_per_point": ctx.pixels_per_point(),
            "mode": format!("{:?}", self.state.mode),
            "light": self.state.theme.is_light(),
            "split_ratio": self.state.split_ratio,
            "active_title": active.map(|d| d.title.as_str()).unwrap_or(""),
            "active_content": active.map(|d| d.content.as_str()).unwrap_or(""),
            "controls": self.ui_probe.controls,
            "regions": self.ui_probe.regions,
        })
        .to_string();
        if snapshot != self.ui_probe.snapshot {
            self.ui_probe.snapshot = snapshot;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(observer) = &mut self.ui_probe.observer {
                observer(&self.ui_probe.snapshot);
            }
        }
    }
}

pub(super) fn toolbar_id(command: Command) -> &'static str {
    match command {
        Command::NewTab => "toolbar:new",
        Command::SelectWorkingFolder => "toolbar:open-folder",
        Command::OpenFile => "toolbar:open-file",
        Command::SaveFile => "toolbar:save",
        Command::SaveFileAs => "toolbar:save-as",
        Command::CloseFolderTab => "toolbar:close",
        Command::SetMode(EditorMode::Edit) => "toolbar:edit",
        Command::SetMode(EditorMode::Split) => "toolbar:split",
        Command::SetMode(EditorMode::Preview) => "toolbar:preview",
        Command::CycleMode => "toolbar:cycle",
        Command::ToggleTheme => "toolbar:theme",
        Command::TogglePalette => "palette",
        Command::SwitchThemeLight => "theme:light",
        Command::SwitchThemeDark => "theme:dark",
    }
}
