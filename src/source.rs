use proc_macro2::LineColumn;

#[derive(Debug)]
pub(crate) struct Source<'a> {
    text: &'a str,
    map: SourceMap,
    newline: &'static str,
}

impl<'a> Source<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text,
            map: SourceMap::new(text),
            newline: if text.contains("\r\n") { "\r\n" } else { "\n" },
        }
    }

    pub(crate) fn text(&self) -> &'a str {
        self.text
    }

    pub(crate) fn offset(&self, location: LineColumn) -> Option<usize> {
        self.map.offset(location)
    }

    pub(crate) fn text_between(&self, start: LineColumn, end: LineColumn) -> Option<&'a str> {
        let start = self.offset(start)?;
        let end = self.offset(end)?;
        self.text.get(start..end)
    }

    pub(crate) fn indentation_at(&self, offset: usize) -> &'a str {
        self.map.indentation(self.text, offset)
    }

    pub(crate) fn newline(&self) -> &'static str {
        self.newline
    }
}

#[derive(Debug)]
struct SourceMap {
    line_starts: Vec<usize>,
}

impl SourceMap {
    fn new(source: &str) -> Self {
        let mut line_starts = vec![0];
        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }

        Self { line_starts }
    }

    fn offset(&self, location: LineColumn) -> Option<usize> {
        let line_index = location.line.checked_sub(1)?;
        let line_start = self.line_starts.get(line_index)?;
        Some(line_start + location.column)
    }

    fn indentation<'a>(&self, source: &'a str, offset: usize) -> &'a str {
        let line_index = self.line_starts.partition_point(|start| *start <= offset) - 1;
        &source[self.line_starts[line_index]..offset]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_line_columns_to_byte_offsets() {
        let source = Source::new("first\n    second\n");

        assert_eq!(source.offset(LineColumn { line: 2, column: 4 }), Some(10));
        assert_eq!(source.indentation_at(10), "    ");
        assert_eq!(
            source.text_between(
                LineColumn { line: 1, column: 2 },
                LineColumn { line: 2, column: 4 },
            ),
            Some("rst\n    ")
        );
    }

    #[test]
    fn detects_the_source_newline_style() {
        assert_eq!(Source::new("a\nb\n").newline(), "\n");
        assert_eq!(Source::new("a\r\nb\r\n").newline(), "\r\n");
    }
}
