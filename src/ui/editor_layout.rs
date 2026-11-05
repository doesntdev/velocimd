use eframe::egui;
use std::sync::Arc;

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
/// Text is checked too: TextEdit may invoke its layouter after mutating the
/// buffer but before the controller advances the document revision.
#[derive(Default)]
pub(super) struct EditorLayoutCache {
    key: Option<LayoutKey>,
    galley: Option<Arc<egui::Galley>>,
    touched_pass: Option<u64>,
    numbered_galley: Option<Arc<egui::Galley>>,
    numbered_rows: Vec<(usize, usize)>,
    #[cfg(test)]
    pub layouts: usize,
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
        if self.key.as_ref() == Some(&key)
            && let Some(galley) = &self.galley
            && galley.job.text == text
        {
            // Retaining an Arc does not mark egui's paragraph-cache children
            // used. Touch its job once per pass so a later small edit can reuse
            // unchanged paragraphs instead of cold-shaping the whole document.
            let pass = ui.ctx().cumulative_pass_nr();
            if self.touched_pass == Some(pass) {
                return Arc::clone(galley);
            }
            let live = ui.fonts_mut(|fonts| fonts.layout_job((*galley.job).clone()));
            self.touched_pass = Some(pass);
            self.galley = Some(Arc::clone(&live));
            return live;
        }
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
        }
        galley
    }

    pub fn numbered_rows(&mut self, galley: &Arc<egui::Galley>) -> &[(usize, usize)] {
        if self
            .numbered_galley
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, galley))
        {
            self.numbered_rows.clear();
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
