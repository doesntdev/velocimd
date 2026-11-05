use super::{preview_sync_tests::test_app, *};

fn frame(
    app: &mut VelocimdApp,
    ctx: &egui::Context,
    time: f64,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        },
        |root| {
            app.handle_shortcuts(root.ctx());
            egui::CentralPanel::default().show_inside(root, |ui| {
                app.top_bar(ui, ctx);
                app.workspace(ui, ctx);
            });
            app.show_dialog_error(ctx);
        },
    )
}

fn text(output: &egui::FullOutput) -> Vec<&str> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect()
}

#[test]
fn header_does_not_paint_the_theme_name() {
    let (_temp, mut app) = test_app("# Note\n\nOriginal.");
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    for theme in [ThemeConfig::default_dark(), ThemeConfig::default_light()] {
        app.state.theme = theme;
        app.state.theme.apply_to(&ctx);
        let output = frame(&mut app, &ctx, 0.0, Vec::new());
        assert!(
            !text(&output)
                .iter()
                .any(|s| s.contains("Theme:") || s.contains(&app.state.theme.name))
        );
    }
}

#[test]
fn real_split_divider_drags_persists_and_double_click_resets_without_note_edits() {
    let content = "# Original\n\n".repeat(30);
    let (temp, mut app) = test_app(&content);
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    app.state.theme.apply_to(&ctx);
    let original_revision = app.state.active_document().unwrap().revision;
    frame(&mut app, &ctx, 0.0, Vec::new());
    let position = egui::pos2(500.0, 350.0);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    frame(
        &mut app,
        &ctx,
        0.1,
        vec![egui::Event::PointerMoved(position), button(position, true)],
    );
    let moved = egui::pos2(700.0, 350.0);
    frame(&mut app, &ctx, 0.2, vec![egui::Event::PointerMoved(moved)]);
    frame(&mut app, &ctx, 0.3, vec![button(moved, false)]);
    assert!(
        app.state.split_ratio > 0.65,
        "divider must move, not remain fixed at 50/50"
    );
    assert!(
        app.state_save_due.is_some(),
        "a changed split must schedule session persistence"
    );
    let path = temp.path().join("resized-session.json");
    app.state.save_to(&path).unwrap();
    let restored = AppState::load_from(&path).unwrap();
    assert_eq!(restored.split_ratio, app.state.split_ratio);
    for (time, pressed) in [(0.5, true), (0.55, false), (0.65, true), (0.7, false)] {
        frame(
            &mut app,
            &ctx,
            time,
            vec![egui::Event::PointerMoved(moved), button(moved, pressed)],
        );
    }
    assert_eq!(
        app.state.split_ratio, 0.5,
        "double-click resets the divider"
    );
    assert_eq!(app.state.active_document().unwrap().content, content);
    assert_eq!(
        app.state.active_document().unwrap().revision,
        original_revision
    );
    assert!(
        app.autosave_due.is_none(),
        "resizing is not a document edit"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn chooser_cancel_and_error_preserve_state_and_error_is_visible_and_retryable() {
    let (temp, mut app) = test_app("# Irreplaceable draft");
    let session = temp.path().join("state.json");
    let before_state = serde_json::to_value(&app.state).unwrap();
    let before_disk = fs::read(&session).unwrap();
    let ctx = egui::Context::default();
    egui_extras::install_image_loaders(&ctx);
    assert!(
        app.dialog_selection(&ctx, "open the folder picker", Ok(None))
            .is_none()
    );
    assert!(app.dialog_error.is_none());
    assert!(
        app.dialog_selection(
            &ctx,
            "open the folder picker",
            Err(native_dialog::Error::Other("fixture failure".into()))
        )
        .is_none()
    );
    assert!(
        app.dialog_error
            .as_deref()
            .unwrap()
            .contains("fixture failure")
    );
    assert_eq!(serde_json::to_value(&app.state).unwrap(), before_state);
    assert_eq!(fs::read(&session).unwrap(), before_disk);
    assert!(app.state_save_due.is_none() && app.autosave_due.is_none());
    let output = frame(&mut app, &ctx, 0.0, Vec::new());
    assert!(
        text(&output)
            .iter()
            .any(|s| s.contains("File dialog failed"))
    );
    let chosen = temp.path().to_path_buf();
    assert_eq!(
        app.dialog_selection(&ctx, "open the folder picker", Ok(Some(chosen.clone()))),
        Some(chosen)
    );
    assert!(app.dialog_error.is_none());
    assert_eq!(
        serde_json::to_value(&app.state).unwrap(),
        before_state,
        "result decoding must not change documents or Save As identity"
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn chooser_location_is_shell_safe_without_changing_canonical_model_paths() {
    let (temp, mut app) = test_app("original");
    let path = fs::canonicalize(temp.path()).unwrap();
    app.state.working_folder = Some(path.clone());
    let builder = app.file_dialog();
    #[cfg(target_os = "windows")]
    assert!(
        builder.location.is_none(),
        "Windows Shell receives no canonical/verbatim hint"
    );
    #[cfg(not(target_os = "windows"))]
    assert_eq!(builder.location, Some(path.clone()));
    assert_eq!(app.state.working_folder.as_ref(), Some(&path));
    app.state.working_folder = Some(temp.path().join("missing-folder"));
    assert!(
        app.file_dialog().location.is_none(),
        "a missing directory must not prevent showing the picker"
    );
}
