use crate::{app_state::normalize_split_ratio, theme::DesignTokens};
use eframe::egui::{self, CursorIcon, PointerButton, Rect, Sense, Stroke, pos2, vec2};

const DIVIDER_WIDTH: f32 = 8.0;
const STACKED_DIVIDER_HEIGHT: f32 = 24.0;
const MIN_PANE_EXTENT: f32 = 180.0;
const DIVIDER_HIT_EXTENT: f32 = 24.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SplitAxis {
    Horizontal,
    Vertical,
}

impl SplitAxis {
    // The application allocation, before chrome consumes height, is authoritative.
    // The width gate also keeps a phone stacked when its keyboard shortens it.
    pub(super) fn for_size(size: egui::Vec2) -> Self {
        if size.x < 720.0 || size.x < size.y {
            Self::Vertical
        } else {
            Self::Horizontal
        }
    }

    fn coordinate(self, point: egui::Pos2) -> f32 {
        match self {
            Self::Horizontal => point.x,
            Self::Vertical => point.y,
        }
    }

    fn extent(self, rect: Rect) -> f32 {
        match self {
            Self::Horizontal => rect.width(),
            Self::Vertical => rect.height(),
        }
    }
}

pub(super) struct SplitLayout {
    pub editor: Rect,
    pub divider: Rect,
    pub preview: Rect,
    pub changed_ratio: Option<f32>,
    pub response_rect: Option<Rect>,
}

fn regions(bounds: Rect, ratio: f32, axis: SplitAxis) -> SplitLayout {
    let divider_extent = match axis {
        SplitAxis::Horizontal => DIVIDER_WIDTH,
        SplitAxis::Vertical => STACKED_DIVIDER_HEIGHT,
    }
    .min(axis.extent(bounds).max(0.0));
    let extent = (axis.extent(bounds) - divider_extent).max(0.0);
    let minimum = minimum_ratio(extent);
    let ratio = normalize_split_ratio(ratio).clamp(minimum, 1.0 - minimum);
    let start = axis.coordinate(bounds.min) + extent * ratio;
    let (editor_max, divider_min, divider_max, preview_min) = match axis {
        SplitAxis::Horizontal => (
            pos2(start, bounds.bottom()),
            pos2(start, bounds.top()),
            pos2(start + divider_extent, bounds.bottom()),
            pos2(start + divider_extent, bounds.top()),
        ),
        SplitAxis::Vertical => (
            pos2(bounds.right(), start),
            pos2(bounds.left(), start),
            pos2(bounds.right(), start + divider_extent),
            pos2(bounds.left(), start + divider_extent),
        ),
    };
    SplitLayout {
        editor: Rect::from_min_max(bounds.min, editor_max),
        divider: Rect::from_min_max(divider_min, divider_max),
        preview: Rect::from_min_max(preview_min, bounds.max),
        changed_ratio: None,
        response_rect: None,
    }
}

fn minimum_ratio(extent: f32) -> f32 {
    if extent > 0.0 {
        (MIN_PANE_EXTENT / extent).min(0.5)
    } else {
        0.5
    }
}

pub(super) fn show(
    ui: &mut egui::Ui,
    ratio: f32,
    tokens: DesignTokens,
    axis: SplitAxis,
) -> SplitLayout {
    let (bounds, _) =
        ui.allocate_exact_size(ui.available_size().max(vec2(0.0, 0.0)), Sense::hover());
    let mut layout = regions(bounds, ratio, axis);
    // Keep desktop pane/line geometry unchanged, while accepting touch near it.
    let extra = ((DIVIDER_HIT_EXTENT - axis.extent(layout.divider)) * 0.5).max(0.0);
    let hit_rect = layout
        .divider
        .expand2(match axis {
            SplitAxis::Horizontal => vec2(extra, 0.0),
            SplitAxis::Vertical => vec2(0.0, extra),
        })
        .intersect(bounds);
    let id = ui.make_persistent_id("split_divider");
    let response = ui
        .interact(hit_rect, id, Sense::click_and_drag())
        .on_hover_text("Drag to resize · Double-click to reset");
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(match axis {
            SplitAxis::Horizontal => CursorIcon::ResizeHorizontal,
            SplitAxis::Vertical => CursorIcon::ResizeVertical,
        });
    }
    let drag_axis_id = id.with("drag_axis");
    if response.drag_started_by(PointerButton::Primary) {
        ui.data_mut(|data| data.insert_temp(drag_axis_id, axis));
    }
    let same_axis = ui.data(|data| data.get_temp::<SplitAxis>(drag_axis_id)) == Some(axis);
    let moving = ui.input(|input| match axis {
        SplitAxis::Horizontal => input.pointer.delta().x != 0.0,
        SplitAxis::Vertical => input.pointer.delta().y != 0.0,
    });
    let requested = if response.double_clicked() {
        Some(0.5)
    } else if response.dragged_by(PointerButton::Primary) && same_axis && moving {
        let extent = axis.extent(bounds) - axis.extent(layout.divider);
        response
            .interact_pointer_pos()
            .filter(|_| extent > 0.0)
            .map(|pointer| {
                let minimum = minimum_ratio(extent);
                normalize_split_ratio(
                    ((axis.coordinate(pointer)
                        - axis.coordinate(bounds.min)
                        - axis.extent(layout.divider) * 0.5)
                        / extent)
                        .clamp(minimum, 1.0 - minimum),
                )
            })
    } else {
        None
    };
    if let Some(requested) = requested
        && (requested - ratio).abs() > f32::EPSILON
    {
        layout = regions(bounds, requested, axis);
        layout.changed_ratio = Some(requested);
    }
    let color = if response.dragged() || response.hovered() {
        tokens.accent
    } else {
        tokens.border
    };
    let line = match axis {
        SplitAxis::Horizontal => [layout.divider.center_top(), layout.divider.center_bottom()],
        SplitAxis::Vertical => [layout.divider.left_center(), layout.divider.right_center()],
    };
    ui.painter().line_segment(line, Stroke::new(1.0_f32, color));
    if axis == SplitAxis::Vertical {
        let grip = Rect::from_center_size(
            layout.divider.center(),
            vec2(
                32.0_f32.min(bounds.width()),
                3.0_f32.min(layout.divider.height()),
            ),
        );
        ui.painter().rect_filled(grip, 2, color);
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Other,
            ui.is_enabled(),
            "Resize editor and preview",
        )
    });
    layout.response_rect = Some(response.rect);
    layout
}

// Pane ScrollAreas also sense drags. Re-register the same hit target after them
// so its invisible desktop touch slop is not stolen by a pane underneath it.
pub(super) fn raise_hit_target(ui: &egui::Ui, layout: &SplitLayout) {
    if let Some(rect) = layout.response_rect {
        let _ = ui.interact_opt(
            rect,
            ui.make_persistent_id("split_divider"),
            Sense::click_and_drag(),
            egui::InteractOptions { move_to_top: true },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn responsive_axis_uses_narrow_or_portrait_app_allocation() {
        for (size, axis) in [
            (vec2(719.0, 200.0), SplitAxis::Vertical),
            (vec2(720.0, 400.0), SplitAxis::Horizontal),
            (vec2(800.0, 830.0), SplitAxis::Vertical),
            (vec2(1000.0, 700.0), SplitAxis::Horizontal),
            (vec2(800.0, 800.0), SplitAxis::Horizontal),
        ] {
            assert_eq!(SplitAxis::for_size(size), axis);
        }
    }

    #[test]
    fn responsive_regions_fill_cramped_and_regular_bounds_in_both_axes() {
        for axis in [SplitAxis::Horizontal, SplitAxis::Vertical] {
            for extent in [0.0, 4.0, 100.0, 360.0, 560.0, 1100.0] {
                let size = match axis {
                    SplitAxis::Horizontal => vec2(extent, 700.0),
                    SplitAxis::Vertical => vec2(390.0, extent),
                };
                let bounds = Rect::from_min_size(pos2(10.0, 20.0), size);
                for ratio in [f32::NAN, f32::INFINITY, -1.0, 0.15, 0.5, 0.85, 2.0] {
                    let layout = regions(bounds, ratio, axis);
                    for pane in [layout.editor, layout.divider, layout.preview] {
                        assert!(pane.width() >= 0.0 && pane.height() >= 0.0);
                        assert!(bounds.contains_rect(pane));
                    }
                    assert_eq!(layout.editor.min, bounds.min);
                    assert_eq!(layout.preview.max, bounds.max);
                    assert_eq!(
                        axis.coordinate(layout.editor.max),
                        axis.coordinate(layout.divider.min)
                    );
                    assert_eq!(
                        axis.coordinate(layout.divider.max),
                        axis.coordinate(layout.preview.min)
                    );
                    let available = extent - axis.extent(layout.divider);
                    if available >= MIN_PANE_EXTENT * 2.0 {
                        assert!(axis.extent(layout.editor) >= MIN_PANE_EXTENT - 0.001);
                        assert!(axis.extent(layout.preview) >= MIN_PANE_EXTENT - 0.001);
                    }
                    assert!(
                        layout.changed_ratio.is_none(),
                        "passive clamping is not a persisted ratio change"
                    );
                }
            }
        }
    }
}
