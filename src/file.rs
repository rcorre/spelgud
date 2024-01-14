type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct File {
    text: String,
}

impl File {
    pub fn new(text: String) -> Result<File> {
        Ok(File { text })
    }

    pub fn edit(&mut self, changes: Vec<lsp_types::TextDocumentContentChangeEvent>) -> Result<()> {
        for change in changes {
            let range = change
                .range
                .ok_or("No range in change notification {change:?}")?;
            let mut lines = self.text.split_inclusive("\n").peekable();
            // First count bytes in all lines preceding the edit.
            let start_byte = lines
                .by_ref()
                .take(range.start.line.try_into()?)
                .map(str::len)
                .sum::<usize>();
            // Now add bytes up to the character within the start line.
            let start_offset = lines
                .peek()
                .map(|line| char_to_byte(&line, range.start.character))
                .unwrap_or(0);
            let start_byte = start_byte + start_offset;
            // Now count bytes in all lines following the edit.
            let end_byte = start_byte
                + lines
                    .by_ref()
                    .take((range.end.line - range.start.line).try_into()?)
                    .map(str::len)
                    .sum::<usize>();
            // Now add bytes up to the character within the end line.
            let end_offset = lines
                .peek()
                .map(|line| char_to_byte(&line, range.end.character))
                .unwrap_or(0);
            let end_byte = end_byte + end_offset - start_offset;

            log::trace!(
                "Computing change {start_byte}..{end_byte} with text {}",
                change.text
            );

            self.text.replace_range(start_byte..end_byte, &change.text);
        }
        log::trace!("Edited text to: {}", self.text);

        Ok(())
    }

    pub fn text(&self) -> &str {
        self.text.as_str()
    }
}

fn char_to_byte(line: &str, char: u32) -> usize {
    line.chars()
        .take(char.try_into().unwrap())
        .map(|c| c.len_utf8())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit() {
        let text = "yn";
        let mut file = File::new(text.into()).unwrap();
        assert_eq!(file.text, text);

        let change = |(start_line, start_char), (end_line, end_char), text: &str| {
            lsp_types::TextDocumentContentChangeEvent {
                range: Some(lsp_types::Range {
                    start: lsp_types::Position {
                        line: start_line,
                        character: start_char,
                    },
                    end: lsp_types::Position {
                        line: end_line,
                        character: end_char,
                    },
                }),
                range_length: None,
                text: text.into(),
            }
        };

        file.edit(vec![]).unwrap();
        assert_eq!(file.text, text);

        file.edit(vec![change((0, 0), (0, 0), "s")]).unwrap();
        assert_eq!(file.text, "syn");

        file.edit(vec![change((0, 3), (0, 3), "tax = \"proto2\";\n")])
            .unwrap();
        assert_eq!(file.text, "syntax = \"proto2\";\n");

        file.edit(vec![change((0, 10), (0, 16), "proto3")]).unwrap();
        assert_eq!(file.text, "syntax = \"proto3\";\n");

        file.edit(vec![change((1, 0), (1, 0), "message Foo {}\n")])
            .unwrap();
        assert_eq!(file.text, "syntax = \"proto3\";\nmessage Foo {}\n");

        file.edit(vec![change((1, 13), (1, 14), "\n\n}")]).unwrap();
        assert_eq!(file.text, "syntax = \"proto3\";\nmessage Foo {\n\n}\n");

        file.edit(vec![change(
            (2, 0),
            (2, 0),
            "uint32 i = 1;\nstring s = 2;\nbytes b = 3;",
        )])
        .unwrap();
        assert_eq!(
            file.text,
            [
                "syntax = \"proto3\";",
                "message Foo {",
                "uint32 i = 1;",
                "string s = 2;",
                "bytes b = 3;",
                "}",
                ""
            ]
            .join("\n")
        );

        file.edit(vec![change(
            (2, 0),
            (5, 0),
            "uint32 i = 2;\nstring s = 3;\nbytes b = 4;\n",
        )])
        .unwrap();
        assert_eq!(
            file.text,
            [
                "syntax = \"proto3\";",
                "message Foo {",
                "uint32 i = 2;",
                "string s = 3;",
                "bytes b = 4;",
                "}",
                ""
            ]
            .join("\n")
        );

        file.edit(vec![change((2, 4), (3, 8), "64 u = 2;\nstring str")])
            .unwrap();
        assert_eq!(
            file.text,
            [
                "syntax = \"proto3\";",
                "message Foo {",
                "uint64 u = 2;",
                "string str = 3;",
                "bytes b = 4;",
                "}",
                ""
            ]
            .join("\n")
        );

        file.edit(vec![change((3, 13), (4, 0), "5;\n")]).unwrap();
        assert_eq!(
            file.text,
            [
                "syntax = \"proto3\";",
                "message Foo {",
                "uint64 u = 2;",
                "string str = 5;",
                "bytes b = 4;",
                "}",
                ""
            ]
            .join("\n")
        );
    }

    #[test]
    fn test_edit_unicode() {
        let text = [
            "syntax = \"proto3\";",
            "import \"examp𐐀e.proto\";",
            "import \"other.proto\";",
            "",
        ]
        .join("\n");
        let mut file = File::new(text.clone()).unwrap();
        assert_eq!(file.text, text);

        let change = |(start_line, start_char), (end_line, end_char), text: &str| {
            lsp_types::TextDocumentContentChangeEvent {
                range: Some(lsp_types::Range {
                    start: lsp_types::Position {
                        line: start_line,
                        character: start_char,
                    },
                    end: lsp_types::Position {
                        line: end_line,
                        character: end_char,
                    },
                }),
                range_length: None,
                text: text.into(),
            }
        };

        file.edit(vec![change((1, 8), (1, 15), "thing")]).unwrap();
        assert_eq!(
            file.text,
            [
                "syntax = \"proto3\";",
                "import \"thing.proto\";",
                "import \"other.proto\";",
                "",
            ]
            .join("\n")
        );
    }
}
