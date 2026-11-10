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
    // Opt-in only. Refuse every pre-existing path (including symlinks). Keep
    // this exact owned handle through the run, even if the pathname is replaced.
    let probe_file = std::env::var_os("VELOCIMD_UI_PROBE_PATH").map(|path| {
        create_probe_file(path).expect("UI probe destination must be new and exclusively owned")
    });
    let options = native_options();
    eframe::run_native(
        "Velocimd",
        options,
        Box::new(move |cc| {
            let mut app = velocimd::ui::VelocimdApp::new_with_files(cc, files);
            if let Some(file) = probe_file {
                let mut file = Some(file);
                app.set_ui_probe(true);
                app.set_ui_probe_observer(Box::new(move |snapshot| {
                    if let Some(writer) = file.as_mut()
                        && let Err(error) = write_probe_snapshot(writer, snapshot)
                    {
                        eprintln!("UI probe write failed; observer disabled: {error}");
                        file = None;
                    }
                }));
            }
            Ok(Box::new(app))
        }),
    )
    .expect("Failed to run Velocimd");
}

#[cfg(not(target_arch = "wasm32"))]
fn create_probe_file(path: impl AsRef<std::path::Path>) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(not(target_arch = "wasm32"))]
fn write_probe_snapshot(file: &mut std::fs::File, snapshot: &str) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};
    file.seek(SeekFrom::Start(0))?;
    file.write_all(snapshot.as_bytes())?;
    file.set_len(snapshot.len() as u64)?;
    file.flush()
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

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn probe_refuses_existing_destination_and_shorter_snapshot_has_no_tail() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("probe.json");
        let mut file = create_probe_file(&path).unwrap();
        write_probe_snapshot(&mut file, "{\"long_field\":true}").unwrap();
        assert!(create_probe_file(&path).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"long_field\":true}"
        );
        write_probe_snapshot(&mut file, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
    }

    #[cfg(unix)]
    #[test]
    fn probe_refuses_symlinks_and_retains_owned_handle_after_path_replacement() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("probe.json");
        let target = temp.path().join("protected.json");
        std::fs::write(&target, "protected").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(create_probe_file(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        let mut file = create_probe_file(&path).unwrap();
        let retired = temp.path().join("owned.json");
        std::fs::rename(&path, &retired).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        write_probe_snapshot(&mut file, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "protected");
        assert_eq!(std::fs::read_to_string(&retired).unwrap(), "{}");
    }
}
