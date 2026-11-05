use crate::{app_state::normalize_split_ratio, theme::DesignTokens};
use eframe::egui::{self, CursorIcon, PointerButton, Rect, Sense, Stroke, pos2, vec2};

const DIVIDER_WIDTH: f32 = 8.0;
const MIN_PANE_WIDTH: f32 = 180.0;

pub(super) struct SplitLayout {
    pub editor: Rect,
    pub divider: Rect,
    pub preview: Rect,
    pub changed_ratio: Option<f32>,
}

fn regions(bounds: Rect, ratio: f32) -> SplitLayout {
    let divider_width = DIVIDER_WIDTH.min(bounds.width().max(0.0));
    let width = (bounds.width() - divider_width).max(0.0);
    let minimum = if width > 0.0 {
        (MIN_PANE_WIDTH / width).min(0.5)
    } else {
        0.5
    };
    let ratio = normalize_split_ratio(ratio).clamp(minimum, 1.0 - minimum);
    let x = bounds.left() + width * ratio;
    SplitLayout {
        editor: Rect::from_min_max(bounds.min, pos2(x, bounds.bottom())),
        divider: Rect::from_min_max(
            pos2(x, bounds.top()),
            pos2(x + divider_width, bounds.bottom()),
        ),
        preview: Rect::from_min_max(pos2(x + divider_width, bounds.top()), bounds.max),
        changed_ratio: None,
    }
}

pub(super) fn show(ui: &mut egui::Ui, ratio: f32, tokens: DesignTokens) -> SplitLayout {
    let (bounds, _) =
        ui.allocate_exact_size(ui.available_size().max(vec2(0.0, 0.0)), Sense::hover());
    let mut layout = regions(bounds, ratio);
    let response = ui
        .interact(
            layout.divider,
            ui.make_persistent_id("split_divider"),
            Sense::click_and_drag(),
        )
        .on_hover_text("Drag to resize · Double-click to reset");
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }
    let requested = if response.double_clicked() {
        Some(0.5)
    } else if response.dragged_by(PointerButton::Primary) {
        let width = bounds.width() - layout.divider.width();
        response
            .interact_pointer_pos()
            .filter(|_| width > 0.0)
            .map(|pointer| {
                let minimum = (MIN_PANE_WIDTH / width).min(0.5);
                normalize_split_ratio(
                    ((pointer.x - bounds.left() - layout.divider.width() * 0.5) / width)
                        .clamp(minimum, 1.0 - minimum),
                )
            })
    } else {
        None
    };
    if let Some(requested) = requested
        && (requested - ratio).abs() > f32::EPSILON
    {
        layout = regions(bounds, requested);
        layout.changed_ratio = Some(requested);
    }
    let color = if response.dragged() || response.hovered() {
        tokens.accent
    } else {
        tokens.border
    };
    ui.painter().line_segment(
        [layout.divider.center_top(), layout.divider.center_bottom()],
        Stroke::new(1.0_f32, color),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            ui.is_enabled(),
            "Resize editor and preview",
        )
    });
    layout
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_regions_fill_bounds_without_overlap_and_keep_usable_panes() {
        for width in [0.0, 4.0, 100.0, 360.0, 560.0, 1100.0] {
            let bounds = Rect::from_min_size(pos2(10.0, 20.0), vec2(width, 700.0));
            for ratio in [f32::NAN, f32::INFINITY, -1.0, 0.15, 0.5, 0.85, 2.0] {
                let layout = regions(bounds, ratio);
                assert!(layout.editor.width() >= 0.0 && layout.preview.width() >= 0.0);
                assert_eq!(layout.editor.left(), bounds.left());
                assert_eq!(layout.editor.right(), layout.divider.left());
                assert_eq!(layout.divider.right(), layout.preview.left());
                assert_eq!(layout.preview.right(), bounds.right());
                if width >= MIN_PANE_WIDTH * 2.0 + DIVIDER_WIDTH {
                    assert!(layout.editor.width() >= MIN_PANE_WIDTH - 0.001);
                    assert!(layout.preview.width() >= MIN_PANE_WIDTH - 0.001);
                }
            }
        }
    }
}
