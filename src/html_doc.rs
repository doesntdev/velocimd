use crate::markdown::{RenderedDocument, escape_html};
use crate::theme::ThemeConfig;

const MERMAID_CDN: &str = "https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js";

pub fn wrap_html_document(rendered: &RenderedDocument, theme: &ThemeConfig, title: &str) -> String {
    let css = theme.to_css();
    let title_escaped = escape_html(title);
    let mermaid_block = if rendered.has_mermaid {
        let mermaid_theme = if theme.name.to_lowercase().contains("light") {
            "default"
        } else {
            "dark"
        };
        format!(
            "<script src=\"{MERMAID_CDN}\"></script>\n\
             <script>mermaid.initialize({{ startOnLoad: true, theme: '{mermaid_theme}' }});</script>\n"
        )
    } else {
        String::new()
    };

    format!(
        "<!DOCTYPE html>\n\
         <html lang=\"en\">\n\
         <head>\n\
         <meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <title>{title_escaped}</title>\n\
         <style>\n{css}</style>\n\
         </head>\n\
         <body>\n\
         {body}\n\
         {mermaid_block}\
         </body>\n\
         </html>\n",
        body = rendered.body,
    )
}
