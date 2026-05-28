use std::ops::Range;

use gpui_component::input::Position;

pub(crate) fn lsp_range_to_input_range(
    text: &str,
    range: &lsp_types::Range,
) -> Option<Range<Position>> {
    let byte_range = lsp_range_to_byte_range(text, range)?;
    Some(
        input_position_from_byte_index(text, byte_range.start)
            ..input_position_from_byte_index(text, byte_range.end),
    )
}

pub(crate) fn lsp_range_to_byte_range(
    text: &str,
    range: &lsp_types::Range,
) -> Option<Range<usize>> {
    let start = lsp_position_to_byte_index(text, range.start)?;
    let end = lsp_position_to_byte_index(text, range.end)?;
    (start <= end).then_some(start..end)
}

pub(crate) fn lsp_position_to_byte_index(
    text: &str,
    position: lsp_types::Position,
) -> Option<usize> {
    let mut line_start = 0usize;
    let mut current_line = 0u32;

    for (idx, ch) in text.char_indices() {
        if current_line == position.line {
            return utf16_character_to_byte_index(&text[line_start..], position.character)
                .map(|offset| line_start + offset);
        }
        if ch == '\n' {
            current_line += 1;
            line_start = idx + ch.len_utf8();
        }
    }

    if current_line == position.line {
        return utf16_character_to_byte_index(&text[line_start..], position.character)
            .map(|offset| line_start + offset);
    }

    None
}

fn utf16_character_to_byte_index(line_text: &str, target_utf16: u32) -> Option<usize> {
    let mut utf16_units = 0u32;

    for (byte_idx, ch) in line_text.char_indices() {
        if ch == '\n' {
            break;
        }
        if utf16_units == target_utf16 {
            return Some(byte_idx);
        }
        utf16_units += ch.len_utf16() as u32;
        if utf16_units > target_utf16 {
            return None;
        }
    }

    (utf16_units == target_utf16).then_some(line_text.find('\n').unwrap_or(line_text.len()))
}

fn input_position_from_byte_index(text: &str, byte_index: usize) -> Position {
    let mut line = 0u32;
    let mut character = 0u32;
    let mut chars = text.char_indices().peekable();

    while let Some((current_byte_index, ch)) = chars.next() {
        if current_byte_index >= byte_index {
            break;
        }

        match ch {
            '\r' => {
                line += 1;
                character = 0;
                if chars.peek().is_some_and(|(next_byte_index, next)| {
                    *next_byte_index < byte_index && *next == '\n'
                }) {
                    chars.next();
                }
            }
            '\n' => {
                line += 1;
                character = 0;
            }
            _ => {
                character += 1;
            }
        }
    }

    Position { line, character }
}

#[cfg(test)]
mod tests {
    use gpui_component::input::Position;

    use super::{
        input_position_from_byte_index, lsp_position_to_byte_index, lsp_range_to_byte_range,
        lsp_range_to_input_range,
    };

    #[test]
    fn spchk_test4_ascii_lsp_range_converts_to_byte_range() {
        let text = "abc def";
        let range = lsp_types::Range::new(
            lsp_types::Position::new(0, 4),
            lsp_types::Position::new(0, 7),
        );

        assert_eq!(lsp_range_to_byte_range(text, &range), Some(4..7));
    }

    #[test]
    fn spchk_test4_emoji_utf16_position_must_not_split_codepoint() {
        let text = "a😊b";

        assert_eq!(
            lsp_position_to_byte_index(text, lsp_types::Position::new(0, 1)),
            Some(1)
        );
        assert_eq!(
            lsp_position_to_byte_index(text, lsp_types::Position::new(0, 2)),
            None
        );
        assert_eq!(
            lsp_position_to_byte_index(text, lsp_types::Position::new(0, 3)),
            Some(5)
        );
    }

    #[test]
    fn spchk_test4_cjk_and_crlf_multiline_ranges_are_supported() {
        let text = "ab\r\n漢字cd\nlast";
        let range = lsp_types::Range::new(
            lsp_types::Position::new(1, 0),
            lsp_types::Position::new(1, 2),
        );

        assert_eq!(lsp_range_to_byte_range(text, &range), Some(4..10));
    }

    #[test]
    fn spchk_may28_test1_lsp_utf16_range_converts_to_input_character_range() {
        let text = "a😊b";
        let range = lsp_types::Range::new(
            lsp_types::Position::new(0, 3),
            lsp_types::Position::new(0, 4),
        );

        assert_eq!(
            lsp_range_to_input_range(text, &range),
            Some(Position::new(0, 2)..Position::new(0, 3))
        );
    }

    #[test]
    fn spchk_may28_test2_byte_index_to_input_position_treats_crlf_as_one_line_break() {
        let text = "ab\r\nc";

        assert_eq!(input_position_from_byte_index(text, 4), Position::new(1, 0));
        assert_eq!(input_position_from_byte_index(text, 5), Position::new(1, 1));
    }
}
