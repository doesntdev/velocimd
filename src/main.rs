#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

#[cfg(not(target_arch = "wasm32"))]
use eframe::NativeOptions;
#[cfg(not(target_arch = "wasm32"))]
use std::ffi::OsString;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    let args: Vec<OsString> = std::env::args_os().collect();

    if args.get(1).is_some_and(|arg| arg == "render") {
        match run_render(&args[2..]) {
            Ok(()) => return,
            Err(err) => {
                eprintln!("velocimd render: {err}");
                std::process::exit(1);
            }
        }
    }

    let files = args.iter().skip(1).map(PathBuf::from).collect::<Vec<_>>();
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
fn run_render(args: &[OsString]) -> anyhow::Result<()> {
    use anyhow::{Context, bail};
    use std::fs;
    use std::io::Write;
    use velocimd::{html_doc, markdown, theme::ThemeConfig};

    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut theme_name = String::from("dark");

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].to_string_lossy();
        match arg.as_ref() {
            "--out" | "-o" => {
                i += 1;
                let value = args.get(i).context("--out requires a path")?;
                output = Some(PathBuf::from(value));
            }
            "--theme" | "-t" => {
                i += 1;
                let value = args.get(i).context("--theme requires a value")?;
                theme_name = value.to_string_lossy().into_owned();
            }
            "--help" | "-h" => {
                print_render_help();
                return Ok(());
            }
            other if !other.starts_with('-') && input.is_none() => {
                input = Some(PathBuf::from(&args[i]));
            }
            other => bail!("unexpected argument: {other}"),
        }
        i += 1;
    }

    let input_path = input.context("missing input file (try: velocimd render INPUT.md)")?;
    let markdown_source = fs::read_to_string(&input_path)
        .with_context(|| format!("failed to read {}", input_path.display()))?;

    let theme = match theme_name.to_lowercase().as_str() {
        "light" | "velocilight" => ThemeConfig::default_light(),
        "dark" | "velocidark" => ThemeConfig::default_dark(),
        other => bail!("unknown theme '{other}', expected 'dark' or 'light'"),
    };

    let title = input_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Velocimd")
        .to_string();

    let rendered = markdown::render_to_html_with_mermaid(&markdown_source);
    let document = html_doc::wrap_html_document(&rendered, &theme, &title);

    match output {
        Some(out_path) => {
            fs::write(&out_path, &document)
                .with_context(|| format!("failed to write {}", out_path.display()))?;
            eprintln!("rendered → {}", out_path.display());
        }
        None => {
            std::io::stdout().write_all(document.as_bytes())?;
        }
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn print_render_help() {
    eprintln!("velocimd render — convert Markdown to themed HTML");
    eprintln!();
    eprintln!("Usage: velocimd render INPUT.md [OPTIONS]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -o, --out FILE      Write output to FILE (default: stdout)");
    eprintln!("  -t, --theme NAME    Theme: 'dark' or 'light' (default: dark)");
    eprintln!("  -h, --help          Show this help");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  velocimd render notes.md > notes.html");
    eprintln!("  velocimd render notes.md --theme light --out notes.html");
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
