pub mod app_state;
#[cfg(any(test, target_arch = "wasm32"))]
mod browser;
pub mod commands;
pub mod document;
mod file_io;
pub mod icons;
pub mod markdown;
pub mod mermaid;
pub mod modes;
pub mod persistence;
pub mod theme;
pub mod ui;

#[cfg(target_arch = "wasm32")]
use eframe::WebRunner;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;

/// HTML generation helper; the editor itself continues to use its egui preview.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub fn render_markdown(source: &str) -> String {
    markdown::render_to_html(source)
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
#[wasm_bindgen]
pub struct WebHandle {
    runner: WebRunner,
    lease: std::rc::Rc<std::cell::RefCell<Option<JsValue>>>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = velocimdAcquireLease)]
    async fn acquire_browser_lease() -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name = velocimdReleaseLease)]
    fn release_browser_lease(token: JsValue);
}

#[cfg(target_arch = "wasm32")]
impl Drop for WebHandle {
    fn drop(&mut self) {
        if std::rc::Rc::strong_count(&self.lease) == 1 {
            // Destroy/flush the real App before releasing its origin-wide writer lease.
            self.runner.destroy();
            if let Some(token) = self.lease.borrow_mut().take() {
                release_browser_lease(token);
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl Default for WebHandle {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebHandle {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WebHandle {
        console_error_panic_hook::set_once();
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();
        WebHandle {
            runner: WebRunner::new(),
            lease: Default::default(),
        }
    }

    pub fn action(&self, action: &str, index: usize) -> Result<(), JsValue> {
        self.runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .browser_action(action, index)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn close_document(&self, generation: u32, index: usize) -> Result<(), JsValue> {
        self.runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .complete_browser_close(generation, index)
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn begin_import(&self) -> Result<u32, JsValue> {
        Ok(self
            .runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .begin_browser_import())
    }

    pub fn complete_import(
        &self,
        generation: u32,
        name: &str,
        bytes: Vec<u8>,
    ) -> Result<(), JsValue> {
        self.runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .complete_browser_import(generation, name, bytes)
            .map_err(|e| JsValue::from_str(&e))
    }

    /// Read-only real egui Response measurements, opt-in and disabled by default.
    pub fn set_ui_probe(&self, enabled: bool) -> Result<(), JsValue> {
        self.runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .set_ui_probe(enabled);
        Ok(())
    }

    pub fn ui_snapshot(&self) -> Result<String, JsValue> {
        Ok(self
            .runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .ui_snapshot())
    }

    pub fn snapshot(&self) -> Result<String, JsValue> {
        Ok(self
            .runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .browser_snapshot())
    }

    pub fn active_document(&self) -> Result<String, JsValue> {
        self.runner
            .app_mut::<crate::ui::VelocimdApp>()
            .ok_or_else(|| JsValue::from_str("Editor is not ready"))?
            .browser_active()
            .map_err(|e| JsValue::from_str(&e))
    }

    pub fn recovery_drafts(&self) -> Option<String> {
        self.runner
            .app_mut::<crate::ui::VelocimdApp>()?
            .browser_recovery()
    }

    #[wasm_bindgen]
    pub async fn start(&self, canvas: HtmlCanvasElement) -> Result<(), JsValue> {
        if self.lease.borrow().is_some() {
            return Err(JsValue::from_str("This editor is already running"));
        }
        // Web Locks is acquired before App construction and held through teardown,
        // covering autosave, retry, recovery replacement and lifecycle flush together.
        let token = acquire_browser_lease().await?;
        *self.lease.borrow_mut() = Some(token);
        let result = self
            .runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(crate::ui::VelocimdApp::new(cc)))),
            )
            .await;
        if let Err(error) = result {
            self.runner.destroy();
            if let Some(token) = self.lease.borrow_mut().take() {
                release_browser_lease(token);
            }
            return Err(JsValue::from_str(&format!("Failed: {error:?}")));
        }
        Ok(())
    }
}
