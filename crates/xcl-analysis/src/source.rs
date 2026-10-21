//! Source positions and the file index used to render and serialize them.
//!
//! [`Span`] from the VM records a start point (line + column) but no length, so
//! it cannot say where a diagnostic *ends*. Analysis needs both: the human
//! renderer underlines a range, and the JSON renderer emits byte offsets so a
//! tool can edit the source.
//!
//! [`SpanRange`] is that pairing — a VM [`Span`] plus a length measured in
//! characters on its line. Multi-line spans are not represented yet; every rule
//! shipped so far points at a single line.

use xcl_vm::Span;

/// A single-line source range: where it starts, and how many characters it spans.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpanRange {
    /// File the range belongs to.
    pub file: String,
    /// 1-based line number.
    pub line: u32,
    /// 1-based column of the first character.
    pub col: u32,
    /// Number of characters covered, at least 1.
    pub len: u32,
}

impl SpanRange {
    /// Builds a range from a VM span and a character length.
    ///
    /// `len` is clamped to at least 1 so a zero-width span still underlines
    /// something.
    pub fn new(span: &Span, len: u32) -> Self {
        Self {
            file: span.file.clone(),
            line: span.line,
            col: span.col,
            len: len.max(1),
        }
    }

    /// A range covering exactly `name` (used for identifier-sized spans).
    pub fn of_name(span: &Span, name: &str) -> Self {
        Self::new(span, name.chars().count() as u32)
    }

    /// 1-based column of the first character after the range.
    pub fn end_col(&self) -> u32 {
        self.col + self.len
    }
}

/// Line index for one source file.
///
/// Built once per program so rendering and serialization agree on byte offsets.
#[derive(Clone, Debug)]
pub struct SourceMap<'a> {
    file: String,
    source: &'a str,
    /// Byte offset of the start of each line, index 0 = line 1.
    line_starts: Vec<usize>,
}

impl<'a> SourceMap<'a> {
    /// Indexes `source`, which was read from `file`.
    pub fn new(file: impl Into<String>, source: &'a str) -> Self {
        let mut line_starts = vec![0usize];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(i + 1);
            }
        }
        Self {
            file: file.into(),
            source,
            line_starts,
        }
    }

    /// The file this map belongs to.
    pub fn file(&self) -> &str {
        &self.file
    }

    /// The source text.
    pub fn source(&self) -> &str {
        self.source
    }

    /// How many lines the file has (at least 1).
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// The text of `line` (1-based), without its newline.
    ///
    /// A trailing `\r` is dropped too, so a file saved with CRLF endings renders
    /// the same as one saved with LF.
    pub fn line(&self, line: u32) -> Option<&'a str> {
        if line == 0 {
            return None;
        }
        let idx = line as usize - 1;
        let start = *self.line_starts.get(idx)?;
        let end = self
            .line_starts
            .get(idx + 1)
            .map(|next| next.saturating_sub(1))
            .unwrap_or(self.source.len());
        let text = self.source.get(start..end).unwrap_or("");
        Some(text.strip_suffix('\r').unwrap_or(text))
    }

    /// Byte offset of (`line`, `col`), both 1-based.
    ///
    /// Columns count characters, so a line containing multi-byte text produces a
    /// byte offset that is larger than the column number.
    pub fn byte_offset(&self, line: u32, col: u32) -> Option<usize> {
        let text = self.line(line)?;
        let start = *self.line_starts.get(line as usize - 1)?;
        let mut byte = start;
        let mut remaining = col.saturating_sub(1);
        for ch in text.chars() {
            if remaining == 0 {
                break;
            }
            byte += ch.len_utf8();
            remaining -= 1;
        }
        Some(byte)
    }

    /// Byte offsets `(start, end)` for a range.
    pub fn byte_range(&self, range: &SpanRange) -> Option<(usize, usize)> {
        let start = self.byte_offset(range.line, range.col)?;
        let end = self.byte_offset(range.line, range.end_col())?;
        Some((start, end.max(start + 1)))
    }

    /// The source text a range covers, when it lies on one line.
    pub fn text_of(&self, range: &SpanRange) -> Option<&'a str> {
        let line = self.line(range.line)?;
        let start_char = range.col.saturating_sub(1) as usize;
        let end_char = start_char + range.len as usize;
        let start = line
            .char_indices()
            .nth(start_char)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        let end = line
            .char_indices()
            .nth(end_char)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        line.get(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(line: u32, col: u32) -> Span {
        Span::new("test.xcl".to_string(), line, col)
    }

    #[test]
    fn line_lookup_strips_the_newline() {
        let map = SourceMap::new("test.xcl", "one\ntwo\nthree");
        assert_eq!(map.line(1), Some("one"));
        assert_eq!(map.line(2), Some("two"));
        assert_eq!(map.line(3), Some("three"));
        assert_eq!(map.line(4), None);
        assert_eq!(map.line_count(), 3);
    }

    #[test]
    fn byte_offsets_follow_columns() {
        let map = SourceMap::new("test.xcl", "abc\ndef");
        assert_eq!(map.byte_offset(1, 1), Some(0));
        assert_eq!(map.byte_offset(1, 3), Some(2));
        assert_eq!(map.byte_offset(2, 1), Some(4));
    }

    #[test]
    fn multi_byte_characters_advance_byte_offsets_correctly() {
        let map = SourceMap::new("test.xcl", "é = 1");
        // `é` is two bytes, so column 3 (the `=`) is byte 3.
        assert_eq!(map.byte_offset(1, 3), Some(3));
    }

    #[test]
    fn text_of_extracts_the_covered_characters() {
        let map = SourceMap::new("test.xcl", "$x = 1");
        let range = SpanRange::new(&span(1, 1), 2);
        assert_eq!(map.text_of(&range), Some("$x"));
        let (start, end) = map.byte_range(&range).unwrap();
        assert_eq!((start, end), (0, 2));
    }
}
