//! Conservative region reparsing. Only an already parser-confirmed plain
//! paragraph, isolated by unchanged empty lines (or document edges), is eligible.
//! Syntax, boundary changes and whole-buffer undo replacements fall back to the
//! existing whole-document parser. Other blocks may contain arbitrary Markdown:
//! edits admitted here cannot introduce or alter its global definitions/context.
use super::*;
use crate::ui::edit::{EditDelta, rebase};

fn body_end(source: &str) -> usize {
    source
        .strip_suffix("\r\n")
        .or_else(|| source.strip_suffix('\n'))
        .unwrap_or(source)
        .len()
}

fn safe_plain_source(source: &str) -> bool {
    !source.is_empty()
        && source.split_inclusive('\n').all(|line| {
            let line = if let Some(line) = line.strip_suffix('\n') {
                line.strip_suffix('\r').unwrap_or(line)
            } else {
                line
            };
            !line.is_empty()
                && line.trim() == line
                && line.chars().all(|c| {
                    c.is_alphanumeric()
                        || (!c.is_ascii() && !c.is_control() && !c.is_whitespace())
                        || " ,.!?'\";()".contains(c)
                })
        })
}

pub(super) fn isolated_plain(source: &str, range: Range<usize>) -> bool {
    let prefix = &source[..range.start];
    let suffix = &source[range.end..];
    (prefix.is_empty() || prefix.ends_with("\n\n") || prefix.ends_with("\n\r\n"))
        && (suffix.is_empty()
            || (source.as_bytes().get(range.end.wrapping_sub(1)) == Some(&b'\n')
                && (suffix.starts_with('\n') || suffix.starts_with("\r\n"))))
        && safe_plain_source(&source[range])
}

impl PreviewRenderer {
    /// Apply only an uninterrupted revision transition for the currently cached
    /// document. No source inspection on missed revisions/tab switches: the next
    /// show_document will rebuild. A failed batch also invalidates any partial
    /// changes, and is never presented as a valid cached revision.
    pub(in crate::ui) fn apply_edits(
        &mut self,
        id: u64,
        before: u64,
        after: u64,
        edits: &[EditDelta],
    ) -> bool {
        if self.identity != Some((id, before)) || edits.is_empty() {
            return false;
        }
        self.identity = None;
        self.standalone = None;
        if before.checked_add(1) != Some(after) {
            return false;
        }
        for edit in edits {
            if edit.replacement || !self.apply_region(edit) {
                return false;
            }
        }
        // Block count/IDs and all unaffected layouts are unchanged. Rebase-only
        // bookkeeping scales with blocks, not their source bytes. No hashes or
        // reparsing of unchanged block contents occur here.
        self.retained.clear();
        self.retained_bytes = 0;
        self.pending_images.clear();
        self.pending_image_set.clear();
        self.refinement = 0;
        for (i, block) in self.blocks.iter_mut().enumerate() {
            let bytes = block.galley.as_ref().map_or(0, |g| g.job.text.len())
                + block.fragments.as_ref().map_or(0, PlainFragments::bytes);
            if bytes > 0 {
                self.retained.push_back(i);
                self.retained_bytes += bytes;
            }
            if !block.segment.images.is_empty() {
                self.pending_images.push_back(i);
                self.pending_image_set.insert(i);
            }
            if !block.measured
                && let Some(key) = &self.layout_key
            {
                block.height = estimated_height(block, key);
            }
        }
        self.heights = Heights::from_blocks(&self.blocks);
        self.identity = Some((id, after));
        true
    }

    fn apply_region(&mut self, edit: &EditDelta) -> bool {
        let index = self
            .blocks
            .partition_point(|block| block.segment.source_range.start <= edit.bytes.start)
            .saturating_sub(1);
        let Some(block) = self.blocks.get_mut(index) else {
            return false;
        };
        let old = &block.segment;
        if !old.editable_plain || edit.bytes.start < old.source_range.start {
            return false;
        }
        let PreviewSegmentKind::Markdown(source) = &old.kind else {
            return false;
        };
        let range =
            edit.bytes.start - old.source_range.start..edit.bytes.end - old.source_range.start;
        // Do not touch the final line terminator or the surrounding blank lines.
        if range.end > body_end(source)
            || !source.is_char_boundary(range.start)
            || !source.is_char_boundary(range.end)
        {
            return false;
        }
        let mut source = source.clone();
        source.replace_range(range, &edit.inserted);
        if !safe_plain_source(&source) {
            return false;
        }
        #[cfg(test)]
        {
            self.region_parses += 1;
            self.region_bytes += source.len();
        }
        let mut parsed = split_preview_segments(&source);
        if parsed.len() != 1
            || !parsed[0].editable_plain
            || parsed[0].plain.is_none()
            || parsed[0].source_range != (0..source.len())
        {
            return false;
        }
        let mut segment = parsed.pop().unwrap();
        segment.start_line += old.start_line;
        segment.source_range.start += old.source_range.start;
        segment.source_range.end += old.source_range.start;
        block.fingerprint = hash((&segment.kind, &segment.embedded_mermaids));
        block.galley = None;
        if segment.plain.as_ref().unwrap().len() > PLAIN_FRAGMENT_BYTES {
            if let Some(fragments) = &mut block.fragments {
                fragments.revise(segment.plain.as_ref().unwrap());
            }
        } else {
            block.fragments = None;
        }
        block.measured = false;
        block.segment = segment;
        for block in &mut self.blocks[index + 1..] {
            block.segment.source_range.start = rebase(
                block.segment.source_range.start,
                edit.bytes.len(),
                edit.inserted.len(),
            );
            block.segment.source_range.end = rebase(
                block.segment.source_range.end,
                edit.bytes.len(),
                edit.inserted.len(),
            );
            block.segment.start_line = rebase(
                block.segment.start_line,
                edit.removed_newlines,
                edit.inserted_newlines,
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::edit::{LineMetadata, RecordingBuffer};
    use eframe::egui::TextBuffer;

    fn initialized(source: &str) -> PreviewRenderer {
        let mut renderer = PreviewRenderer::default();
        renderer.update(source, false);
        renderer.identity = Some((1, 0));
        renderer
    }

    fn assert_full(renderer: &PreviewRenderer, source: &str) {
        let actual = renderer
            .blocks
            .iter()
            .map(|b| b.segment.clone())
            .collect::<Vec<_>>();
        assert_eq!(actual, split_preview_segments(source), "{source:?}");
    }

    fn update(renderer: &mut PreviewRenderer, source: &str, revision: u64) {
        if renderer.identity != Some((1, revision)) {
            renderer.update(source, true);
            renderer.identity = Some((1, revision));
        }
        assert_full(renderer, source);
    }

    #[test]
    fn ordinary_local_edits_parse_only_the_paragraph_and_preserve_other_blocks() {
        let original = "Plain paragraph\r\nsecond line\r\n\r\n# Heading\r\n\r\nAnother paragraph\r\n\r\n[link][id]\r\n\r\n[id]: https://example.test\r\n";
        for (offset, inserted, delete) in [
            (0, "猫", 0),
            (5, "e\u{301}👩\u{200d}🚀", 0),
            (10, "\r\nnew line", 0),
            (8, "", 3),
            (28, "end", 0),
        ] {
            let mut source = original.to_owned();
            let mut index = LineMetadata::from_content(&source);
            let mut renderer = initialized(&source);
            let ids = renderer.blocks.iter().map(|b| b.id).collect::<Vec<_>>();
            let mut buffer = RecordingBuffer::new(&mut source, &mut index);
            buffer.delete_char_range(offset..offset + delete);
            buffer.insert_text(inserted, offset);
            let edits = buffer.finish();
            assert!(renderer.apply_edits(1, 0, 1, &edits), "offset {offset}");
            assert_full(&renderer, &source);
            assert_eq!(renderer.parses, 1);
            assert_eq!(renderer.full_segment_bytes, original.len());
            assert_eq!(
                ids,
                renderer.blocks.iter().map(|b| b.id).collect::<Vec<_>>()
            );
            assert!(renderer.region_bytes < original.len() * 2);
        }
    }

    #[test]
    fn operation_order_and_source_mapping_after_multiline_and_multiblock_edits() {
        let mut source = "Alpha words\n\nBeta words\n\nGamma words".to_owned();
        let mut renderer = initialized(&source);
        let mut index = LineMetadata::from_content(&source);
        let mut buffer = RecordingBuffer::new(&mut source, &mut index);
        buffer.insert_text("new\n", 0);
        buffer.delete_char_range(6..8);
        buffer.insert_text("猫", 8);
        buffer.insert_text("end", 35);
        let edits = buffer.finish();
        assert!(renderer.apply_edits(1, 0, 1, &edits));
        assert_full(&renderer, &source);
        assert_eq!(renderer.parses, 1);
        assert_eq!(renderer.region_parses, 4);
    }

    #[test]
    fn structural_context_boundaries_replacements_and_missed_revisions_rebuild() {
        let contexts = [
            "Alpha words\n\nBeta words",
            "Alpha words\n---\n\nBeta words",
            "- item\n  continuation\n\nBeta words",
            "> quote\n\nBeta words",
            "```\nAlpha words\n\nBeta words\n```",
            "    Alpha words\n\nBeta words",
            "[ref]: https://example.test\n\n[ref]\n\nBeta words",
            "[^note]: Alpha words\n\ntext[^note]\n\nBeta words",
            " Alpha words\n\nBeta words",
            "Alpha words  \n\nBeta words",
        ];
        for original in contexts {
            for inserted in [
                "[x]: /url\n",
                "[^x]: note\n",
                "```\n",
                "- item\n",
                "\n---",
                " ",
                "\n\n",
                "# ",
            ] {
                let mut source = original.to_owned();
                let mut index = LineMetadata::from_content(&source);
                let mut renderer = initialized(&source);
                let mut buffer = RecordingBuffer::new(&mut source, &mut index);
                buffer.insert_text(inserted, 0);
                let edits = buffer.finish();
                assert!(
                    !renderer.apply_edits(1, 0, 1, &edits),
                    "{original:?} + {inserted:?}"
                );
                update(&mut renderer, &source, 1);
                assert_eq!(renderer.parses, 2);
            }
            let mut source = original.to_owned();
            let mut index = LineMetadata::from_content(&source);
            let mut renderer = initialized(&source);
            let mut buffer = RecordingBuffer::new(&mut source, &mut index);
            buffer.replace_with("Undo replacement\r\n");
            let edits = buffer.finish();
            assert!(!renderer.apply_edits(1, 0, 1, &edits));
            update(&mut renderer, &source, 1);
        }
        let mut renderer = initialized("Alpha words");
        let mut source = "Alpha words".to_owned();
        let mut index = LineMetadata::from_content(&source);
        let mut buffer = RecordingBuffer::new(&mut source, &mut index);
        buffer.insert_text("x", 1);
        let edits = buffer.finish();
        assert!(!renderer.apply_edits(1, 2, 3, &edits));
        assert!(!renderer.apply_edits(2, 0, 1, &edits));
        assert!(!renderer.apply_edits(1, 0, 2, &edits));
        update(&mut renderer, &source, 3);
    }

    #[test]
    fn failed_later_operation_never_publishes_partially_updated_preview() {
        let mut source = "Alpha words\n\nBeta words".to_owned();
        let mut renderer = initialized(&source);
        let mut index = LineMetadata::from_content(&source);
        let mut buffer = RecordingBuffer::new(&mut source, &mut index);
        buffer.insert_text("x", 1); // eligible first operation
        buffer.insert_text("[ref]: /path\n\n", 0); // global structural change
        let edits = buffer.finish();
        assert!(!renderer.apply_edits(1, 0, 1, &edits));
        assert_eq!(renderer.identity, None);
        update(&mut renderer, &source, 1);
    }

    #[test]
    fn large_document_work_counts_show_bounded_source_segmentation() {
        let mut source = "Ordinary paragraph with words\r\n\r\n".repeat(10_000);
        let mut renderer = initialized(&source);
        let mut index = LineMetadata::from_content(&source);
        let mut buffer = RecordingBuffer::new(&mut source, &mut index);
        buffer.insert_text("猫", 160_010);
        let edits = buffer.finish();
        assert!(renderer.apply_edits(1, 0, 1, &edits));
        assert_full(&renderer, &source);
        assert_eq!(renderer.parses, 1);
        assert_eq!(renderer.region_parses, 1);
        assert_eq!(
            renderer.region_bytes,
            "Ordinary paragraph with words猫\r\n".len()
        );
        println!(
            "issue9 work: document_bytes={} unchanged_full_parses={} region_parses={} region_bytes={} (offset/height rebasing remains block-count dependent)",
            source.len(),
            renderer.parses,
            renderer.region_parses,
            renderer.region_bytes
        );
    }
}
