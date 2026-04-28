use velocimd::{markdown, modes::EditorMode, theme::ThemeConfig};

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
