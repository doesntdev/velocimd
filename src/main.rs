#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

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
    let options = native_options();
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

#[cfg(not(target_arch = "wasm32"))]
fn native_options() -> NativeOptions {
    let mut options = NativeOptions::default();
    if let Ok(icon) =
        eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/velocimd.png"))
    {
        options.viewport = options.viewport.with_icon(icon);
    }
    options
}

#[cfg(target_arch = "wasm32")]
fn main() {}
