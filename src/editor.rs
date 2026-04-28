use std::{
    ops::Range,
    path::{Path, PathBuf},
};

use gpui::*;
use gpui_component::{
    ActiveTheme,
    input::{Backspace, Input, InputState, Position},
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct NarrowBoxBackspaceGuard {
    byte_range: Range<usize>,
    utf16_range: Range<usize>,
    new_cursor: Position,
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

fn byte_index_from_position(value: &str, position: &Position) -> usize {
    let target_line = position.line as usize;
    let target_character = position.character as usize;
    let mut line = 0usize;
    let mut character = 0usize;
    let mut chars = value.char_indices().peekable();

    while let Some((byte_index, ch)) = chars.next() {
        if line == target_line && character >= target_character {
            return byte_index;
        }

        match ch {
            '\r' => {
                if line == target_line {
                    return byte_index;
                }
                if chars.peek().is_some_and(|(_, next)| *next == '\n') {
                    chars.next();
                }
                line += 1;
                character = 0;
            }
            '\n' => {
                if line == target_line {
                    return byte_index;
                }
                line += 1;
                character = 0;
            }
            _ => {
                character += 1;
            }
        }
    }

    value.len()
}

fn position_from_byte_index(value: &str, byte_index: usize) -> Position {
    let mut line = 0u32;
    let mut character = 0u32;
    let mut chars = value.char_indices().peekable();

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

fn utf16_offset_from_byte_index(value: &str, byte_index: usize) -> Option<usize> {
    if !value.is_char_boundary(byte_index) {
        return None;
    }

    Some(value[..byte_index].encode_utf16().count())
}

fn narrow_box_backspace_guard(
    key: &str,
    has_modifiers: bool,
    value: &str,
    cursor: &Position,
    has_selection: bool,
) -> Option<NarrowBoxBackspaceGuard> {
    if !key.eq_ignore_ascii_case("backspace") || has_modifiers || has_selection {
        return None;
    }
    if cursor.line == 0 && cursor.character == 0 {
        return None;
    }

    let cursor_byte_index = byte_index_from_position(value, cursor);
    if cursor_byte_index == 0 || !value.is_char_boundary(cursor_byte_index) {
        return None;
    }

    let (previous_start, previous_char) = value[..cursor_byte_index].char_indices().last()?;
    if previous_char != '□' {
        return None;
    }

    let previous_end = cursor_byte_index;
    if !value.is_char_boundary(previous_start) || !value.is_char_boundary(previous_end) {
        return None;
    }

    let utf16_start = utf16_offset_from_byte_index(value, previous_start)?;
    let utf16_end = utf16_start + previous_char.len_utf16();

    Some(NarrowBoxBackspaceGuard {
        byte_range: previous_start..previous_end,
        utf16_range: utf16_start..utf16_end,
        new_cursor: position_from_byte_index(value, previous_start),
    })
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
            let input_state = if editor_config.code_editor {
                InputState::new(window, cx)
                    .code_editor(editor_config.code_editor_lang.clone())
                    .line_number(editor_config.line_number)
                    .indent_guides(crate::app::req_editor_effective_indent_guides(
                        &editor_config,
                    ))
            } else {
                InputState::new(window, cx).multi_line(true)
            };

            input_state
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
            "req-editor startup editor_config code_editor={} code_editor_lang={} soft_wrap={} line_number={} show_whitespaces={} indent_guides={} effective_indent_guides={} searchable=true",
            editor_config.code_editor,
            editor_config.code_editor_lang,
            editor_config.soft_wrap,
            editor_config.line_number,
            editor_config.show_whitespaces,
            editor_config.indent_guides,
            crate::app::req_editor_effective_indent_guides(&editor_config)
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

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if !event.is_held {
            cx.emit(EditorEvent::UserInteraction);
        }
        let key_raw = event.keystroke.key.as_str();
        let key = key_raw.to_ascii_lowercase();
        let has_modifiers = event.keystroke.modifiers.modified();
        crate::log::trace_debug(format!(
            "editor keydown raw='{}' key='{}' held={} modified={} key_char={}",
            key_raw,
            key,
            event.is_held,
            has_modifiers,
            event.keystroke.key_char.as_deref().unwrap_or("<none>")
        ));

        cx.propagate();
    }

    fn on_backspace_action(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.handle_narrow_box_backspace_guard("backspace", false, window, cx) {
            cx.stop_propagation();
            return;
        }

        cx.propagate();
    }

    fn handle_narrow_box_backspace_guard(
        &mut self,
        key: &str,
        has_modifiers: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let (cursor, value, has_selection) = self.input_state.update(cx, |state, cx| {
            let has_selection = state
                .selected_text_range(false, window, cx)
                .map(|selection| !selection.range.is_empty())
                .unwrap_or(false);

            (
                state.cursor_position(),
                state.value().to_string(),
                has_selection,
            )
        });

        let Some(guard) =
            narrow_box_backspace_guard(key, has_modifiers, &value, &cursor, has_selection)
        else {
            let cursor_byte_index = byte_index_from_position(&value, &cursor);
            let previous_char =
                if cursor_byte_index > 0 && value.is_char_boundary(cursor_byte_index) {
                    value[..cursor_byte_index].chars().next_back()
                } else {
                    None
                };
            crate::log::trace_debug(format!(
                "req-editor16 narrow_backspace_box_guard_skipped cursor=({}, {}) has_selection={} previous_char={:?} value_len={}",
                cursor.line,
                cursor.character,
                has_selection,
                previous_char,
                value.len()
            ));
            return false;
        };

        crate::log::trace_debug(format!(
            "req-editor16 narrow_backspace_box_guard_applied cursor=({}, {}) byte_range={:?} utf16_range={:?} value_len={}",
            cursor.line,
            cursor.character,
            guard.byte_range,
            guard.utf16_range,
            value.len()
        ));

        self.input_state.update(cx, |state, cx| {
            gpui::EntityInputHandler::replace_text_in_range(
                state,
                Some(guard.utf16_range.clone()),
                "",
                window,
                cx,
            );
            state.set_cursor_position(guard.new_cursor, window, cx);
        });

        true
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
    fn editor16_test1_narrow_box_guard_matches_box_after_cursor() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 3,
        };

        let guard = super::narrow_box_backspace_guard("backspace", false, "- □", &cursor, false)
            .expect("box guard");

        assert_eq!(guard.byte_range, 2..5);
        assert_eq!(guard.utf16_range, 2..3);
        assert_eq!(guard.new_cursor.line, 0);
        assert_eq!(guard.new_cursor.character, 2);
    }

    #[test]
    fn editor16_test2_narrow_box_guard_rejects_normal_ascii() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 3,
        };

        assert!(
            super::narrow_box_backspace_guard("backspace", false, "- a", &cursor, false).is_none()
        );
    }

    #[test]
    fn editor16_test3_narrow_box_guard_rejects_emoji_and_cjk_text() {
        let emoji_cursor = gpui_component::input::Position {
            line: 0,
            character: 2,
        };
        let cjk_cursor = gpui_component::input::Position {
            line: 0,
            character: 3,
        };

        assert!(
            super::narrow_box_backspace_guard("backspace", false, "a🙂", &emoji_cursor, false,)
                .is_none()
        );
        assert!(
            super::narrow_box_backspace_guard("backspace", false, "日本語", &cjk_cursor, false,)
                .is_none()
        );
    }

    #[test]
    fn editor16_test4_narrow_box_guard_rejects_selection_modified_key_and_delete() {
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 3,
        };

        assert!(
            super::narrow_box_backspace_guard("backspace", false, "- □", &cursor, true).is_none()
        );
        assert!(
            super::narrow_box_backspace_guard("backspace", true, "- □", &cursor, false).is_none()
        );
        assert!(
            super::narrow_box_backspace_guard("delete", false, "- □", &cursor, false).is_none()
        );
    }

    #[test]
    fn editor16_test5_narrow_box_guard_rejects_line_head_association_and_trailing_space() {
        let origin_cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };
        let trailing_space_cursor = gpui_component::input::Position {
            line: 0,
            character: 4,
        };

        assert!(
            super::narrow_box_backspace_guard("backspace", false, "abc", &origin_cursor, false,)
                .is_none()
        );
        assert!(
            super::narrow_box_backspace_guard(
                "backspace",
                false,
                "- □ ",
                &trailing_space_cursor,
                false,
            )
            .is_none()
        );
    }

    #[test]
    fn editor16_test6_narrow_box_guard_matches_crlf_and_cr_second_line() {
        for value in ["head\r\n- □\r\ntail", "head\r- □\rtail"] {
            for character in [3, 4] {
                let cursor = gpui_component::input::Position { line: 1, character };
                let box_start = value.find('□').expect("box char");
                let guard =
                    super::narrow_box_backspace_guard("backspace", false, value, &cursor, false)
                        .expect("box guard with CR line endings");

                assert_eq!(guard.byte_range, box_start..box_start + '□'.len_utf8());
                assert_eq!(
                    guard.utf16_range,
                    value[..box_start].encode_utf16().count()
                        ..value[..box_start].encode_utf16().count() + 1
                );
                assert_eq!(guard.new_cursor.line, 1);
                assert_eq!(guard.new_cursor.character, 2);
            }
        }
    }

    #[test]
    fn editor_delete_test1_changed_multibyte_backspace_stays_native_only() {
        let previous_cursor = gpui_component::input::Position {
            line: 0,
            character: 3,
        };
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 2,
        };

        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "- □",
            &previous_cursor,
            "- ",
            &cursor,
        ));
    }

    #[test]
    fn editor_delete_test2_changed_emoji_backspace_stays_native_only() {
        let previous_cursor = gpui_component::input::Position {
            line: 0,
            character: 2,
        };
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 1,
        };

        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "a🙂",
            &previous_cursor,
            "a",
            &cursor,
        ));
    }

    #[test]
    fn editor_delete_test3_changed_multiline_selection_stays_native_only() {
        let previous_cursor = gpui_component::input::Position {
            line: 2,
            character: 0,
        };
        let cursor = gpui_component::input::Position {
            line: 1,
            character: 0,
        };

        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "alpha\n- □\nbeta",
            &previous_cursor,
            "alpha\nbeta",
            &cursor,
        ));
    }

    #[test]
    fn editor_delete_test4_line_head_noop_remains_association_trigger() {
        let previous_cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 0,
        };

        assert!(super::should_emit_backspace_at_line_head_on_change(
            "abc\nxyz",
            &previous_cursor,
            "abc\nxyz",
            &cursor,
        ));
    }

    #[test]
    fn editor_undo_test1_native_first_has_no_custom_delete_history_unit_path() {
        let previous_cursor = gpui_component::input::Position {
            line: 0,
            character: 4,
        };
        let cursor = gpui_component::input::Position {
            line: 0,
            character: 3,
        };

        assert!(!super::should_emit_backspace_at_line_head_on_change(
            "- a ",
            &previous_cursor,
            "- a",
            &cursor,
        ));
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
