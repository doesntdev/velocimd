#[test]
fn exported_helper_forwards_existing_html_renderer_without_semantic_changes() {
    for source in [
        "",
        "# Heading\n\n**bold** and *italic*",
        "<script>alert(1)</script>\n\n[bad](javascript:alert(1))",
        "| A | B |\n|---|---|\n| 1 | 2 |",
        "```rust\nfn main() {}\n```\n",
        "# café 🦀\r\n",
    ] {
        assert_eq!(
            velocimd::render_markdown(source),
            velocimd::markdown::render_to_html(source)
        );
    }
}
