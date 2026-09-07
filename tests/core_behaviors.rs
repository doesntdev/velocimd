use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use velocimd::{
    app_state::AppState,
    commands::Command,
    document::Document,
    markdown,
    mermaid::{Direction, NodeShape, PreviewBlock, parse_flowchart, split_markdown_and_mermaid},
    modes::EditorMode,
    theme::ThemeConfig,
    ui::PreviewRenderer,
};

fn unique_temp_path(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be valid")
        .as_nanos();
    std::env::temp_dir().join(format!("velocimd-{nonce}-{name}"))
}

fn unique_temp_dir(name: &str) -> std::path::PathBuf {
    let path = unique_temp_path(name);
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

fn canonical_path(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
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
fn preview_renderer_supports_code_highlighting_and_local_images() {
    let renderer = PreviewRenderer::default();

    assert!(renderer.supports_code_highlighting());
    assert!(renderer.supports_local_images());
}

#[test]
fn markdown_preview_splits_mermaid_fenced_blocks() {
    let blocks = split_markdown_and_mermaid(
        "# Plan\n\n```mermaid\ngraph TD\nA[Start] --> B[Done]\n```\n\nAfter.\n",
    );

    assert_eq!(blocks.len(), 3);
    assert!(matches!(&blocks[0], PreviewBlock::Markdown(markdown) if markdown.contains("# Plan")));
    assert!(matches!(&blocks[1], PreviewBlock::Mermaid(source) if source.contains("A[Start]")));
    assert!(matches!(&blocks[2], PreviewBlock::Markdown(markdown) if markdown.contains("After.")));
}

#[test]
fn mermaid_flowchart_parser_supports_direction_nodes_and_edge_labels() {
    let diagram = parse_flowchart(
        r#"
        flowchart LR
          A[Start] -->|yes| B{Decision}
          B --> C(Done)
        "#,
    )
    .expect("flowchart should parse");

    assert_eq!(diagram.direction, Direction::LeftRight);
    assert_eq!(diagram.edges.len(), 2);
    assert_eq!(diagram.edges[0].label.as_deref(), Some("yes"));
    assert!(diagram.nodes.iter().any(|node| {
        node.id == "B" && node.label == "Decision" && node.shape == NodeShape::Diamond
    }));
}

#[test]
fn markdown_renderer_strips_obvious_script_tags() {
    let html = markdown::render_to_html("# Safe\n\n<script>alert('nope')</script>");

    assert!(html.contains("<h1>Safe</h1>"));
    assert!(!html.to_lowercase().contains("<script"));
}

#[test]
fn markdown_renderer_strips_inline_html() {
    let html = markdown::render_to_html(
        r#"# Safe

<img src=x onerror="alert('nope')">
"#,
    );

    assert!(html.contains("<h1>Safe</h1>"));
    assert!(!html.to_lowercase().contains("<img"));
    assert!(!html.to_lowercase().contains("onerror"));
}

#[test]
fn markdown_renderer_neutralizes_dangerous_urls() {
    let html = markdown::render_to_html("[bad](javascript:alert(1)) ![bad](file:///etc/passwd)");

    assert!(!html.to_lowercase().contains("javascript:"));
    assert!(!html.to_lowercase().contains("file:///"));
    assert!(html.contains("href=\"#\""));
    assert!(html.contains("src=\"#\""));
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
fn default_theme_names_match_velocimd_branding() {
    assert_eq!(ThemeConfig::default_dark().name, "Velocidark");
    assert_eq!(ThemeConfig::default_light().name, "Velocilight");
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
    let expected_path = canonical_path(&path);

    let mut state = AppState::fresh();
    assert!(state.open_file(path.clone()));

    let document = state
        .active_document()
        .expect("opened file should become active document");
    assert_eq!(
        document.title,
        path.file_name().and_then(|name| name.to_str()).unwrap()
    );
    assert_eq!(document.path.as_ref(), Some(&expected_path));
    assert_eq!(document.content, "# Opened\n\nBody");
    assert!(!document.dirty);

    let _ = fs::remove_file(path);
}

#[test]
fn save_file_as_updates_current_document_without_switching_tabs() {
    let path = unique_temp_path("saved-as.md");
    let mut state = AppState::fresh();
    state.new_tab();
    state.new_tab();
    state.active_document = 1;
    state
        .active_document_mut()
        .expect("document should exist")
        .set_content("# Saved\n".to_string());

    assert!(state.save_file_as(path.clone()));
    let expected_path = canonical_path(&path);

    let document = state
        .active_document()
        .expect("saved document should remain active");
    assert_eq!(state.active_document, 1);
    assert_eq!(document.path.as_ref(), Some(&expected_path));
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
    let mut state = AppState::fresh();

    state.close_tab();

    assert_eq!(state.documents.len(), 1);
    assert_eq!(state.active_document, 0);
}

#[test]
fn working_folder_assigns_default_path_and_streams_new_tab() {
    let folder = unique_temp_dir("working-folder");
    let expected_folder = canonical_path(&folder);
    let mut state = AppState::fresh();

    assert!(state.set_working_folder(folder.clone()));
    assert_eq!(state.folder_tabs.len(), 1);
    assert_eq!(state.folder_tabs[0].path, expected_folder);
    state.new_tab();

    let document = state
        .active_document_mut()
        .expect("new tab should be active");
    document.set_content("# Streaming\n\nSaved as typing happens.\n".to_string());

    let path = state
        .stream_active_document()
        .expect("active document should stream to disk");

    assert!(path.starts_with(&expected_folder));
    assert_eq!(
        fs::read_to_string(&path).expect("streamed document should exist"),
        "# Streaming\n\nSaved as typing happens.\n"
    );
    assert!(!state.active_document().unwrap().dirty);

    let _ = fs::remove_dir_all(folder);
}

#[test]
fn folder_tabs_deduplicate_and_drive_default_save_location() {
    let first_folder = unique_temp_dir("folder-tab-first");
    let second_folder = unique_temp_dir("folder-tab-second");
    let expected_first_folder = canonical_path(&first_folder);
    let mut state = AppState::fresh();

    assert!(state.add_folder_tab(first_folder.clone()));
    assert!(state.add_folder_tab(second_folder.clone()));
    assert!(state.add_folder_tab(first_folder.clone()));

    assert_eq!(state.folder_tabs.len(), 2);
    assert_eq!(
        state
            .active_folder_path()
            .expect("active folder should exist"),
        expected_first_folder.as_path()
    );

    let path = state
        .save_file()
        .expect("pathless document should save into active folder tab");

    assert!(path.starts_with(&expected_first_folder));

    let _ = fs::remove_dir_all(first_folder);
    let _ = fs::remove_dir_all(second_folder);
}

#[test]
fn opening_same_markdown_file_switches_to_existing_document() {
    let folder = unique_temp_dir("open-existing-document");
    let path = folder.join("Note.md");
    fs::write(&path, "# Note\n").expect("fixture should write");
    let expected_folder = canonical_path(&folder);

    let mut state = AppState::fresh();
    assert!(state.open_file(path.clone()));
    let document_count = state.documents.len();
    let opened_index = state.active_document;

    state.active_document = 0;
    assert!(state.open_file(path));

    assert_eq!(state.documents.len(), document_count);
    assert_eq!(state.active_document, opened_index);
    assert_eq!(state.folder_tabs.len(), 1);
    assert_eq!(
        state.active_folder_path().unwrap(),
        expected_folder.as_path()
    );

    let _ = fs::remove_dir_all(folder);
}

#[test]
fn save_file_uses_working_folder_for_pathless_document() {
    let folder = unique_temp_dir("working-folder-save");
    let mut state = AppState::fresh();

    state.working_folder = Some(folder.clone());
    state
        .active_document_mut()
        .expect("document should exist")
        .set_content("# Saved in folder\n".to_string());

    let path = state
        .save_file()
        .expect("pathless document should save into working folder");

    assert!(path.starts_with(&folder));
    assert_eq!(
        fs::read_to_string(&path).expect("saved document should exist"),
        "# Saved in folder\n"
    );

    let _ = fs::remove_dir_all(folder);
}

#[test]
fn document_display_title_hides_markdown_extension() {
    let mut document = Document::scratch("Notes.md", "");

    assert_eq!(document.visible_title(), "Notes");
    assert_eq!(document.display_title(), "Notes");

    document.dirty = true;
    assert_eq!(document.display_title(), "Notes •");
}

#[test]
fn rename_document_updates_file_name_with_markdown_extension() {
    let folder = unique_temp_dir("rename-document");
    let old_path = folder.join("Old.md");
    fs::write(&old_path, "# Old\n").expect("fixture should write");
    let expected_new_path = canonical_path(&folder).join("New notes.md");

    let mut state = AppState::fresh();
    assert!(state.open_file(old_path.clone()));

    let new_path = state
        .rename_document(state.active_document, "New notes")
        .expect("rename should update file path");

    assert_eq!(new_path, expected_new_path);
    assert!(!old_path.exists());
    assert_eq!(
        fs::read_to_string(&new_path).expect("renamed file should exist"),
        "# Old\n"
    );
    assert_eq!(
        state
            .active_document()
            .expect("document should exist")
            .title,
        "New notes.md"
    );
    assert_eq!(
        state
            .active_document()
            .expect("document should exist")
            .visible_title(),
        "New notes"
    );

    let _ = fs::remove_dir_all(folder);
}

#[test]
fn command_registry_includes_file_theme_and_mode_commands() {
    let commands = Command::all();

    assert!(commands.contains(&Command::NewTab));
    assert!(commands.contains(&Command::OpenFile));
    assert!(commands.contains(&Command::SaveFile));
    assert!(commands.contains(&Command::SwitchThemeLight));
    assert!(commands.contains(&Command::SetMode(EditorMode::Split)));
}
