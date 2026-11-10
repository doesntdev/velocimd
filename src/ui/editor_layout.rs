use super::edit::{MutationEpoch, RecordingBuffer};
use eframe::egui;
use std::{rc::Rc, sync::Arc};

#[derive(Clone, PartialEq)]
struct LayoutKey {
    document_id: u64,
    revision: u64,
    width: f32,
    font: egui::FontId,
    color: egui::Color32,
    scale: f32,
    definitions: egui::FontDefinitions,
}

/// One bounded document layout, shared by height measurement and TextEdit.
/// Unknown buffers and ordinary notes retain content checks. Eligible audited
/// buffers use immediate mutation epochs before the controller publishes a revision.
#[derive(Default)]
pub(super) struct EditorLayoutCache {
    key: Option<LayoutKey>,
    epoch: Rc<MutationEpoch>,
    tracked_origin: Option<(u64, u64)>,
    tracked_epoch: Option<u64>,
    token: Option<egui::epaint::text::CachedLayoutToken>,
    galley: Option<Arc<egui::Galley>>,
    touched_pass: Option<u64>,
    numbered_galley: Option<Arc<egui::Galley>>,
    numbered_rows: Vec<(usize, usize)>,
    #[cfg(test)]
    pub layouts: usize,
    #[cfg(test)]
    qualification_work: QualificationWork,
}

/// Qualification counts are call-site work, not hardware byte-read counters.
#[cfg(test)]
#[derive(Clone, Copy, Default, serde::Serialize)]
struct QualificationWork {
    calls: usize,
    comparison_calls: usize,
    equal_length_comparison_bytes: usize,
    fonts_jobs: usize,
    outer_job_copy_bytes: usize,
    outer_job_hash_bytes: usize,
    fonts_ns: u128,
    numbered_rows_scanned: usize,
}

fn large_layout_eligible(bytes: usize, lines: usize) -> bool {
    (64 * 1024..=4 * 1024 * 1024).contains(&bytes) && lines <= 32_768
}

impl EditorLayoutCache {
    pub fn layout(
        &mut self,
        ui: &egui::Ui,
        identity: (u64, u64),
        text: &str,
        font: &egui::FontId,
        width: f32,
    ) -> Arc<egui::Galley> {
        self.token = None;
        self.tracked_epoch = None;
        let color = ui
            .visuals()
            .override_text_color
            .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
        let key = LayoutKey {
            document_id: identity.0,
            revision: identity.1,
            width,
            font: font.clone(),
            color,
            scale: ui.ctx().pixels_per_point(),
            definitions: ui.fonts(|fonts| fonts.definitions().clone()),
        };
        #[cfg(test)]
        {
            self.qualification_work.calls += 1;
        }
        if self.key.as_ref() == Some(&key)
            && let Some(galley) = &self.galley
            && {
                #[cfg(test)]
                {
                    self.qualification_work.comparison_calls += 1;
                    if galley.job.text.len() == text.len() {
                        self.qualification_work.equal_length_comparison_bytes += text.len();
                    }
                }
                galley.job.text == text
            }
        {
            // Retaining an Arc does not mark egui's paragraph-cache children
            // used. Touch its job once per pass so a later small edit can reuse
            // unchanged paragraphs instead of cold-shaping the whole document.
            let pass = ui.ctx().cumulative_pass_nr();
            if self.touched_pass == Some(pass) {
                return Arc::clone(galley);
            }
            #[cfg(test)]
            let started = std::time::Instant::now();
            let live = ui.fonts_mut(|fonts| fonts.layout_job((*galley.job).clone()));
            #[cfg(test)]
            {
                self.qualification_work.fonts_jobs += 1;
                self.qualification_work.outer_job_copy_bytes += text.len();
                self.qualification_work.outer_job_hash_bytes += text.len();
                self.qualification_work.fonts_ns += started.elapsed().as_nanos();
            }
            self.touched_pass = Some(pass);
            self.galley = Some(Arc::clone(&live));
            return live;
        }
        #[cfg(test)]
        let started = std::time::Instant::now();
        let galley = ui.fonts_mut(|fonts| {
            fonts.layout_job(egui::text::LayoutJob::simple(
                text.to_owned(),
                font.clone(),
                color,
                width,
            ))
        });
        self.key = Some(key);
        self.touched_pass = Some(ui.ctx().cumulative_pass_nr());
        self.galley = Some(Arc::clone(&galley));
        #[cfg(test)]
        {
            self.layouts += 1;
            self.qualification_work.fonts_jobs += 1;
            self.qualification_work.outer_job_copy_bytes += text.len();
            self.qualification_work.outer_job_hash_bytes += text.len();
            self.qualification_work.fonts_ns += started.elapsed().as_nanos();
        }
        galley
    }

    /// Admit only the audited editor_content/RecordingBuffer seam. Never reset
    /// the epoch at frame boundaries or when adopting controller revisions.
    pub fn begin_tracked(&mut self, identity: (u64, u64), lines: usize) -> Rc<MutationEpoch> {
        if self.tracked_origin != Some(identity) {
            self.epoch.advance();
            self.tracked_epoch = None;
            self.token = None;
            self.tracked_origin = Some(identity);
        }
        self.epoch.set_lines(lines);
        Rc::clone(&self.epoch)
    }

    pub fn layout_recording(
        &mut self,
        ui: &egui::Ui,
        identity: (u64, u64),
        text: &dyn egui::TextBuffer,
        epoch: &MutationEpoch,
        font: &egui::FontId,
        width: f32,
    ) -> Arc<egui::Galley> {
        if !RecordingBuffer::is_recording(text) || !epoch.matches_recording_source(text.as_str()) {
            return self.layout(ui, identity, text.as_str(), font, width);
        }
        self.layout_known(ui, identity, text.as_str(), epoch, font, width)
    }

    /// Complete stock geometry; only redundant source copies/hashes are skipped.
    pub fn layout_known(
        &mut self,
        ui: &egui::Ui,
        identity: (u64, u64),
        text: &str,
        epoch: &MutationEpoch,
        font: &egui::FontId,
        width: f32,
    ) -> Arc<egui::Galley> {
        let eligible = large_layout_eligible(text.len(), epoch.lines())
            && identity.1 != u64::MAX
            && std::ptr::eq(epoch, self.epoch.as_ref())
            && self.tracked_origin == Some(identity);
        let Some(current_epoch) = epoch.value().filter(|_| eligible) else {
            return self.layout(ui, identity, text, font, width);
        };
        let color = ui
            .visuals()
            .override_text_color
            .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
        let key = LayoutKey {
            document_id: identity.0,
            revision: identity.1,
            width,
            font: font.clone(),
            color,
            scale: ui.ctx().pixels_per_point(),
            definitions: ui.fonts(|fonts| fonts.definitions().clone()),
        };
        #[cfg(test)]
        {
            self.qualification_work.calls += 1;
        }
        let pass = ui.ctx().cumulative_pass_nr();
        if self.key.as_ref() == Some(&key)
            && self.tracked_epoch == Some(current_epoch)
            && let (Some(token), Some(galley)) = (&self.token, &self.galley)
            && ui.fonts_mut(|fonts| {
                (self.touched_pass == Some(pass) && fonts.cached_layout_is_current(token, galley))
                    || fonts.touch_cached_layout(token, galley)
            })
        {
            self.touched_pass = Some(pass);
            return Arc::clone(galley);
        }
        #[cfg(test)]
        let started = std::time::Instant::now();
        let (galley, token) = ui.fonts_mut(|fonts| {
            fonts.layout_job_with_token(egui::text::LayoutJob::simple(
                text.to_owned(),
                font.clone(),
                color,
                width,
            ))
        });
        self.key = Some(key);
        self.tracked_epoch = Some(current_epoch);
        self.token = Some(token);
        self.galley = Some(Arc::clone(&galley));
        self.touched_pass = Some(ui.ctx().cumulative_pass_nr());
        #[cfg(test)]
        {
            self.layouts += 1;
            self.qualification_work.fonts_jobs += 1;
            self.qualification_work.outer_job_copy_bytes += text.len();
            self.qualification_work.outer_job_hash_bytes += text.len();
            self.qualification_work.fonts_ns += started.elapsed().as_nanos();
        }
        galley
    }

    pub fn finish_tracked(
        &mut self,
        before: (u64, u64),
        after: u64,
        final_epoch: Option<u64>,
        output: &Arc<egui::Galley>,
    ) {
        if after > before.1
            && after != u64::MAX
            && self.tracked_origin == Some(before)
            && final_epoch.is_some()
            && final_epoch == self.epoch.value()
            && self.tracked_epoch == final_epoch
            && self.galley.as_ref().is_some_and(|g| Arc::ptr_eq(g, output))
            && let Some(key) = &mut self.key
            && (key.document_id, key.revision) == before
        {
            key.revision = after;
            self.tracked_origin = Some((before.0, after));
        } else {
            self.tracked_origin = None;
            self.tracked_epoch = None;
            self.token = None;
        }
    }

    pub fn numbered_rows(&mut self, galley: &Arc<egui::Galley>) -> &[(usize, usize)] {
        if self
            .numbered_galley
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, galley))
        {
            self.numbered_rows.clear();
            #[cfg(test)]
            {
                self.qualification_work.numbered_rows_scanned += galley.rows.len();
            }
            let mut line = 1;
            let mut number = true;
            for (index, row) in galley.rows.iter().enumerate() {
                if number {
                    self.numbered_rows.push((index, line));
                }
                number = row.ends_with_newline;
                if number {
                    line += 1;
                }
            }
            self.numbered_galley = Some(Arc::clone(galley));
        }
        &self.numbered_rows
    }
}

#[cfg(test)]
mod issue12_qualification {
    use super::*;
    use crate::ui::edit::{EditDelta, LineMetadata, RecordingBuffer};
    use egui::TextBuffer;
    use std::{cell::Cell, collections::HashSet, time::Instant};

    // Deliberately test-only alternatives. Neither is a production change.
    #[derive(Clone, Copy, Debug)]
    enum Mode {
        Baseline,
        EpochTouch,
        EpochOuterOnly,
        ProductionTracked,
    }

    struct ProbeCache {
        base: EditorLayoutCache,
        mode: Mode,
        epoch: u64,
        layout_ns: u128,
    }

    impl ProbeCache {
        fn new(mode: Mode) -> Self {
            Self {
                base: EditorLayoutCache::default(),
                mode,
                epoch: 0,
                layout_ns: 0,
            }
        }

        fn layout(
            &mut self,
            ui: &egui::Ui,
            revision: u64,
            epoch: u64,
            text: &str,
            width: f32,
        ) -> Arc<egui::Galley> {
            let started = Instant::now();
            let result = self.layout_inner(ui, revision, epoch, text, width);
            self.layout_ns += started.elapsed().as_nanos();
            result
        }

        fn layout_inner(
            &mut self,
            ui: &egui::Ui,
            revision: u64,
            epoch: u64,
            text: &str,
            width: f32,
        ) -> Arc<egui::Galley> {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            if matches!(self.mode, Mode::ProductionTracked) {
                let epoch = Rc::clone(&self.base.epoch);
                return self
                    .base
                    .layout_known(ui, (1, revision), text, &epoch, &font, width);
            }
            if matches!(self.mode, Mode::Baseline) {
                return self.base.layout(ui, (1, revision), text, &font, width);
            }
            let color = ui
                .visuals()
                .override_text_color
                .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());
            let key = LayoutKey {
                document_id: 1,
                revision,
                width,
                font: font.clone(),
                color,
                scale: ui.ctx().pixels_per_point(),
                definitions: ui.fonts(|fonts| fonts.definitions().clone()),
            };
            self.base.qualification_work.calls += 1;
            let pass = ui.ctx().cumulative_pass_nr();
            let hit = self.base.key.as_ref() == Some(&key) && self.epoch == epoch;
            if hit
                && let Some(galley) = &self.base.galley
                && (matches!(self.mode, Mode::EpochOuterOnly)
                    || self.base.touched_pass == Some(pass))
            {
                return Arc::clone(galley);
            }
            let started = Instant::now();
            let job = if hit {
                (*self.base.galley.as_ref().unwrap().job).clone()
            } else {
                self.base.layouts += 1;
                egui::text::LayoutJob::simple(text.to_owned(), font, color, width)
            };
            let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
            self.base.qualification_work.fonts_jobs += 1;
            self.base.qualification_work.outer_job_copy_bytes += text.len();
            self.base.qualification_work.outer_job_hash_bytes += text.len();
            self.base.qualification_work.fonts_ns += started.elapsed().as_nanos();
            self.base.key = Some(key);
            self.epoch = epoch;
            self.base.touched_pass = Some(pass);
            self.base.galley = Some(Arc::clone(&galley));
            galley
        }
    }

    // Delegate actual edits to the existing production RecordingBuffer. The
    // epoch changes immediately, including within one TextEdit::show call.
    struct EpochBuffer<'a> {
        inner: RecordingBuffer<'a>,
        epoch: &'a Cell<u64>,
    }

    impl TextBuffer for EpochBuffer<'_> {
        fn is_mutable(&self) -> bool {
            true
        }
        fn as_str(&self) -> &str {
            self.inner.as_str()
        }
        fn insert_text(&mut self, text: &str, at: usize) -> usize {
            let count = self.inner.insert_text(text, at);
            if count != 0 {
                self.epoch.set(self.epoch.get() + 1);
            }
            count
        }
        fn delete_char_range(&mut self, range: std::ops::Range<usize>) {
            if !range.is_empty() {
                self.epoch.set(self.epoch.get() + 1);
            }
            self.inner.delete_char_range(range);
        }
        fn byte_index_from_char_index(&self, index: usize) -> usize {
            self.inner.byte_index_from_char_index(index)
        }
        fn replace_with(&mut self, text: &str) {
            self.epoch.set(self.epoch.get() + 1);
            self.inner.replace_with(text);
        }
        fn type_id(&self) -> std::any::TypeId {
            std::any::TypeId::of::<EpochBuffer<'static>>()
        }
    }

    struct Harness {
        ctx: egui::Context,
        cache: ProbeCache,
        text: String,
        index: LineMetadata,
        epoch: Cell<u64>,
        revision: u64,
        time: f64,
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

    fn galley_payload(g: &egui::Galley) -> usize {
        // Reachable Vec/String capacities only, not allocator/RSS attribution.
        let mut seen = HashSet::new();
        let mut n = g.job.text.capacity()
            + g.job.sections.capacity() * std::mem::size_of::<egui::text::LayoutSection>()
            + g.rows.capacity() * std::mem::size_of::<egui::epaint::text::PlacedRow>();
        for placed in &g.rows {
            if seen.insert(Arc::as_ptr(&placed.row)) {
                n += std::mem::size_of_val(placed.row.as_ref())
                    + placed.glyphs.capacity() * std::mem::size_of::<egui::epaint::text::Glyph>()
                    + placed.visuals.mesh.vertices.capacity()
                        * std::mem::size_of::<egui::epaint::Vertex>()
                    + placed.visuals.mesh.indices.capacity() * std::mem::size_of::<u32>();
            }
        }
        n
    }

    fn rss_kib() -> usize {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmRSS:")
                    .and_then(|s| s.split_whitespace().next())
                    .and_then(|s| s.parse().ok())
            })
            .unwrap()
    }

    impl Harness {
        fn new(mode: Mode, text: String) -> Self {
            let ctx = egui::Context::default();
            ctx.set_os(egui::os::OperatingSystem::Nix);
            ctx.memory_mut(|m| m.request_focus(egui::Id::new("issue12_editor")));
            let mut state = egui::text_edit::TextEditState::default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(0),
                )));
            state.store(&ctx, egui::Id::new("issue12_editor"));
            Self {
                index: LineMetadata::from_content(&text),
                text,
                ctx,
                cache: ProbeCache::new(mode),
                epoch: Cell::new(0),
                revision: 0,
                time: 0.0,
            }
        }

        fn select(&self, start: usize, end: usize) {
            let id = egui::Id::new("issue12_editor");
            let mut state = egui::TextEdit::load_state(&self.ctx, id).unwrap();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(start),
                    egui::text::CCursor::new(end),
                )));
            state.store(&self.ctx, id);
        }

        fn frame(
            &mut self,
            shape: &str,
            phase: &str,
            width: f32,
            scroll: f32,
            dt: f64,
            events: Vec<egui::Event>,
        ) -> (serde_json::Value, Vec<EditDelta>) {
            self.time += dt;
            self.cache.base.qualification_work = QualificationWork::default();
            self.cache.layout_ns = 0;
            let tracked_epoch = self
                .cache
                .base
                .begin_tracked((1, self.revision), self.index.line_starts.len());
            let mut actual_galley = None;
            let previous = self.cache.base.galley.clone();
            let prior_rows: HashSet<_> = previous
                .as_ref()
                .map(|g| g.rows.iter().map(|r| Arc::as_ptr(&r.row)).collect())
                .unwrap_or_default();
            let before_bytes = self.text.len();
            let before_pass = self.ctx.cumulative_pass_nr();
            let event_count = events.len();
            let mut edits = Vec::new();
            let mut actual_offset = 0.0;
            let started = Instant::now();
            let output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width + 32.0, 400.0),
                    )),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show_inside(root, |ui| {
                        let scroll_output = egui::ScrollArea::vertical()
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                            )
                            .vertical_scroll_offset(scroll)
                            .show(ui, |ui| {
                                let width = ui.available_width();
                                let pre = self.cache.layout(
                                    ui,
                                    self.revision,
                                    self.epoch.get(),
                                    &self.text,
                                    width,
                                );
                                let height = pre.size().y;
                                let mut buffer = EpochBuffer {
                                    inner: if matches!(self.cache.mode, Mode::ProductionTracked) {
                                        RecordingBuffer::tracked(
                                            &mut self.text,
                                            &mut self.index,
                                            &tracked_epoch,
                                        )
                                    } else {
                                        RecordingBuffer::new(&mut self.text, &mut self.index)
                                    },
                                    epoch: &self.epoch,
                                };
                                let production = matches!(self.cache.mode, Mode::ProductionTracked);
                                let mut layouter =
                                    |ui: &egui::Ui, text: &dyn TextBuffer, wrap_width: f32| {
                                        if matches!(self.cache.mode, Mode::ProductionTracked) {
                                            let started = Instant::now();
                                            let font =
                                                egui::TextStyle::Monospace.resolve(ui.style());
                                            let galley = self.cache.base.layout_recording(
                                                ui,
                                                (1, self.revision),
                                                text,
                                                &tracked_epoch,
                                                &font,
                                                wrap_width,
                                            );
                                            self.cache.layout_ns += started.elapsed().as_nanos();
                                            galley
                                        } else {
                                            self.cache.layout(
                                                ui,
                                                self.revision,
                                                self.epoch.get(),
                                                text.as_str(),
                                                wrap_width,
                                            )
                                        }
                                    };
                                let input: &mut dyn TextBuffer = if production {
                                    &mut buffer.inner
                                } else {
                                    &mut buffer
                                };
                                let out = egui::TextEdit::multiline(input)
                                    .id(egui::Id::new("issue12_editor"))
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(pre.rows.len().max(1))
                                    .frame(egui::Frame::new().inner_margin(0))
                                    .lock_focus(true)
                                    .min_size(egui::vec2(width, height))
                                    .layouter(&mut layouter)
                                    .show(ui);
                                // Stock selection painting can COW-clone the outer Galley.
                                // Production must refuse revision adoption for that output.
                                if out
                                    .cursor_range
                                    .is_none_or(|r| r.primary.index == r.secondary.index)
                                {
                                    assert!(Arc::ptr_eq(
                                        &out.galley,
                                        self.cache.base.galley.as_ref().unwrap()
                                    ));
                                }
                                assert_eq!(out.galley.job.text, buffer.inner.as_str());
                                assert!(out.response.has_focus());
                                actual_galley = Some(Arc::clone(&out.galley));
                                self.cache.base.numbered_rows(&out.galley);
                                edits.extend(buffer.inner.finish());
                            });
                        actual_offset = scroll_output.state.offset.y;
                    });
                },
            );
            let frame_ns = started.elapsed().as_nanos();
            // Source equality, metadata oracle and memory statistics stay
            // outside the timer and outside the measured source-work counts.
            let g = self.cache.base.galley.as_ref().unwrap();
            assert_eq!(g.job.text, self.text);
            let reused_rows = g
                .rows
                .iter()
                .filter(|r| prior_rows.contains(&Arc::as_ptr(&r.row)))
                .count();
            let rows = g.rows.len();
            let payload = galley_payload(g);
            let cache_entries = self.ctx.fonts_mut(|f| f.num_galleys_in_cache());
            let changed = !edits.is_empty();
            if changed {
                let before = self.revision;
                self.revision += 1;
                if matches!(self.cache.mode, Mode::ProductionTracked) {
                    self.cache.base.finish_tracked(
                        (1, before),
                        self.revision,
                        tracked_epoch.value(),
                        actual_galley.as_ref().unwrap(),
                    );
                }
            }
            assert_eq!(
                self.index.line_starts,
                LineMetadata::from_content(&self.text).line_starts
            );
            if matches!(self.cache.mode, Mode::ProductionTracked)
                && matches!(phase, "warm" | "scroll" | "warm_after")
            {
                let work = self.cache.base.qualification_work;
                if shape == "normal" {
                    assert!(work.comparison_calls > 0);
                    assert!(work.outer_job_copy_bytes > 0);
                } else {
                    assert_eq!(work.comparison_calls, 0);
                    assert_eq!(work.outer_job_copy_bytes, 0);
                    assert_eq!(work.outer_job_hash_bytes, 0);
                }
            }
            let full_passes = self.ctx.cumulative_pass_nr() - before_pass;
            assert_eq!(
                full_passes, 1,
                "qualification intentionally has no controller discard replay"
            );
            drop(previous);
            let result = serde_json::json!({
                "shape": shape, "mode": format!("{:?}", self.cache.mode), "phase": phase,
                "bytes_before": before_bytes, "bytes_after": self.text.len(), "width": width,
                "frame_ns": frame_ns, "layout_ns": self.cache.layout_ns,
                "work": self.cache.base.qualification_work,
                "passes": full_passes, "events": event_count, "mutation_ops": edits.len(),
                "replacement_ops": edits.iter().filter(|e| e.replacement).count(),
                "rows": rows, "reused_row_arcs": reused_rows, "cache_entries": cache_entries,
                "current_galley_payload_capacity_bytes": payload, "process_rss_kib": rss_kib(),
                "scroll_requested": scroll, "scroll_actual": actual_offset,
                "shapes": output.shapes.len(),
                "stock_focused_copy_lower_bound_bytes_source_derived": before_bytes * 2 + self.text.len()
            });
            (result, edits)
        }
    }

    fn fixture(shape: &str) -> String {
        match shape {
            "long_wrapped" => {
                "Words e\u{301} 日本語 👩\u{200d}🚀 and ordinary text for wrapping. ".repeat(4_500)
            }
            "many_short" => (0..6_000)
                .map(|i| format!("unique line {i:05}: e\u{301} 日本語 ordinary text\n"))
                .collect(),
            "normal" => (0..35)
                .map(|i| format!("note {i}: ordinary e\u{301} 日本語 text\n"))
                .collect(),
            _ => unreachable!(),
        }
    }

    #[test]
    #[ignore = "bounded issue12 CPU qualification; invoke explicitly with --ignored --nocapture --test-threads=1"]
    fn actual_textedit_matrix() {
        for shape in ["long_wrapped", "many_short", "normal"] {
            for mode in [
                Mode::Baseline,
                Mode::EpochTouch,
                Mode::EpochOuterOnly,
                Mode::ProductionTracked,
            ] {
                if std::env::var_os("ISSUE12_TRACKED_ONLY").is_some()
                    && !matches!(mode, Mode::Baseline | Mode::ProductionTracked)
                {
                    continue;
                }
                let original = fixture(shape);
                if shape != "normal" {
                    assert!((200_000..=500_000).contains(&original.len()));
                }
                let mut h = Harness::new(mode, original.clone());
                let emit = |record: serde_json::Value| println!("ISSUE12 {}", record);
                emit(h.frame(shape, "cold", 640.0, 0.0, 0.1, vec![]).0);
                for _ in 0..6 {
                    emit(h.frame(shape, "warm", 640.0, 0.0, 0.1, vec![]).0);
                }
                for i in 1..=6 {
                    emit(
                        h.frame(shape, "scroll", 640.0, i as f32 * 80.0, 0.1, vec![])
                            .0,
                    );
                }
                let at = original.chars().count() / 2;
                h.select(at, at);
                for _ in 0..2 {
                    emit(h.frame(shape, "settle_before", 640.0, 0.0, 1.2, vec![]).0);
                }
                for i in 0..6 {
                    let events = if i == 0 {
                        vec![egui::Event::Text("猫".into())]
                    } else if i == 2 {
                        vec![
                            egui::Event::Text("x".into()),
                            egui::Event::Paste("e\u{301}".into()),
                        ]
                    } else {
                        vec![egui::Event::Text("x".into())]
                    };
                    let (record, edits) = h.frame(shape, "typing", 640.0, 0.0, 0.1, events);
                    assert!(!edits.is_empty());
                    assert!(!edits.iter().any(|e| e.replacement));
                    emit(record);
                }
                let edited = h.text.clone();
                assert_ne!(edited, original);
                for _ in 0..2 {
                    emit(h.frame(shape, "settle_after", 640.0, 0.0, 1.2, vec![]).0);
                }
                let (record, edits) = h.frame(
                    shape,
                    "undo",
                    640.0,
                    0.0,
                    0.1,
                    vec![key(egui::Key::Z, egui::Modifiers::COMMAND)],
                );
                assert!(edits.iter().any(|e| e.replacement));
                assert_eq!(h.text, original);
                emit(record);
                let (record, edits) = h.frame(
                    shape,
                    "redo",
                    640.0,
                    0.0,
                    0.1,
                    vec![key(
                        egui::Key::Z,
                        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    )],
                );
                assert!(edits.iter().any(|e| e.replacement));
                assert_eq!(h.text, edited);
                emit(record);
                for width in [360.0, 480.0, 640.0, 360.0, 480.0, 640.0] {
                    emit(h.frame(shape, "resize", width, 0.0, 0.1, vec![]).0);
                    assert_eq!(h.text, edited);
                }
                for _ in 0..4 {
                    emit(h.frame(shape, "warm_after", 640.0, 0.0, 0.1, vec![]).0);
                }
            }
        }
    }

    #[test]
    fn production_tracked_stock_ime_selection_delete_and_controller_publication() {
        let mut h = Harness::new(Mode::ProductionTracked, fixture("many_short"));
        h.frame("many_short", "cold", 640.0, 0.0, 0.1, vec![]);
        let original = h.text.clone();
        h.select(0, 1);
        h.frame(
            "many_short",
            "selected_replace",
            640.0,
            0.0,
            0.1,
            vec![
                egui::Event::Text("U".into()),
                egui::Event::Paste("猫\r\ne\u{301}".into()),
            ],
        );
        assert!(h.text.starts_with("U猫\r\ne\u{301}nique"));
        h.select(0, 0);
        h.frame(
            "many_short",
            "ime",
            640.0,
            0.0,
            0.1,
            vec![
                egui::Event::Ime(egui::ImeEvent::Enabled),
                egui::Event::Ime(egui::ImeEvent::Preedit("に".into())),
            ],
        );
        h.frame(
            "many_short",
            "ime",
            640.0,
            0.0,
            0.1,
            vec![
                egui::Event::Ime(egui::ImeEvent::Preedit("日本".into())),
                egui::Event::Ime(egui::ImeEvent::Commit("日本語".into())),
                egui::Event::Ime(egui::ImeEvent::Disabled),
            ],
        );
        assert!(h.text.starts_with("日本語U猫"));
        h.select(0, 3);
        h.frame(
            "many_short",
            "delete",
            640.0,
            0.0,
            0.1,
            vec![key(egui::Key::Backspace, egui::Modifiers::NONE)],
        );
        assert!(h.text.starts_with("U猫"));
        assert_ne!(h.text, original);
        let last = Arc::clone(h.cache.base.galley.as_ref().unwrap());
        let final_epoch = h.cache.base.epoch.value();
        h.frame("many_short", "warm_after", 640.0, 0.0, 0.1, vec![]);
        assert!(Arc::ptr_eq(&last, h.cache.base.galley.as_ref().unwrap()));
        assert_eq!(final_epoch, h.cache.base.epoch.value());
    }

    #[test]
    fn outer_arc_does_not_keep_paragraph_cache_entries_alive() {
        let mut h = Harness::new(Mode::EpochOuterOnly, fixture("many_short"));
        let first = h.frame("many_short", "cold", 640.0, 0.0, 0.1, vec![]).0;
        assert!(first["cache_entries"].as_u64().unwrap() >= 6_000);
        for _ in 0..4 {
            h.frame("many_short", "warm", 640.0, 0.0, 0.1, vec![]);
        }
        let entries = h.ctx.fonts_mut(|f| f.num_galleys_in_cache());
        assert!(
            entries < 10,
            "unreferenced epaint entries must be evicted: {entries}"
        );
        assert!(h.cache.base.galley.as_ref().unwrap().rows.len() >= 6_000);
    }

    #[test]
    fn epoch_observes_multiple_same_revision_mutations_without_text_comparison() {
        let mut h = Harness::new(Mode::EpochTouch, "alpha e\u{301} 日本語\r\nlast".into());
        h.frame("normal", "cold", 640.0, 0.0, 0.1, vec![]);
        h.select(1, 3);
        let (record, edits) = h.frame(
            "normal",
            "typing",
            640.0,
            0.0,
            0.1,
            vec![
                egui::Event::Text("猫".into()),
                egui::Event::Paste("e\u{301}".into()),
            ],
        );
        assert_eq!(h.text, "a猫e\u{301}ha e\u{301} 日本語\r\nlast");
        assert!(edits.len() >= 3);
        assert_eq!(record["work"]["comparison_calls"], 0);
        assert!(h.epoch.get() >= 3);
    }

    #[test]
    fn public_concat_matches_stock_plain_multiline_layout_on_edge_cases() {
        // This materially different composition shape is supported, unlike
        // manually fabricating private Galley fields. It still needs exact
        // full source in the outer job and correct newline/section handling.
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let wrapped = "long wrapped words ".repeat(20);
            for text in [
                "\n",
                "a\n",
                "a\n\n",
                "a\r\nb",
                "猫e\u{301} 👩\u{200d}🚀\nlast",
                &wrapped,
            ] {
                let font = egui::TextStyle::Monospace.resolve(ui.style());
                let job = egui::text::LayoutJob::simple(
                    text.to_owned(),
                    font.clone(),
                    egui::Color32::WHITE,
                    100.0,
                );
                let stock = ui.fonts_mut(|f| f.layout_job(job.clone()));
                let mut children = Vec::new();
                let mut start = 0;
                while start < text.len() {
                    let mut end = text[start..].find('\n').map_or(text.len(), |i| start + i);
                    if end == text.len() - 1 && text.ends_with('\n') {
                        end += 1;
                    }
                    children.push(ui.fonts_mut(|f| {
                        f.layout_job(egui::text::LayoutJob::simple(
                            text[start..end].to_owned(),
                            font.clone(),
                            egui::Color32::WHITE,
                            100.0,
                        ))
                    }));
                    start = end + 1;
                }
                let joined =
                    egui::Galley::concat(Arc::new(job), &children, ui.ctx().pixels_per_point());
                assert_eq!(
                    joined, *stock,
                    "public composition must preserve complete stock geometry and source: {text:?}"
                );
            }
        });
    }
}

#[cfg(test)]
mod issue12_implementation {
    use super::super::edit::{LineMetadata, RecordingBuffer};
    use super::*;
    use egui::TextBuffer;

    fn frame(
        cache: &mut EditorLayoutCache,
        ctx: &egui::Context,
        identity: (u64, u64),
        text: &str,
        width: f32,
    ) -> Arc<egui::Galley> {
        let epoch = cache.begin_tracked(identity, 2);
        let mut result = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            result = Some(cache.layout_known(ui, identity, text, &epoch, &font, width));
        });
        result.unwrap()
    }

    #[test]
    fn public_backend_token_geometry_residency_eviction_and_replacement() {
        use egui::epaint::text::{Fonts, FontsImpl, TextOptions};
        let options = TextOptions::default();
        let mut fonts = Fonts::new(options, egui::FontDefinitions::default());
        let job = egui::text::LayoutJob::simple(
            "first unique\nsecond unique\nlast".into(),
            egui::FontId::monospace(14.0),
            egui::Color32::WHITE,
            120.0,
        );
        let (first, token) = fonts
            .with_pixels_per_point(1.0)
            .layout_job_with_token(job.clone());
        let stock = fonts.with_pixels_per_point(1.0).layout_job(job.clone());
        assert_eq!(*first, *stock);
        assert_eq!(Arc::strong_count(&first), 3);
        for _ in 0..5 {
            fonts.begin_pass(options);
            assert!(
                fonts
                    .with_pixels_per_point(1.0)
                    .touch_cached_layout(&token, &first)
            );
        }
        let mut edited = job.clone();
        edited.text.replace_range(0..5, "FIRST");
        let next = fonts.with_pixels_per_point(1.0).layout_job(edited);
        assert!(Arc::ptr_eq(&first.rows[1].row, &next.rows[1].row));
        assert!(
            !fonts
                .with_pixels_per_point(1.5)
                .touch_cached_layout(&token, &first)
        );
        let mut other = Fonts::new(options, egui::FontDefinitions::default());
        let other_g = other.with_pixels_per_point(1.0).layout_job(job.clone());
        assert!(
            !other
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &first)
        );
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &other_g)
        );
        fonts.begin_pass(options);
        fonts.begin_pass(options);
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &first)
        );
        let (old, token) = fonts
            .with_pixels_per_point(1.0)
            .layout_job_with_token(job.clone());
        let replacement = TextOptions {
            font_hinting: !options.font_hinting,
            ..options
        };
        fonts.begin_pass(replacement);
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &old)
        );
        let (old, token) = fonts
            .with_pixels_per_point(1.0)
            .layout_job_with_token(job.clone());
        fonts.fonts = FontsImpl::new(replacement, egui::FontDefinitions::default());
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &old)
        );
        let new = fonts.with_pixels_per_point(1.0).layout_job(job.clone());
        assert!(!Arc::ptr_eq(&old, &new));
        let small = TextOptions {
            max_texture_side: 1024,
            ..options
        };
        let mut fonts = Fonts::new(small, egui::FontDefinitions::default());
        let (old, token) = fonts.with_pixels_per_point(1.0).layout_job_with_token(job);
        for size in [40.0, 80.0, 120.0, 160.0, 200.0] {
            let _ = fonts
                .with_pixels_per_point(1.0)
                .layout_job(egui::text::LayoutJob::simple(
                    ('!'..='~').collect(),
                    egui::FontId::monospace(size),
                    egui::Color32::WHITE,
                    1000.0,
                ));
            if fonts.font_atlas_fill_ratio() > 0.8 {
                break;
            }
        }
        assert!(fonts.font_atlas_fill_ratio() > 0.8);
        fonts.begin_pass(small);
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &old)
        );
    }

    #[test]
    fn actual_editor_content_large_input_and_discard_replay_keep_publication_stock() {
        let original = "unique ordinary source line\r\n".repeat(3000);
        let (_temp, mut app) = super::super::preview_sync_tests::test_app(&original);
        let ctx = egui::Context::default();
        let mut output = None;
        let input = |events| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700.0, 400.0),
            )),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(input(vec![]), |ui| {
            ui.set_max_width(640.0);
            output = Some(app.editor_content(ui, 400.0));
        });
        let top = output.as_ref().unwrap().galley_top;
        let pos = egui::pos2(70.0, top + 5.0);
        let _ = ctx.run_ui(
            input(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]),
            |ui| {
                ui.set_max_width(640.0);
                let _ = app.editor_content(ui, 400.0);
            },
        );
        assert!(ctx.memory(|m| m.focused().is_some()));
        let _ = ctx.run_ui(
            input(vec![
                egui::Event::Text("猫".into()),
                egui::Event::Paste("e\u{301}".into()),
            ]),
            |ui| {
                ui.set_max_width(640.0);
                output = Some(app.editor_content(ui, 400.0));
            },
        );
        let document = app.state.active_document().unwrap();
        assert_eq!(
            document.revision, 1,
            "several actual operations publish one revision, including discard replay"
        );
        assert_eq!(document.content.len(), original.len() + "猫e\u{301}".len());
        assert_eq!(
            output.as_ref().unwrap().galley.as_ref().unwrap().job.text,
            document.content
        );
        app.editor_layout.qualification_work = QualificationWork::default();
        let _ = ctx.run_ui(input(vec![]), |ui| {
            ui.set_max_width(640.0);
            let _ = app.editor_content(ui, 400.0);
        });
        assert_eq!(app.editor_layout.qualification_work.outer_job_copy_bytes, 0);
        assert_eq!(app.editor_layout.qualification_work.comparison_calls, 0);
    }

    #[test]
    fn trusted_immediate_mutations_and_exact_revision_adoption() {
        let ctx = egui::Context::default();
        let mut cache = EditorLayoutCache::default();
        let mut text = "abc e\u{301} 日本語\r\n".repeat(5000);
        let mut index = LineMetadata::from_content(&text);
        let epoch = cache.begin_tracked((7, 0), index.line_starts.len());
        let mut final_galley = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::FontId::monospace(14.0);
            let first = cache.layout_known(ui, (7, 0), &text, &epoch, &font, 400.0);
            let mut buffer = RecordingBuffer::tracked(&mut text, &mut index, &epoch);
            assert_eq!(buffer.insert_text("", 0), 0);
            let before = epoch.value();
            buffer.delete_char_range(usize::MAX..usize::MAX);
            assert_eq!(epoch.value(), before, "no-op does not advance");
            buffer.delete_char_range(0..1);
            buffer.insert_text("z", 0);
            let same_length = cache.layout_recording(ui, (7, 0), &buffer, &epoch, &font, 400.0);
            assert_eq!(same_length.job.text.len(), first.job.text.len());
            assert!(!Arc::ptr_eq(&first, &same_length));
            assert_eq!(same_length.job.text, buffer.as_str());
            buffer.insert_text("猫\r\ne\u{301}👩\u{200d}🚀", 2);
            let multiple = cache.layout_recording(ui, (7, 0), &buffer, &epoch, &font, 400.0);
            assert_eq!(multiple.job.text, buffer.as_str());
            let replacement = "replacement\r\n猫".repeat(5000);
            buffer.replace_with(&replacement);
            let final_result = cache.layout_recording(ui, (7, 0), &buffer, &epoch, &font, 400.0);
            assert_eq!(final_result.job.text, replacement);
            final_galley = Some(final_result);
            assert_eq!(buffer.finish().len(), 4);
        });
        let galley = final_galley.unwrap();
        let final_epoch = epoch.value();
        cache.finish_tracked((7, 0), 1, final_epoch, &galley);
        assert_eq!(
            epoch.value(),
            final_epoch,
            "controller publication must not reset epoch"
        );
        cache.qualification_work = QualificationWork::default();
        // Same actual font/width as above, next pass and newly published revision.
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::FontId::monospace(14.0);
            let next_epoch = cache.begin_tracked((7, 1), index.line_starts.len());
            let next = cache.layout_known(ui, (7, 1), &text, &next_epoch, &font, 400.0);
            assert!(Arc::ptr_eq(&galley, &next));
        });
        assert_eq!(cache.qualification_work.comparison_calls, 0);
        assert_eq!(cache.qualification_work.outer_job_copy_bytes, 0);
        assert_eq!(cache.qualification_work.outer_job_hash_bytes, 0);
        assert_eq!(
            index.line_starts,
            LineMetadata::from_content(&text).line_starts
        );
    }

    #[test]
    fn same_pass_cache_identity_check_never_replaces_a_new_pass_refresh() {
        use egui::epaint::text::{Fonts, FontsImpl, TextOptions};
        let options = TextOptions::default();
        let mut fonts = Fonts::new(options, egui::FontDefinitions::default());
        let job = egui::text::LayoutJob::simple(
            "first\nsecond".into(),
            egui::FontId::monospace(14.0),
            egui::Color32::WHITE,
            200.0,
        );
        let (galley, token) = fonts.with_pixels_per_point(1.0).layout_job_with_token(job);
        assert!(
            fonts
                .with_pixels_per_point(1.0)
                .cached_layout_is_current(&token, &galley)
        );
        fonts.begin_pass(options);
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .cached_layout_is_current(&token, &galley)
        );
        assert!(
            fonts
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &galley)
        );
        assert!(
            fonts
                .with_pixels_per_point(1.0)
                .cached_layout_is_current(&token, &galley)
        );
        fonts.fonts = FontsImpl::new(options, egui::FontDefinitions::default());
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .cached_layout_is_current(&token, &galley)
        );
        assert!(
            !fonts
                .with_pixels_per_point(1.0)
                .touch_cached_layout(&token, &galley)
        );
    }

    #[test]
    fn untracked_recording_buffer_cannot_reuse_an_unchanged_epoch() {
        let ctx = egui::Context::default();
        let mut cache = EditorLayoutCache::default();
        let mut text = "large ordinary source ".repeat(4000);
        let mut index = LineMetadata::from_content(&text);
        let epoch = cache.begin_tracked((9, 0), 1);
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::FontId::monospace(14.0);
            let _ = cache.layout_known(ui, (9, 0), &text, &epoch, &font, 400.0);
            let tracked = RecordingBuffer::tracked(&mut text, &mut index, &epoch);
            assert!(epoch.matches_recording_source(tracked.as_str()));
            let _ = tracked.finish();
            assert!(
                !epoch.matches_recording_source(&text),
                "completed buffer must release its origin proof"
            );
            let mut buffer = RecordingBuffer::new(&mut text, &mut index);
            buffer.delete_char_range(0..1);
            buffer.insert_text("L", 0);
            let actual = cache.layout_recording(ui, (9, 0), &buffer, &epoch, &font, 400.0);
            assert!(
                actual.job.text == buffer.as_str(),
                "untracked same-type input must not return stale text"
            );
            assert!(
                cache.token.is_none(),
                "untracked input must take the guarded fallback"
            );
        });
    }

    #[test]
    fn stale_epoch_wrong_output_unknown_buffer_and_overflow_fail_closed() {
        let ctx = egui::Context::default();
        let mut cache = EditorLayoutCache::default();
        let mut text = "large ordinary source ".repeat(4000);
        let first = frame(&mut cache, &ctx, (1, 0), &text, 400.0);
        let clock = Rc::clone(&cache.epoch);
        clock.advance();
        cache.finish_tracked((1, 0), 1, clock.value(), &first);
        assert!(
            cache.tracked_origin.is_none(),
            "a mutation not laid out cannot be adopted"
        );
        let old = frame(&mut cache, &ctx, (1, 1), &text, 200.0);
        cache.finish_tracked((1, 1), 2, clock.value(), &first);
        assert!(
            cache.tracked_origin.is_none(),
            "wrong actual output cannot be adopted"
        );
        let _ = frame(&mut cache, &ctx, (1, 2), &text, 400.0);
        let epoch = cache.begin_tracked((1, 2), 1);
        text.replace_range(0..1, "L");
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            let unknown = cache.layout_recording(ui, (1, 2), &text, &epoch, &font, 400.0);
            assert_eq!(unknown.job.text, text);
            assert!(!Arc::ptr_eq(&old, &unknown));
            assert!(cache.token.is_none());
        });
        clock.force_overflow();
        let _ = frame(&mut cache, &ctx, (1, 2), &text, 400.0);
        assert!(cache.token.is_none());
        assert_eq!(clock.value(), None);
        let _ = frame(&mut cache, &ctx, (2, 0), &text, 400.0);
        assert_eq!(
            clock.value(),
            None,
            "document transition cannot resurrect an exhausted epoch"
        );
        let mut cache = EditorLayoutCache::default();
        let _ = frame(&mut cache, &ctx, (1, u64::MAX), &text, 400.0);
        assert!(
            cache.token.is_none(),
            "saturated controller revisions cannot be trusted"
        );
    }

    #[test]
    fn context_font_scale_style_width_document_and_size_guards() {
        let ctx = egui::Context::default();
        let other = egui::Context::default();
        let mut cache = EditorLayoutCache::default();
        let text = "source line\n".repeat(7000);
        let first = frame(&mut cache, &ctx, (1, 0), &text, 400.0);
        let warm = frame(&mut cache, &ctx, (1, 0), &text, 400.0);
        assert!(Arc::ptr_eq(&first, &warm));
        let context = frame(&mut cache, &other, (1, 0), &text, 400.0);
        assert!(!Arc::ptr_eq(&warm, &context));
        let narrow = frame(&mut cache, &other, (1, 0), &text, 200.0);
        assert!(!Arc::ptr_eq(&context, &narrow));
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            egui::FontFamily::Monospace,
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        other.set_fonts(fonts);
        let font = frame(&mut cache, &other, (1, 0), &text, 200.0);
        assert!(!Arc::ptr_eq(&narrow, &font));
        other.set_pixels_per_point(1.5);
        let scale = frame(&mut cache, &other, (1, 0), &text, 200.0);
        assert!(!Arc::ptr_eq(&font, &scale));
        other.global_style_mut(|s| s.visuals.override_text_color = Some(egui::Color32::RED));
        let style = frame(&mut cache, &other, (1, 0), &text, 200.0);
        assert!(!Arc::ptr_eq(&scale, &style));
        let layouts = cache.layouts;
        let _ = frame(&mut cache, &other, (2, 0), &text, 200.0);
        assert_eq!(cache.layouts, layouts + 1);
        let small = frame(&mut cache, &other, (2, 1), "normal note", 200.0);
        assert!(cache.token.is_none());
        assert_eq!(small.job.text, "normal note");
        let epoch = cache.begin_tracked((2, 2), 32_769);
        let _ = other.run_ui(egui::RawInput::default(), |ui| {
            let _ = cache.layout_known(
                ui,
                (2, 2),
                &text,
                &epoch,
                &egui::FontId::monospace(14.0),
                200.0,
            );
        });
        assert!(
            cache.token.is_none(),
            "too many lines uses unchanged fallback"
        );
        // No oversized layout allocation is needed to qualify the exact bound.
        assert!(large_layout_eligible(64 * 1024, 32_768));
        assert!(large_layout_eligible(4 * 1024 * 1024, 1));
        assert!(!large_layout_eligible(64 * 1024 - 1, 1));
        assert!(!large_layout_eligible(4 * 1024 * 1024 + 1, 1));
        assert!(!large_layout_eligible(64 * 1024, 32_769));
        drop((first, warm, context, narrow, font, scale, style, small));
        let previous = Arc::downgrade(cache.galley.as_ref().unwrap());
        let final_note = frame(&mut cache, &other, (3, 0), "small replacement", 200.0);
        cache.numbered_rows(&final_note);
        for _ in 0..3 {
            let _ = frame(&mut cache, &other, (3, 0), "small replacement", 200.0);
        }
        assert!(
            previous.upgrade().is_none(),
            "one active layout; no application history"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        cache: &mut EditorLayoutCache,
        ctx: &egui::Context,
        id: (u64, u64),
        text: &str,
        width: f32,
    ) -> Arc<egui::Galley> {
        let mut result = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            result = Some(cache.layout(ui, id, text, &font, width));
        });
        result.unwrap()
    }

    #[test]
    fn warm_editor_layout_is_shared_and_text_edit_same_revision_still_invalidates() {
        let ctx = egui::Context::default();
        let mut cache = EditorLayoutCache::default();
        let first = frame(&mut cache, &ctx, (1, 0), "alpha", 200.0);
        let warm = frame(&mut cache, &ctx, (1, 0), "alpha", 200.0);
        assert!(Arc::ptr_eq(&first, &warm));
        assert_eq!(cache.layouts, 1);
        let edited = frame(&mut cache, &ctx, (1, 0), "alpha beta", 200.0);
        assert_eq!(edited.job.text, "alpha beta");
        assert_eq!(cache.layouts, 2);
        let _ = frame(&mut cache, &ctx, (1, 1), "alpha beta", 200.0);
        let _ = frame(&mut cache, &ctx, (2, 1), "alpha beta", 200.0);
        let _ = frame(&mut cache, &ctx, (2, 1), "alpha beta", 100.0);
        assert_eq!(cache.layouts, 5);
    }

    #[test]
    fn retained_editor_keeps_incremental_paragraph_cache_resident() {
        let ctx = egui::Context::default();
        let mut cache = EditorLayoutCache::default();
        let text = (0..100)
            .map(|i| format!("unique paragraph {i}\n"))
            .collect::<String>();
        for _ in 0..6 {
            let _ = frame(&mut cache, &ctx, (1, 0), &text, 200.0);
        }
        let resident = ctx.fonts_mut(|fonts| fonts.num_galleys_in_cache());
        assert!(
            resident >= 100,
            "warm retained layout must keep egui paragraph entries resident, got {resident}"
        );
    }

    #[test]
    fn changed_font_definitions_invalidate_retained_editor_galley() {
        let ctx = egui::Context::default();
        let mut cache = EditorLayoutCache::default();
        let first = frame(&mut cache, &ctx, (1, 0), "iiiiii", 200.0);
        let mut definitions = egui::FontDefinitions::default();
        let proportional = definitions.families[&egui::FontFamily::Proportional].clone();
        definitions
            .families
            .insert(egui::FontFamily::Monospace, proportional);
        ctx.set_fonts(definitions);
        let changed = frame(&mut cache, &ctx, (1, 0), "iiiiii", 200.0);
        assert!(
            !Arc::ptr_eq(&first, &changed),
            "same FontId may refer to changed font bytes/family mapping"
        );
        assert_eq!(cache.layouts, 2);
    }
}
