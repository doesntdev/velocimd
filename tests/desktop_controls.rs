use velocimd::{app_state::AppState, commands::Command, modes::EditorMode, theme::ThemeConfig};

#[test]
fn toolbar_mode_buttons_are_edit_split_preview() {
    let modes: Vec<_> = Command::toolbar()
        .iter()
        .filter_map(|command| match command {
            Command::SetMode(mode) => Some(*mode),
            _ => None,
        })
        .collect();
    assert_eq!(
        modes,
        [EditorMode::Edit, EditorMode::Split, EditorMode::Preview]
    );
}

#[test]
fn one_toolbar_theme_button_toggles_both_ways_without_editing_or_changing_font_sizes() {
    let commands: Vec<_> = Command::toolbar()
        .iter()
        .copied()
        .filter(|command| command.label().to_lowercase().contains("theme"))
        .collect();
    assert_eq!(
        commands.len(),
        1,
        "the toolbar needs one theme toggle, not two preset buttons"
    );
    let mut state = AppState::fresh();
    state.theme = ThemeConfig::default_dark();
    state.theme.editor_font_size = 19.0;
    state.theme.preview_font_size = 21.0;
    let body = state.active_document().unwrap().content.clone();
    let revision = state.active_document().unwrap().revision;
    state.execute(commands[0]);
    assert_eq!(state.theme.name, "Velocilight");
    assert_eq!(state.theme.editor_font_size, 19.0);
    assert_eq!(state.theme.preview_font_size, 21.0);
    state.execute(commands[0]);
    assert_eq!(state.theme.name, "Velocidark");
    assert_eq!(state.theme.editor_font_size, 19.0);
    assert_eq!(state.theme.preview_font_size, 21.0);
    assert_eq!(state.active_document().unwrap().content, body);
    assert_eq!(state.active_document().unwrap().revision, revision);
}

#[test]
fn explicit_theme_commands_and_mode_shortcuts_remain_available() {
    assert!(Command::all().contains(&Command::SwitchThemeLight));
    assert!(Command::all().contains(&Command::SwitchThemeDark));
    assert_eq!(
        Command::matching("light").first(),
        Some(&Command::SwitchThemeLight)
    );
    assert_eq!(
        Command::matching("dark").first(),
        Some(&Command::SwitchThemeDark)
    );
    assert_eq!(Command::SwitchThemeLight.shortcut(), "Alt+L");
    assert_eq!(Command::SwitchThemeDark.shortcut(), "Alt+D");
    assert_eq!(Command::SetMode(EditorMode::Edit).shortcut(), "Ctrl+1");
    assert_eq!(Command::SetMode(EditorMode::Preview).shortcut(), "Ctrl+2");
    assert_eq!(Command::SetMode(EditorMode::Split).shortcut(), "Ctrl+3");
}

#[test]
fn split_ratio_round_trips_and_legacy_sessions_default_to_balanced() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.json");
    let mut state = AppState::fresh();
    state.split_ratio = 0.72;
    let body = state.active_document().unwrap().content.clone();
    state.save_to(&path).unwrap();
    let restored = AppState::load_from(&path).unwrap();
    assert_eq!(restored.split_ratio, 0.72);
    assert_eq!(restored.active_document().unwrap().content, body);
    let mut legacy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    legacy.as_object_mut().unwrap().remove("split_ratio");
    std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
    let restored = AppState::load_from(&path).unwrap();
    assert_eq!(restored.split_ratio, 0.5);
    assert_eq!(restored.active_document().unwrap().content, body);
}

#[test]
fn invalid_split_ratios_cannot_poison_saved_sessions_or_hide_a_pane() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.json");
    let mut state = AppState::fresh();
    for (ratio, expected) in [
        (f32::NAN, 0.5),
        (f32::INFINITY, 0.5),
        (f32::NEG_INFINITY, 0.5),
        (-10.0, 0.15),
        (10.0, 0.85),
    ] {
        state.split_ratio = ratio;
        state.save_to(&path).unwrap();
        let restored = AppState::load_from(&path).unwrap();
        assert_eq!(restored.split_ratio, expected);
        assert!(
            !std::fs::read_to_string(&path)
                .unwrap()
                .contains("\"split_ratio\": null")
        );
    }
}

#[test]
fn an_explicit_session_keeps_theme_settings_in_its_own_directory() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("state.json");
    AppState::fresh().save_to(&path).unwrap();
    let mut state = AppState::load_from(&path).unwrap();
    state.execute(Command::ToggleTheme);
    state.save_theme().unwrap();
    let theme = ThemeConfig::load_from(temp.path().join("theme.toml")).unwrap();
    assert_eq!(theme, state.theme);
}
