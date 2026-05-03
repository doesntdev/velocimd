use anyhow::Result;
use eframe::egui::{
    Color32, Context as EguiContext, FontId, Stroke, TextStyle, Visuals, style::ScrollStyle,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Debug, Clone, Copy)]
pub struct DesignTokens {
    pub app_bg: Color32,
    pub chrome_bg: Color32,
    pub panel_bg: Color32,
    pub panel_bg_active: Color32,
    pub hover_bg: Color32,
    pub border: Color32,
    pub border_active: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub success: Color32,
    pub danger: Color32,
}

impl DesignTokens {
    pub fn from_theme(theme: &ThemeConfig) -> Self {
        if theme.name.to_lowercase().contains("light") {
            Self::zed_light()
        } else {
            Self::zed_dark()
        }
    }

    pub fn zed_dark() -> Self {
        Self {
            app_bg: Color32::from_rgb(13, 15, 21),
            chrome_bg: Color32::from_rgb(18, 20, 27),
            panel_bg: Color32::from_rgb(22, 24, 32),
            panel_bg_active: Color32::from_rgb(30, 33, 43),
            hover_bg: Color32::from_rgb(36, 39, 51),
            border: Color32::from_rgb(48, 52, 67),
            border_active: Color32::from_rgb(86, 113, 173),
            text: Color32::from_rgb(221, 225, 234),
            text_muted: Color32::from_rgb(143, 151, 166),
            accent: Color32::from_rgb(122, 162, 255),
            success: Color32::from_rgb(113, 184, 138),
            danger: Color32::from_rgb(235, 111, 146),
        }
    }

    pub fn zed_light() -> Self {
        Self {
            app_bg: Color32::from_rgb(244, 245, 247),
            chrome_bg: Color32::from_rgb(235, 237, 242),
            panel_bg: Color32::from_rgb(250, 250, 252),
            panel_bg_active: Color32::from_rgb(255, 255, 255),
            hover_bg: Color32::from_rgb(226, 231, 241),
            border: Color32::from_rgb(205, 211, 224),
            border_active: Color32::from_rgb(66, 107, 184),
            text: Color32::from_rgb(28, 32, 40),
            text_muted: Color32::from_rgb(98, 107, 124),
            accent: Color32::from_rgb(44, 99, 194),
            success: Color32::from_rgb(40, 138, 82),
            danger: Color32::from_rgb(190, 66, 87),
        }
    }
}

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

    pub fn to_toml(&self) -> Result<String> {
        toml::to_string(self)
            .map_err(|err| anyhow::anyhow!("failed to serialize theme TOML: {err}"))
    }

    pub fn save_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let toml = self.to_toml()?;
        fs::write(path, toml)?;
        Ok(())
    }

    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(path)?;
        Self::from_toml(&content)
    }

    pub fn default_dark() -> Self {
        Self {
            name: "Velocidark".to_string(),
            background: "#0d0f15".to_string(),
            foreground: "#dde1ea".to_string(),
            accent: "#7aa2ff".to_string(),
            editor_font_size: default_editor_font_size(),
            preview_font_size: default_preview_font_size(),
        }
    }

    pub fn default_light() -> Self {
        Self {
            name: "Velocilight".to_string(),
            background: "#f4f5f7".to_string(),
            foreground: "#1c2028".to_string(),
            accent: "#2c63c2".to_string(),
            editor_font_size: default_editor_font_size(),
            preview_font_size: default_preview_font_size(),
        }
    }

    pub fn to_css(&self) -> String {
        let tokens = DesignTokens::from_theme(self);
        let bg = parse_hex_color(&self.background).unwrap_or(tokens.app_bg);
        let fg = parse_hex_color(&self.foreground).unwrap_or(tokens.text);
        let accent = parse_hex_color(&self.accent).unwrap_or(tokens.accent);
        let bg_str = format_color(bg);
        let fg_str = format_color(fg);
        let accent_str = format_color(accent);
        let panel_bg = format_color(tokens.panel_bg);
        let chrome_bg = format_color(tokens.chrome_bg);
        let border = format_color(tokens.border);
        let muted = format_color(tokens.text_muted);
        let preview_size = safe_font_size(self.preview_font_size, default_preview_font_size());
        let editor_size = safe_font_size(self.editor_font_size, default_editor_font_size());
        format!(
            r#":root {{
  --vd-bg: {bg_str};
  --vd-fg: {fg_str};
  --vd-accent: {accent_str};
  --vd-panel-bg: {panel_bg};
  --vd-chrome-bg: {chrome_bg};
  --vd-border: {border};
  --vd-muted: {muted};
  --vd-preview-size: {preview_size}px;
  --vd-editor-size: {editor_size}px;
}}
* {{ box-sizing: border-box; }}
body {{
  background: var(--vd-bg);
  color: var(--vd-fg);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  font-size: var(--vd-preview-size);
  line-height: 1.6;
  margin: 0 auto;
  padding: 2rem;
  max-width: 56rem;
}}
h1, h2, h3, h4, h5, h6 {{
  color: var(--vd-fg);
  border-bottom: 1px solid var(--vd-border);
  padding-bottom: 0.25em;
  margin-top: 1.5em;
}}
h1 {{ font-size: 2em; }}
h2 {{ font-size: 1.5em; }}
h3 {{ font-size: 1.25em; }}
a {{ color: var(--vd-accent); text-decoration: none; }}
a:hover {{ text-decoration: underline; }}
code {{
  background: var(--vd-chrome-bg);
  padding: 0.15em 0.35em;
  border-radius: 3px;
  font-family: ui-monospace, "SF Mono", Menlo, Consolas, monospace;
  font-size: var(--vd-editor-size);
}}
pre {{
  background: var(--vd-chrome-bg);
  padding: 1em;
  border-radius: 6px;
  overflow-x: auto;
  border: 1px solid var(--vd-border);
}}
pre code {{ background: transparent; padding: 0; }}
blockquote {{
  border-left: 4px solid var(--vd-accent);
  margin: 1em 0;
  padding-left: 1em;
  color: var(--vd-muted);
}}
table {{ border-collapse: collapse; width: 100%; margin: 1em 0; }}
th, td {{ border: 1px solid var(--vd-border); padding: 0.5em 0.75em; text-align: left; }}
th {{ background: var(--vd-chrome-bg); }}
hr {{ border: none; border-top: 1px solid var(--vd-border); margin: 2em 0; }}
img {{ max-width: 100%; height: auto; }}
ul, ol {{ padding-left: 1.5em; }}
.mermaid {{ background: var(--vd-panel-bg); padding: 1em; border-radius: 6px; }}
"#
        )
    }

    pub fn apply_to(&self, ctx: &EguiContext) {
        let tokens = DesignTokens::from_theme(self);
        let editor_font_size = safe_font_size(self.editor_font_size, default_editor_font_size());
        let preview_font_size = safe_font_size(self.preview_font_size, default_preview_font_size());
        let mut visuals = if self.name.to_lowercase().contains("light") {
            Visuals::light()
        } else {
            Visuals::dark()
        };

        let bg = parse_hex_color(&self.background).unwrap_or(tokens.app_bg);
        let fg = parse_hex_color(&self.foreground).unwrap_or(tokens.text);
        let accent = parse_hex_color(&self.accent).unwrap_or(tokens.accent);

        visuals.panel_fill = bg;
        visuals.window_fill = tokens.panel_bg;
        visuals.extreme_bg_color = bg;
        visuals.faint_bg_color = tokens.chrome_bg;
        visuals.override_text_color = Some(fg);
        visuals.selection.bg_fill = accent.gamma_multiply(0.32);
        visuals.selection.stroke = Stroke::new(1.0, accent);
        visuals.hyperlink_color = accent;
        visuals.widgets.noninteractive.bg_fill = tokens.panel_bg;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, tokens.border);
        visuals.widgets.inactive.bg_fill = tokens.panel_bg;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.border);
        visuals.widgets.hovered.bg_fill = tokens.hover_bg;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, tokens.border_active);
        visuals.widgets.active.bg_fill = tokens.panel_bg_active;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, accent);
        ctx.set_visuals(visuals);

        let mut style = (*ctx.global_style()).clone();
        style.spacing.item_spacing = eframe::egui::vec2(6.0, 5.0);
        style.spacing.button_padding = eframe::egui::vec2(7.0, 4.0);
        style.spacing.window_margin = eframe::egui::Margin::same(10);
        style.spacing.scroll = ScrollStyle {
            floating: true,
            ..style.spacing.scroll
        };
        let mut text_styles = BTreeMap::new();
        text_styles.insert(
            TextStyle::Heading,
            FontId::proportional(preview_font_size + 8.0),
        );
        text_styles.insert(TextStyle::Body, FontId::proportional(preview_font_size));
        text_styles.insert(TextStyle::Monospace, FontId::monospace(editor_font_size));
        text_styles.insert(TextStyle::Button, FontId::proportional(preview_font_size));
        text_styles.insert(
            TextStyle::Small,
            FontId::proportional(preview_font_size - 2.0),
        );
        style.text_styles = text_styles;
        ctx.set_global_style(style);
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

fn format_color(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
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

fn safe_font_size(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(8.0, 40.0)
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_font_size_rejects_non_finite_values() {
        assert_eq!(safe_font_size(f32::NAN, 15.0), 15.0);
        assert_eq!(safe_font_size(f32::INFINITY, 15.0), 15.0);
    }

    #[test]
    fn safe_font_size_clamps_extreme_values() {
        assert_eq!(safe_font_size(-20.0, 15.0), 8.0);
        assert_eq!(safe_font_size(2000.0, 15.0), 40.0);
    }
}
