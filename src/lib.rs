pub mod app_state;
pub mod commands;
pub mod document;
mod file_io;
pub mod icons;
pub mod markdown;
pub mod mermaid;
pub mod modes;
pub mod theme;
pub mod ui;

#[cfg(target_arch = "wasm32")]
use eframe::WebRunner;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;
#[cfg(target_arch = "wasm32")]
use web_sys::HtmlCanvasElement;

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
#[wasm_bindgen]
pub struct WebHandle {
    runner: WebRunner,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl WebHandle {
    #[wasm_bindgen(constructor)]
    pub fn new() -> WebHandle {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();
        WebHandle {
            runner: WebRunner::new(),
        }
    }

    #[wasm_bindgen]
    pub async fn start(&self, canvas: HtmlCanvasElement) -> Result<(), JsValue> {
        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| Ok(Box::new(crate::ui::VelocimdApp::new(cc)))),
            )
            .await
            .map_err(|e| JsValue::from_str(&format!("Failed: {e:?}")))?;
        Ok(())
    }
}
