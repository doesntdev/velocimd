use once_cell::sync::Lazy;
use pulldown_cmark::{Options, Parser, html};
use regex::Regex;

static SCRIPT_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)<script[^>]*>.*?</script>").unwrap());

static STYLE_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)<style[^>]*>.*?</style>").unwrap());

static ON_ERROR_REGEX: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)on\w+\s*=").unwrap());

pub fn render_to_html(markdown: &str) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_TASKLISTS);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(markdown, options);
    let mut output = String::with_capacity(markdown.len().saturating_mul(2));
    html::push_html(&mut output, parser);

    // Sanitize the HTML to prevent XSS
    let sanitized = SCRIPT_REGEX.replace_all(&output, "");
    let sanitized = STYLE_REGEX.replace_all(&sanitized, "");
    let sanitized = ON_ERROR_REGEX.replace_all(&sanitized, "");

    sanitized.into_owned()
}
