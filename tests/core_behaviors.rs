use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};
use velocimd::{
    app_state::AppState, commands::Command, markdown, modes::EditorMode, theme::ThemeConfig,
    ui::PreviewRenderer,
};

fn unique_temp_path(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("velocimd-{nonce}-{name}"))
}

#[test]
fn editor_mode_cycles_edit_preview_split() {
    assert_eq!(EditorMode::Edit.next(), EditorMode::Preview);
    assert_eq!(EditorMode::Preview.next(), EditorMode::Split);
    assert_eq!(EditorMode::Split.next(), EditorMode::Edit);
}

#[test]
fn markdown_renderer_outputs_headings_and_emphasis() {
    let html = markdown::render_to_html("# Velocimd\n\nThis is **fast**.");

    assert!(html.contains("<h1>Velocimd</h1>"));
    assert!(html.contains("<strong>fast</strong>"));
}

#[test]
fn preview_renderer_is_egui_native_not_raw_html_text() {
    assert_eq!(PreviewRenderer::default().name(), "egui-commonmark");
}

#[test]
fn markdown_renderer_strips_obvious_script_tags() {
    let html = markdown::render_to_html("# Safe\n\n<script>alert('nope')</script>");

    assert!(html.contains("<h1>Safe</h1>"));
    assert!(!html.to_lowercase().contains("<script"));
}

#[test]
fn theme_config_loads_from_toml() {
    let config = ThemeConfig::from_toml(
        r##"
        name = "Midnight"
        background = "#10131a"
        foreground = "#f7f7ff"
        accent = "#7aa2ff"
        editor_font_size = 15.5
        preview_font_size = 16.0
        "##,
    )
    .expect("theme should parse");

    assert_eq!(config.name, "Midnight");
    assert_eq!(config.background, "#10131a");
    assert_eq!(config.editor_font_size, 15.5);
}

#[test]
fn theme_config_round_trips_through_toml_file() {
    let path = unique_temp_path("theme.toml");
    let config = ThemeConfig::default_light();

    config.save_to(&path).expect("theme should save");
    let loaded = ThemeConfig::load_from(&path).expect("theme should load");
    let _ = fs::remove_file(path);

    assert_eq!(loaded, config);
}

#[test]
fn invalid_theme_missing_name_fails_fast() {
    let error = ThemeConfig::from_toml(
        r##"
        background = "#10131a"
        foreground = "#f7f7ff"
        accent = "#7aa2ff"
        "##,
    )
    .expect_err("theme without name should fail");

    assert!(error.to_string().contains("missing field `name`"));
}

#[test]
fn app_state_opens_markdown_file_as_clean_active_tab() {
    let path = unique_temp_path("opened.md");
    fs::write(&path, "# Opened\n\nBody").expect("fixture should write");

    let mut state = AppState::new();
    assert!(state.open_file(path.clone()));

    let document = state
        .active_document()
        .expect("opened file should become active document");
    assert_eq!(
        document.title,
        path.file_name().and_then(|name| name.to_str()).unwrap()
    );
    assert_eq!(document.path.as_ref(), Some(&path));
    assert_eq!(document.content, "# Opened\n\nBody");
    assert!(!document.dirty);

    let _ = fs::remove_file(path);
}

#[test]
fn save_file_as_updates_current_document_without_switching_tabs() {
    let path = unique_temp_path("saved-as.md");
    let mut state = AppState::new();
    state.new_tab();
    state.new_tab();
    state.active_document = 1;
    state
        .active_document_mut()
        .expect("document should exist")
        .set_content("# Saved\n".to_string());

    assert!(state.save_file_as(path.clone()));

    let document = state
        .active_document()
        .expect("saved document should remain active");
    assert_eq!(state.active_document, 1);
    assert_eq!(document.path.as_ref(), Some(&path));
    assert_eq!(
        document.title,
        path.file_name().and_then(|name| name.to_str()).unwrap()
    );
    assert_eq!(
        fs::read_to_string(&path).expect("saved file should exist"),
        "# Saved\n"
    );
    assert!(!document.dirty);

    let _ = fs::remove_file(path);
}

#[test]
fn close_tab_keeps_at_least_one_document() {
    let mut state = AppState::new();

    state.execute(Command::CloseTab);

    assert_eq!(state.documents.len(), 1);
    assert_eq!(state.active_document, 0);
}

#[test]
fn command_registry_includes_file_theme_and_mode_commands() {
    let commands = Command::all();

    assert!(commands.contains(&Command::OpenFile));
    assert!(commands.contains(&Command::SaveFile));
    assert!(commands.contains(&Command::SwitchThemeLight));
    assert!(commands.contains(&Command::SetMode(EditorMode::Split)));
}
