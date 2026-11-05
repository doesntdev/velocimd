//! Revision-aware preview virtualization. Only parser-delimited top-level blocks
//! are culled. Plain paragraphs retain parsed text and their actual galley;
//! interactive/complex blocks deliberately use CommonMarkViewer::show (which
//! reparses that visible block). Its hidden scrollable API is not used.
use super::{PREVIEW_FRAME_MARGIN, PreviewSyncAnchor, PreviewSyncTarget, scroll_preview_line};
use crate::{mermaid::MermaidRenderCache, theme::DesignTokens};
use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::{Hash, Hasher},
    ops::Range,
    sync::{Arc, Mutex},
};

const MAX_RETAINED_GALLEYS: usize = 64;
const PLAIN_FRAGMENT_BYTES: usize = 8 * 1024;
const REVISION_FRAGMENT_BATCH: usize = 4;
const RETAINED_TEXT_BUDGET: usize = 2 * 1024 * 1024;

#[cfg(test)]
thread_local! { pub(super) static PREVIEW_BLOCK_WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PreviewSegment {
    pub(super) start_line: usize,
    pub(super) line_count: usize,
    pub(super) kind: PreviewSegmentKind,
    // None is the conservative rich/interactive fallback, not a cached AST.
    plain: Option<String>,
    heading: bool,
    embedded_mermaids: Vec<(String, String)>,
    images: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum PreviewSegmentKind {
    Markdown(String),
    Mermaid(String),
}

fn parser_options() -> Options {
    // Match egui_commonmark 0.23's parser options exactly.
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
}

struct ParsedRange {
    range: Range<usize>,
    plain: Option<String>,
    heading: bool,
    mermaid: Option<String>,
    references: Vec<Range<usize>>,
    footnote: bool,
    images: Vec<String>,
}

/// Keep all container descendants together, including loose list paragraphs,
/// tables, quotes, indented code and literal fences. Offset ranges come from
/// the same Markdown parser as the rich renderer, not from blank-line guesses.
pub(super) fn split_preview_segments(markdown: &str) -> Vec<PreviewSegment> {
    let parser = Parser::new_ext(markdown, parser_options());
    let mut events = parser.into_offset_iter();
    let mut ranges = Vec::<ParsedRange>::new();
    let mut current: Option<ParsedRange> = None;
    let mut depth = 0usize;
    while let Some((event, span)) = events.next() {
        if depth == 0 {
            current = Some(ParsedRange {
                range: span.clone(),
                plain: matches!(event, Event::Start(Tag::Paragraph)).then(String::new),
                heading: matches!(event, Event::Start(Tag::Heading { .. })),
                mermaid: match &event {
                    Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info)))
                        if info.trim().eq_ignore_ascii_case("mermaid") =>
                    {
                        Some(String::new())
                    }
                    _ => None,
                },
                references: Vec::new(),
                footnote: false,
                images: Vec::new(),
            });
        }
        let block = current.as_mut().expect("top-level event starts a block");
        block.range.end = block.range.end.max(span.end);
        match &event {
            Event::Start(tag) => {
                if depth > 0 {
                    block.plain = None;
                }
                if let Tag::Link { id, .. } | Tag::Image { id, .. } = tag
                    && let Some(definition) = events.reference_definitions().get(id)
                {
                    block.references.push(definition.span.clone());
                }
                if let Tag::Image { dest_url, .. } = tag {
                    block.images.push(
                        if dest_url.contains("://") || dest_url.starts_with("data:") {
                            dest_url.to_string()
                        } else {
                            format!("file://{dest_url}")
                        },
                    );
                }
                if matches!(tag, Tag::FootnoteDefinition(_)) {
                    block.footnote = true;
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
            }
            Event::Text(text) => {
                if let Some(plain) = &mut block.plain {
                    plain.push_str(text);
                }
                if let Some(source) = &mut block.mermaid {
                    source.push_str(text);
                }
            }
            Event::SoftBreak => {
                if let Some(plain) = &mut block.plain {
                    plain.push(' ');
                }
            }
            Event::FootnoteReference(_) => {
                block.footnote = true;
                block.plain = None;
            }
            _ => {
                block.plain = None;
            }
        }
        if depth == 0 {
            let mut block = current.take().unwrap();
            if block.mermaid.is_some() && !has_closing_fence(&markdown[block.range.clone()]) {
                block.mermaid = None;
            }
            ranges.push(block);
        }
    }

    // Footnote references only parse as such in the presence of their
    // definitions. CommonMarkViewer has no event-input API. Keep the smallest
    // enclosing contextual region intact rather than duplicating definitions
    // after every block (which would visibly render the footnotes repeatedly).
    let footnote_region = ranges
        .iter()
        .position(|range| range.footnote)
        .zip(ranges.iter().rposition(|range| range.footnote));
    let mut segments = Vec::new();
    let mut index = 0;
    let line_starts: Vec<_> = std::iter::once(0)
        .chain(markdown.match_indices('\n').map(|(i, _)| i + 1))
        .collect();
    while index < ranges.len() {
        let end_index = footnote_region
            .filter(|(start, _)| *start == index)
            .map_or(index, |(_, end)| end);
        let first = &ranges[index];
        let end = ranges[end_index].range.end;
        let start_line = line_starts
            .partition_point(|offset| *offset <= first.range.start)
            .saturating_sub(1);
        // Parser ranges commonly include a final newline, which belongs to
        // the last source line, not an extra phantom line.
        let last_byte = if markdown.as_bytes().get(end.saturating_sub(1)) == Some(&b'\n') {
            end.saturating_sub(2)
        } else {
            end.saturating_sub(1)
        };
        let last_line = line_starts
            .partition_point(|offset| *offset <= last_byte)
            .saturating_sub(1);
        let mut embedded_mermaids = Vec::new();
        let kind = if index == end_index
            && let Some(mermaid) = &first.mermaid
        {
            PreviewSegmentKind::Mermaid(mermaid.trim().to_owned())
        } else {
            let mut source = String::new();
            let mut cursor = first.range.start;
            for block in &ranges[index..=end_index] {
                if let Some(mermaid) = &block.mermaid {
                    source.push_str(&markdown[cursor..block.range.start]);
                    // A collision-free marker selected against the source, not
                    // user-authored HTML. Preserve all other HTML as literal text.
                    let mut marker = format!("<velocimd-diagram-{}>", embedded_mermaids.len());
                    while markdown.contains(&marker) {
                        marker.insert(marker.len() - 1, 'x');
                    }
                    source.push_str(&format!("\n\n{marker}\n\n"));
                    embedded_mermaids.push((marker, mermaid.trim().to_owned()));
                    cursor = block.range.end;
                }
            }
            source.push_str(&markdown[cursor..end]);
            let mut refs = ranges[index..=end_index]
                .iter()
                .flat_map(|range| range.references.iter().cloned())
                .collect::<Vec<_>>();
            refs.sort_by_key(|range| range.start);
            refs.dedup();
            for definition in refs {
                if definition.start < first.range.start || definition.end > end {
                    source.push_str("\n\n");
                    source.push_str(&markdown[definition]);
                    source.push('\n');
                }
            }
            PreviewSegmentKind::Markdown(source)
        };
        segments.push(PreviewSegment {
            start_line,
            line_count: last_line.saturating_sub(start_line) + 1,
            kind,
            plain: if index == end_index {
                first.plain.clone()
            } else {
                None
            },
            heading: first.heading,
            embedded_mermaids,
            images: ranges[index..=end_index]
                .iter()
                .flat_map(|range| range.images.iter().cloned())
                .collect(),
        });
        index = end_index + 1;
    }
    if segments.is_empty() {
        segments.push(PreviewSegment {
            start_line: 0,
            line_count: line_starts.len(),
            kind: PreviewSegmentKind::Markdown(String::new()),
            plain: Some(String::new()),
            heading: false,
            embedded_mermaids: Vec::new(),
            images: Vec::new(),
        });
    }
    segments
}

// This only validates closure of an already parser-confirmed top-level fence.
// It is not used to discover or split blocks.
fn has_closing_fence(source: &str) -> bool {
    let mut lines = source.lines();
    let Some(opening) = lines.next() else {
        return false;
    };
    let opening = opening.trim_start();
    let Some(marker) = opening.chars().next() else {
        return false;
    };
    let length = opening
        .chars()
        .take_while(|character| *character == marker)
        .count();
    let Some(closing) = lines.next_back() else {
        return false;
    };
    let indentation = closing.len() - closing.trim_start_matches(' ').len();
    let closing = closing.trim_start_matches(' ');
    indentation <= 3
        && closing
            .chars()
            .take_while(|character| *character == marker)
            .count()
            >= length
        && closing.trim_start_matches(marker).trim().is_empty()
}

#[derive(Clone, PartialEq)]
struct LayoutKey {
    width: f32,
    scale: f32,
    style: Arc<egui::Style>,
    fonts: egui::FontDefinitions,
    // Mermaid uses these tokens, not just the egui style.
    colors: [egui::Color32; 13],
}

impl LayoutKey {
    fn new(ui: &egui::Ui, tokens: DesignTokens) -> Self {
        Self {
            width: ui.available_width().max(1.0),
            scale: ui.ctx().pixels_per_point(),
            style: Arc::clone(ui.style()),
            fonts: ui.fonts(|fonts| fonts.definitions().clone()),
            colors: [
                tokens.app_bg,
                tokens.chrome_bg,
                tokens.panel_bg,
                tokens.panel_bg_active,
                tokens.hover_bg,
                tokens.border,
                tokens.border_active,
                tokens.text,
                tokens.text_muted,
                tokens.accent,
                tokens.success,
                tokens.danger,
                ui.visuals().text_color(),
            ],
        }
    }
}

struct PlainFragment {
    start: usize,
    y: f32,
    advance: f32,
    galley: Arc<egui::Galley>,
}

#[derive(Default)]
struct PlainFragments {
    rows: Vec<PlainFragment>,
    end: usize,
    height: f32,
    complete: bool,
    // Cold layout is allowed to establish the extent once. Revisions rebuild
    // only a bounded batch; no delayed whole-paragraph UI stall is scheduled.
    revision: bool,
    frame: Option<u64>,
}

impl PlainFragments {
    fn revise(&mut self, text: &str) {
        let keep = self
            .rows
            .iter()
            .take_while(|fragment| {
                text.get(fragment.start..)
                    .is_some_and(|rest| rest.starts_with(&fragment.galley.job.text))
            })
            .count();
        // The final old row can absorb appended text. Restart at its fragment,
        // never introduce a hard line/paragraph break at a cache boundary.
        let keep = keep.min(self.rows.len().saturating_sub(1));
        self.rows.truncate(keep);
        self.end = self
            .rows
            .last()
            .map_or(0, |row| row.start + row.galley.job.text.len());
        self.height = self.rows.last().map_or(0.0, |row| row.y + row.advance);
        self.complete = self.end == text.len();
        self.revision = true;
        self.frame = None;
    }

    fn bytes(&self) -> usize {
        self.rows.iter().map(|row| row.galley.job.text.len()).sum()
    }

    fn layout(&mut self, ui: &egui::Ui, text: &str, width: f32) -> usize {
        if self.frame == Some(ui.ctx().cumulative_frame_nr()) {
            return 0;
        }
        self.frame = Some(ui.ctx().cumulative_frame_nr());
        let limit = if self.revision {
            REVISION_FRAGMENT_BATCH
        } else {
            // Every iteration consumes at least one actual UTF-8 byte. The
            // finite input length is also a defensive cold-iteration bound.
            text.len().saturating_sub(self.end)
        };
        let mut work = 0;
        for _ in 0..limit {
            if self.end == text.len() {
                self.complete = true;
                break;
            }
            let rest = &text[self.end..];
            let mut end = rest.len().min(PLAIN_FRAGMENT_BYTES);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            let layout = |value: &str| {
                ui.fonts_mut(|fonts| {
                    fonts.layout_job(egui::text::LayoutJob::simple(
                        value.to_owned(),
                        egui::TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                        width,
                    ))
                })
            };
            let probe = layout(&rest[..end]);
            work += end;
            let (galley, advance) = if end < rest.len() && probe.rows.len() > 2 {
                // Commit only complete visual rows. Keep a lookahead row so
                // word wrapping, soft breaks, Unicode and whitespace are not
                // altered by the fragment boundary.
                let count = probe.rows.len() - 2;
                let bytes = probe.rows[..count]
                    .iter()
                    .map(|row| {
                        // egui deliberately omits a newline from glyphs and keeps
                        // it in PlacedRow metadata. Decoded Markdown entities may
                        // contain many such empty rows inside a single paragraph.
                        row.glyphs
                            .iter()
                            .map(|glyph| glyph.chr.len_utf8())
                            .sum::<usize>()
                            + usize::from(row.ends_with_newline)
                    })
                    .sum::<usize>();
                if bytes == 0 || bytes > end || !rest.is_char_boundary(bytes) {
                    // Never loop without progress or invent a one-byte UTF-8
                    // boundary. If the engine cannot expose a consuming prefix,
                    // retain its canonical layout of the remaining text intact.
                    end = rest.len();
                    let galley = layout(rest);
                    work += end;
                    let advance = galley.size().y;
                    (galley, advance)
                } else {
                    end = bytes;
                    let galley = layout(&rest[..end]);
                    work += end;
                    (galley, probe.rows[count].pos.y)
                }
            } else {
                let advance = probe.size().y;
                (probe, advance)
            };
            self.rows.push(PlainFragment {
                start: self.end,
                y: self.height,
                advance,
                galley,
            });
            self.end += end;
            self.height += advance;
        }
        self.complete = self.end == text.len();
        work
    }
}

struct CachedBlock {
    segment: PreviewSegment,
    fingerprint: u64,
    id: u64,
    galley: Option<Arc<egui::Galley>>,
    fragments: Option<PlainFragments>,
    height: f32,
    measured: bool,
    content_inset: f32,
}

/// Fenwick sums allow both block lookup and height corrections without scanning
/// the full document in a warm frame. There is one bounded active document.
#[derive(Default)]
struct Heights(Vec<f32>);
impl Heights {
    fn from_blocks(blocks: &[CachedBlock]) -> Self {
        let mut result = Self(vec![0.0; blocks.len() + 1]);
        for (i, block) in blocks.iter().enumerate() {
            result.add(i, block.height);
        }
        result
    }
    fn add(&mut self, index: usize, delta: f32) {
        let mut i = index + 1;
        while i < self.0.len() {
            self.0[i] += delta;
            // Advance directly to the next one-based Fenwick parent.
            i = (i | (i - 1)) + 1;
        }
    }
    fn prefix(&self, end: usize) -> f32 {
        let mut i = end;
        let mut sum = 0.0;
        while i > 0 {
            sum += self.0[i];
            i &= i - 1;
        }
        sum
    }
    fn block_at(&self, y: f32) -> usize {
        let count = self.0.len().saturating_sub(1);
        let mut index = 0;
        let mut sum = 0.0;
        let mut bit = count.next_power_of_two();
        while bit > 0 {
            let next = index + bit;
            if next <= count && sum + self.0[next] <= y {
                index = next;
                sum += self.0[next];
            }
            bit >>= 1;
        }
        index.min(count.saturating_sub(1))
    }
}

#[derive(Clone, Copy, PartialEq)]
struct SyncGeometry {
    target: PreviewSyncTarget,
    source_y: f32,
    extent: f32,
    origin_y: f32,
    clip: egui::Rect,
}

#[derive(Default)]
pub struct PreviewRenderer {
    cache: CommonMarkCache,
    mermaid: Arc<Mutex<MermaidRenderCache>>,
    identity: Option<(u64, u64)>,
    standalone: Option<(u64, usize)>,
    blocks: Vec<CachedBlock>,
    heights: Heights,
    layout_key: Option<LayoutKey>,
    next_id: u64,
    retained: VecDeque<usize>,
    retained_bytes: usize,
    refinement: usize,
    refinement_frame: Option<u64>,
    sync_geometry: Option<SyncGeometry>,
    pending_images: VecDeque<usize>,
    pending_image_set: HashSet<usize>,
    #[cfg(test)]
    parses: usize,
    #[cfg(test)]
    layout_bytes: usize,
    #[cfg(test)]
    layouts: usize,
    #[cfg(test)]
    rich_parses: usize,
}

fn hash(value: impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

impl PreviewRenderer {
    pub fn name(&self) -> &'static str {
        "egui-commonmark"
    }
    pub fn supports_code_highlighting(&self) -> bool {
        true
    }
    pub fn supports_local_images(&self) -> bool {
        true
    }

    /// Compatibility API for consumers without revisions. Hashing is confined
    /// to this API. The editor's revision-aware path never hashes warm source.
    pub fn show(&mut self, ui: &mut egui::Ui, markdown: &str, tokens: DesignTokens) {
        let identity = (hash(markdown), markdown.len());
        if self.identity.is_some() || self.standalone != Some(identity) {
            self.update(markdown, self.identity.is_none());
            self.standalone = Some(identity);
            self.identity = None;
        }
        self.show_cached(ui, tokens, None);
    }

    pub(super) fn show_document(
        &mut self,
        ui: &mut egui::Ui,
        document_id: u64,
        revision: u64,
        markdown: &str,
        tokens: DesignTokens,
        sync_target: Option<PreviewSyncTarget>,
    ) {
        if self.identity != Some((document_id, revision)) {
            self.update(
                markdown,
                self.identity.is_some_and(|(id, _)| id == document_id),
            );
            self.identity = Some((document_id, revision));
            self.standalone = None;
        }
        self.show_cached(ui, tokens, sync_target);
    }

    fn update(&mut self, markdown: &str, reuse: bool) {
        #[cfg(test)]
        {
            self.parses += 1;
        }
        let mut old = HashMap::<u64, VecDeque<CachedBlock>>::new();
        for block in self.blocks.drain(..) {
            if reuse {
                old.entry(block.fingerprint).or_default().push_back(block);
            }
        }
        self.retained.clear();
        self.retained_bytes = 0;
        self.refinement = 0;
        self.pending_images.clear();
        self.pending_image_set.clear();
        for segment in split_preview_segments(markdown) {
            let fingerprint = hash((&segment.kind, &segment.embedded_mermaids));
            let matched = old.get_mut(&fingerprint).and_then(|candidates| {
                let index = candidates.iter().position(|block| {
                    block.segment.kind == segment.kind
                        && block.segment.embedded_mermaids == segment.embedded_mermaids
                })?;
                candidates.remove(index)
            });
            let changed_plain = if matched.is_none()
                && segment
                    .plain
                    .as_ref()
                    .is_some_and(|text| text.len() > PLAIN_FRAGMENT_BYTES)
            {
                old.values_mut().find_map(|candidates| {
                    let index = candidates.iter().position(|block| {
                        block.segment.start_line == segment.start_line && block.fragments.is_some()
                    })?;
                    candidates.remove(index)
                })
            } else {
                None
            };
            let block = if let Some(mut block) = changed_plain {
                block
                    .fragments
                    .as_mut()
                    .unwrap()
                    .revise(segment.plain.as_ref().unwrap());
                block.segment = segment;
                block.fingerprint = fingerprint;
                block.measured = false;
                block
            } else if let Some(mut block) = matched {
                block.segment.start_line = segment.start_line;
                block.segment.line_count = segment.line_count;
                block
            } else {
                self.next_id = self.next_id.wrapping_add(1);
                CachedBlock {
                    segment,
                    fingerprint,
                    id: self.next_id,
                    galley: None,
                    fragments: None,
                    height: 32.0,
                    measured: false,
                    content_inset: 0.0,
                }
            };
            let bytes = block
                .galley
                .as_ref()
                .map_or(0, |galley| galley.job.text.len())
                + block.fragments.as_ref().map_or(0, PlainFragments::bytes);
            if bytes > 0 {
                self.retained.push_back(self.blocks.len());
                self.retained_bytes += bytes;
            }
            if !block.segment.images.is_empty() {
                self.pending_images.push_back(self.blocks.len());
                self.pending_image_set.insert(self.blocks.len());
            }
            self.blocks.push(block);
        }
        // Re-estimate new blocks using the last known layout environment.
        if let Some(key) = &self.layout_key {
            for block in &mut self.blocks {
                if !block.measured {
                    block.height = estimated_height(block, key);
                }
            }
        }
        self.heights = Heights::from_blocks(&self.blocks);
    }

    fn show_cached(
        &mut self,
        ui: &mut egui::Ui,
        tokens: DesignTokens,
        sync_target: Option<PreviewSyncTarget>,
    ) {
        ui.style_mut().url_in_tooltip = true;
        let min_size =
            ui.available_size() - egui::Vec2::splat(f32::from(PREVIEW_FRAME_MARGIN) * 2.0);
        egui::Frame::new()
            .fill(ui.visuals().widgets.noninteractive.bg_fill)
            .stroke(egui::Stroke::new(1.0_f32, tokens.border))
            .corner_radius(6)
            .inner_margin(egui::Margin::same(PREVIEW_FRAME_MARGIN))
            .show(ui, |ui| {
                ui.set_min_size(min_size.max(egui::Vec2::ZERO));
                let key = LayoutKey::new(ui, tokens);
                if self.layout_key.as_ref() != Some(&key) {
                    for block in &mut self.blocks {
                        block.galley = None;
                        block.fragments = None;
                        block.measured = false;
                        block.height = estimated_height(block, &key);
                    }
                    self.retained.clear();
                    self.retained_bytes = 0;
                    self.refinement = 0;
                    self.heights = Heights::from_blocks(&self.blocks);
                    self.layout_key = Some(key);
                }
                let mut origin = ui.cursor().min;
                let width = ui.available_width().max(1.0);
                let clip = ui.clip_rect();
                let overscan = clip.height().clamp(100.0, 800.0) * 0.5;
                let anchor_index = self.heights.block_at((clip.top() - origin.y).max(0.0));
                let anchor_prefix = self.heights.prefix(anchor_index);
                // Refine a finite, fixed-size batch per frame. Estimates must
                // eventually become a real extent even if the user jumps over
                // unvisited blocks. This is not a full-document warm layout.
                let mut rendered = Vec::new();
                let refine = self.refinement_frame != Some(ui.ctx().cumulative_frame_nr());
                self.refinement_frame = Some(ui.ctx().cumulative_frame_nr());
                for _ in 0..if refine {
                    self.pending_images.len().min(2)
                } else {
                    0
                } {
                    let index = self.pending_images.pop_front().unwrap();
                    self.pending_image_set.remove(&index);
                    let y = origin.y + self.heights.prefix(index);
                    self.measure_block(ui, index, origin.x, y, width, tokens);
                    // Loaders request repaint when pending bytes/images arrive.
                    // Drain ready entries in finite batches, not a timer loop.
                    if !self.pending_image_set.contains(&index) && !self.pending_images.is_empty() {
                        ui.ctx().request_repaint();
                    }
                }
                for _ in 0..if refine { 4 } else { 0 } {
                    while self.refinement < self.blocks.len()
                        && self.blocks[self.refinement].measured
                    {
                        self.refinement += 1;
                    }
                    if self.refinement == self.blocks.len() {
                        break;
                    }
                    let index = self.refinement;
                    let y = origin.y + self.heights.prefix(index);
                    self.measure_block(ui, index, origin.x, y, width, tokens);
                    if self.blocks[index].measured {
                        self.refinement += 1;
                    } else {
                        break;
                    }
                }
                if self.refinement < self.blocks.len() {
                    ui.ctx().request_repaint();
                }
                // Offscreen refinement changes the document coordinate prefix,
                // not the user's source anchor. Paint with that same correction
                // now and apply the matching ScrollArea offset for the next frame.
                // Otherwise every finite refinement batch needs a discard pass.
                let preserving_anchor = sync_target.is_none()
                    || self.sync_geometry.is_some_and(|previous| {
                        Some(previous.target) == sync_target && previous.clip == clip
                    });
                let correction = self.heights.prefix(anchor_index) - anchor_prefix;
                if preserving_anchor && correction.abs() > 0.25 {
                    origin.y -= correction;
                    ui.scroll_with_delta_animation(
                        egui::vec2(0.0, -correction),
                        egui::style::ScrollAnimation::none(),
                    );
                }
                let visible_top = (clip.top() - origin.y - overscan).max(0.0);
                let visible_end = clip.bottom() - origin.y + overscan;
                let target_index = sync_target.map(|target| {
                    self.blocks
                        .partition_point(|block| block.segment.start_line <= target.line)
                        .saturating_sub(1)
                });
                // A source jump must measure its destination even if it has never
                // been visible, before asking the enclosing ScrollArea to jump.
                let target_render_y = target_index.map(|index| {
                    let y = origin.y + self.heights.prefix(index);
                    if !rendered.contains(&index) {
                        rendered.push(index);
                        self.render_block(ui, index, origin.x, y, width, tokens);
                    }
                    y
                });
                let mut index = self.heights.block_at(visible_top);
                while index < self.blocks.len() {
                    let y = self.heights.prefix(index);
                    if y > visible_end {
                        break;
                    }
                    if !rendered.contains(&index) {
                        self.render_block(ui, index, origin.x, origin.y + y, width, tokens);
                    }
                    index += 1;
                }
                if let (Some(index), Some(target), Some(render_y)) =
                    (target_index, sync_target, target_render_y)
                {
                    let y = origin.y + self.heights.prefix(index);
                    let block = &self.blocks[index];
                    let lines = block.segment.line_count.max(1);
                    let line_height = (block
                        .height
                        .max(ui.text_style_height(&egui::TextStyle::Body))
                        / lines as f32)
                        .max(1.0);
                    let line_y = render_y
                        + block.content_inset
                        + line_height
                            * target
                                .line
                                .saturating_sub(block.segment.start_line)
                                .min(lines - 1) as f32;
                    // ScrollArea adds item spacing for Min alignment; Center
                    // aligns the center of the helper's one-point rectangle.
                    // The clipping rectangle is expanded by up to the visual
                    // clip margin, then intersected with ancestor clips. Its
                    // top is NOT ScrollArea's physical inner top (in Split it
                    // can be only partly expanded). Check the feasible band,
                    // not an invented exact clip-to-viewport translation.
                    let expected_y = match target.anchor {
                        PreviewSyncAnchor::Top => clip.top() + ui.spacing().item_spacing.y,
                        PreviewSyncAnchor::ScreenY(y) => y.clamp(clip.top(), clip.bottom()) - 0.5,
                    };
                    let expected_end = expected_y + ui.visuals().clip_rect_margin;
                    let geometry = SyncGeometry {
                        target,
                        source_y: y - origin.y + line_y - render_y,
                        extent: self.heights.prefix(self.blocks.len()),
                        origin_y: origin.y,
                        clip,
                    };
                    // An unchanged origin after the identical request is an
                    // observed clamp, not an ever-pending source jump. Require
                    // the requested point to be beyond the feasible extent too.
                    // Extent estimates for unrelated offscreen blocks may
                    // change for many frames. Clamp convergence depends on
                    // the actual target point and its remaining tail, not the
                    // absolute document origin/extent staying unchanged.
                    let stationary = self.sync_geometry.is_some_and(|previous| {
                        // New offscreen measurements can shift the point by
                        // exactly their extent correction before ScrollArea
                        // reapplies its clamp. Account for that measured delta,
                        // not an arbitrary alignment tolerance or retry limit.
                        previous.target == target
                            && previous.clip == clip
                            && (previous.source_y + previous.origin_y - line_y).abs()
                                <= (previous.extent - geometry.extent).abs() + 0.25
                    });
                    let clamped = stationary
                        && ((line_y > expected_y
                            && geometry.extent - geometry.source_y < clip.bottom() - expected_y
                            && origin.y + geometry.extent <= clip.bottom()
                            && line_y >= clip.top())
                            || (line_y < expected_y && origin.y >= clip.top()));
                    let moved = (render_y - y).abs() > 0.25;
                    let needs_scroll =
                        (line_y < expected_y - 1.0 || line_y > expected_end + 1.0) && !clamped;
                    if needs_scroll || moved {
                        scroll_preview_line(
                            ui,
                            target,
                            block.segment.start_line,
                            block.segment.line_count,
                            y + block.content_inset,
                            y + block.height,
                        );
                        ui.ctx()
                            .request_discard("preview source target moved during measurement");
                    }
                    self.sync_geometry = Some(geometry);
                }
                if sync_target.is_none() {
                    self.sync_geometry = None;
                }
                let total = self.heights.prefix(self.blocks.len());
                ui.allocate_space(egui::vec2(width, total));
                if clip.top() - origin.y > total {
                    // ScrollArea clamps after the closure. Re-run this egui
                    // pass against that clamp instead of displaying one blank
                    // frame after a large edit or width-dependent shrink.
                    ui.ctx()
                        .request_discard("preview extent shrank past its viewport");
                }
                // Only actual measurements change the estimate. They may change
                // the scroll clamp, so request a convergence frame, never a timer.
                self.trim_retained();
            });
    }

    fn measure_block(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        x: f32,
        y: f32,
        width: f32,
        tokens: DesignTokens,
    ) {
        // Measurement may happen away from the viewport. Never emit stale
        // painted/interactive widgets before the anchor correction is known.
        let mut measurement =
            ui.new_child(egui::UiBuilder::new().id_salt(("preview_measure", index)));
        measurement.set_clip_rect(egui::Rect::NOTHING);
        measurement.set_invisible();
        self.render_block(&mut measurement, index, x, y, width, tokens);
    }

    fn render_block(
        &mut self,
        ui: &mut egui::Ui,
        index: usize,
        x: f32,
        y: f32,
        width: f32,
        tokens: DesignTokens,
    ) -> f32 {
        #[cfg(test)]
        PREVIEW_BLOCK_WORK.with(|work| work.set(work.get() + 1));
        let block = &mut self.blocks[index];
        // The rich renderer inserts a body-height newline before headings.
        // A source anchor denotes the heading, not that synthetic spacer.
        block.content_inset = if block.segment.heading {
            ui.text_style_height(&egui::TextStyle::Body)
        } else {
            0.0
        };
        let mut child = ui.new_child(
            egui::UiBuilder::new()
                // Refinement inserts/removes sibling UIs. Keep live widget IDs
                // bound to this frame's parent and block, not sibling position.
                .id(ui.unique_id().with(("preview_block", block.id)))
                .max_rect(egui::Rect::from_min_size(
                    egui::pos2(x, y),
                    egui::vec2(width, 0.0),
                ))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_min_width(width);
        match &block.segment.kind {
            PreviewSegmentKind::Mermaid(source) => {
                self.mermaid
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .render(&mut child, source, tokens);
            }
            PreviewSegmentKind::Markdown(source) => {
                if let Some(text) = &block.segment.plain {
                    if !text.is_empty() {
                        if text.len() > PLAIN_FRAGMENT_BYTES {
                            let fragments =
                                block.fragments.get_or_insert_with(PlainFragments::default);
                            let before = fragments.bytes();
                            let work = fragments.layout(&child, text, width);
                            self.retained_bytes += fragments.bytes() - before;
                            #[cfg(test)]
                            {
                                self.layout_bytes += work;
                            }
                            #[cfg(not(test))]
                            let _ = work;
                            let extent = if fragments.complete {
                                fragments.height
                            } else {
                                // Only unfinished suffix is estimated; completed
                                // rows retain their exact geometry and galleys.
                                fragments.height * text.len() as f32 / fragments.end.max(1) as f32
                            };
                            child.set_min_height(extent);
                            let fragment_clip = child.clip_rect();
                            for fragment in fragments.rows.iter().skip_while(|fragment| {
                                y + fragment.y + fragment.advance < fragment_clip.top()
                            }) {
                                if y + fragment.y > child.clip_rect().bottom() {
                                    break;
                                }
                                child.scope_builder(
                                    egui::UiBuilder::new().id_salt(fragment.start).max_rect(
                                        egui::Rect::from_min_size(
                                            egui::pos2(x, y + fragment.y),
                                            egui::vec2(width, fragment.advance),
                                        ),
                                    ),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(Arc::clone(&fragment.galley))
                                                .selectable(true),
                                        );
                                    },
                                );
                            }
                            if !fragments.complete {
                                ui.ctx().request_repaint();
                            }
                        } else {
                            if block.galley.is_none() {
                                let galley = child.fonts_mut(|fonts| {
                                    fonts.layout_job(egui::text::LayoutJob::simple(
                                        text.clone(),
                                        egui::TextStyle::Body.resolve(child.style()),
                                        child.visuals().text_color(),
                                        width,
                                    ))
                                });
                                self.retained_bytes += galley.job.text.len();
                                block.galley = Some(galley);
                                #[cfg(test)]
                                {
                                    self.layouts += 1;
                                }
                            }
                            child.add(
                                egui::Label::new(Arc::clone(block.galley.as_ref().unwrap()))
                                    .selectable(true),
                            );
                        }
                        if let Some(position) = self.retained.iter().position(|old| *old == index) {
                            self.retained.remove(position);
                        }
                        self.retained.push_back(index);
                    }
                } else {
                    #[cfg(test)]
                    {
                        self.rich_parses += 1;
                    }
                    if block.segment.embedded_mermaids.is_empty() {
                        CommonMarkViewer::new().show(&mut child, &mut self.cache, source);
                    } else {
                        // The contextual footnote fallback may enclose diagrams.
                        // Replace only parser-confirmed top-level Mermaid ranges;
                        // nested/literal fences remain ordinary Markdown code.
                        let diagrams = block.segment.embedded_mermaids.clone();
                        let mermaid = Arc::clone(&self.mermaid);
                        let render = move |ui: &mut egui::Ui, html: &str| {
                            if let Some((_, source)) =
                                diagrams.iter().find(|(marker, _)| marker == html.trim())
                            {
                                mermaid
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                                    .render(ui, source, tokens);
                            } else {
                                ui.label(html);
                            }
                        };
                        CommonMarkViewer::new().render_html_fn(Some(&render)).show(
                            &mut child,
                            &mut self.cache,
                            source,
                        );
                    }
                }
            }
        }
        if block.segment.images.iter().any(|uri| {
            matches!(
                ui.ctx()
                    .try_load_image(uri, egui::load::SizeHint::default()),
                Ok(egui::load::ImagePoll::Pending { .. })
            )
        }) && !self.pending_image_set.contains(&index)
        {
            self.pending_images.push_back(index);
            self.pending_image_set.insert(index);
        }
        let height = child.min_rect().height().max(0.0) + ui.spacing().item_spacing.y;
        let delta = height - block.height;
        if !block.measured || delta.abs() > 0.25 {
            block.height = height;
            block.measured = block
                .fragments
                .as_ref()
                .is_none_or(|fragments| fragments.complete);
            self.heights.add(index, delta);
            ui.ctx().request_repaint();
        }
        height
    }

    fn trim_retained(&mut self) {
        // Always allow one oversized paragraph: evicting the currently visible
        // 1 MiB galley every frame would defeat retention. All other entries are
        // LRU bounded by count and source bytes, not by document history.
        while self.retained.len() > 1
            && (self.retained.len() > MAX_RETAINED_GALLEYS
                || self.retained_bytes > RETAINED_TEXT_BUDGET)
        {
            if let Some(index) = self.retained.pop_front() {
                let block = &mut self.blocks[index];
                if let Some(galley) = block.galley.take() {
                    self.retained_bytes -= galley.job.text.len();
                }
                if let Some(fragments) = block.fragments.take() {
                    self.retained_bytes -= fragments.bytes();
                }
            }
        }
    }
}

fn estimated_height(block: &CachedBlock, key: &LayoutKey) -> f32 {
    let body = egui::TextStyle::Body.resolve(&key.style).size;
    let source_len = match &block.segment.kind {
        PreviewSegmentKind::Markdown(source) | PreviewSegmentKind::Mermaid(source) => source.len(),
    };
    let wrapped = (source_len as f32 * body * 0.48 / key.width)
        .ceil()
        .max(1.0);
    (wrapped * (body + 3.0) + key.style.spacing.item_spacing.y).max(body + 3.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::PreviewSyncAnchor;

    fn input(width: f32) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 480.0),
            )),
            ..Default::default()
        }
    }

    fn frame(
        ctx: &egui::Context,
        renderer: &mut PreviewRenderer,
        source: &str,
        revision: u64,
        width: f32,
        target: Option<PreviewSyncTarget>,
        offset: Option<f32>,
    ) -> (egui::FullOutput, f32, f32) {
        let mut scroll = (0.0, 0.0);
        let output = ctx.run_ui(input(width), |ui| {
            let mut area = egui::ScrollArea::vertical()
                .id_salt("preview-test")
                .auto_shrink([false, false]);
            if let Some(offset) = offset {
                area = area.vertical_scroll_offset(offset);
            }
            let output = area.show(ui, |ui| {
                renderer.show_document(ui, 1, revision, source, DesignTokens::zed_dark(), target)
            });
            scroll = (output.state.offset.y, output.content_size.y);
        });
        (output, scroll.0, scroll.1)
    }

    fn text_position(output: &egui::FullOutput, needle: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text)
                if text.galley.job.text == needle
                    && shape
                        .clip_rect
                        .contains(text.pos + text.galley.size() * 0.5) =>
            {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
    }

    fn click(
        ctx: &egui::Context,
        renderer: &mut PreviewRenderer,
        source: &str,
        position: egui::Pos2,
    ) -> egui::FullOutput {
        let mut output = None;
        for pressed in [true, false] {
            let mut raw = input(640.0);
            raw.events = vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ];
            output = Some(ctx.run_ui(raw, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("preview-test")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        renderer.show_document(ui, 1, 1, source, DesignTokens::zed_dark(), None);
                    });
            }));
        }
        output.unwrap()
    }

    #[test]
    fn whole_preview_pane_clamped_jump_converges_without_discard_or_idle_repaint() {
        use crate::{app_state::AppState, document::Document, ui::VelocimdApp};
        for (line, anchor) in [
            (780, PreviewSyncAnchor::Top),
            (798, PreviewSyncAnchor::Top),
            (780, PreviewSyncAnchor::ScreenY(240.0)),
            (800, PreviewSyncAnchor::ScreenY(776.0)),
        ] {
            let ctx = egui::Context::default();
            let mut state = AppState::fresh();
            state.theme = crate::theme::ThemeConfig::default_dark();
            let source = (0..200)
                .map(|i| {
                    format!(
                        "## SECTION {i:04}\n\n{}\n\n",
                        "Clear ideas in a distant paragraph. ".repeat(14)
                    )
                })
                .collect::<String>()
                + "## END";
            state.documents = vec![Document::scratch("Synthetic.md", &source)];
            let temp = tempfile::tempdir().unwrap();
            let session = temp.path().join("state.json");
            state.save_to(&session).unwrap();
            let mut app = VelocimdApp::from_state(AppState::load_from(&session).unwrap());
            app.state.theme.apply_to(&ctx);
            app.preview_sync_line = line;
            app.preview_sync_anchor = anchor;
            let mut last = None;
            let mut late_discards = 0;
            for number in 0..120 {
                let output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(550.0, 800.0),
                        )),
                        ..Default::default()
                    },
                    |root| {
                        egui::CentralPanel::default()
                            .show_inside(root, |ui| app.preview_pane(ui, true));
                    },
                );
                if number >= 115 && output.platform_output.requested_discard() {
                    late_discards += 1;
                }
                last = Some(output);
            }
            let output = last.unwrap();
            assert_eq!(
                late_discards, 0,
                "stable clamped target must stop discarding: {:?}",
                output.platform_output.request_discard_reasons
            );
            assert!(
                text_position(&output, if line == 780 { "SECTION 0195" } else { "END" }).is_some(),
                "requested source context must actually be painted"
            );
            assert!(
                output.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                    > std::time::Duration::from_secs(1),
                "finite refinement must become idle"
            );
            app.state.documents.clear();
        }
    }

    #[test]
    fn mebibyte_append_reuses_unchanged_layout_prefix() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let mut source =
            "A calm line of plain text for realistic editing and wrapping. More words here.\n"
                .repeat(14_000);
        frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        let prefix = Arc::clone(&renderer.blocks[0].fragments.as_ref().unwrap().rows[0].galley);
        let before = renderer.layout_bytes;
        source.push('x');
        frame(&ctx, &mut renderer, &source, 2, 640.0, None, None);
        assert!(
            Arc::ptr_eq(
                &renderer.blocks[0].fragments.as_ref().unwrap().rows[0].galley,
                &prefix
            ),
            "one-byte append must retain lossless unchanged layout fragments, not relayout the whole megabyte"
        );
        assert!(
            renderer.layout_bytes - before <= 2 * PLAIN_FRAGMENT_BYTES * REVISION_FRAGMENT_BATCH
        );
        assert!(renderer.blocks[0].fragments.as_ref().unwrap().complete);
    }

    #[test]
    fn whole_split_editor_end_jump_has_bounded_passes_during_refinement() {
        use crate::{app_state::AppState, document::Document, ui::VelocimdApp};
        let ctx = egui::Context::default();
        let mut state = AppState::fresh();
        state.theme = crate::theme::ThemeConfig::default_dark();
        let begin = "# NOTE 000 BEGIN\n\n";
        let block = (0..2200)
            .map(|i| {
                format!(
                    "## NOTE 000 SECTION {i:04}\n\n{}\n\n",
                    "Clear ideas in a distant paragraph. ".repeat(14)
                )
            })
            .collect::<String>();
        let end = "\n\n## NOTE 000 END";
        let source = format!(
            "{begin}{}{end}",
            &block[..1024 * 1024 - begin.len() - end.len()]
        );
        state.documents = vec![Document::scratch("Synthetic.md", &source)];
        let temp = tempfile::tempdir().unwrap();
        let session = temp.path().join("state.json");
        state.save_to(&session).unwrap();
        let mut app = VelocimdApp::from_state(AppState::load_from(&session).unwrap());
        app.state.theme.apply_to(&ctx);
        let mut expected_top = 0;
        for number in 0..150 {
            if number == 90 || number == 130 {
                app.state.theme = if number == 90 {
                    crate::theme::ThemeConfig::default_light()
                } else {
                    crate::theme::ThemeConfig::default_dark()
                };
                app.state.theme.apply_to(&ctx);
            }
            let size = if (70..110).contains(&number) {
                egui::vec2(560.0, 700.0)
            } else {
                egui::vec2(1100.0, 800.0)
            };
            let position = egui::pos2(250.0, 240.0);
            let events = match number {
                1 | 2 => vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: number == 1,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                3 => vec![egui::Event::Key {
                    key: egui::Key::End,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::CTRL,
                }],
                _ => vec![],
            };
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    time: Some(number as f64 / 60.0),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show_inside(root, |ui| {
                        ui.add_space(82.0);
                        ui.columns(2, |columns| {
                            let viewport_top = columns[0].cursor().min.y;
                            let editor = app.editor_pane(&mut columns[0], &ctx, true);
                            expected_top =
                                crate::ui::top_visible_editor_line(viewport_top, &editor);
                            app.preview_pane(&mut columns[1], true);
                        });
                    });
                },
            );
            let transition = if number >= 130 {
                130
            } else if number >= 110 {
                110
            } else if number >= 90 {
                90
            } else if number >= 70 {
                70
            } else {
                0
            };
            if number >= 50 && number - transition >= 5 {
                if number < 70 {
                    assert!(
                        app.preview_sync_line > 7500,
                        "actual editor End input must jump to late source context, got {}",
                        app.preview_sync_line
                    );
                }
                assert_eq!(
                    app.preview_sync_line, expected_top,
                    "source sync must match the ACTUAL visible editor context, not the pre-scroll galley position"
                );
                assert_eq!(
                    output.platform_output.num_completed_passes, 1,
                    "stable source following must not discard the first pass of every frame during offscreen refinement"
                );
                assert!(
                    !output.platform_output.requested_discard(),
                    "frame {number}: stable jump must converge while unrelated offscreen refinement continues: {:?}",
                    output.platform_output.request_discard_reasons
                );
            }
        }
        app.state.documents.clear();
    }

    #[test]
    fn clamped_early_source_target_ignores_unrelated_offscreen_refinement() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let source = "## Heading\n\nearly paragraph\n\n".repeat(10_000);
        let target = Some(PreviewSyncTarget {
            line: 2,
            anchor: PreviewSyncAnchor::ScreenY(240.0),
        });
        let mut discards = 0;
        for number in 0..12 {
            let (output, _, _) = frame(&ctx, &mut renderer, &source, 1, 550.0, target, None);
            if number >= 8 && output.platform_output.requested_discard() {
                discards += 1;
            }
        }
        assert!(
            renderer.refinement < renderer.blocks.len(),
            "offscreen refinement is genuinely still active"
        );
        assert_eq!(
            discards, 0,
            "an unrelated changing total extent must not turn a top-clamped ScreenY target into a discard storm"
        );
    }

    #[test]
    fn early_mebibyte_revision_never_moves_whole_layout_into_a_later_frame() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let mut source =
            "A calm line of plain text for realistic editing and wrapping. More words here.\n"
                .repeat(14_000);
        frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        source.insert_str(0, "edited ");
        for _ in 0..100 {
            let before = renderer.layout_bytes;
            frame(&ctx, &mut renderer, &source, 2, 640.0, None, None);
            assert!(
                renderer.layout_bytes - before
                    <= 2 * PLAIN_FRAGMENT_BYTES * REVISION_FRAGMENT_BATCH,
                "every ordinary frame is bounded, not just the input frame"
            );
            if renderer.blocks[0].measured {
                break;
            }
        }
        let fragments = renderer.blocks[0].fragments.as_ref().unwrap();
        assert!(fragments.complete);
        assert_eq!(
            fragments
                .rows
                .iter()
                .map(|row| row.galley.job.text.as_str())
                .collect::<String>(),
            renderer.blocks[0].segment.plain.as_ref().unwrap().as_str()
        );
        let mut output = None;
        for _ in 0..3 {
            let before = renderer.layout_bytes;
            output = Some(frame(&ctx, &mut renderer, &source, 2, 640.0, None, None).0);
            assert_eq!(renderer.layout_bytes, before);
        }
        assert!(
            output.unwrap().viewport_output[&egui::ViewportId::ROOT].repaint_delay
                > std::time::Duration::from_secs(1)
        );
    }

    #[test]
    fn fragments_preserve_whole_paragraph_wrapping_and_unicode_without_hard_breaks() {
        for (width, scale) in [(263.0, 1.0), (640.0, 1.0), (263.0, 1.5)] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            let text = "Some λ and café words;  multiple spaces — 中文。 a-long-hyphenated-word "
                .repeat(300);
            let _ = ctx.run_ui(input(width + 50.0), |ui| {
                let mut fragments = PlainFragments::default();
                fragments.layout(ui, &text, width);
                let complete = ui.fonts_mut(|fonts| {
                    fonts.layout_job(egui::text::LayoutJob::simple(
                        text.clone(),
                        egui::TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                        width,
                    ))
                });
                assert_eq!(
                    fragments
                        .rows
                        .iter()
                        .map(|row| row.galley.job.text.as_str())
                        .collect::<String>(),
                    text
                );
                let rows = fragments
                    .rows
                    .iter()
                    .flat_map(|fragment| {
                        fragment
                            .galley
                            .rows
                            .iter()
                            .map(move |row| (fragment.y + row.pos.y, row))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    rows.len(),
                    complete.rows.len(),
                    "same soft-wrap rows, not artificial paragraphs"
                );
                for ((y, row), reference) in rows.iter().zip(&complete.rows) {
                    assert_eq!(
                        row.glyphs.iter().map(|g| g.chr).collect::<String>(),
                        reference.glyphs.iter().map(|g| g.chr).collect::<String>()
                    );
                    assert!(
                        (y - reference.pos.y).abs() <= 1.0,
                        "row y={y}, reference={}",
                        reference.pos.y
                    );
                    for (glyph, expected) in row.glyphs.iter().zip(&reference.glyphs) {
                        assert!(
                            (glyph.pos.x - expected.pos.x).abs() <= 1.1 / scale + 0.01,
                            "width={width} scale={scale} char={} x={} expected={} row_y={y}",
                            glyph.chr,
                            glyph.pos.x,
                            expected.pos.x
                        );
                    }
                }
            });
        }
    }

    #[test]
    fn decoded_newline_fragments_advance_and_preserve_newline_unicode_geometry() {
        let sources = [
            format!("x{}y", "&#10;".repeat(9000)),
            "&#10;".repeat(9000),
            format!("{}tail", "λ&#10;café&#13;&#10;中文&#10;&#10;".repeat(700)),
        ];
        for source in sources {
            let blocks = split_preview_segments(&source);
            assert_eq!(blocks.len(), 1);
            let text = blocks[0].plain.as_ref().unwrap();
            assert!(text.contains('\n'));
            for (width, scale) in [(263.0, 1.0), (640.0, 1.5), (1.0, 1.0)] {
                let ctx = egui::Context::default();
                ctx.set_pixels_per_point(scale);
                // The revision batch makes the predecessor's non-progress
                // observable without hanging the unit-test process itself.
                let mut fragments = PlainFragments {
                    revision: true,
                    ..Default::default()
                };
                for _ in 0..16 {
                    let before = fragments.end;
                    let _ = ctx.run_ui(input(width + 50.0), |ui| {
                        fragments.layout(ui, text, width);
                    });
                    assert!(
                        fragments.complete || fragments.end > before,
                        "nonempty fragment input must consume actual UTF-8 bytes, including newlines"
                    );
                    assert!(text.is_char_boundary(fragments.end));
                    if fragments.complete {
                        break;
                    }
                }
                assert!(fragments.complete);
                assert_eq!(
                    fragments
                        .rows
                        .iter()
                        .map(|row| row.galley.job.text.as_str())
                        .collect::<String>(),
                    *text
                );
                let mut complete = None;
                let _ = ctx.run_ui(input(width + 50.0), |ui| {
                    complete = Some(ui.fonts_mut(|fonts| {
                        fonts.layout_job(egui::text::LayoutJob::simple(
                            text.clone(),
                            egui::TextStyle::Body.resolve(ui.style()),
                            ui.visuals().text_color(),
                            width,
                        ))
                    }));
                });
                let complete = complete.unwrap();
                // A standalone prefix ending in newline has a final empty row
                // representing the next fragment's first row. It has no ink and
                // must be counted only once in the concatenated geometry.
                let rows = fragments
                    .rows
                    .iter()
                    .enumerate()
                    .flat_map(|(index, fragment)| {
                        let duplicate = index + 1 < fragments.rows.len()
                            && fragment.galley.job.text.ends_with('\n');
                        let count = fragment.galley.rows.len() - usize::from(duplicate);
                        fragment.galley.rows[..count]
                            .iter()
                            .map(move |row| (fragment.y + row.pos.y, row))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(rows.len(), complete.rows.len());
                assert!((fragments.height - complete.size().y).abs() <= 1.1);
                for ((y, row), reference) in rows.iter().zip(&complete.rows) {
                    assert_eq!(row.ends_with_newline, reference.ends_with_newline);
                    assert_eq!(
                        row.glyphs.iter().map(|glyph| glyph.chr).collect::<String>(),
                        reference
                            .glyphs
                            .iter()
                            .map(|glyph| glyph.chr)
                            .collect::<String>()
                    );
                    assert!((y - reference.pos.y).abs() <= 1.1);
                    for (glyph, expected) in row.glyphs.iter().zip(&reference.glyphs) {
                        assert!((glyph.pos.x - expected.pos.x).abs() <= 1.1 / scale + 0.01);
                    }
                }
            }
        }
    }

    #[test]
    fn parser_ranges_preserve_all_container_and_fence_semantics() {
        let fixtures = [
            "- first\n\n  second paragraph\n\n  - nested\n\n- last\n",
            "> first\n>\n> second\n>\n> - child\n",
            "| a | b |\n|---|---|\n| c | d |\n",
            "Term\n: definition\n\n  continuation\n",
            "    literal\n\n    still code\n",
            "````text\n```mermaid\ngraph TD\nA --> B\n```\n````\n",
            "```mermaid\ngraph TD\nA --> B\n",
            "```mermaid\ngraph TD\n    ```\n",
        ];
        for fixture in fixtures {
            let blocks = split_preview_segments(fixture);
            assert_eq!(blocks.len(), 1, "{fixture:?}");
            assert!(matches!(blocks[0].kind, PreviewSegmentKind::Markdown(_)));
            assert_eq!(blocks[0].line_count, fixture.lines().count());
        }
        let blocks = split_preview_segments(
            "# one\n## two\n\n~~~~mermaid\ngraph TD\nA --> B\n~~~\nC --> D\n~~~~\n\nlast",
        );
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[2].start_line, 3);
        assert_eq!(blocks[2].line_count, 6);
        assert!(
            matches!(&blocks[2].kind, PreviewSegmentKind::Mermaid(text) if text.contains("~~~\nC --> D"))
        );
        let crlf = split_preview_segments("λ\r\n\r\n```mermaid\r\ngraph TD\r\nA --> B\r\n```\r\n");
        assert_eq!((crlf[1].start_line, crlf[1].line_count), (2, 4));
    }

    #[test]
    fn reference_context_and_footnote_fallback_survive_segmentation() {
        let source = "[go][dest]\n\n![pic][image]\n\n[^note]: note [go][dest]\n\ntext[^note]\n\n```mermaid\ngraph TD\nA --> B\n```\n\nagain[^note]\n\n[dest]: https://example.test/path \"Title\"\n[image]: fixture.png\n";
        let blocks = split_preview_segments(source);
        let mut links = Vec::new();
        let mut notes = 0;
        for block in &blocks {
            if let PreviewSegmentKind::Markdown(text) = &block.kind {
                for event in Parser::new_ext(text, parser_options()) {
                    match event {
                        Event::Start(Tag::Link { dest_url, .. }) => {
                            links.push(dest_url.to_string())
                        }
                        Event::FootnoteReference(_) => notes += 1,
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(
            links,
            ["https://example.test/path", "https://example.test/path"]
        );
        assert_eq!(notes, 2);
        let context = blocks
            .iter()
            .find(|block| !block.embedded_mermaids.is_empty())
            .unwrap();
        assert!(
            context.plain.is_none(),
            "contextual footnotes explicitly use the rich fallback"
        );
        let mut renderer = PreviewRenderer::default();
        let ctx = egui::Context::default();
        let (output, _, _) = frame(&ctx, &mut renderer, source, 1, 640.0, None, None);
        assert!(
            text_position(&output, "A").is_some(),
            "Mermaid still renders within the contextual fallback"
        );
        assert!(renderer.rich_parses > 0);
    }

    #[test]
    fn unchanged_blocks_keep_actual_galleys_and_heights_after_shifted_edit() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        frame(
            &ctx,
            &mut renderer,
            "first\n\nretained paragraph\n\nlast",
            1,
            640.0,
            None,
            None,
        );
        let galley = Arc::clone(renderer.blocks[1].galley.as_ref().unwrap());
        let height = renderer.blocks[1].height;
        let id = renderer.blocks[1].id;
        frame(
            &ctx,
            &mut renderer,
            "inserted\n\nfirst\n\nretained paragraph\n\nchanged last",
            2,
            640.0,
            None,
            None,
        );
        assert!(Arc::ptr_eq(
            &galley,
            renderer.blocks[2].galley.as_ref().unwrap()
        ));
        assert_eq!(height, renderer.blocks[2].height);
        assert_eq!(id, renderer.blocks[2].id);
        let parse_count = renderer.parses;
        frame(
            &ctx,
            &mut renderer,
            "deliberately inaccessible warm source",
            2,
            640.0,
            None,
            None,
        );
        assert_eq!(
            renderer.parses, parse_count,
            "revision-aware warm path must not inspect, hash or parse the source"
        );
    }

    #[test]
    fn single_mebibyte_paragraph_retains_parsed_text_and_layout_on_warm_frames() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let source = "word ".repeat(1024 * 1024 / 5 + 1);
        frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        assert_eq!(renderer.blocks.len(), 1);
        let galley = Arc::clone(&renderer.blocks[0].fragments.as_ref().unwrap().rows[0].galley);
        let layouts = renderer.layouts;
        let layout_bytes = renderer.layout_bytes;
        let parses = renderer.parses;
        for _ in 0..3 {
            frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        }
        assert!(Arc::ptr_eq(
            &galley,
            &renderer.blocks[0].fragments.as_ref().unwrap().rows[0].galley
        ));
        assert_eq!(renderer.layout_bytes, layout_bytes);
        assert_eq!(
            (renderer.parses, renderer.layouts, renderer.rich_parses),
            (parses, layouts, 0)
        );
    }

    #[test]
    fn layout_environment_invalidates_height_and_actual_layout_not_source_parse() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let source = "many words here ".repeat(70);
        frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        let wide_height = renderer.blocks[0].height;
        let mut previous = Arc::clone(renderer.blocks[0].galley.as_ref().unwrap());
        frame(&ctx, &mut renderer, &source, 1, 240.0, None, None);
        assert!(renderer.blocks[0].height > wide_height);
        assert!(!Arc::ptr_eq(
            &previous,
            renderer.blocks[0].galley.as_ref().unwrap()
        ));
        previous = Arc::clone(renderer.blocks[0].galley.as_ref().unwrap());
        ctx.global_style_mut(|style| {
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(22.0));
        });
        frame(&ctx, &mut renderer, &source, 1, 240.0, None, None);
        assert!(!Arc::ptr_eq(
            &previous,
            renderer.blocks[0].galley.as_ref().unwrap()
        ));
        previous = Arc::clone(renderer.blocks[0].galley.as_ref().unwrap());
        ctx.set_visuals(egui::Visuals::light());
        frame(&ctx, &mut renderer, &source, 1, 240.0, None, None);
        assert!(!Arc::ptr_eq(
            &previous,
            renderer.blocks[0].galley.as_ref().unwrap()
        ));
        previous = Arc::clone(renderer.blocks[0].galley.as_ref().unwrap());
        ctx.set_pixels_per_point(1.5);
        frame(&ctx, &mut renderer, &source, 1, 240.0, None, None);
        assert!(!Arc::ptr_eq(
            &previous,
            renderer.blocks[0].galley.as_ref().unwrap()
        ));
        assert_eq!(renderer.parses, 1);
    }

    #[test]
    fn unseen_distant_source_target_renders_and_scrolls_to_late_content() {
        for anchor in [PreviewSyncAnchor::Top, PreviewSyncAnchor::ScreenY(240.0)] {
            let ctx = egui::Context::default();
            let mut renderer = PreviewRenderer::default();
            let mut source = "early paragraph\n\n".repeat(5_000);
            source.push_str("DISTANT DESTINATION\n\n");
            source.push_str(&"later paragraph\n\n".repeat(5_000));
            frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
            assert!(!renderer.blocks[5_000].measured);
            let target = Some(PreviewSyncTarget {
                line: 10_000,
                anchor,
            });
            let mut output = None;
            let mut offset = 0.0;
            for _ in 0..4 {
                let (frame_output, scroll, _) =
                    frame(&ctx, &mut renderer, &source, 1, 640.0, target, None);
                output = Some(frame_output);
                offset = scroll;
            }
            let position = text_position(output.as_ref().unwrap(), "DISTANT DESTINATION")
                .expect("late content is really painted, not an estimated empty spacer");
            assert!(
                position.y > 0.0 && position.y < 480.0,
                "destination screen y={}",
                position.y
            );
            match anchor {
                PreviewSyncAnchor::Top => assert!(position.y < 80.0, "top anchor y={}", position.y),
                PreviewSyncAnchor::ScreenY(y) => assert!(
                    (position.y - y).abs() < 40.0,
                    "screen anchor y={}",
                    position.y
                ),
            }
            assert!(offset > 50_000.0);
            assert!(renderer.blocks[5_000].measured);
        }
    }

    #[test]
    fn unknown_extents_converge_and_retained_layouts_are_bounded() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let source = (0..200)
            .map(|i| format!("paragraph {i} {}\n\n", "words ".repeat(i % 20)))
            .collect::<String>();
        for _ in 0..60 {
            frame(&ctx, &mut renderer, &source, 1, 400.0, None, None);
        }
        assert!(renderer.blocks.iter().all(|block| block.measured));
        let total = renderer.heights.prefix(renderer.blocks.len());
        assert!(
            (total
                - renderer
                    .blocks
                    .iter()
                    .map(|block| block.height)
                    .sum::<f32>())
            .abs()
                < 0.5
        );
        assert!(renderer.retained.len() <= MAX_RETAINED_GALLEYS);
        let (_, _, old_extent) = frame(&ctx, &mut renderer, &source, 1, 400.0, None, None);
        let (output, _, new_extent) = frame(
            &ctx,
            &mut renderer,
            &source,
            1,
            400.0,
            None,
            Some(old_extent),
        );
        assert!(!output.shapes.is_empty());
        assert!(
            (new_extent - old_extent).abs() < 1.0,
            "extent changed {old_extent} -> {new_extent}"
        );
    }

    #[test]
    fn local_image_loader_resize_updates_height_without_a_source_revision() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("preview.png");
        image::RgbaImage::from_pixel(80, 40, image::Rgba([20, 40, 60, 255]))
            .save(&path)
            .unwrap();
        let source = format!("![fixture]({})\n\nfollowing paragraph", path.display());
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let uri = format!("file://{}", path.display());
        for _ in 0..100 {
            frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
            if matches!(
                ctx.try_load_image(&uri, egui::load::SizeHint::default()),
                Ok(egui::load::ImagePoll::Ready { .. })
            ) && renderer.pending_images.is_empty()
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(matches!(
            ctx.try_load_image(&uri, egui::load::SizeHint::default()),
            Ok(egui::load::ImagePoll::Ready { .. })
        ));
        let original = renderer.blocks[0].height;
        image::RgbaImage::from_pixel(80, 240, image::Rgba([60, 40, 20, 255]))
            .save(&path)
            .unwrap();
        ctx.forget_image(&uri);
        for _ in 0..100 {
            frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
            if renderer.blocks[0].height > original + 150.0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(renderer.blocks[0].height > original + 150.0);
        assert_eq!(renderer.parses, 1);
        let total = renderer
            .blocks
            .iter()
            .map(|block| block.height)
            .sum::<f32>();
        assert!((renderer.heights.prefix(renderer.blocks.len()) - total).abs() < 0.5);
    }

    #[test]
    fn references_change_only_the_dependent_cached_block_and_keep_case_folding() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let source = "[go][MiXeD]\n\nunchanged\n\n[mixed]: https://example.test/old\n";
        frame(&ctx, &mut renderer, source, 1, 640.0, None, None);
        let reference = renderer.blocks[0].id;
        let plain = Arc::clone(renderer.blocks[1].galley.as_ref().unwrap());
        frame(
            &ctx,
            &mut renderer,
            &source.replace("/old", "/new"),
            2,
            640.0,
            None,
            None,
        );
        assert_ne!(reference, renderer.blocks[0].id);
        assert!(Arc::ptr_eq(
            &plain,
            renderer.blocks[1].galley.as_ref().unwrap()
        ));
        let PreviewSegmentKind::Markdown(text) = &renderer.blocks[0].segment.kind else {
            panic!()
        };
        assert!(Parser::new_ext(text, parser_options()).any(|event| matches!(event, Event::Start(Tag::Link { dest_url, .. }) if dest_url.ends_with("/new"))));
    }

    #[test]
    fn scrolled_shrink_and_width_change_do_not_leave_empty_viewports() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let source = (0..300)
            .map(|i| format!("paragraph {i} {}\n\n", "long words ".repeat(8)))
            .collect::<String>();
        for _ in 0..80 {
            frame(&ctx, &mut renderer, &source, 1, 300.0, None, None);
        }
        let (_, _, extent) = frame(&ctx, &mut renderer, &source, 1, 300.0, None, None);
        frame(
            &ctx,
            &mut renderer,
            &source,
            1,
            300.0,
            None,
            Some(extent - 500.0),
        );
        let (output, _, _) = frame(
            &ctx,
            &mut renderer,
            "SHORT REPLACEMENT",
            2,
            900.0,
            None,
            None,
        );
        assert!(
            text_position(&output, "SHORT REPLACEMENT").is_some(),
            "shrink must discard/reconcile stale scroll offset before painting an empty viewport"
        );
    }

    #[test]
    fn pending_offscreen_image_converges_on_loader_wake_and_does_not_spin() {
        #[derive(Default)]
        struct DelayedImage(std::sync::Mutex<bool>);
        impl egui::load::ImageLoader for DelayedImage {
            fn id(&self) -> &str {
                "velocimd-test-delayed"
            }
            fn load(
                &self,
                _: &egui::Context,
                uri: &str,
                _: egui::load::SizeHint,
            ) -> egui::load::ImageLoadResult {
                if uri != "test://delayed" {
                    return Err(egui::load::LoadError::NotSupported);
                }
                if *self.0.lock().unwrap() {
                    Ok(egui::load::ImagePoll::Ready {
                        image: Arc::new(egui::ColorImage::filled([80, 300], egui::Color32::WHITE)),
                    })
                } else {
                    Ok(egui::load::ImagePoll::Pending { size: None })
                }
            }
            fn forget(&self, _: &str) {}
            fn forget_all(&self) {}
            fn byte_size(&self) -> usize {
                0
            }
        }
        let loader = Arc::new(DelayedImage::default());
        let ctx = egui::Context::default();
        // Install the production loaders first, then the controlled test loader
        // so its unsupported result leaves real file/image behavior intact.
        egui_extras::install_image_loaders(&ctx);
        ctx.add_image_loader(loader.clone());
        let mut renderer = PreviewRenderer::default();
        let source = format!(
            "{}\n\n![delayed](test://delayed)\n\nlast",
            "prefix paragraph\n\n".repeat(100)
        );
        for _ in 0..30 {
            frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        }
        let index = renderer
            .blocks
            .iter()
            .position(|block| !block.segment.images.is_empty())
            .unwrap();
        assert!(
            renderer.blocks[index].measured,
            "offscreen refinement must have measured the placeholder"
        );
        assert!(renderer.pending_image_set.contains(&index));
        let (idle, _, _) = frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        assert!(
            idle.viewport_output[&egui::ViewportId::ROOT].repaint_delay
                > std::time::Duration::from_secs(1),
            "pending images must wait for loader wake, not spin repainting"
        );
        let before = renderer.blocks[index].height;
        *loader.0.lock().unwrap() = true;
        ctx.request_repaint();
        frame(&ctx, &mut renderer, &source, 1, 640.0, None, None);
        assert!(renderer.blocks[index].height > before + 200.0);
        assert!(renderer.pending_images.is_empty());
        assert_eq!(renderer.parses, 1);
    }

    #[test]
    fn contextual_reference_definitions_inside_containers_keep_destinations() {
        for source in [
            "> [dest]: https://example.test/inside\n\n[go][dest]\n",
            "- [dest]: https://example.test/inside\n\n[go][dest]\n",
            "[go][dest]\n\n[dest]:\n  https://example.test/inside\n  \"multi line title\"\n",
        ] {
            let original = Parser::new_ext(source, parser_options())
                .filter_map(|event| match event {
                    Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let segmented = split_preview_segments(source)
                .iter()
                .flat_map(|block| {
                    let PreviewSegmentKind::Markdown(text) = &block.kind else {
                        return Vec::new();
                    };
                    Parser::new_ext(text, parser_options())
                        .filter_map(|event| match event {
                            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            assert_eq!(segmented, original, "{source}");
        }
    }

    #[test]
    fn rich_links_and_copyable_code_keep_live_interaction() {
        let ctx = egui::Context::default();
        let mut renderer = PreviewRenderer::default();
        let source =
            "[go][ref]\n\n```rust\nlet answer = 42;\n```\n\n[ref]: https://example.test/target\n";
        let (output, _, _) = frame(&ctx, &mut renderer, source, 1, 640.0, None, None);
        let position = text_position(&output, "go").unwrap();
        let clicked = click(&ctx, &mut renderer, source, position);
        assert!(clicked.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::OpenUrl(url) if url.url == "https://example.test/target")));
        let (output, _, _) = frame(&ctx, &mut renderer, source, 1, 640.0, None, None);
        let position = text_position(&output, "🗐").unwrap();
        let clicked = click(&ctx, &mut renderer, source, position);
        assert!(clicked.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == "let answer = 42;")));
        assert!(
            renderer.rich_parses > 0,
            "these blocks intentionally use the live rich renderer"
        );
    }
}
