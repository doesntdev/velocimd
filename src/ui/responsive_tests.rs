//! Real shared egui frames, not a second model of responsive layout.
use super::{preview_sync_tests::test_app, *};

struct Harness {
    app: VelocimdApp,
    _temp: tempfile::TempDir,
    ctx: egui::Context,
    frame: usize,
    size: egui::Vec2,
}

impl Harness {
    fn new(size: egui::Vec2, content: &str) -> Self {
        let (temp, mut app) = test_app(content);
        app.state.add_folder_tab(temp.path().to_path_buf());
        app.set_ui_probe(true);
        let ctx = egui::Context::default();
        app.state.theme.apply_to(&ctx);
        egui_extras::install_image_loaders(&ctx);
        Self {
            _temp: temp,
            app,
            ctx,
            frame: 0,
            size,
        }
    }

    fn render(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
        self.frame += 1;
        self.ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                time: Some(self.frame as f64 / 60.0),
                events,
                ..Default::default()
            },
            |root| {
                self.app.ui_probe.begin();
                egui::Frame::central_panel(root.style())
                    .inner_margin(egui::Margin::same(APP_PADDING))
                    .show(root, |ui| {
                        self.app.top_bar(ui, &self.ctx);
                        self.app.workspace(ui, &self.ctx);
                    });
                self.app.finish_ui_probe(&self.ctx);
            },
        )
    }

    fn settle(&mut self) {
        for _ in 0..5 {
            self.render(vec![]);
        }
    }

    fn snapshot(&self) -> serde_json::Value {
        serde_json::from_str(&self.app.ui_snapshot()).unwrap()
    }

    fn region(&self, name: &str) -> egui::Rect {
        rect(&self.snapshot()["regions"][name])
    }

    fn click(&mut self, pos: egui::Pos2) {
        self.render(vec![egui::Event::PointerMoved(pos), button(pos, true)]);
        self.render(vec![button(pos, false)]);
    }
}

fn rect(value: &serde_json::Value) -> egui::Rect {
    let values: Vec<_> = value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap() as f32)
        .collect();
    egui::Rect::from_min_size(
        egui::pos2(values[0], values[1]),
        egui::vec2(values[2], values[3]),
    )
}

fn button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        pressed,
        button: egui::PointerButton::Primary,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn responsive_split_uses_app_bounds_and_resize_does_not_edit_or_persist() {
    let content = "# Irreplaceable\n\nUnicode: λ 🦀\n".repeat(10);
    let mut h = Harness::new(egui::vec2(1000.0, 700.0), &content);
    h.app.state.split_ratio = 0.63;
    let state_before = serde_json::to_value(&h.app.state).unwrap();
    for (size, stacked) in [
        (egui::vec2(1000.0, 700.0), false),
        (egui::vec2(320.0, 720.0), true),
        (egui::vec2(430.0, 260.0), true),
        (egui::vec2(800.0, 830.0), true),
        (egui::vec2(1000.0, 700.0), false),
    ] {
        h.size = size;
        h.settle();
        let editor = h.region("editor_pane");
        let preview = h.region("preview_pane");
        if stacked {
            assert!(
                editor.bottom() <= preview.top(),
                "must stack at {size:?}: {editor:?} {preview:?}"
            );
            assert!((editor.left() - preview.left()).abs() < 1.0);
        } else {
            assert!(editor.right() <= preview.left());
            assert!((editor.top() - preview.top()).abs() < 1.0);
        }
        for pane in [editor, preview] {
            assert!(pane.width() > 0.0 && pane.height() > 0.0);
            assert!(pane.left() >= 0.0 && pane.right() <= size.x);
            assert!(pane.top() >= 0.0 && pane.bottom() <= size.y);
        }
        assert_eq!(serde_json::to_value(&h.app.state).unwrap(), state_before);
        assert!(h.app.state_save_due.is_none() && h.app.autosave_due.is_none());
    }
}

#[test]
fn responsive_chrome_keeps_every_command_and_filename_inside_narrow_viewports() {
    for width in [320.0, 360.0, 390.0, 430.0] {
        let mut h = Harness::new(egui::vec2(width, 720.0), "# Draft");
        h.app.status_message =
            Some("Local draft pending: ".to_owned() + &"long status ".repeat(20));
        for index in 0..6 {
            let path = h._temp.path().join(format!("long-folder-title-{index}"));
            std::fs::create_dir(&path).unwrap();
            h.app.state.add_folder_tab(path);
        }
        h.settle();
        let snapshot = h.snapshot();
        let controls = snapshot["controls"].as_array().unwrap();
        let toolbar: Vec<_> = controls
            .iter()
            .filter(|c| c["id"].as_str().unwrap().starts_with("toolbar:"))
            .collect();
        assert_eq!(toolbar.len(), Command::toolbar().len());
        for control in toolbar
            .into_iter()
            .chain(controls.iter().filter(|c| c["id"] == "filename"))
        {
            let bounds = rect(&control["rect"]);
            assert!(
                bounds.left() >= 0.0 && bounds.right() <= width,
                "offscreen {} at {width}: {bounds:?}",
                control["id"]
            );
            assert!(bounds.height() > 0.0);
        }
        assert!(h.region("editor").right() <= width);
        assert!(h.region("preview").right() <= width);
    }
}

#[test]
fn responsive_tab_strip_is_bounded_and_scrolls_to_the_last_control() {
    let mut h = Harness::new(egui::vec2(320.0, 720.0), "# Tabs");
    for index in 0..6 {
        let path = h._temp.path().join(format!("folder-{index}"));
        std::fs::create_dir(&path).unwrap();
        h.app.state.add_folder_tab(path);
    }
    h.settle();
    let viewport = h.region("tabs_viewport");
    assert!(viewport.left() >= 0.0 && viewport.right() <= 320.0);
    let before = h.snapshot();
    let add_rect = |s: &serde_json::Value| {
        rect(
            &s["controls"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == "tab-add")
                .unwrap()["rect"],
        )
    };
    assert!(add_rect(&before).left() > viewport.right());
    h.render(vec![
        egui::Event::PointerMoved(viewport.center()),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(-2000.0, 0.0),
            phase: egui::TouchPhase::Move,
            modifiers: egui::Modifiers::NONE,
        },
    ]);
    for _ in 0..30 {
        h.render(vec![]);
    }
    assert!(
        viewport.contains_rect(add_rect(&h.snapshot())),
        "horizontal navigation must reach the add control"
    );
}

#[test]
fn responsive_stacked_pointer_drag_uses_y_and_double_click_resets() {
    let mut h = Harness::new(egui::vec2(390.0, 844.0), "# Unchanged\n\nSource.");
    h.settle();
    let start = h.region("divider").center();
    let revision = h.app.state.active_document().unwrap().revision;
    h.render(vec![egui::Event::PointerMoved(start), button(start, true)]);
    h.render(vec![egui::Event::PointerMoved(
        start + egui::vec2(80.0, 0.0),
    )]);
    assert_eq!(
        h.app.state.split_ratio, 0.5,
        "cross-axis motion must not resize"
    );
    let end = start + egui::vec2(80.0, 80.0);
    h.render(vec![egui::Event::PointerMoved(end)]);
    h.render(vec![button(end, false)]);
    let editor = h.region("editor_pane");
    let preview = h.region("preview_pane");
    let divider = h.region("divider");
    let expected = (end.y - editor.top() - divider.height() * 0.5)
        / (preview.bottom() - editor.top() - divider.height());
    assert!((h.app.state.split_ratio - expected).abs() < 0.00001);
    assert!(h.app.state.split_ratio > 0.6);
    assert!(h.app.state_save_due.is_some());
    h.settle();
    let reset = h.region("divider").center();
    for pressed in [true, false, true, false] {
        h.render(vec![
            egui::Event::PointerMoved(reset),
            button(reset, pressed),
        ]);
    }
    assert_eq!(h.app.state.split_ratio, 0.5);
    assert_eq!(h.app.state.active_document().unwrap().revision, revision);
    assert!(h.app.autosave_due.is_none());
}

#[test]
fn responsive_touch_drags_both_axes_with_a_bounded_hit_target() {
    for size in [egui::vec2(1000.0, 700.0), egui::vec2(390.0, 844.0)] {
        let mut h = Harness::new(size, "# Touch keeps source intact\n");
        h.settle();
        let vertical = size.x < size.y;
        let divider = h.region("divider");
        let hit = h.region("divider_hit");
        assert!(if vertical { hit.height() } else { hit.width() } >= 24.0);
        // Start near the target's edge, outside the desktop's painted 8pt line.
        let start = if vertical {
            hit.center()
        } else {
            hit.center() + egui::vec2(9.0, 0.0)
        };
        let end = start
            + if vertical {
                egui::vec2(0.0, 65.0)
            } else {
                egui::vec2(65.0, 0.0)
            };
        let touch = |pos, phase| egui::Event::Touch {
            device_id: egui::TouchDeviceId(7),
            id: egui::TouchId(1),
            phase,
            pos,
            force: None,
        };
        h.render(vec![
            touch(start, egui::TouchPhase::Start),
            egui::Event::PointerMoved(start),
            button(start, true),
        ]);
        h.render(vec![
            touch(end, egui::TouchPhase::Move),
            egui::Event::PointerMoved(end),
        ]);
        h.render(vec![
            touch(end, egui::TouchPhase::End),
            button(end, false),
            egui::Event::PointerGone,
        ]);
        let editor = h.region("editor_pane");
        let preview = h.region("preview_pane");
        let expected = if vertical {
            (end.y - editor.top() - divider.height() * 0.5)
                / (preview.bottom() - editor.top() - divider.height())
        } else {
            (end.x - editor.left() - divider.width() * 0.5)
                / (preview.right() - editor.left() - divider.width())
        };
        assert!(
            (h.app.state.split_ratio - expected).abs() < 0.00001,
            "touch must resize {size:?}: {} vs {expected}",
            h.app.state.split_ratio
        );
        assert_eq!(
            h.app.state.active_document().unwrap().content,
            "# Touch keeps source intact\n"
        );
        assert!(h.app.autosave_due.is_none());
    }
}

#[test]
fn responsive_resize_preserves_editor_selection_focus_and_pending_filename() {
    let original = "alpha e\u{301} 🦀\r\nsecond line\n";
    let mut h = Harness::new(egui::vec2(1000.0, 700.0), original);
    h.settle();
    h.click(h.region("editor").min + egui::vec2(80.0, 30.0));
    let key = |key, modifiers| egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    };
    h.render(vec![
        key(egui::Key::Home, egui::Modifiers::CTRL),
        key(egui::Key::ArrowRight, egui::Modifiers::SHIFT),
    ]);
    let id = h
        .ctx
        .memory(|memory| memory.focused())
        .expect("real editor focus");
    let selected = egui::TextEdit::load_state(&h.ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap();
    assert!(!selected.is_empty());
    for size in [
        egui::vec2(390.0, 844.0),
        egui::vec2(430.0, 260.0),
        egui::vec2(1000.0, 700.0),
    ] {
        h.size = size;
        h.settle();
        assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(id));
        assert_eq!(
            egui::TextEdit::load_state(&h.ctx, id)
                .unwrap()
                .cursor
                .char_range()
                .unwrap(),
            selected
        );
        assert_eq!(h.app.state.active_document().unwrap().content, original);
        assert!(h.app.state_save_due.is_none() && h.app.autosave_due.is_none());
    }
    // A genuine text event still replaces the preserved selection after resizing.
    h.render(vec![egui::Event::Text("猫".into())]);
    assert_eq!(
        h.app.state.active_document().unwrap().content,
        "猫lpha e\u{301} 🦀\r\nsecond line\n"
    );
    h.app.autosave_due = None;
    let snapshot = h.snapshot();
    let field = snapshot["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "filename")
        .unwrap();
    h.click(rect(&field["rect"]).center());
    h.render(vec![
        key(
            egui::Key::A,
            egui::Modifiers {
                ctrl: true,
                command: true,
                ..egui::Modifiers::NONE
            },
        ),
        egui::Event::Text("Still typing".into()),
    ]);
    let filename_id = h.ctx.memory(|memory| memory.focused()).unwrap();
    h.size = egui::vec2(320.0, 720.0);
    h.settle();
    assert_eq!(h.ctx.memory(|memory| memory.focused()), Some(filename_id));
    assert_eq!(h.app.file_name_editor_text, "Still typing");
    assert_eq!(h.app.state.active_document().unwrap().title, "Test.md");
}

#[test]
fn responsive_click_anchor_is_in_preview_local_coordinates_then_resize_recomputes() {
    let mut h = Harness::new(egui::vec2(390.0, 844.0), &"paragraph\n\n".repeat(80));
    h.settle();
    let click = h.region("editor").min + egui::vec2(80.0, 50.0);
    h.click(click);
    let PreviewSyncAnchor::ScreenY(anchor) = h.app.preview_sync_anchor else {
        panic!("click must select a source anchor")
    };
    let expected = click.y + h.region("preview_pane").top() - h.region("editor_pane").top();
    assert!((anchor - expected).abs() < 1.0);
    assert!(anchor >= h.region("preview_pane").top());
    h.size = egui::vec2(1000.0, 700.0);
    h.settle();
    assert!(h.app.preview_sync_anchor == PreviewSyncAnchor::Top);
}

#[test]
fn responsive_preview_wheel_forwards_once_after_each_orientation_change() {
    let content = (0..100)
        .map(|i| format!("## SECTION {i:04}\n\nParagraph {i}.\n\n"))
        .collect::<String>();
    let mut h = Harness::new(egui::vec2(1000.0, 700.0), &content);
    for size in [
        egui::vec2(1000.0, 700.0),
        egui::vec2(390.0, 844.0),
        egui::vec2(1000.0, 700.0),
    ] {
        h.size = size;
        h.settle();
        let before = h.app.synced_scroll_y;
        let pointer = h.region("preview").center();
        h.render(vec![
            egui::Event::PointerMoved(pointer),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -160.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        for _ in 0..30 {
            h.render(vec![]);
        }
        let change = h.app.synced_scroll_y - before;
        assert!(
            (change - 160.0).abs() < 2.0,
            "wheel must apply once at {size:?}: {change}"
        );
        assert!(h.app.preview_sync_line > 0);
        assert!(h.app.preview_sync_anchor == PreviewSyncAnchor::Top);
        assert_eq!(h.app.state.active_document().unwrap().content, content);
    }
}
