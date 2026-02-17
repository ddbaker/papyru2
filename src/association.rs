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
        return None;
    }

    let new_editor_text = if editor_text.is_empty() {
        right.to_string()
    } else {
        format!("{right}\n{editor_text}")
    };

    Some(EnterTransferResult {
        new_singleline_text: left.to_string(),
        new_singleline_cursor_char: left.chars().count(),
        new_editor_text,
        new_editor_cursor_line: 0,
        new_editor_cursor_char: 0,
        focus_target: FocusTarget::Editor,
    })
}

pub fn transfer_on_backspace(
    singleline_text: &str,
    singleline_cursor_char: usize,
    editor_text: &str,
) -> Option<BackspaceTransferResult> {
    let (editor_head, editor_tail) = split_first_line(editor_text);
    if editor_head.is_empty() {
        return None;
    }

    let (prefix, suffix) = split_at_char_index(singleline_text, singleline_cursor_char)?;

    let mut new_singleline_text =
        String::with_capacity(prefix.len() + editor_head.len() + suffix.len());
    new_singleline_text.push_str(prefix);
    new_singleline_text.push_str(editor_head);
    new_singleline_text.push_str(suffix);

    Some(BackspaceTransferResult {
        new_singleline_text,
        new_singleline_cursor_char: prefix.chars().count(),
        new_editor_text: editor_tail.to_string(),
        new_editor_cursor_line: 0,
        new_editor_cursor_char: 0,
        focus_target: FocusTarget::SingleLine,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        FocusTarget, should_transfer_backspace, transfer_on_backspace, transfer_on_enter,
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
        let result = transfer_on_backspace("こんにち", 4, "は世界\n大好き").expect("expected transfer");

        assert_eq!(result.new_singleline_text, "こんにちは世界");
        assert_eq!(result.new_singleline_cursor_char, 4);
        assert_eq!(result.new_editor_text, "大好き");
        assert_eq!(result.new_editor_cursor_line, 0);
        assert_eq!(result.new_editor_cursor_char, 0);
        assert_eq!(result.focus_target, FocusTarget::SingleLine);
    }

    #[test]
    fn assoc_test5_enter_at_end_is_no_op() {
        assert!(transfer_on_enter("abcdef", 6, "xyz").is_none());
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
}
