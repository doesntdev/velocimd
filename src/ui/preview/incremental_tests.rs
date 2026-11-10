//! Differential and real-widget regressions for the issue 9 mutation path.
use super::*;
use crate::ui::{
    VelocimdApp,
    edit::{LineMetadata, RecordingBuffer},
    preview_sync_tests::test_app,
};
use eframe::egui::TextBuffer;

fn assert_segments(renderer: &PreviewRenderer, source: &str) {
    let actual = renderer
        .blocks
        .iter()
        .map(|b| b.segment.clone())
        .collect::<Vec<_>>();
    assert_eq!(actual, split_preview_segments(source), "source={source:?}");
}

#[test]
fn differential_all_edit_positions_across_contextual_markdown_and_whitespace() {
    let fixtures = [
        "Alpha words\n\nBeta words\n\nGamma words",
        "日本語 e\u{301} 👩\u{200d}🚀\r\nsecond\r\n\r\nlast\r\n",
        "\n\nAlpha words\n\n\nBeta words\n\n",
        "# Heading\n\nAlpha words\n\n- outer\n  - inner\n\nBeta words\n",
        "Alpha words\n---\n\nBeta words",
        "Alpha words\n\n```\nliteral\n\n```\n\nBeta words",
        "[x]: /url\n\nAlpha words\n\n[x]\n\nBeta words",
        "[^x]: note\n\nAlpha words\n\ntext[^x]\n\nBeta words",
        "Alpha words\n\n> quoted\n>\n> - item\n\nBeta words",
        "   Alpha words\n\nBeta words  \n\n    indented code\n",
        "Alpha words\n\n[missing]\n\nBeta words",
        "<div>\nAlpha words\n\nBeta words\n</div>\n\nLast words",
    ];
    for original in fixtures {
        let mut renderer = PreviewRenderer::default();
        for position in 0..=original.chars().count() {
            for (inserted, removed) in [
                ("猫", 0),
                ("e\u{301}", 1),
                ("\r\n", 0),
                ("\n\n", 0),
                ("", 1),
                ("[x]: /new\n", 0),
                (" ", 1),
                ("---", 2),
            ] {
                let mut source = original.to_owned();
                let mut index = LineMetadata::from_content(original);
                renderer.update(original, false);
                renderer.identity = Some((1, 0));
                let mut buffer = RecordingBuffer::new(&mut source, &mut index);
                buffer.delete_char_range(position..position + removed);
                buffer.insert_text(inserted, position);
                let edits = buffer.finish();
                if !renderer.apply_edits(1, 0, 1, &edits) {
                    renderer.update(&source, false);
                }
                assert_segments(&renderer, &source);
                assert_eq!(
                    index.line_starts,
                    LineMetadata::from_content(&source).line_starts
                );
            }
        }
    }
}

fn frame(app: &mut VelocimdApp, ctx: &egui::Context, time: f64, events: Vec<egui::Event>) {
    let _ = ctx.run_ui(
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
            egui::CentralPanel::default().show_inside(root, |ui| app.workspace(ui, ctx));
        },
    );
}

fn select(ctx: &egui::Context, range: Range<usize>) {
    let id = ctx
        .memory(|m| m.focused())
        .expect("actual editor has keyboard focus");
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(range.start),
            egui::text::CCursor::new(range.end),
        )));
    state.store(ctx, id);
}

fn key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn real_text_edit_orders_operations_publishes_one_revision_and_handles_undo_redo() {
    let original = "Alpha words\r\nsecond line\r\n\r\n".repeat(20);
    let (_temp, mut app) = test_app(&original);
    let ctx = egui::Context::default();
    ctx.set_os(egui::os::OperatingSystem::Nix);
    egui_extras::install_image_loaders(&ctx);
    frame(&mut app, &ctx, 0.0, vec![]);
    let pos = egui::pos2(180.0, 180.0);
    for (time, pressed) in [(0.1, true), (0.2, false)] {
        frame(
            &mut app,
            &ctx,
            time,
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
    select(&ctx, 2..4);
    let before = app.state.active_document().unwrap().revision;
    let full_scans = app.line_metadata.full_scans;
    let parses = app.preview_renderer.parses;
    let galley = app.preview_renderer.blocks[1].galley.clone().unwrap();
    let height = app.preview_renderer.blocks[1].height;
    let block_id = app.preview_renderer.blocks[1].id;
    // Selected deletion plus three insertions in one TextEdit call. Its layouter
    // can run before the controller advances revision; none may be lost/reordered.
    frame(
        &mut app,
        &ctx,
        0.4,
        vec![
            egui::Event::Text("猫".into()),
            egui::Event::Text("x".into()),
            egui::Event::Paste("e\u{301}".into()),
        ],
    );
    let mut expected = original.clone();
    expected.replace_range(2..4, "猫xe\u{301}");
    let document = app.state.active_document().unwrap();
    assert_eq!(document.content, expected);
    assert_eq!(document.revision, before + 1);
    assert_eq!(app.line_metadata.full_scans, full_scans);
    assert_eq!(app.preview_renderer.parses, parses);
    assert!(app.preview_renderer.region_parses >= 4);
    assert_segments(&app.preview_renderer, &expected);
    assert_eq!(app.preview_renderer.blocks[1].id, block_id);
    assert_eq!(app.preview_renderer.blocks[1].height, height);
    assert!(Arc::ptr_eq(
        app.preview_renderer.blocks[1].galley.as_ref().unwrap(),
        &galley
    ));
    let region_parses = app.preview_renderer.region_parses;
    for time in [0.5, 1.5, 2.5] {
        frame(&mut app, &ctx, time, vec![]);
    }
    assert_eq!(app.state.active_document().unwrap().revision, before + 1);
    assert_eq!(app.line_metadata.full_scans, full_scans);
    assert_eq!(app.preview_renderer.parses, parses);
    assert_eq!(app.preview_renderer.region_parses, region_parses);
    // egui's undo and redo are whole-buffer replace_with operations. Preserve
    // exact bytes and rebuild the preview rather than guessing a local delta.
    frame(
        &mut app,
        &ctx,
        3.0,
        vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
    );
    assert_eq!(app.state.active_document().unwrap().content, original);
    assert_segments(&app.preview_renderer, &original);
    assert_eq!(app.preview_renderer.parses, parses + 1);
    frame(
        &mut app,
        &ctx,
        3.1,
        vec![key(
            egui::Key::Z,
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        )],
    );
    assert_eq!(app.state.active_document().unwrap().content, expected);
    assert_segments(&app.preview_renderer, &expected);
    assert_eq!(app.preview_renderer.parses, parses + 2);
    assert_eq!(
        app.line_metadata.metadata.line_starts,
        LineMetadata::from_content(&expected).line_starts
    );
    // A skipped view plus non-TextEdit replacements is not an edit journal.
    // The next display must use the authoritative body and a full rebuild.
    app.state.mode = crate::modes::EditorMode::Edit;
    app.state
        .active_document_mut()
        .unwrap()
        .set_content("[id]: /new\n\n[go][id]".into());
    frame(&mut app, &ctx, 3.2, vec![]);
    app.state
        .active_document_mut()
        .unwrap()
        .set_content("# External replacement\n\nLast paragraph".into());
    app.state.mode = crate::modes::EditorMode::Split;
    frame(&mut app, &ctx, 3.3, vec![]);
    let replacement = &app.state.active_document().unwrap().content;
    assert_segments(&app.preview_renderer, replacement);
    assert_eq!(app.preview_renderer.parses, parses + 3);
    let first_id = app.state.active_document().unwrap().id;
    app.state.documents.push(crate::document::Document::scratch(
        "other",
        "Other tab\r\n\r\nBody",
    ));
    app.state.active_document = app.state.documents.len() - 1;
    frame(&mut app, &ctx, 3.4, vec![]);
    assert_segments(&app.preview_renderer, "Other tab\r\n\r\nBody");
    app.state.active_document = app
        .state
        .documents
        .iter()
        .position(|d| d.id == first_id)
        .unwrap();
    frame(&mut app, &ctx, 3.5, vec![]);
    assert_segments(
        &app.preview_renderer,
        &app.state.active_document().unwrap().content,
    );
    assert_eq!(app.preview_renderer.parses, parses + 5);
}
