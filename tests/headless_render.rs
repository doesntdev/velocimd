use velocimd::{html_doc, markdown, theme::ThemeConfig};

#[test]
fn render_to_html_with_mermaid_passes_through_diagram_source() {
    let input = "# Plan\n\n```mermaid\ngraph TD\nA --> B\n```\n\nDone.";
    let rendered = markdown::render_to_html_with_mermaid(input);

    assert!(rendered.body.contains("<h1>Plan</h1>"));
    assert!(rendered.body.contains("<pre class=\"mermaid\">"));
    assert!(rendered.body.contains("graph TD"));
    assert!(rendered.body.contains("A --&gt; B"));
    assert!(rendered.body.contains("<p>Done.</p>"));
    assert!(rendered.has_mermaid);
}

#[test]
fn render_to_html_with_mermaid_marks_no_mermaid_when_absent() {
    let rendered = markdown::render_to_html_with_mermaid("# Hello\n\nNo diagrams here.");
    assert!(!rendered.has_mermaid);
    assert!(!rendered.body.contains("class=\"mermaid\""));
}

#[test]
fn render_to_html_with_mermaid_preserves_xss_sanitization() {
    let rendered = markdown::render_to_html_with_mermaid("# Safe\n\n<script>alert(1)</script>");
    assert!(!rendered.body.to_lowercase().contains("<script"));
    assert!(rendered.body.contains("<h1>Safe</h1>"));
}

#[test]
fn escape_html_neutralizes_special_characters() {
    let escaped = markdown::escape_html("<a href=\"x\" onclick='y'>&");
    assert_eq!(
        escaped,
        "&lt;a href=&quot;x&quot; onclick=&#39;y&#39;&gt;&amp;"
    );
}

#[test]
fn theme_to_css_emits_color_variables_for_dark() {
    let css = ThemeConfig::default_dark().to_css();
    assert!(css.contains("--vd-bg: #0d0f15"));
    assert!(css.contains("--vd-fg: #dde1ea"));
    assert!(css.contains("--vd-accent: #7aa2ff"));
    assert!(css.contains("body {"));
    assert!(css.contains("max-width: 56rem"));
}

#[test]
fn theme_to_css_dark_and_light_differ() {
    let dark = ThemeConfig::default_dark().to_css();
    let light = ThemeConfig::default_light().to_css();
    assert_ne!(dark, light);
    assert!(light.contains("--vd-bg: #f4f5f7"));
}

#[test]
fn theme_to_css_rejects_injection_via_hex_color() {
    let mut malicious = ThemeConfig::default_dark();
    malicious.background = String::from("#fff;}body{display:none;}<script>alert(1)</script>");
    let css = malicious.to_css();
    assert!(!css.contains("<script"));
    assert!(!css.contains("display:none"));
    // Falls back to a known-safe parsed color
    assert!(css.contains("--vd-bg: #0d0f15"));
}

#[test]
fn wrap_html_document_includes_doctype_title_and_theme_css() {
    let rendered = markdown::render_to_html_with_mermaid("# Hello");
    let doc = html_doc::wrap_html_document(&rendered, &ThemeConfig::default_dark(), "test");

    assert!(doc.starts_with("<!DOCTYPE html>"));
    assert!(doc.contains("<title>test</title>"));
    assert!(doc.contains("--vd-bg: #0d0f15"));
    assert!(doc.contains("<h1>Hello</h1>"));
    assert!(doc.trim_end().ends_with("</html>"));
}

#[test]
fn wrap_html_document_omits_mermaid_script_when_no_diagrams() {
    let rendered = markdown::render_to_html_with_mermaid("# Plain\n\nNo diagrams.");
    let doc = html_doc::wrap_html_document(&rendered, &ThemeConfig::default_dark(), "plain");
    assert!(!doc.contains("mermaid.min.js"));
    assert!(!doc.contains("mermaid.initialize"));
}

#[test]
fn wrap_html_document_includes_mermaid_script_when_diagrams_present() {
    let rendered =
        markdown::render_to_html_with_mermaid("# With\n\n```mermaid\ngraph TD\nA-->B\n```\n");
    let doc = html_doc::wrap_html_document(&rendered, &ThemeConfig::default_dark(), "doc");
    assert!(doc.contains("mermaid.min.js"));
    assert!(doc.contains("startOnLoad: true"));
    assert!(doc.contains("theme: 'dark'"));
}

#[test]
fn wrap_html_document_picks_light_mermaid_theme_for_velocilight() {
    let rendered = markdown::render_to_html_with_mermaid("```mermaid\ngraph TD\nA-->B\n```\n");
    let doc = html_doc::wrap_html_document(&rendered, &ThemeConfig::default_light(), "doc");
    assert!(doc.contains("theme: 'default'"));
}

#[test]
fn wrap_html_document_escapes_title() {
    let rendered = markdown::render_to_html_with_mermaid("body");
    let doc = html_doc::wrap_html_document(
        &rendered,
        &ThemeConfig::default_dark(),
        "<script>alert(1)</script>",
    );
    assert!(!doc.contains("<title><script>"));
    assert!(doc.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
}
