#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;
use eframe::WebOptions;

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    // Initialize panic hook for better error messages
    console_error_panic_hook::set_once();
    
    let web_options = WebOptions::default();
    
    // The canvas ID where the app will render
    let canvas_id = "velocimd-canvas";
    
    wasm_bindgen_futures::future_to_promise(async {
        eframe::start_web(canvas_id, Box::new(|cc| {
            Ok(Box::new(velocimd::ui::VelocimdApp::new(cc)))
        }))
        .await
        .map_err(|e| JsValue::from_str(&format!("Failed to start: {}", e)))?;
        
        Ok(JsValue::NULL)
    });
    
    Ok(())
}
