use anyhow::Result;
use eframe::egui::{Color32, Context as EguiContext, FontId, TextStyle, Visuals};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub name: String,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    #[serde(default = "default_editor_font_size")]
    pub editor_font_size: f32,
    #[serde(default = "default_preview_font_size")]
    pub preview_font_size: f32,
}

impl ThemeConfig {
    pub fn from_toml(source: &str) -> Result<Self> {
        toml::from_str(source).map_err(|err| anyhow::anyhow!("failed to parse theme TOML: {err}"))
    }

    pub fn default_dark() -> Self {
        Self {
            name: "VelociDark".to_string(),
            background: "#10131a".to_string(),
            foreground: "#f7f7ff".to_string(),
            accent: "#7aa2ff".to_string(),
            editor_font_size: default_editor_font_size(),
            preview_font_size: default_preview_font_size(),
        }
    }

    pub fn apply_to(&self, ctx: &EguiContext) {
        let mut visuals = Visuals::dark();
        if let Some(bg) = parse_hex_color(&self.background) {
            visuals.panel_fill = bg;
            visuals.window_fill = bg;
            visuals.extreme_bg_color = bg;
        }
        if let Some(accent) = parse_hex_color(&self.accent) {
            visuals.selection.bg_fill = accent;
            visuals.hyperlink_color = accent;
            visuals.widgets.active.bg_fill = accent;
        }
        ctx.set_visuals(visuals);

        let mut style = (*ctx.style()).clone();
        let mut text_styles = BTreeMap::new();
        text_styles.insert(
            TextStyle::Heading,
            FontId::proportional(self.preview_font_size + 8.0),
        );
        text_styles.insert(
            TextStyle::Body,
            FontId::proportional(self.preview_font_size),
        );
        text_styles.insert(
            TextStyle::Monospace,
            FontId::monospace(self.editor_font_size),
        );
        text_styles.insert(
            TextStyle::Button,
            FontId::proportional(self.preview_font_size),
        );
        text_styles.insert(
            TextStyle::Small,
            FontId::proportional(self.preview_font_size - 2.0),
        );
        style.text_styles = text_styles;
        ctx.set_style(style);
    }
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self::default_dark()
    }
}

fn default_editor_font_size() -> f32 {
    15.0
}

fn default_preview_font_size() -> f32 {
    16.0
}

fn parse_hex_color(value: &str) -> Option<Color32> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}
