use super::preview_sync_tests::test_app;
use super::*;

fn render(
    app: &mut VelocimdApp,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            time: Some(1.0),
            events,
            ..Default::default()
        },
        |root| {
            app.ui_probe.begin();
            app.handle_shortcuts(ctx);
            egui::Frame::central_panel(root.style())
                .inner_margin(egui::Margin::same(APP_PADDING))
                .show(root, |ui| {
                    app.top_bar(ui, ctx);
                    app.workspace(ui, ctx);
                });
            app.finish_ui_probe(ctx);
        },
    )
}

fn setup() -> (tempfile::TempDir, VelocimdApp, egui::Context) {
    let (temp, mut app) = test_app("# Probe fixture\n\nExact content.");
    app.state.add_folder_tab(temp.path().to_path_buf());
    let ctx = egui::Context::default();
    app.state.theme.apply_to(&ctx);
    egui_extras::install_image_loaders(&ctx);
    for _ in 0..4 {
        render(&mut app, &ctx, vec![]);
    }
    (temp, app, ctx)
}

#[test]
fn probe_is_opt_in_read_only_and_observes_changed_frames_only() {
    let (_temp, mut app, ctx) = setup();
    assert_eq!(app.ui_snapshot(), "{}");
    let state_before = serde_json::to_value(&app.state).unwrap();
    let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
    let sink = calls.clone();
    app.set_ui_probe_observer(Box::new(move |s| sink.borrow_mut().push(s.into())));
    let disabled = render(&mut app, &ctx, vec![]);
    assert!(calls.borrow().is_empty());
    app.set_ui_probe(true);
    let enabled = render(&mut app, &ctx, vec![]);
    assert_eq!(
        format!("{:?}", disabled.shapes),
        format!("{:?}", enabled.shapes),
        "probe must not change any rendered shape"
    );
    assert_eq!(serde_json::to_value(&app.state).unwrap(), state_before);
    assert_eq!(calls.borrow().len(), 1);
    render(&mut app, &ctx, vec![]);
    assert_eq!(
        calls.borrow().len(),
        1,
        "unchanged snapshots must not be emitted"
    );
    app.set_ui_probe(false);
    render(&mut app, &ctx, vec![]);
    assert_eq!(calls.borrow().len(), 1);
    assert_eq!(app.ui_snapshot(), "{}");
}

#[test]
fn real_response_geometry_order_and_pointer_mode_routing() {
    let (_temp, mut app, ctx) = setup();
    app.set_ui_probe(true);
    render(&mut app, &ctx, vec![]);
    let snapshot: serde_json::Value = serde_json::from_str(&app.ui_snapshot()).unwrap();
    assert_eq!(snapshot["schema"], 1);
    assert_eq!(snapshot["viewport"], serde_json::json!([1000.0, 700.0]));
    assert_eq!(
        snapshot["active_content"],
        "# Probe fixture\n\nExact content."
    );
    let controls = snapshot["controls"].as_array().unwrap();
    let toolbar: Vec<_> = controls
        .iter()
        .filter(|c| c["id"].as_str().unwrap().starts_with("toolbar:"))
        .collect();
    assert_eq!(
        toolbar
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "toolbar:new",
            "toolbar:open-folder",
            "toolbar:open-file",
            "toolbar:save",
            "toolbar:save-as",
            "toolbar:close",
            "toolbar:edit",
            "toolbar:split",
            "toolbar:preview",
            "toolbar:cycle",
            "toolbar:theme"
        ]
    );
    for (index, c) in toolbar.iter().enumerate() {
        let r = c["rect"].as_array().unwrap();
        assert_eq!(r[2], 30.0);
        assert_eq!(r[3], 28.0);
        assert_eq!(c["enabled"], true);
        if index > 0 {
            assert!(r[0].as_f64().unwrap() > toolbar[index - 1]["rect"][0].as_f64().unwrap());
        }
    }
    assert_eq!(snapshot["regions"]["top_bar"][3], 42.0);
    assert_eq!(snapshot["regions"]["command_bar"][3], 38.0);
    for id in ["filename", "tab:0"] {
        assert!(controls.iter().any(|c| c["id"] == id));
    }
    for id in ["editor", "preview", "divider"] {
        assert!(snapshot["regions"][id][2].as_f64().unwrap() > 0.0);
    }
    let edit = toolbar.iter().find(|c| c["id"] == "toolbar:edit").unwrap();
    let r = &edit["rect"];
    let pos = egui::pos2(
        (r[0].as_f64().unwrap() + r[2].as_f64().unwrap() / 2.0) as f32,
        (r[1].as_f64().unwrap() + r[3].as_f64().unwrap() / 2.0) as f32,
    );
    for pressed in [true, false] {
        render(
            &mut app,
            &ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    render(&mut app, &ctx, vec![]);
    assert_eq!(app.state.mode, EditorMode::Edit);
    let after: serde_json::Value = serde_json::from_str(&app.ui_snapshot()).unwrap();
    assert_eq!(after["mode"], "Edit");
    assert!(after["regions"].get("preview").is_none());
    assert!(after["regions"].get("divider").is_none());
    assert!(
        after["controls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == "toolbar:edit" && c["selected"] == true)
    );
}

#[test]
fn chrome_has_one_target_independent_painter_and_no_html_toolbar() {
    let browser = include_str!("browser.rs");
    assert!(
        !browser.contains("fn command_bar("),
        "browser must use the shared icon bar"
    );
    assert!(
        !browser.contains("fn folder_tabs("),
        "browser must use the shared tab painter"
    );
    let host = include_str!("../../index.html");
    assert!(
        !host.contains("<nav"),
        "ordinary host must not duplicate editor controls"
    );
}
