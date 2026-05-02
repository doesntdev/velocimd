use once_cell::sync::Lazy;
use pulldown_cmark::{CowStr, Event, Options, Parser, Tag, TagEnd, html};
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

    let parser = Parser::new_ext(markdown, options).filter_map(sanitize_event);
    let mut output = String::with_capacity(markdown.len().saturating_mul(2));
    html::push_html(&mut output, parser);

    // Sanitize the HTML to prevent XSS
    let sanitized = SCRIPT_REGEX.replace_all(&output, "");
    let sanitized = STYLE_REGEX.replace_all(&sanitized, "");
    let sanitized = ON_ERROR_REGEX.replace_all(&sanitized, "");

    sanitized.into_owned()
}

fn sanitize_event(event: Event<'_>) -> Option<Event<'_>> {
    match event {
        Event::Html(_) | Event::InlineHtml(_) => None,
        Event::Start(Tag::HtmlBlock) | Event::End(TagEnd::HtmlBlock) => None,
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => Some(Event::Start(Tag::Link {
            link_type,
            dest_url: sanitize_url(dest_url),
            title,
            id,
        })),
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => Some(Event::Start(Tag::Image {
            link_type,
            dest_url: sanitize_url(dest_url),
            title,
            id,
        })),
        event => Some(event),
    }
}

fn sanitize_url(url: CowStr<'_>) -> CowStr<'_> {
    let normalized = url
        .as_ref()
        .chars()
        .filter(|character| !character.is_ascii_control() && !character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>();

    if matches!(
        normalized.split(':').next(),
        Some("javascript" | "vbscript" | "data" | "file")
    ) {
        CowStr::from("#")
    } else {
        url
    }
}
