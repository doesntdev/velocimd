//! Actual TextEdit mutations, in operation order and in pre-operation coordinates.
//! No shadow copy, source diff, or warm-frame content probe is needed.
use eframe::egui::TextBuffer;
use std::{any::TypeId, ops::Range};

#[derive(Debug)]
pub(super) struct EditDelta {
    pub bytes: Range<usize>,
    pub inserted: String,
    pub removed_newlines: usize,
    pub inserted_newlines: usize,
    // egui undo/redo uses replace_with, not a localized edit.
    pub replacement: bool,
}

pub(super) fn rebase(value: usize, removed: usize, inserted: usize) -> usize {
    value - removed + inserted
}

#[derive(Default)]
pub(super) struct LineMetadataCache {
    document_id: Option<u64>,
    revision: u64,
    pub metadata: LineMetadata,
    #[cfg(test)]
    pub full_scans: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LineMetadata {
    pub line_starts: Vec<usize>, // Unicode scalar offsets, as used by egui.
    byte_starts: Vec<usize>,
    chars: usize,
    bytes: usize,
    #[cfg(test)]
    pub inserted_chars_scanned: usize,
}

impl Default for LineMetadata {
    fn default() -> Self {
        Self {
            line_starts: vec![0],
            byte_starts: vec![0],
            chars: 0,
            bytes: 0,
            #[cfg(test)]
            inserted_chars_scanned: 0,
        }
    }
}

impl LineMetadataCache {
    pub fn line_count(&mut self, id: u64, revision: u64, content: &str) -> usize {
        self.metadata(id, revision, content).line_starts.len()
    }

    pub fn metadata(&mut self, id: u64, revision: u64, content: &str) -> &LineMetadata {
        if self.document_id != Some(id) || self.revision != revision {
            self.metadata = LineMetadata::from_content(content);
            self.document_id = Some(id);
            self.revision = revision;
            #[cfg(test)]
            {
                self.full_scans += 1;
            }
        }
        &self.metadata
    }

    /// The recording buffer updated this index before the document's revision
    /// advanced. Publish only the exact transition that it observed.
    pub fn finish_edit(&mut self, id: u64, before: u64, after: u64) {
        if self.document_id == Some(id) && self.revision == before {
            self.revision = after;
        } else {
            self.document_id = None;
        }
    }
}

impl LineMetadata {
    pub fn from_content(content: &str) -> Self {
        let mut result = Self::default();
        for (index, (byte, character)) in content.char_indices().enumerate() {
            if character == '\n' {
                result.line_starts.push(index + 1);
                result.byte_starts.push(byte + 1);
            }
            result.chars += 1;
        }
        result.bytes = content.len();
        result
    }

    fn byte_for_char(&self, content: &str, index: usize) -> usize {
        let index = index.min(self.chars);
        let line = self.line_starts.partition_point(|&start| start <= index) - 1;
        let byte = self.byte_starts[line];
        byte + content[byte..]
            .char_indices()
            .nth(index - self.line_starts[line])
            .map_or(content.len() - byte, |(i, _)| i)
    }

    fn apply(
        &mut self,
        chars: Range<usize>,
        bytes: Range<usize>,
        inserted: &str,
    ) -> (usize, usize, usize) {
        let start = self
            .line_starts
            .partition_point(|&offset| offset <= chars.start);
        let end = self
            .line_starts
            .partition_point(|&offset| offset <= chars.end);
        let removed_newlines = end - start;
        let mut new_chars = Vec::new();
        let mut new_bytes = Vec::new();
        let mut count = 0;
        for (byte, character) in inserted.char_indices() {
            if character == '\n' {
                new_chars.push(chars.start + count + 1);
                new_bytes.push(bytes.start + byte + 1);
            }
            count += 1;
        }
        let added_newlines = new_chars.len();
        for offset in &mut self.line_starts[end..] {
            *offset = rebase(*offset, chars.len(), count);
        }
        for offset in &mut self.byte_starts[end..] {
            *offset = rebase(*offset, bytes.len(), inserted.len());
        }
        self.line_starts.splice(start..end, new_chars);
        self.byte_starts.splice(start..end, new_bytes);
        self.chars = rebase(self.chars, chars.len(), count);
        self.bytes = rebase(self.bytes, bytes.len(), inserted.len());
        #[cfg(test)]
        {
            self.inserted_chars_scanned += count;
        }
        (count, removed_newlines, added_newlines)
    }
}

pub(super) struct RecordingBuffer<'a> {
    content: &'a mut String,
    index: &'a mut LineMetadata,
    edits: Vec<EditDelta>,
}

impl<'a> RecordingBuffer<'a> {
    pub fn new(content: &'a mut String, index: &'a mut LineMetadata) -> Self {
        debug_assert_eq!(content.len(), index.bytes);
        Self {
            content,
            index,
            edits: Vec::new(),
        }
    }

    pub fn finish(self) -> Vec<EditDelta> {
        self.edits
    }

    fn replace(&mut self, chars: Range<usize>, text: &str, replacement: bool) -> usize {
        let chars = chars.start.min(self.index.chars)..chars.end.min(self.index.chars);
        assert!(chars.start <= chars.end);
        if chars.is_empty() && text.is_empty() {
            return 0;
        }
        let bytes = self.index.byte_for_char(self.content, chars.start)
            ..self.index.byte_for_char(self.content, chars.end);
        let (count, removed_newlines, inserted_newlines) =
            self.index.apply(chars, bytes.clone(), text);
        self.content.replace_range(bytes.clone(), text);
        self.edits.push(EditDelta {
            bytes,
            inserted: text.to_owned(),
            removed_newlines,
            inserted_newlines,
            replacement,
        });
        count
    }
}

impl TextBuffer for RecordingBuffer<'_> {
    fn is_mutable(&self) -> bool {
        true
    }
    fn as_str(&self) -> &str {
        self.content
    }
    fn insert_text(&mut self, text: &str, char_index: usize) -> usize {
        self.replace(char_index..char_index, text, false)
    }
    fn delete_char_range(&mut self, range: Range<usize>) {
        assert!(range.start <= range.end);
        self.replace(range, "", false);
    }
    fn byte_index_from_char_index(&self, index: usize) -> usize {
        self.index.byte_for_char(self.content, index)
    }
    fn clear(&mut self) {
        self.replace(0..self.index.chars, "", true);
    }
    fn replace_with(&mut self, text: &str) {
        self.replace(0..self.index.chars, text, true);
    }
    fn type_id(&self) -> TypeId {
        TypeId::of::<RecordingBuffer<'static>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_index(actual: &LineMetadata, content: &str) {
        let full = LineMetadata::from_content(content);
        assert_eq!(actual.line_starts, full.line_starts, "{content:?}");
        assert_eq!(actual.byte_starts, full.byte_starts, "{content:?}");
        assert_eq!(actual.chars, full.chars);
        assert_eq!(actual.bytes, full.bytes);
        for i in
            (0..=actual.chars.min(100)).chain([actual.chars / 2, actual.chars, actual.chars + 1])
        {
            assert_eq!(
                actual.byte_for_char(content, i),
                content
                    .char_indices()
                    .nth(i)
                    .map_or(content.len(), |(b, _)| b)
            );
        }
    }

    #[test]
    fn differential_ordered_mutations_unicode_crlf_and_replacements() {
        for original in [
            "",
            "a\r\nb\n",
            "日本語 e\u{301} 👩\u{200d}🚀\r\nlast",
            "\n\nlast\n",
        ] {
            let mut content = original.to_owned();
            let mut index = LineMetadata::from_content(original);
            // All character boundaries, beginning/middle/end, including CR/LF
            // separately and combining/ZWJ scalars (not UTF-8 byte slicing).
            for pos in 0..=original.chars().count() {
                let mut buffer = RecordingBuffer::new(&mut content, &mut index);
                buffer.insert_text("猫\r\ne\u{301}", pos);
                assert_index(buffer.index, buffer.content);
                buffer.delete_char_range(pos..pos + 5);
                assert_eq!(buffer.as_str(), original);
                assert_index(buffer.index, buffer.content);
                buffer.insert_text("!", usize::MAX);
                assert_index(buffer.index, buffer.content);
                buffer.replace_with(original);
                assert_index(buffer.index, buffer.content);
                let edits = buffer.finish();
                assert_eq!(edits.len(), 4);
                assert!(edits.last().unwrap().replacement);
                assert_eq!(content, original);
            }
        }
    }

    #[test]
    fn all_deletion_ranges_match_string_and_take_clear_the_live_index() {
        let original = "猫e\u{301}\r\n👩\u{200d}🚀\nlast\r\n";
        for start in 0..=original.chars().count() {
            for end in start..=original.chars().count() + 1 {
                let mut content = original.to_owned();
                let mut reference = content.clone();
                let mut index = LineMetadata::from_content(original);
                let mut buffer = RecordingBuffer::new(&mut content, &mut index);
                buffer.delete_char_range(start..end);
                reference.delete_char_range(start..end);
                assert_eq!(buffer.as_str(), reference);
                assert_index(buffer.index, &reference);
                assert_eq!(buffer.take(), reference);
                assert_index(buffer.index, "");
                buffer.insert_text("again\r\n", 0);
                buffer.clear();
                assert_index(buffer.index, "");
            }
        }
    }

    #[test]
    fn localized_edit_has_no_full_metadata_scan_and_revision_gaps_fall_back() {
        let mut content = "ordinary paragraph\r\n\r\n".repeat(10_000);
        let mut cache = LineMetadataCache::default();
        cache.metadata(7, 0, &content);
        let mut buffer = RecordingBuffer::new(&mut content, &mut cache.metadata);
        buffer.insert_text("猫", 100_000);
        let edits = buffer.finish();
        assert_eq!(edits.len(), 1);
        cache.finish_edit(7, 0, 1);
        cache.metadata(7, 1, &content);
        assert_eq!(cache.full_scans, 1);
        assert_eq!(cache.metadata.inserted_chars_scanned, 1);
        assert_index(&cache.metadata, &content[..]);
        cache.metadata(7, 3, &content);
        assert_eq!(cache.full_scans, 2);
        cache.metadata(8, 3, &content);
        assert_eq!(cache.full_scans, 3);
    }
}
