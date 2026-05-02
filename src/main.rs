#[cfg(not(target_arch = "wasm32"))]
use eframe::NativeOptions;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let files = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    let options = NativeOptions::default();
    eframe::run_native(
        "Velocimd",
        options,
        Box::new(move |cc| {
            Ok(Box::new(velocimd::ui::VelocimdApp::new_with_files(
                cc, files,
            )))
        }),
    )
    .expect("Failed to run Velocimd");
}

#[cfg(target_arch = "wasm32")]
use eframe::WebOptions;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    let canvas_id = "velocimd-canvas";
    wasm_bindgen_futures::future_to_promise(async {
        eframe::start_web(
            canvas_id,
            Box::new(|cc| Ok(Box::new(velocimd::ui::VelocimdApp::new(cc)))),
        )
        .await
        .expect("Failed to start Velocimd");
        Ok(JsValue::NULL)
    });
}
