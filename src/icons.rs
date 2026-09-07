use crate::{commands::Command, modes::EditorMode};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Plus,
    Folder,
    File,
    Save,
    SaveAs,
    X,
    Edit,
    Eye,
    Columns,
    Rotate,
    Sun,
    Moon,
    Check,
}

impl Icon {
    pub fn for_command(command: Command) -> Self {
        match command {
            Command::NewTab => Self::Plus,
            Command::SelectWorkingFolder => Self::Folder,
            Command::OpenFile => Self::File,
            Command::SaveFile => Self::Save,
            Command::SaveFileAs => Self::SaveAs,
            Command::CloseFolderTab => Self::X,
            Command::TogglePalette => Self::File,
            Command::SetMode(EditorMode::Edit) => Self::Edit,
            Command::SetMode(EditorMode::Preview) => Self::Eye,
            Command::SetMode(EditorMode::Split) => Self::Columns,
            Command::CycleMode => Self::Rotate,
            Command::SwitchThemeLight => Self::Sun,
            Command::SwitchThemeDark => Self::Moon,
        }
    }
}

pub fn icon_button(ui: &mut Ui, icon: Icon, active: bool, tooltip: String) -> Response {
    icon_button_sized(ui, icon, active, tooltip, vec2(30.0, 28.0), 7.0)
}

pub fn compact_icon_button(ui: &mut Ui, icon: Icon, tooltip: String) -> Response {
    icon_button_sized(ui, icon, false, tooltip, vec2(20.0, 20.0), 5.0)
}

fn icon_button_sized(
    ui: &mut Ui,
    icon: Icon,
    active: bool,
    tooltip: String,
    size: egui::Vec2,
    icon_padding: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals();
    let fill = if active {
        visuals.widgets.active.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        Color32::TRANSPARENT
    };
    let stroke_color = if active {
        visuals.selection.stroke.color
    } else if response.hovered() {
        visuals.widgets.hovered.bg_stroke.color
    } else {
        Color32::TRANSPARENT
    };

    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect.shrink(1.0), 5, fill);
        painter.rect_stroke(
            rect.shrink(1.0),
            5,
            Stroke::new(1.0_f32, stroke_color),
            StrokeKind::Inside,
        );
        paint_icon(
            &painter,
            icon,
            rect.shrink(icon_padding),
            if active || response.hovered() {
                visuals.text_color()
            } else {
                visuals.weak_text_color()
            },
        );
    }

    response.on_hover_text(tooltip)
}

pub fn paint_logo(painter: &egui::Painter, rect: Rect, accent: Color32, text: Color32) {
    let stroke = Stroke::new(1.8_f32, accent);
    let left = rect.left();
    let center_y = rect.center().y;
    let chevron_w = 8.0;
    let chevron_h = 11.0;

    for offset in [0.0, 8.0] {
        let x = left + offset + 2.0;
        painter.line_segment(
            [
                pos2(x, center_y - chevron_h * 0.5),
                pos2(x + chevron_w * 0.55, center_y),
            ],
            stroke,
        );
        painter.line_segment(
            [
                pos2(x + chevron_w * 0.55, center_y),
                pos2(x, center_y + chevron_h * 0.5),
            ],
            stroke,
        );
    }

    painter.text(
        pos2(left + 20.0, center_y),
        Align2::LEFT_CENTER,
        "md",
        FontId::monospace(13.0),
        text,
    );
}

pub fn paint_icon(painter: &egui::Painter, icon: Icon, rect: Rect, color: Color32) {
    let stroke = Stroke::new(1.65_f32, color);
    let center = rect.center();
    let left = rect.left();
    let right = rect.right();
    let top = rect.top();
    let bottom = rect.bottom();
    let width = rect.width();
    let height = rect.height();

    match icon {
        Icon::Plus => {
            line(painter, center.x, top + 2.0, center.x, bottom - 2.0, stroke);
            line(painter, left + 2.0, center.y, right - 2.0, center.y, stroke);
        }
        Icon::Folder => {
            let y0 = top + height * 0.32;
            let y1 = bottom - 2.0;
            let tab_right = left + width * 0.42;
            polyline(
                painter,
                &[
                    pos2(left + 1.0, y1),
                    pos2(left + 1.0, y0),
                    pos2(left + width * 0.28, y0),
                    pos2(left + width * 0.36, top + 2.0),
                    pos2(tab_right, top + 2.0),
                    pos2(tab_right + 2.0, y0),
                    pos2(right - 1.0, y0),
                    pos2(right - 1.0, y1),
                    pos2(left + 1.0, y1),
                ],
                stroke,
            );
        }
        Icon::File => {
            polyline(
                painter,
                &[
                    pos2(left + 3.0, top + 1.0),
                    pos2(right - 5.0, top + 1.0),
                    pos2(right - 1.0, top + 5.0),
                    pos2(right - 1.0, bottom - 1.0),
                    pos2(left + 3.0, bottom - 1.0),
                    pos2(left + 3.0, top + 1.0),
                ],
                stroke,
            );
            polyline(
                painter,
                &[
                    pos2(right - 5.0, top + 1.0),
                    pos2(right - 5.0, top + 5.0),
                    pos2(right - 1.0, top + 5.0),
                ],
                stroke,
            );
        }
        Icon::Save | Icon::SaveAs => {
            painter.rect_stroke(rect.shrink2(vec2(2.0, 1.0)), 2, stroke, StrokeKind::Inside);
            line(
                painter,
                left + 5.0,
                top + 1.0,
                left + 5.0,
                top + height * 0.38,
                stroke,
            );
            line(
                painter,
                left + 5.0,
                top + height * 0.38,
                right - 5.0,
                top + height * 0.38,
                stroke,
            );
            painter.rect_stroke(
                Rect::from_min_max(
                    pos2(left + 5.0, bottom - 6.0),
                    pos2(right - 5.0, bottom - 1.0),
                ),
                1,
                stroke,
                StrokeKind::Inside,
            );
            if icon == Icon::SaveAs {
                line(
                    painter,
                    right - 1.0,
                    top + 2.0,
                    right - 1.0,
                    top + 8.0,
                    stroke,
                );
                line(
                    painter,
                    right - 4.0,
                    top + 5.0,
                    right + 2.0,
                    top + 5.0,
                    stroke,
                );
            }
        }
        Icon::X => {
            line(
                painter,
                left + 3.0,
                top + 3.0,
                right - 3.0,
                bottom - 3.0,
                stroke,
            );
            line(
                painter,
                right - 3.0,
                top + 3.0,
                left + 3.0,
                bottom - 3.0,
                stroke,
            );
        }
        Icon::Edit => {
            line(
                painter,
                left + 3.0,
                bottom - 3.0,
                right - 4.0,
                top + 4.0,
                stroke,
            );
            line(
                painter,
                right - 7.0,
                top + 3.0,
                right - 3.0,
                top + 7.0,
                stroke,
            );
            line(
                painter,
                left + 2.0,
                bottom - 2.0,
                left + 7.0,
                bottom - 3.0,
                stroke,
            );
        }
        Icon::Eye => {
            polyline(
                painter,
                &[
                    pos2(left + 1.0, center.y),
                    pos2(left + width * 0.3, top + 3.0),
                    pos2(center.x, top + 2.0),
                    pos2(right - width * 0.3, top + 3.0),
                    pos2(right - 1.0, center.y),
                    pos2(right - width * 0.3, bottom - 3.0),
                    pos2(center.x, bottom - 2.0),
                    pos2(left + width * 0.3, bottom - 3.0),
                    pos2(left + 1.0, center.y),
                ],
                stroke,
            );
            painter.circle_stroke(center, 2.5, stroke);
        }
        Icon::Columns => {
            painter.rect_stroke(rect.shrink(2.0), 2, stroke, StrokeKind::Inside);
            line(painter, center.x, top + 2.0, center.x, bottom - 2.0, stroke);
        }
        Icon::Rotate => {
            painter.circle_stroke(center, width.min(height) * 0.34, stroke);
            polyline(
                painter,
                &[
                    pos2(right - 3.0, center.y - 4.0),
                    pos2(right - 1.0, center.y + 1.0),
                    pos2(right - 6.0, center.y + 1.0),
                ],
                stroke,
            );
        }
        Icon::Sun => {
            painter.circle_stroke(center, 3.2, stroke);
            for (dx, dy) in [
                (0.0, -1.0),
                (0.0, 1.0),
                (-1.0, 0.0),
                (1.0, 0.0),
                (-0.7, -0.7),
                (0.7, -0.7),
                (-0.7, 0.7),
                (0.7, 0.7),
            ] {
                let from = center + vec2(dx, dy) * 6.0;
                let to = center + vec2(dx, dy) * 8.0;
                painter.line_segment([from, to], stroke);
            }
        }
        Icon::Moon => {
            polyline(
                painter,
                &[
                    pos2(center.x + 4.5, top + 2.0),
                    pos2(center.x + 1.0, top + 3.5),
                    pos2(center.x - 1.0, center.y),
                    pos2(center.x + 1.0, bottom - 3.5),
                    pos2(center.x + 4.5, bottom - 2.0),
                    pos2(center.x + 1.0, bottom - 1.5),
                    pos2(center.x - 5.0, center.y),
                    pos2(center.x + 1.0, top + 1.5),
                    pos2(center.x + 4.5, top + 2.0),
                ],
                stroke,
            );
        }
        Icon::Check => {
            polyline(
                painter,
                &[
                    pos2(left + 2.0, center.y),
                    pos2(center.x - 1.0, bottom - 3.0),
                    pos2(right - 2.0, top + 3.0),
                ],
                stroke,
            );
        }
    }
}

fn line(painter: &egui::Painter, x1: f32, y1: f32, x2: f32, y2: f32, stroke: Stroke) {
    painter.line_segment([Pos2::new(x1, y1), Pos2::new(x2, y2)], stroke);
}

fn polyline(painter: &egui::Painter, points: &[Pos2], stroke: Stroke) {
    for segment in points.windows(2) {
        painter.line_segment([segment[0], segment[1]], stroke);
    }
}
