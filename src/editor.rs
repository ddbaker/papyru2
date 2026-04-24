use std::path::{Path, PathBuf};

use gpui::*;
use gpui_component::{
    ActiveTheme,
    input::{Backspace, Delete, Input, InputState, Position},
};

use gpui_component::input::InputEvent;
#[derive(Clone, Debug)]
pub enum EditorEvent {
    BackspaceAtLineHead,
    PressUpAtFirstLine,
    FocusGained,
    UserInteraction,
    UserBufferChanged { value: String },
}

#[derive(Clone, Debug)]
pub struct EditorSnapshot {
    pub value: String,
    pub cursor_line: u32,
    pub cursor_char: u32,
}

pub struct Papyru2Editor {
    input_state: Entity<InputState>,
    last_value: String,
    last_cursor: gpui_component::input::Position,
    pending_programmatic_change_events: usize,
    current_editing_file_path: Option<PathBuf>,
    _subscriptions: Vec<Subscription>,
    font_size_logged_once: bool,
    ui_color_config: crate::app::UiColorConfig,
    req_assoc18_editor_input_guard_active: bool,
}

impl EventEmitter<EditorEvent> for Papyru2Editor {}

pub(crate) fn req_editor_editor_font_size_policy() -> &'static str {
    crate::app::req_editor_shared_text_size_policy()
}

pub(crate) fn read_editor_text_from_disk(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

fn should_emit_backspace_at_line_head_on_change(
    previous_value: &str,
    previous_cursor: &Position,
    value: &str,
    cursor: &Position,
) -> bool {
    let is_noop_change = value == previous_value;
    let at_editor_origin = cursor.line == 0 && cursor.character == 0;
    if !is_noop_change || !at_editor_origin {
        return false;
    }

    let first_line_non_empty = value
        .split('\n')
        .next()
        .is_some_and(|line| !line.is_empty());
    let has_non_empty_tail_line = value.split('\n').skip(1).any(|line| !line.is_empty());

    let req_assoc12_candidate = first_line_non_empty || has_non_empty_tail_line;
    let req_assoc14_candidate = value.is_empty()
        && previous_value.is_empty()
        && previous_cursor.line == 0
        && previous_cursor.character == 0;
    let req_assoc17_blank_multiline_noop = !value.is_empty()
        && value.contains('\n')
        && value.split('\n').all(|line| line.is_empty())
        && previous_cursor.line == 0
        && previous_cursor.character == 0;

    req_assoc12_candidate || req_assoc14_candidate || req_assoc17_blank_multiline_noop
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PlainDeleteDirection {
    Backward,
    Forward,
}

fn byte_index_from_position(value: &str, position: &Position) -> usize {
    let target_line = position.line as usize;
    let target_character = position.character as usize;
    let mut line = 0usize;
    let mut character = 0usize;

    for (byte_index, ch) in value.char_indices() {
        if line == target_line && character >= target_character {
            return byte_index;
        }

        if ch == '\n' {
            if line == target_line {
                return byte_index;
            }
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    value.len()
}

fn position_from_byte_index(value: &str, byte_index: usize) -> Position {
    let mut line = 0u32;
    let mut character = 0u32;

    for (current_byte_index, ch) in value.char_indices() {
        if current_byte_index >= byte_index {
            break;
        }

        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += 1;
        }
    }

    Position { line, character }
}

fn byte_index_from_utf16_offset(value: &str, target_utf16_offset: usize) -> usize {
    let mut utf16_offset = 0usize;

    for (byte_index, ch) in value.char_indices() {
        if utf16_offset == target_utf16_offset {
            return byte_index;
        }

        let next_utf16_offset = utf16_offset + ch.len_utf16();
        if next_utf16_offset > target_utf16_offset {
            return byte_index;
        }
        if next_utf16_offset == target_utf16_offset {
            return byte_index + ch.len_utf8();
        }

        utf16_offset = next_utf16_offset;
    }

    value.len()
}

fn apply_delete_byte_range_to_text(
    value: &str,
    byte_range: std::ops::Range<usize>,
) -> Option<(String, Position)> {
    let start = byte_range.start.min(byte_range.end).min(value.len());
    let end = byte_range.start.max(byte_range.end).min(value.len());

    if start == end || !value.is_char_boundary(start) || !value.is_char_boundary(end) {
        return None;
    }

    let mut new_value = String::with_capacity(value.len() - (end - start));
    new_value.push_str(&value[..start]);
    new_value.push_str(&value[end..]);
    let new_cursor = position_from_byte_index(value, start);

    Some((new_value, new_cursor))
}

fn apply_delete_utf16_range_to_text(
    value: &str,
    utf16_range: std::ops::Range<usize>,
) -> Option<(String, Position)> {
    let byte_range = byte_index_from_utf16_offset(value, utf16_range.start)
        ..byte_index_from_utf16_offset(value, utf16_range.end);
    apply_delete_byte_range_to_text(value, byte_range)
}

fn previous_delete_range(value: &str, cursor_byte_index: usize) -> std::ops::Range<usize> {
    let Some((previous_start, previous_char)) = value[..cursor_byte_index].char_indices().last()
    else {
        return 0..0;
    };

    if previous_char == '\n' {
        if let Some((cr_start, '\r')) = value[..previous_start].char_indices().last() {
            return cr_start..cursor_byte_index;
        }
    }

    if previous_char.is_whitespace() {
        let line_start = value[..previous_start]
            .rfind('\n')
            .map_or(0, |index| index + '\n'.len_utf8());
        let line_end = value[cursor_byte_index..]
            .find('\n')
            .map_or(value.len(), |index| cursor_byte_index + index);
        let whitespace_is_at_visual_line_tail = value[previous_start..line_end]
            .chars()
            .all(|ch| ch != '\n' && ch.is_whitespace());

        if whitespace_is_at_visual_line_tail {
            let previous_visible_char = value[line_start..previous_start]
                .char_indices()
                .rev()
                .find(|(_, ch)| !ch.is_whitespace());

            if let Some((visible_start, _)) = previous_visible_char {
                return (line_start + visible_start)..cursor_byte_index;
            }
        }
    }

    previous_start..cursor_byte_index
}

fn next_delete_boundary(value: &str, cursor_byte_index: usize) -> usize {
    let Some(first_char) = value[cursor_byte_index..].chars().next() else {
        return cursor_byte_index;
    };

    let next = cursor_byte_index + first_char.len_utf8();
    if first_char == '\r' && value[next..].starts_with('\n') {
        return next + '\n'.len_utf8();
    }

    next
}

fn apply_plain_delete_to_text(
    value: &str,
    cursor: &Position,
    direction: PlainDeleteDirection,
) -> Option<(String, Position)> {
    let cursor_byte_index = byte_index_from_position(value, cursor);
    let range = match direction {
        PlainDeleteDirection::Backward if cursor_byte_index == 0 => return None,
        PlainDeleteDirection::Backward => previous_delete_range(value, cursor_byte_index),
        PlainDeleteDirection::Forward if cursor_byte_index >= value.len() => return None,
        PlainDeleteDirection::Forward => {
            cursor_byte_index..next_delete_boundary(value, cursor_byte_index)
        }
    };

    if range.is_empty() {
        return None;
    }

    apply_delete_byte_range_to_text(value, range)
}

const RPC_SCROLL_CENTERING_HALF_LINES_ESTIMATE: u32 = 9;

fn rpc_centering_anchor_line(target_line_0_based: u32, total_lines: usize) -> u32 {
    let bounded_total_lines = total_lines.max(1).min(u32::MAX as usize) as u32;
    let target_line = target_line_0_based.min(bounded_total_lines.saturating_sub(1));

    if bounded_total_lines <= RPC_SCROLL_CENTERING_HALF_LINES_ESTIMATE {
        return target_line;
    }

    target_line
        .saturating_add(RPC_SCROLL_CENTERING_HALF_LINES_ESTIMATE)
        .min(bounded_total_lines.saturating_sub(1))
}

impl Papyru2Editor {
    pub fn new(
        window: &mut Window,
        ui_color_config: crate::app::UiColorConfig,
        editor_config: crate::app::EditorConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor(editor_config.code_editor.clone())
                .line_number(editor_config.line_number)
                .soft_wrap(editor_config.soft_wrap)
                .searchable(true)
                .placeholder("File is auto saved")
        });

        let (last_value, last_cursor) = {
            let initial = input_state.read(cx);
            (initial.value().to_string(), initial.cursor_position())
        };

        let _subscriptions = vec![cx.subscribe_in(&input_state, window, {
            move |this, state, event: &InputEvent, _window, cx| match event {
                InputEvent::Change => {
                    let state = state.read(cx);
                    let cursor = state.cursor_position();
                    let value = state.value().to_string();
                    crate::log::trace_debug_lazy(|| {
                        format!(
                            "editor InputEvent::Change cursor=({}, {}) value_len={}",
                            cursor.line,
                            cursor.character,
                            value.len()
                        )
                    });

                    if this.pending_programmatic_change_events > 0 {
                        this.pending_programmatic_change_events -= 1;
                        crate::log::trace_debug(format!(
                            "editor InputEvent::Change ignored as programmatic (remaining={})",
                            this.pending_programmatic_change_events
                        ));
                        this.last_value = value;
                        this.last_cursor = cursor;
                        return;
                    }

                    let should_emit_backspace = should_emit_backspace_at_line_head_on_change(
                        &this.last_value,
                        &this.last_cursor,
                        &value,
                        &cursor,
                    );

                    if should_emit_backspace {
                        let first_line_non_empty =
                            value.split('\n').next().is_some_and(|line| !line.is_empty());
                        let has_non_empty_tail_line =
                            value.split('\n').skip(1).any(|line| !line.is_empty());
                        let req_assoc14_blank_origin_noop = value.is_empty()
                            && this.last_value.is_empty()
                            && this.last_cursor.line == 0
                            && this.last_cursor.character == 0
                            && cursor.line == 0
                            && cursor.character == 0;
                        let req_assoc17_blank_multiline_noop = !value.is_empty()
                            && value.contains('\n')
                            && value.split('\n').all(|line| line.is_empty())
                            && this.last_cursor.line == 0
                            && this.last_cursor.character == 0
                            && cursor.line == 0
                            && cursor.character == 0;

                        crate::log::trace_debug(format!(
                            "editor InputEvent::Change detected no-op backspace candidate at head (last_cursor=({}, {}), first_line_non_empty={}, has_non_empty_tail_line={}, req_assoc14_blank_origin_noop={}, req_assoc17_blank_multiline_noop={})",
                            this.last_cursor.line,
                            this.last_cursor.character,
                            first_line_non_empty,
                            has_non_empty_tail_line,
                            req_assoc14_blank_origin_noop,
                            req_assoc17_blank_multiline_noop
                        ));
                        cx.emit(EditorEvent::BackspaceAtLineHead);
                    }

                    if value != this.last_value {
                        crate::log::trace_debug(format!(
                            "editor emit UserBufferChanged len={} cursor=({}, {})",
                            value.len(),
                            cursor.line,
                            cursor.character
                        ));
                        cx.emit(EditorEvent::UserBufferChanged {
                            value: value.clone(),
                        });
                    }

                    this.last_value = value;
                    this.last_cursor = cursor;
                }
                InputEvent::PressEnter { secondary } => {
                    crate::log::trace_debug(format!(
                        "editor InputEvent::PressEnter secondary={secondary}"
                    ));
                }
                InputEvent::Focus => {
                    crate::log::trace_debug("editor InputEvent::Focus");
                    cx.emit(EditorEvent::FocusGained);
                }
                InputEvent::Blur => {
                    crate::log::trace_debug("editor InputEvent::Blur");
                }
            }
        })];

        crate::log::trace_debug(format!(
            "req-editor8 editor font_size_policy={}",
            req_editor_editor_font_size_policy()
        ));
        crate::log::trace_debug(format!(
            "req-editor startup editor_config code_editor={} soft_wrap={} line_number={} show_whitespaces={} searchable=true",
            editor_config.code_editor,
            editor_config.soft_wrap,
            editor_config.line_number,
            editor_config.show_whitespaces
        ));
        if editor_config.show_whitespaces {
            crate::log::trace_debug(
                "req-editor10 show_whitespaces=true requested but current gpui-component API has no show_whitespaces toggle; preserving config for future API support",
            );
        }

        Self {
            input_state,
            last_value,
            last_cursor,
            pending_programmatic_change_events: 0,
            current_editing_file_path: None,
            _subscriptions,
            font_size_logged_once: false,
            ui_color_config,
            req_assoc18_editor_input_guard_active: true,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !event.is_held {
            cx.emit(EditorEvent::UserInteraction);
        }
        let key_raw = event.keystroke.key.as_str();
        let key = key_raw.to_ascii_lowercase();
        crate::log::trace_debug(format!(
            "editor keydown raw='{}' key='{}' held={} key_char={}",
            key_raw,
            key,
            event.is_held,
            event.keystroke.key_char.as_deref().unwrap_or("<none>")
        ));

        let plain_delete_direction = if !event.keystroke.modifiers.modified() {
            match key.as_str() {
                "backspace" => Some(PlainDeleteDirection::Backward),
                "delete" | "forwarddelete" | "del" => Some(PlainDeleteDirection::Forward),
                _ => None,
            }
        } else {
            None
        };

        if let Some(direction) = plain_delete_direction {
            self.handle_plain_delete_action(key.as_str(), direction, window, cx);
            return;
        }

        cx.propagate();
    }

    fn handle_plain_delete_action(
        &mut self,
        key: &str,
        direction: PlainDeleteDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (cursor, value, selected_range_utf16) = self.input_state.update(cx, |state, cx| {
            let selection = state
                .selected_text_range(false, window, cx)
                .map(|selection| selection.range)
                .filter(|range| !range.is_empty());
            (
                state.cursor_position(),
                state.value().to_string(),
                selection,
            )
        });

        crate::log::trace_debug(format!(
            "editor action {key} captured cursor=({}, {}) value_len={} selection_utf16={:?}",
            cursor.line,
            cursor.character,
            value.len(),
            selected_range_utf16
        ));

        let selection_deleted = selected_range_utf16.is_some();
        let replacement = if let Some(selected_range_utf16) = selected_range_utf16 {
            apply_delete_utf16_range_to_text(&value, selected_range_utf16)
        } else {
            apply_plain_delete_to_text(&value, &cursor, direction)
        };

        if let Some((new_value, new_cursor)) = replacement {
            crate::log::trace_debug(format!(
                "editor handled action {key} char-safe cursor=({}, {}) new_cursor=({}, {}) old_len={} new_len={} selection_deleted={}",
                cursor.line,
                cursor.character,
                new_cursor.line,
                new_cursor.character,
                value.len(),
                new_value.len(),
                selection_deleted
            ));
            self.input_state.update(cx, |state, cx| {
                state.set_value(new_value, window, cx);
                state.set_cursor_position(new_cursor, window, cx);
            });
        } else if direction == PlainDeleteDirection::Backward
            && should_emit_backspace_at_line_head_on_change(
                &self.last_value,
                &self.last_cursor,
                &value,
                &cursor,
            )
        {
            crate::log::trace_debug("editor handled action backspace no-op at origin char-safe");
            cx.emit(EditorEvent::BackspaceAtLineHead);
        }

        cx.stop_propagation();
    }

    fn on_backspace_action(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        self.handle_plain_delete_action("Backspace", PlainDeleteDirection::Backward, window, cx);
    }

    fn on_delete_action(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        self.handle_plain_delete_action("Delete", PlainDeleteDirection::Forward, window, cx);
    }

    fn on_move_up_action(
        &mut self,
        _: &gpui_component::input::MoveUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let state = self.input_state.read(cx);
        let cursor = state.cursor_position();
        crate::log::trace_debug(format!(
            "editor action MoveUp captured cursor=({}, {}) value_len={}",
            cursor.line,
            cursor.character,
            state.value().len()
        ));

        if cursor.line == 0 {
            crate::log::trace_debug("editor action MoveUp emit PressUpAtFirstLine");
            cx.emit(EditorEvent::PressUpAtFirstLine);
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }

    pub fn snapshot(&self, cx: &App) -> EditorSnapshot {
        let state = self.input_state.read(cx);
        let cursor = state.cursor_position();

        EditorSnapshot {
            value: state.value().to_string(),
            cursor_line: cursor.line,
            cursor_char: cursor.character,
        }
    }

    pub fn apply_text_and_cursor(
        &mut self,
        text: impl Into<SharedString>,
        cursor_line: u32,
        cursor_char: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text: SharedString = text.into();
        let text_owned = text.to_string();

        self.pending_programmatic_change_events += 1;
        crate::log::trace_debug(format!(
            "editor mark programmatic change (apply_text_and_cursor, pending={})",
            self.pending_programmatic_change_events
        ));

        self.input_state.update(cx, move |state, cx| {
            state.set_value(text.clone(), window, cx);
            state.set_cursor_position(
                gpui_component::input::Position {
                    line: cursor_line,
                    character: cursor_char,
                },
                window,
                cx,
            );
        });

        self.last_value = text_owned;
        self.last_cursor = gpui_component::input::Position {
            line: cursor_line,
            character: cursor_char,
        };
    }

    pub fn apply_cursor(
        &mut self,
        cursor_line: u32,
        cursor_char: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.input_state.update(cx, move |state, cx| {
            state.set_cursor_position(
                gpui_component::input::Position {
                    line: cursor_line,
                    character: cursor_char,
                },
                window,
                cx,
            );
        });

        self.last_cursor = gpui_component::input::Position {
            line: cursor_line,
            character: cursor_char,
        };
    }

    pub fn open_content_from_rpc(
        &mut self,
        path: PathBuf,
        content: String,
        cursor_line: u32,
        cursor_char: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_string();
        let total_lines = crate::quic_rpc_protocol::content_line_count(&content);
        let anchor_line = rpc_centering_anchor_line(cursor_line, total_lines);

        self.pending_programmatic_change_events += 1;
        crate::log::trace_debug(format!(
            "editor mark programmatic change (open_content_from_rpc, pending={}, target_line={}, anchor_line={}, total_lines={})",
            self.pending_programmatic_change_events, cursor_line, anchor_line, total_lines
        ));

        self.input_state.update(cx, |state, cx| {
            state.set_highlighter(language, cx);
            state.set_value(content.clone(), window, cx);
            state.set_cursor_position(
                gpui_component::input::Position {
                    line: cursor_line,
                    character: cursor_char,
                },
                window,
                cx,
            );
        });

        if anchor_line != cursor_line {
            let target_line = cursor_line;
            let target_char = cursor_char;
            cx.on_next_frame(window, move |this, window, cx| {
                this.apply_cursor(anchor_line, target_char, window, cx);
                crate::log::trace_debug(format!(
                    "editor rpc centering frame1 anchor_line={} target_line={}",
                    anchor_line, target_line
                ));

                cx.on_next_frame(window, move |this, window, cx| {
                    this.apply_cursor(target_line, target_char, window, cx);
                    crate::log::trace_debug(format!(
                        "editor rpc centering frame2 restore_target_line={target_line}"
                    ));
                });
            });
        }

        self.last_value = content;
        self.last_cursor = gpui_component::input::Position {
            line: cursor_line,
            character: cursor_char,
        };
        self.current_editing_file_path = Some(path);
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_state
            .update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.input_state
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    pub fn open_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let content = match read_editor_text_from_disk(path.as_path()) {
            Ok(content) => content,
            Err(error) => {
                crate::log::trace_debug(format!(
                    "editor open_file read_failed path={} error={error}",
                    path.display()
                ));
                return false;
            }
        };
        crate::log::trace_debug(format!(
            "editor open_file content_loaded path={} bytes={}",
            path.display(),
            content.len()
        ));

        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_string();

        self.pending_programmatic_change_events += 1;
        crate::log::trace_debug(format!(
            "editor mark programmatic change (open_file, pending={})",
            self.pending_programmatic_change_events
        ));

        self.input_state.update(cx, |state, cx| {
            state.set_highlighter(language, cx);
            state.set_value(content.clone(), window, cx);
            state.set_cursor_position(
                gpui_component::input::Position {
                    line: 0,
                    character: 0,
                },
                window,
                cx,
            );
        });

        self.last_value = content;
        self.last_cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };
        true
    }

    pub fn set_current_editing_file_path(&mut self, path: Option<PathBuf>) {
        self.current_editing_file_path = path;
    }

    pub fn set_req_assoc18_editor_input_guard_active(
        &mut self,
        active: bool,
        cx: &mut Context<Self>,
    ) {
        if self.req_assoc18_editor_input_guard_active == active {
            return;
        }

        self.req_assoc18_editor_input_guard_active = active;
        crate::log::trace_debug(format!(
            "assoc req-assoc18 editor_input_guard active={active}"
        ));
        cx.notify();
    }

    pub fn current_editing_file_path(&self) -> Option<PathBuf> {
        self.current_editing_file_path.clone()
    }
}

impl Render for Papyru2Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let experimental_text_size_px = px(f32::from(cx.theme().font_size) + 0.5);
        let background_rgb_hex = self.ui_color_config.background_rgb_hex;
        let foreground_rgb_hex = self.ui_color_config.foreground_rgb_hex;
        let req_assoc18_editor_input_guard_active = self.req_assoc18_editor_input_guard_active;

        if !self.font_size_logged_once {
            crate::log::trace_debug(format!(
                "req-editor-font-size snapshot component=editor policy={} input_size_variant=medium_default wrapper_text_size=text_sm experimental_text_size_plus_0p5px={:?} mono_font_family={} theme.font_size={:?} theme.mono_font_size={:?} req_colr_background=#{:06x} req_colr_foreground=#{:06x}",
                req_editor_editor_font_size_policy(),
                experimental_text_size_px,
                cx.theme().mono_font_family,
                cx.theme().font_size,
                cx.theme().mono_font_size,
                background_rgb_hex,
                foreground_rgb_hex,
            ));
            self.font_size_logged_once = true;
        }

        div()
            .size_full()
            .bg(crate::app::req_colr_rgb_hex_to_hsla(background_rgb_hex))
            .text_color(crate::app::req_colr_rgb_hex_to_hsla(foreground_rgb_hex))
            .capture_key_down(cx.listener(Self::on_key_down))
            .capture_action(cx.listener(Self::on_backspace_action))
            .capture_action(cx.listener(Self::on_delete_action))
            .capture_action(cx.listener(Self::on_move_up_action))
            .on_mouse_down(MouseButton::Left, move |_, _, _| {
                if req_assoc18_editor_input_guard_active {
                    crate::log::trace_debug("assoc req-assoc18 editor_click_ignored state=NEUTRAL");
                }
            })
            .child(
                crate::app::apply_req_editor_shared_text_size(
                    Input::new(&self.input_state)
                        .disabled(self.req_assoc18_editor_input_guard_active)
                        .appearance(false)
                        .size_full()
                        .font_family(cx.theme().mono_font_family.clone())
                        .text_color(crate::app::req_colr_rgb_hex_to_hsla(foreground_rgb_hex)),
                )
                .text_size(experimental_text_size_px),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::read_editor_text_from_disk;
    use crate::file_update_handler::{
        EditorAutoSavePayload, FileWorkflowEventDispatcher, SinglelineCreateFileWorkflow,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn new_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "gpui_papyru2_editor_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn remove_temp_root(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn qsrv_editor_test1_rpc_anchor_moves_target_toward_viewport_center() {
        // target line 30 (0-based 29) in a large file should apply centering anchor offset.
        let anchor = super::rpc_centering_anchor_line(29, 100);
        assert_eq!(anchor, 38);
    }

    #[test]
    fn qsrv_editor_test2_rpc_anchor_keeps_target_for_short_files() {
        // Requirement: when file has fewer lines than half viewport estimate, no offset adjustment.
        let anchor = super::rpc_centering_anchor_line(3, 5);
        assert_eq!(anchor, 3);
    }

    #[test]
    fn qsrv_editor_test3_rpc_anchor_clamps_to_last_line() {
        let anchor = super::rpc_centering_anchor_line(98, 100);
        assert_eq!(anchor, 99);
    }

    #[test]
    fn ftr_test37_req_ftr16_selection_reads_file_content_for_editor_sync() {
        let root = new_temp_root("ftr_test37");
        let selected_path = root.join("fileA.txt");
        fs::write(&selected_path, "alpha\nbeta").expect("seed selected file");

        let loaded = read_editor_text_from_disk(selected_path.as_path())
            .expect("read selected file for editor sync");
        assert_eq!(loaded, "alpha\nbeta");

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test38_req_ftr16_selection_reads_utf8_file_content_losslessly() {
        let root = new_temp_root("ftr_test38");
        let selected_path = root.join("multibyte.txt");
        let expected = "テスト🙂\n二行目";
        fs::write(&selected_path, expected).expect("seed utf8 selected file");

        let loaded = read_editor_text_from_disk(selected_path.as_path())
            .expect("read utf8 selected file for editor sync");
        assert_eq!(loaded, expected);

        remove_temp_root(root.as_path());
    }

    #[test]
    fn editor_delete_test1_backspace_after_box_deletes_whole_utf8_character() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 3,
        };

        let (new_value, new_cursor) = super::apply_plain_delete_to_text(
            "- □",
            &cursor,
            super::PlainDeleteDirection::Backward,
        )
        .expect("delete box character");

        assert_eq!(new_value, "- ");
        assert_eq!(new_cursor.line, 0);
        assert_eq!(new_cursor.character, 2);
    }

    #[test]
    fn editor_delete_test2_backspace_after_surrogate_pair_deletes_whole_character() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 2,
        };

        let (new_value, new_cursor) = super::apply_plain_delete_to_text(
            "a🙂",
            &cursor,
            super::PlainDeleteDirection::Backward,
        )
        .expect("delete emoji character");

        assert_eq!(new_value, "a");
        assert_eq!(new_cursor.line, 0);
        assert_eq!(new_cursor.character, 1);
    }

    #[test]
    fn editor_delete_test3_backspace_after_box_with_trailing_space_deletes_visible_character() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 4,
        };

        let (new_value, new_cursor) = super::apply_plain_delete_to_text(
            "- □ ",
            &cursor,
            super::PlainDeleteDirection::Backward,
        )
        .expect("delete box character and trailing space");

        assert_eq!(new_value, "- ");
        assert_eq!(new_cursor.line, 0);
        assert_eq!(new_cursor.character, 2);
    }

    #[test]
    fn editor_delete_test4_backspace_after_ascii_with_trailing_space_deletes_visible_character() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 4,
        };

        let (new_value, new_cursor) = super::apply_plain_delete_to_text(
            "- a ",
            &cursor,
            super::PlainDeleteDirection::Backward,
        )
        .expect("delete visible ascii character with trailing space");

        assert_eq!(new_value, "- ");
        assert_eq!(new_cursor.line, 0);
        assert_eq!(new_cursor.character, 2);
    }

    #[test]
    fn editor_delete_test5_space_inside_line_deletes_only_space() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 4,
        };

        let (new_value, new_cursor) = super::apply_plain_delete_to_text(
            "- a b",
            &cursor,
            super::PlainDeleteDirection::Backward,
        )
        .expect("delete space inside line");

        assert_eq!(new_value, "- ab");
        assert_eq!(new_cursor.line, 0);
        assert_eq!(new_cursor.character, 3);
    }

    #[test]
    fn editor_delete_test5_forward_delete_removes_whole_utf8_character() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 2,
        };

        let (new_value, new_cursor) =
            super::apply_plain_delete_to_text("- □", &cursor, super::PlainDeleteDirection::Forward)
                .expect("forward delete box character");

        assert_eq!(new_value, "- ");
        assert_eq!(new_cursor.line, 0);
        assert_eq!(new_cursor.character, 2);
    }

    #[test]
    fn editor_delete_test6_selection_deletes_whole_utf8_range() {
        let (new_value, new_cursor) = super::apply_delete_utf16_range_to_text("- □ text", 2..3)
            .expect("delete selected box character");

        assert_eq!(new_value, "-  text");
        assert_eq!(new_cursor.line, 0);
        assert_eq!(new_cursor.character, 2);
    }

    #[test]
    fn editor_delete_test7_selection_deletes_multiple_lines() {
        let value = "alpha\n- □\nbeta";
        let selection_start = "alpha\n".encode_utf16().count();
        let selection_end = "alpha\n- □\n".encode_utf16().count();

        let (new_value, new_cursor) =
            super::apply_delete_utf16_range_to_text(value, selection_start..selection_end)
                .expect("delete selected multiline text");

        assert_eq!(new_value, "alpha\nbeta");
        assert_eq!(new_cursor.line, 1);
        assert_eq!(new_cursor.character, 0);
    }

    #[test]
    fn ftr_test39_req_ftr16_selected_file_edit_save_updates_selected_path_not_stale_buffer() {
        let root = new_temp_root("ftr_test39");
        let path_a = root.join("fileA.txt");
        let path_b = root.join("fileB.txt");
        fs::write(&path_a, "A-old").expect("seed fileA");
        fs::write(&path_b, "B-old").expect("seed fileB");

        // Simulate editor currently having stale text from previously edited file A.
        let stale_text_from_previous_file = "A-stale";
        let dispatcher = FileWorkflowEventDispatcher::new();
        let workflow = SinglelineCreateFileWorkflow::with_dispatcher(dispatcher.clone());
        workflow.set_edit_from_open_file(path_a.clone());
        let flushed = workflow
            .flush_editor_content_in_edit(stale_text_from_previous_file, root.as_path())
            .expect("flush stale fileA content before selection switch");
        assert!(flushed);
        let path_a_after_flush = workflow
            .current_edit_path()
            .expect("current fileA path after pre-switch flush");

        // File-tree selection must load fileB content into editor and move edit context to fileB.
        let loaded_selected_text =
            read_editor_text_from_disk(path_b.as_path()).expect("load selected fileB content");
        assert_eq!(loaded_selected_text, "B-old");
        workflow.set_edit_from_open_file(path_b.clone());

        let saved = workflow
            .try_autosave_in_edit(EditorAutoSavePayload {
                user_document_dir: root.clone(),
                current_path: path_b.clone(),
                editor_text: format!("{loaded_selected_text}\nB-new"),
            })
            .expect("autosave edited selected file");
        assert!(saved);
        let path_b_after_save = workflow
            .current_edit_path()
            .expect("current fileB path after autosave");

        assert_eq!(
            fs::read_to_string(&path_a_after_flush).expect("read fileA after switch"),
            "A-stale"
        );
        assert_eq!(
            fs::read_to_string(&path_b_after_save).expect("read fileB after selected-file save"),
            "B-old\nB-new"
        );

        dispatcher.shutdown();
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test43_req_ftr16_regression_content_sync_path_remains_available() {
        let root = new_temp_root("ftr_test43");
        let selected_path = root.join("selected.txt");
        fs::write(&selected_path, "line-a\nline-b\n").expect("seed selected file");

        let loaded =
            read_editor_text_from_disk(selected_path.as_path()).expect("read selected file text");
        assert_eq!(loaded, "line-a\nline-b\n");

        remove_temp_root(root.as_path());
    }

    #[test]
    fn assoc_test21_req_assoc14_blank_origin_noop_change_emits_backspace_signal() {
        let previous_cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };

        assert!(super::should_emit_backspace_at_line_head_on_change(
            "",
            &previous_cursor,
            "",
            &cursor,
        ));
    }

    #[test]
    fn assoc_test22_req_assoc14_non_origin_or_non_noop_does_not_emit_backspace_signal() {
        let origin_cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };
        let non_origin_cursor = gpui_component::input::Position {
            line: 0,
            character: 1,
        };

        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "",
            &origin_cursor,
            "",
            &non_origin_cursor,
        ));
        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "",
            &non_origin_cursor,
            "",
            &origin_cursor,
        ));
        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "abc",
            &origin_cursor,
            "",
            &origin_cursor,
        ));
    }

    #[test]
    fn assoc_test23_req_assoc17_blank_multiline_noop_change_emits_backspace_signal() {
        let origin_cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };

        assert!(super::should_emit_backspace_at_line_head_on_change(
            "\n\n",
            &origin_cursor,
            "\n\n",
            &origin_cursor,
        ));
    }

    #[test]
    fn assoc_test24_req_assoc17_changed_multiline_does_not_emit_duplicate_backspace_signal() {
        let origin_cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };

        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "\n\n",
            &origin_cursor,
            "\n",
            &origin_cursor,
        ));
    }
}
