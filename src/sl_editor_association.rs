#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    SingleLine,
    Editor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnterTransferResult {
    pub new_singleline_text: String,
    pub new_singleline_cursor_char: usize,
    pub new_editor_text: String,
    pub new_editor_cursor_line: u32,
    pub new_editor_cursor_char: u32,
    pub focus_target: FocusTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackspaceTransferResult {
    pub new_singleline_text: String,
    pub new_singleline_cursor_char: usize,
    pub new_editor_text: String,
    pub new_editor_cursor_line: u32,
    pub new_editor_cursor_char: u32,
    pub focus_target: FocusTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownCursorTransferResult {
    pub new_editor_cursor_line: u32,
    pub new_editor_cursor_char: u32,
    pub focus_target: FocusTarget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpCursorTransferResult {
    pub new_singleline_cursor_char: usize,
    pub focus_target: FocusTarget,
}

fn byte_index_at_char(text: &str, char_index: usize) -> Option<usize> {
    if char_index == text.chars().count() {
        return Some(text.len());
    }

    text.char_indices().nth(char_index).map(|(idx, _)| idx)
}

fn split_at_char_index(text: &str, char_index: usize) -> Option<(&str, &str)> {
    let byte_idx = byte_index_at_char(text, char_index)?;
    Some((&text[..byte_idx], &text[byte_idx..]))
}

fn split_first_line(text: &str) -> (&str, &str) {
    if let Some(idx) = text.find('\n') {
        (&text[..idx], &text[idx + 1..])
    } else {
        (text, "")
    }
}

fn clamp_char_index(index: usize, text: &str) -> usize {
    index.min(text.chars().count())
}

const ORIGIN_LINE: u32 = 0;
const ORIGIN_CHAR: u32 = 0;

fn make_enter_result(
    new_singleline_text: String,
    new_singleline_cursor_char: usize,
    new_editor_text: String,
) -> EnterTransferResult {
    EnterTransferResult {
        new_singleline_text,
        new_singleline_cursor_char,
        new_editor_text,
        new_editor_cursor_line: ORIGIN_LINE,
        new_editor_cursor_char: ORIGIN_CHAR,
        focus_target: FocusTarget::Editor,
    }
}

fn make_backspace_result(
    new_singleline_text: String,
    new_singleline_cursor_char: usize,
    new_editor_text: String,
) -> BackspaceTransferResult {
    BackspaceTransferResult {
        new_singleline_text,
        new_singleline_cursor_char,
        new_editor_text,
        new_editor_cursor_line: ORIGIN_LINE,
        new_editor_cursor_char: ORIGIN_CHAR,
        focus_target: FocusTarget::SingleLine,
    }
}

fn make_down_result(new_editor_cursor_char: u32) -> DownCursorTransferResult {
    DownCursorTransferResult {
        new_editor_cursor_line: ORIGIN_LINE,
        new_editor_cursor_char,
        focus_target: FocusTarget::Editor,
    }
}

fn make_up_result(new_singleline_cursor_char: usize) -> UpCursorTransferResult {
    UpCursorTransferResult {
        new_singleline_cursor_char,
        focus_target: FocusTarget::SingleLine,
    }
}

pub fn should_transfer_backspace(editor_cursor_line: u32, editor_cursor_char: u32) -> bool {
    editor_cursor_line == 0 && editor_cursor_char == 0
}

pub fn transfer_on_enter(
    singleline_text: &str,
    singleline_cursor_char: usize,
    editor_text: &str,
) -> Option<EnterTransferResult> {
    let (left, right) = split_at_char_index(singleline_text, singleline_cursor_char)?;
    if right.is_empty() {
        let new_editor_text = if editor_text.is_empty() {
            String::new()
        } else {
            format!("\n{editor_text}")
        };

        return Some(make_enter_result(
            left.to_string(),
            left.chars().count(),
            new_editor_text,
        ));
    }

    let new_editor_text = if editor_text.is_empty() {
        right.to_string()
    } else {
        format!("{right}\n{editor_text}")
    };

    Some(make_enter_result(
        left.to_string(),
        left.chars().count(),
        new_editor_text,
    ))
}

pub fn transfer_on_backspace(
    singleline_text: &str,
    singleline_cursor_char: usize,
    editor_text: &str,
) -> Option<BackspaceTransferResult> {
    let (editor_head, editor_tail) = split_first_line(editor_text);
    if editor_head.is_empty() {
        if editor_tail.is_empty() {
            return None;
        }

        return Some(make_backspace_result(
            singleline_text.to_string(),
            singleline_text.chars().count(),
            editor_tail.to_string(),
        ));
    }

    let (prefix, suffix) = split_at_char_index(singleline_text, singleline_cursor_char)?;

    if editor_tail.is_empty() {
        let mut new_singleline_text =
            String::with_capacity(prefix.len() + suffix.len() + editor_head.len());
        new_singleline_text.push_str(prefix);
        new_singleline_text.push_str(suffix);
        new_singleline_text.push_str(editor_head);
        let new_singleline_cursor_char = new_singleline_text.chars().count();

        return Some(make_backspace_result(
            new_singleline_text,
            new_singleline_cursor_char,
            String::new(),
        ));
    }

    let mut new_singleline_text =
        String::with_capacity(prefix.len() + editor_head.len() + suffix.len());
    new_singleline_text.push_str(prefix);
    new_singleline_text.push_str(editor_head);
    new_singleline_text.push_str(suffix);

    Some(make_backspace_result(
        new_singleline_text,
        prefix.chars().count(),
        editor_tail.to_string(),
    ))
}

pub fn transfer_on_down(
    singleline_cursor_char: usize,
    editor_text: &str,
) -> DownCursorTransferResult {
    let (editor_head, _) = split_first_line(editor_text);
    let clamped_cursor_char = clamp_char_index(singleline_cursor_char, editor_head);

    make_down_result(clamped_cursor_char.min(u32::MAX as usize) as u32)
}

pub fn transfer_on_up(
    editor_cursor_line: u32,
    editor_cursor_char: u32,
    singleline_text: &str,
) -> Option<UpCursorTransferResult> {
    if editor_cursor_line != 0 {
        return None;
    }

    let clamped_cursor_char = clamp_char_index(editor_cursor_char as usize, singleline_text);

    Some(make_up_result(clamped_cursor_char))
}

#[cfg(test)]
mod tests {
    use super::{
        FocusTarget, should_transfer_backspace, transfer_on_backspace, transfer_on_down,
        transfer_on_enter, transfer_on_up,
    };

    #[test]
    fn assoc_test1_req_assoc1_ascii_forward_transfer() {
        let result = transfer_on_enter("abcdefghijkl", 6, "xyz").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "abcdef");
        assert_eq!(result.new_singleline_cursor_char, 6);
        assert_eq!(result.new_editor_text, "ghijkl\nxyz");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::Editor);
    }

    #[test]
    fn assoc_test2_req_assoc2_ascii_reverse_transfer() {
        let result = transfer_on_backspace("abcdef", 6, "ghijkl\nxyz").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "abcdefghijkl");
        assert_eq!(result.new_singleline_cursor_char, 6);
        assert_eq!(result.new_editor_text, "xyz");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test3_req_assoc3_multibyte_forward_transfer() {
        let result = transfer_on_enter("こんにちは世界", 4, "大好き").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "こんにち");
        assert_eq!(result.new_singleline_cursor_char, 4);
        assert_eq!(result.new_editor_text, "は世界\n大好き");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::Editor);
    }

    #[test]
    fn assoc_test4_req_assoc4_multibyte_reverse_transfer() {
        let result =
            transfer_on_backspace("こんにち", 4, "は世界\n大好き").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "こんにちは世界");
        assert_eq!(result.new_singleline_cursor_char, 4);
        assert_eq!(result.new_editor_text, "大好き");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test5_enter_with_invalid_cursor_is_no_op() {
        assert!(transfer_on_enter("abcdef", 100, "").is_none());
    }

    #[test]
    fn assoc_test6_non_head_backspace_does_not_transfer() {
        assert!(!should_transfer_backspace(0, 1));
        assert!(!should_transfer_backspace(1, 0));
        assert!(should_transfer_backspace(0, 0));
    }

    #[test]
    fn assoc_test7_utf8_boundary_safety_no_panic() {
        let enter_result = std::panic::catch_unwind(|| transfer_on_enter("こんにちは", 100, "x"));
        assert!(enter_result.is_ok());
        assert!(enter_result.expect("enter result").is_none());

        let backspace_result =
            std::panic::catch_unwind(|| transfer_on_backspace("こんにち", 100, "は世界"));
        assert!(backspace_result.is_ok());
        assert!(backspace_result.expect("backspace result").is_none());
    }

    #[test]
    fn assoc_test8_focus_target_is_deterministic() {
        let enter = transfer_on_enter("abcdef", 3, "xyz").expect("enter transfer");
        assert_eq!(enter.focus_target, FocusTarget::Editor);

        let backspace = transfer_on_backspace("abc", 3, "def\nxyz").expect("backspace transfer");
        assert_eq!(backspace.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test9_reverse_transfer_from_single_editor_line_appends_at_end() {
        let result = transfer_on_backspace("abcdefghijkl", 6, "xyz").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "abcdefghijklxyz");
        assert_eq!(result.new_singleline_cursor_char, 15);
        assert_eq!(result.new_editor_text, "");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test10_req_assoc5_down_same_position_ascii() {
        let result = transfer_on_down(5, "123456789");

        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 5);
        assert_eq!(result.focus_target, FocusTarget::Editor);
    }

    #[test]
    fn assoc_test11_req_assoc6_up_same_position_ascii() {
        let result = transfer_on_up(0, 5, "123456789").expect("expected transfer");

        assert_eq!(result.new_singleline_cursor_char, 5);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test12_req_assoc7_down_clamp_to_editor_tail_ascii() {
        let result = transfer_on_down(8, "123");

        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 3);
        assert_eq!(result.focus_target, FocusTarget::Editor);
    }

    #[test]
    fn assoc_test13_req_assoc8_up_clamp_to_singleline_tail_ascii() {
        let result = transfer_on_up(0, 8, "123").expect("expected transfer");

        assert_eq!(result.new_singleline_cursor_char, 3);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test14_req_assoc9_multibyte_up_down_and_clamp() {
        let down_same = transfer_on_down(2, "は世界\n大好き");
        assert_eq!(down_same.new_editor_cursor_char, 2);

        let down_clamped = transfer_on_down(5, "は世界\n大好き");
        assert_eq!(down_clamped.new_editor_cursor_char, 3);

        let up_same = transfer_on_up(0, 3, "こんにち").expect("expected transfer");
        assert_eq!(up_same.new_singleline_cursor_char, 3);

        let up_clamped = transfer_on_up(0, 9, "こんにち").expect("expected transfer");
        assert_eq!(up_clamped.new_singleline_cursor_char, 4);
    }

    #[test]
    fn assoc_test15_up_from_non_first_editor_line_is_no_transfer() {
        assert!(transfer_on_up(1, 2, "123456").is_none());
    }

    #[test]
    fn assoc_test16_down_to_empty_editor_line_clamps_to_zero() {
        let result = transfer_on_down(7, "");

        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::Editor);
    }

    #[test]
    fn assoc_test17_up_to_empty_singleline_clamps_to_zero() {
        let result = transfer_on_up(0, 9, "").expect("expected transfer");

        assert_eq!(result.new_singleline_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test18_req_assoc11_enter_at_singleline_tail_inserts_empty_editor_head() {
        let result = transfer_on_enter("abcdefg", 7, "xyz").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "abcdefg");
        assert_eq!(result.new_singleline_cursor_char, 7);
        assert_eq!(result.new_editor_text, "\nxyz");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::Editor);
    }

    #[test]
    fn assoc_test19_req_assoc12_backspace_at_empty_editor_head_moves_cursor_to_singleline_tail() {
        let result = transfer_on_backspace("abcdefg", 3, "\nxyz").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "abcdefg");
        assert_eq!(result.new_singleline_cursor_char, 7);
        assert_eq!(result.new_editor_text, "xyz");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test20_req_assoc13_enter_at_singleline_tail_with_blank_editor_moves_focus_to_editor_head()
     {
        let result = transfer_on_enter("abcdefg", 7, "").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "abcdefg");
        assert_eq!(result.new_singleline_cursor_char, 7);
        assert_eq!(result.new_editor_text, "");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::Editor);
    }
}
