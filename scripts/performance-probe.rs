//! Included only in generated, immutable source snapshots by performance-probe.py.
//! CPU Context::run_ui work only: no tessellation, GPU upload or presentation.
use super::*;
use crate::document::Document;
use std::hint::black_box;
thread_local! { static FRAME: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; static COMPONENTS: std::cell::RefCell<Vec<serde_json::Value>> = const { std::cell::RefCell::new(Vec::new()) }; }
pub(super) fn record_component(name: &str, ms: f64) {
    COMPONENTS.with_borrow_mut(|rows| {
        rows.push(serde_json::json!({"component":name,"ms":ms,"frame":FRAME.get()}))
    });
}
fn timed<T>(name: &str, f: impl FnOnce() -> T) -> T {
    let t = Instant::now();
    let result = f();
    record_component(name, t.elapsed().as_secs_f64() * 1000.);
    result
}

fn samples(n: usize, mut f: impl FnMut()) -> Vec<f64> {
    (0..n)
        .map(|_| {
            let t = Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect()
}
fn frame(
    app: &mut VelocimdApp,
    ctx: &egui::Context,
    number: &mut usize,
    mode: EditorMode,
) -> usize {
    *number += 1;
    FRAME.set(*number);
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1100.0, 800.0),
        )),
        time: Some(*number as f64 / 60.0),
        ..Default::default()
    };
    let out = ctx.run_ui(input, |root| {
        egui::CentralPanel::default().show_inside(root, |ui| match mode {
            EditorMode::Edit => {
                timed("editor", || app.editor_pane(ui, ctx, false));
            }
            EditorMode::Preview => timed("preview", || app.preview_pane(ui, false)),
            EditorMode::Split => ui.columns(2, |cols| {
                timed("editor", || app.editor_pane(&mut cols[0], ctx, false));
                timed("preview", || app.preview_pane(&mut cols[1], false));
            }),
        });
    });
    black_box(out.shapes.len())
}
fn ui_case(root: &Path, fixtures: &Path, name: &str, mode: EditorMode) -> serde_json::Value {
    COMPONENTS.with_borrow_mut(Vec::clear);
    let content = fs::read_to_string(fixtures.join(format!("{name}.md"))).unwrap();
    let mut state = AppState::fresh();
    state.documents = vec![Document::scratch("Synthetic.md", &content)];
    let session = root.join(format!("{name}-session.json"));
    state.save_to(&session).unwrap();
    let state = AppState::load_from(&session).unwrap();
    let mut app = VelocimdApp::from_state(state);
    let ctx = egui::Context::default();
    app.state.theme.apply_to(&ctx);
    egui_extras::install_image_loaders(&ctx);
    let mut number = 0;
    let t = Instant::now();
    let shapes = frame(&mut app, &ctx, &mut number, mode);
    let first = t.elapsed().as_secs_f64() * 1000.0;
    for _ in 0..5 {
        frame(&mut app, &ctx, &mut number, mode);
    }
    let warm = samples(24, || {
        frame(&mut app, &ctx, &mut number, mode);
    });
    let mut revision = Vec::new();
    let mut post_revision = Vec::new();
    for _ in 0..8 {
        let d = app.state.active_document_mut().unwrap();
        d.content.push('x');
        d.mark_changed();
        let t = Instant::now();
        frame(&mut app, &ctx, &mut number, mode);
        revision.push(t.elapsed().as_secs_f64() * 1000.);
        post_revision.push(samples(4, || {
            frame(&mut app, &ctx, &mut number, mode);
        }));
    }
    let components = COMPONENTS.with_borrow_mut(std::mem::take);
    app.state.documents.clear();
    serde_json::json!({"case":name,"mode":format!("{mode:?}"),"bytes":content.len(),"lines":content.split('\n').count(),"first_frame_ms":first,"first_shapes":shapes,"warm_ms":warm,"revision_ms":revision,"post_revision_ms":post_revision,"components":components})
}
fn rss_kib() -> Option<usize> {
    fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find(|l| l.starts_with("VmRSS:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
pub fn run() {
    let root = PathBuf::from(std::env::var("PERF_ROOT").unwrap());
    let fixtures = PathBuf::from(std::env::var("PERF_FIXTURES").unwrap());
    fs::create_dir_all(&root).unwrap();
    let preview_only = std::env::var_os("PERF_PREVIEW_ONLY").is_some();
    let mut ui = Vec::new();
    if !preview_only {
        ui.push(ui_case(
            &root,
            &fixtures,
            "short-lines-100KiB",
            EditorMode::Edit,
        ));
    }
    ui.push(ui_case(&root, &fixtures, "plain-1MiB", EditorMode::Split));
    ui.push(ui_case(
        &root,
        &fixtures,
        "rich-100KiB",
        EditorMode::Preview,
    ));
    if preview_only {
        println!(
            "{}",
            serde_json::json!({"arm":std::env::var("PERF_ARM").unwrap(),"measurement":"optimized CPU-only Context::run_ui; same fixtures/compiler; component costs and four following ordinary frames per revision; no native typing latency claim","ui":ui})
        );
        return;
    }
    let folder = fixtures.join("folder-10000");
    let scan = samples(8, || {
        assert_eq!(black_box(folder_entries(&folder)).len(), 10_000)
    });
    let mut cache = FolderEntryCache::default();
    cache.entries(&folder);
    let hit = samples(24, || {
        assert_eq!(black_box(cache.entries(&folder)).len(), 10_000)
    });
    let ctx = egui::Context::default();
    ThemeConfig::default_dark().apply_to(&ctx);
    let mut selected = None;
    let mut selected_folder = None;
    let mut menu_frame = || {
        let out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(500.0, 700.0),
                )),
                ..Default::default()
            },
            |root| {
                egui::CentralPanel::default().show_inside(root, |ui| {
                    folder_tab_menu(ui, &mut cache, &folder, &mut selected, &mut selected_folder)
                });
            },
        );
        black_box(out);
    };
    for _ in 0..5 {
        menu_frame();
    }
    let menu = samples(24, &mut menu_frame);
    let session = fixtures.join("session-55.json");
    let before = rss_kib();
    let t = Instant::now();
    let state = AppState::load_from(&session).unwrap();
    let first_restore = t.elapsed().as_secs_f64() * 1000.0;
    let after = rss_kib();
    let hydrated = state
        .documents
        .iter()
        .filter(|d| !d.content.is_empty())
        .count();
    let bytes: usize = state.documents.iter().map(|d| d.content.len()).sum();
    assert_eq!(state.documents.len(), 55);
    assert_eq!(
        state.documents[state.active_document].content.len(),
        1024 * 1024
    );
    drop(state);
    let restore = samples(12, || {
        black_box(AppState::load_from(&session).unwrap());
    });
    let result = serde_json::json!({"arm":std::env::var("PERF_ARM").unwrap(),"measurement":"optimized CPU-only Context::run_ui; fixed viewport; warm filesystem; no GPU tessellation or presentation","ui":ui,"folder":{"entries":10_000,"scan_sort_ms":scan,"cache_hit_ms":hit,"menu_warm_ms":menu},"restore":{"notes":55,"bytes_per_note":1024*1024,"first_ms":first_restore,"warm_ms":restore,"hydrated_nonempty_buffers":hydrated,"resident_content_bytes":bytes,"rss_before_kib":before,"rss_after_kib":after}});
    println!("{}", serde_json::to_string(&result).unwrap());
}
