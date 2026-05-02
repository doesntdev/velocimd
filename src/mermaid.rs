use crate::theme::DesignTokens;
use eframe::egui::{self, Align2, FontId, Pos2, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    hash::{Hash, Hasher},
};

const MAX_MERMAID_CACHE_ENTRIES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewBlock {
    Markdown(String),
    Mermaid(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    TopDown,
    LeftRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    Rectangle,
    Round,
    Diamond,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidNode {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidEdge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MermaidDiagram {
    pub direction: Direction,
    pub nodes: Vec<MermaidNode>,
    pub edges: Vec<MermaidEdge>,
}

#[derive(Default)]
pub struct MermaidRenderCache {
    entries: HashMap<MermaidCacheKey, CachedMermaid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MermaidCacheKey {
    source_hash: u64,
    source_len: usize,
    width: u32,
}

#[derive(Clone)]
struct CachedMermaid {
    diagram: Option<MermaidDiagram>,
    layout: Option<DiagramLayout>,
}

pub fn split_markdown_and_mermaid(markdown: &str) -> Vec<PreviewBlock> {
    let mut blocks = Vec::new();
    let mut markdown_buffer = String::new();
    let mut mermaid_buffer = String::new();
    let mut in_mermaid = false;
    let mut closing_fence = "```";

    for line in markdown.lines() {
        let trimmed = line.trim_start();

        if in_mermaid {
            if trimmed.starts_with(closing_fence) {
                blocks.push(PreviewBlock::Mermaid(mermaid_buffer.trim().to_string()));
                mermaid_buffer.clear();
                in_mermaid = false;
            } else {
                mermaid_buffer.push_str(line);
                mermaid_buffer.push('\n');
            }
            continue;
        }

        if is_mermaid_fence(trimmed) {
            if !markdown_buffer.is_empty() {
                blocks.push(PreviewBlock::Markdown(std::mem::take(&mut markdown_buffer)));
            }
            closing_fence = if trimmed.starts_with("~~~") {
                "~~~"
            } else {
                "```"
            };
            in_mermaid = true;
        } else {
            markdown_buffer.push_str(line);
            markdown_buffer.push('\n');
        }
    }

    if in_mermaid {
        markdown_buffer.push_str("```mermaid\n");
        markdown_buffer.push_str(&mermaid_buffer);
    }

    if !markdown_buffer.is_empty() {
        blocks.push(PreviewBlock::Markdown(markdown_buffer));
    }

    blocks
}

pub fn parse_flowchart(source: &str) -> Option<MermaidDiagram> {
    let mut direction = Direction::TopDown;
    let mut nodes: BTreeMap<String, MermaidNode> = BTreeMap::new();
    let mut edges = Vec::new();
    let mut saw_header = false;

    for statement in statements(source) {
        let statement = statement.trim();
        if statement.is_empty() {
            continue;
        }

        if !saw_header && (statement.starts_with("graph ") || statement.starts_with("flowchart ")) {
            saw_header = true;
            direction = parse_direction(statement);
            continue;
        }

        if let Some(edge) = parse_edge(statement) {
            insert_node(&mut nodes, edge.0);
            insert_node(&mut nodes, edge.1);
            edges.push(edge.2);
        } else {
            let node = parse_node_ref(statement);
            insert_node(&mut nodes, node);
        }
    }

    if !saw_header || nodes.is_empty() {
        return None;
    }

    Some(MermaidDiagram {
        direction,
        nodes: nodes.into_values().collect(),
        edges,
    })
}

impl MermaidRenderCache {
    pub fn render(&mut self, ui: &mut Ui, source: &str, tokens: DesignTokens) {
        let available_width = ui.available_width().max(1.0);
        let key = MermaidCacheKey {
            source_hash: source_hash(source),
            source_len: source.len(),
            width: available_width.round() as u32,
        };

        if !self.entries.contains_key(&key) {
            self.insert(key, source, available_width);
        }

        let Some(cached) = self.entries.get(&key) else {
            render_parse_error(ui, tokens);
            return;
        };
        let (Some(diagram), Some(layout)) = (&cached.diagram, &cached.layout) else {
            render_parse_error(ui, tokens);
            return;
        };

        paint_diagram(ui, diagram, layout, tokens);
    }

    fn insert(&mut self, key: MermaidCacheKey, source: &str, available_width: f32) {
        if self.entries.len() >= MAX_MERMAID_CACHE_ENTRIES
            && let Some(oldest_key) = self.entries.keys().next().copied()
        {
            self.entries.remove(&oldest_key);
        }

        let diagram = parse_flowchart(source);
        let layout = diagram
            .as_ref()
            .map(|diagram| DiagramLayout::new(diagram, available_width));
        self.entries.insert(key, CachedMermaid { diagram, layout });
    }
}

pub fn render_mermaid(ui: &mut Ui, source: &str, tokens: DesignTokens) {
    MermaidRenderCache::default().render(ui, source, tokens);
}

fn render_parse_error(ui: &mut Ui, tokens: DesignTokens) {
    egui::Frame::new()
        .fill(tokens.panel_bg)
        .stroke(Stroke::new(1.0, tokens.border))
        .corner_radius(6)
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new("Mermaid diagram could not be parsed")
                    .small()
                    .color(tokens.danger),
            );
        });
}

fn paint_diagram(
    ui: &mut Ui,
    diagram: &MermaidDiagram,
    layout: &DiagramLayout,
    tokens: DesignTokens,
) {
    let (rect, _) = ui.allocate_exact_size(layout.canvas_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6, tokens.panel_bg_active);
    painter.rect_stroke(rect, 6, Stroke::new(1.0, tokens.border), StrokeKind::Inside);

    for edge in &diagram.edges {
        let Some(from) = layout.node_rects.get(&edge.from) else {
            continue;
        };
        let Some(to) = layout.node_rects.get(&edge.to) else {
            continue;
        };
        paint_edge(
            &painter,
            rect.min.to_vec2(),
            *from,
            *to,
            edge.label.as_deref(),
            tokens,
        );
    }

    for node in &diagram.nodes {
        if let Some(node_rect) = layout.node_rects.get(&node.id) {
            paint_node(&painter, rect.min.to_vec2(), *node_rect, node, tokens);
        }
    }
}

fn source_hash(source: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

#[derive(Clone)]
struct DiagramLayout {
    canvas_size: egui::Vec2,
    node_rects: BTreeMap<String, Rect>,
}

impl DiagramLayout {
    fn new(diagram: &MermaidDiagram, available_width: f32) -> Self {
        let levels = levels(diagram);
        let node_size = vec2(132.0, 46.0);
        let margin = vec2(24.0, 24.0);
        let gap = match diagram.direction {
            Direction::LeftRight => vec2(84.0, 28.0),
            Direction::TopDown => vec2(34.0, 78.0),
        };
        let mut node_rects = BTreeMap::new();
        let mut max_x: f32 = 0.0;
        let mut max_y: f32 = 0.0;

        for (level_index, level) in levels.iter().enumerate() {
            for (node_index, id) in level.iter().enumerate() {
                let min = match diagram.direction {
                    Direction::LeftRight => pos2(
                        margin.x + level_index as f32 * (node_size.x + gap.x),
                        margin.y + node_index as f32 * (node_size.y + gap.y),
                    ),
                    Direction::TopDown => pos2(
                        margin.x + node_index as f32 * (node_size.x + gap.x),
                        margin.y + level_index as f32 * (node_size.y + gap.y),
                    ),
                };
                let rect = Rect::from_min_size(min, node_size);
                max_x = max_x.max(rect.right());
                max_y = max_y.max(rect.bottom());
                node_rects.insert(id.clone(), rect);
            }
        }

        Self {
            canvas_size: vec2(available_width, max_y + margin.y),
            node_rects,
        }
    }
}

fn levels(diagram: &MermaidDiagram) -> Vec<Vec<String>> {
    let mut level_by_id = BTreeMap::new();
    for node in &diagram.nodes {
        level_by_id.insert(node.id.clone(), 0usize);
    }

    for _ in 0..diagram.nodes.len().max(1) {
        let mut changed = false;
        for edge in &diagram.edges {
            let from_level = *level_by_id.get(&edge.from).unwrap_or(&0);
            let to_level = level_by_id.entry(edge.to.clone()).or_insert(0);
            if *to_level <= from_level {
                *to_level = from_level + 1;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut grouped: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for node in &diagram.nodes {
        grouped
            .entry(*level_by_id.get(&node.id).unwrap_or(&0))
            .or_default()
            .push(node.id.clone());
    }

    grouped.into_values().collect()
}

fn paint_node(
    painter: &egui::Painter,
    offset: egui::Vec2,
    rect: Rect,
    node: &MermaidNode,
    tokens: DesignTokens,
) {
    let rect = rect.translate(offset);
    let stroke = Stroke::new(1.2, tokens.border_active);
    match node.shape {
        NodeShape::Diamond => {
            let points = [
                pos2(rect.center().x, rect.top()),
                pos2(rect.right(), rect.center().y),
                pos2(rect.center().x, rect.bottom()),
                pos2(rect.left(), rect.center().y),
                pos2(rect.center().x, rect.top()),
            ];
            for segment in points.windows(2) {
                painter.line_segment([segment[0], segment[1]], stroke);
            }
        }
        NodeShape::Round => {
            painter.rect_filled(rect, 18, tokens.panel_bg);
            painter.rect_stroke(rect, 18, stroke, StrokeKind::Inside);
        }
        NodeShape::Rectangle => {
            painter.rect_filled(rect, 6, tokens.panel_bg);
            painter.rect_stroke(rect, 6, stroke, StrokeKind::Inside);
        }
    }

    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        &node.label,
        FontId::proportional(13.0),
        tokens.text,
    );
}

fn paint_edge(
    painter: &egui::Painter,
    offset: egui::Vec2,
    from: Rect,
    to: Rect,
    label: Option<&str>,
    tokens: DesignTokens,
) {
    let from = from.translate(offset);
    let to = to.translate(offset);
    let horizontal =
        (to.center().x - from.center().x).abs() >= (to.center().y - from.center().y).abs();
    let (start, end) = if horizontal {
        if to.center().x >= from.center().x {
            (
                pos2(from.right(), from.center().y),
                pos2(to.left(), to.center().y),
            )
        } else {
            (
                pos2(from.left(), from.center().y),
                pos2(to.right(), to.center().y),
            )
        }
    } else if to.center().y >= from.center().y {
        (
            pos2(from.center().x, from.bottom()),
            pos2(to.center().x, to.top()),
        )
    } else {
        (
            pos2(from.center().x, from.top()),
            pos2(to.center().x, to.bottom()),
        )
    };

    let stroke = Stroke::new(1.2, tokens.text_muted);
    painter.line_segment([start, end], stroke);
    paint_arrow_head(painter, start, end, stroke);

    if let Some(label) = label.filter(|label| !label.trim().is_empty()) {
        let label_pos = pos2((start.x + end.x) / 2.0, (start.y + end.y) / 2.0 - 8.0);
        painter.rect_filled(
            Rect::from_center_size(label_pos, vec2(label.len() as f32 * 7.0 + 12.0, 18.0)),
            4,
            tokens.panel_bg_active,
        );
        painter.text(
            label_pos,
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(11.0),
            tokens.text_muted,
        );
    }
}

fn paint_arrow_head(painter: &egui::Painter, start: Pos2, end: Pos2, stroke: Stroke) {
    let direction = (end - start).normalized();
    let normal = vec2(-direction.y, direction.x);
    let tip = end;
    let back = tip - direction * 8.0;
    painter.line_segment([tip, back + normal * 4.0], stroke);
    painter.line_segment([tip, back - normal * 4.0], stroke);
}

fn statements(source: &str) -> Vec<String> {
    source
        .lines()
        .flat_map(|line| {
            line.split("%%")
                .next()
                .unwrap_or_default()
                .split(';')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn is_mermaid_fence(trimmed: &str) -> bool {
    let fence = trimmed
        .strip_prefix("```")
        .or_else(|| trimmed.strip_prefix("~~~"));
    fence.is_some_and(|language| language.trim().eq_ignore_ascii_case("mermaid"))
}

fn parse_direction(statement: &str) -> Direction {
    if statement.contains(" LR") || statement.contains(" RL") {
        Direction::LeftRight
    } else {
        Direction::TopDown
    }
}

fn parse_edge(statement: &str) -> Option<(MermaidNode, MermaidNode, MermaidEdge)> {
    let operator_index = statement.find("-->")?;
    let left = statement[..operator_index].trim();
    let mut right = statement[operator_index + 3..].trim();
    let mut label = None;

    if let Some(rest) = right.strip_prefix('|') {
        let label_end = rest.find('|')?;
        label = Some(rest[..label_end].trim().to_string());
        right = rest[label_end + 1..].trim();
    }

    let from = parse_node_ref(left);
    let to = parse_node_ref(right);
    let edge = MermaidEdge {
        from: from.id.clone(),
        to: to.id.clone(),
        label,
    };
    Some((from, to, edge))
}

fn parse_node_ref(raw: &str) -> MermaidNode {
    let raw = raw.trim();
    let Some((index, opener)) = raw
        .char_indices()
        .find(|(_, character)| matches!(character, '[' | '(' | '{'))
    else {
        return MermaidNode {
            id: raw.to_string(),
            label: raw.to_string(),
            shape: NodeShape::Rectangle,
        };
    };

    let id = raw[..index].trim().to_string();
    let (closer, shape) = match opener {
        '[' => (']', NodeShape::Rectangle),
        '(' => (')', NodeShape::Round),
        '{' => ('}', NodeShape::Diamond),
        _ => (']', NodeShape::Rectangle),
    };
    let label = raw[index + opener.len_utf8()..]
        .trim_end_matches(closer)
        .trim_matches(['"', '\''])
        .trim()
        .to_string();

    MermaidNode {
        id: id.clone(),
        label: if label.is_empty() { id } else { label },
        shape,
    }
}

fn insert_node(nodes: &mut BTreeMap<String, MermaidNode>, node: MermaidNode) {
    nodes
        .entry(node.id.clone())
        .and_modify(|existing| {
            if existing.label == existing.id && node.label != node.id {
                *existing = node.clone();
            }
        })
        .or_insert(node);
}

#[allow(dead_code)]
fn known_ids(diagram: &MermaidDiagram) -> BTreeSet<&str> {
    diagram.nodes.iter().map(|node| node.id.as_str()).collect()
}
