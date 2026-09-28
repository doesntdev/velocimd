use crate::{commands::Command, modes::EditorMode};
use eframe::egui::{
    self, Align2, Color32, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Plus,
    NewFile,
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
    Command,
}

impl Icon {
    pub fn for_command(command: Command) -> Self {
        match command {
            Command::NewTab => Self::NewFile,
            Command::SelectWorkingFolder => Self::Folder,
            Command::OpenFile => Self::File,
            Command::SaveFile => Self::Save,
            Command::SaveFileAs => Self::SaveAs,
            Command::CloseFolderTab => Self::X,
            Command::TogglePalette => Self::Command,
            Command::SetMode(EditorMode::Edit) => Self::Edit,
            Command::SetMode(EditorMode::Preview) => Self::Eye,
            Command::SetMode(EditorMode::Split) => Self::Columns,
            Command::CycleMode => Self::Rotate,
            Command::SwitchThemeLight => Self::Sun,
            Command::SwitchThemeDark => Self::Moon,
        }
    }

    /// One embedded, tintable SVG per action; the URI also keys egui's image cache.
    pub fn source(self) -> egui::ImageSource<'static> {
        match self {
            Self::Plus => egui::include_image!("../assets/icons/toolbar/plus.svg"),
            Self::NewFile => egui::include_image!("../assets/icons/toolbar/file-plus.svg"),
            Self::Folder => egui::include_image!("../assets/icons/toolbar/folder.svg"),
            Self::File => egui::include_image!("../assets/icons/toolbar/file.svg"),
            Self::Save => egui::include_image!("../assets/icons/toolbar/save.svg"),
            Self::SaveAs => egui::include_image!("../assets/icons/toolbar/save-as.svg"),
            Self::X => egui::include_image!("../assets/icons/toolbar/close.svg"),
            Self::Edit => egui::include_image!("../assets/icons/toolbar/edit.svg"),
            Self::Eye => egui::include_image!("../assets/icons/toolbar/preview.svg"),
            Self::Columns => egui::include_image!("../assets/icons/toolbar/split.svg"),
            Self::Rotate => egui::include_image!("../assets/icons/toolbar/cycle.svg"),
            Self::Sun => egui::include_image!("../assets/icons/toolbar/sun.svg"),
            Self::Moon => egui::include_image!("../assets/icons/toolbar/moon.svg"),
            Self::Check => egui::include_image!("../assets/icons/toolbar/check.svg"),
            Self::Command => egui::include_image!("../assets/icons/toolbar/command.svg"),
        }
    }
}

pub fn icon_button(ui: &mut Ui, icon: Icon, active: bool, tooltip: String) -> Response {
    icon_button_sized(ui, icon, active, tooltip, vec2(30.0, 28.0), 18.0)
}

pub fn compact_icon_button(ui: &mut Ui, icon: Icon, tooltip: String) -> Response {
    icon_button_sized(ui, icon, false, tooltip, vec2(20.0, 20.0), 14.0)
}

fn icon_button_sized(
    ui: &mut Ui,
    icon: Icon,
    active: bool,
    tooltip: String,
    size: Vec2,
    icon_size: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, &tooltip)
    });
    let visuals = ui.visuals();
    let highlighted = response.hovered() || response.has_focus();
    let fill = if active {
        visuals.widgets.active.bg_fill
    } else if highlighted {
        visuals.widgets.hovered.bg_fill
    } else {
        Color32::TRANSPARENT
    };
    let stroke_color = if active || response.has_focus() {
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
            ui,
            icon,
            Rect::from_center_size(rect.center(), Vec2::splat(icon_size)),
            if active {
                visuals.selection.stroke.color
            } else if highlighted {
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

/// Paint without stretching the shared square grid. The app installs the existing
/// egui_extras loaders at startup; Image::paint_at caches at physical pixel size.
pub fn paint_icon(ui: &Ui, icon: Icon, rect: Rect, color: Color32) {
    let size = rect.width().min(rect.height());
    if size <= 0.0 {
        return;
    }
    let rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    egui::Image::new(icon.source())
        .tint(color)
        .show_loading_spinner(false)
        .paint_at(ui, rect);
}
