use anyhow::Result;
use velocimd::ui::VelocimdApp;

fn main() -> Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Velocimd",
        options,
        Box::new(|cc| Ok(Box::new(VelocimdApp::new(cc)))),
    )
    .map_err(|err| anyhow::anyhow!(err.to_string()))
}
