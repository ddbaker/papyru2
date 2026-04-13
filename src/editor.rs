use std::path::{Path, PathBuf};

use gpui::*;
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState},
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
    previous_cursor: &gpui_component::input::Position,
    value: &str,
    cursor: &gpui_component::input::Position,
) -> bool {
    let is_noop_change = value == previous_value;
    let at_editor_origin = cursor.line == 0 && cursor.character == 0;
    if !is_noop_change || !at_editor_origin {
        return false;
    }

    let first_line_non_empty = value.split('\n').next().is_some_and(|line| !line.is_empty());
    let has_non_empty_tail_line = value.split('\n').skip(1).any(|line| !line.is_empty());

    let req_assoc12_candidate = first_line_non_empty || has_non_empty_tail_line;
    let req_assoc14_candidate = value.is_empty()
        && previous_value.is_empty()
        && previous_cursor.line == 0
        && previous_cursor.character == 0;

    req_assoc12_candidate || req_assoc14_candidate
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
                    crate::log::trace_debug(format!(
                        "editor InputEvent::Change cursor=({}, {}) value='{}'",
                        cursor.line,
                        cursor.character,
                        crate::app::compact_text(&value)
                    ));

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

                        crate::log::trace_debug(format!(
                            "editor InputEvent::Change detected no-op backspace candidate at head (last_cursor=({}, {}), first_line_non_empty={}, has_non_empty_tail_line={}, req_assoc14_blank_origin_noop={})",
                            this.last_cursor.line,
                            this.last_cursor.character,
                            first_line_non_empty,
                            has_non_empty_tail_line,
                            req_assoc14_blank_origin_noop
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
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
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

        if key == "backspace" || key == "delete" {
            let snapshot = self.snapshot(cx);
            crate::log::trace_debug(format!(
                "editor backspace candidate cursor=({}, {}) value='{}'",
                snapshot.cursor_line,
                snapshot.cursor_char,
                crate::app::compact_text(&snapshot.value)
            ));
        }

        cx.propagate();
    }

    fn on_move_up_action(
        &mut self,
        _: &gpui_component::input::MoveUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let snapshot = self.snapshot(cx);
        crate::log::trace_debug(format!(
            "editor action MoveUp captured cursor=({}, {}) value='{}'",
            snapshot.cursor_line,
            snapshot.cursor_char,
            crate::app::compact_text(&snapshot.value)
        ));

        if snapshot.cursor_line == 0 {
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

    pub fn current_editing_file_path(&self) -> Option<PathBuf> {
        self.current_editing_file_path.clone()
    }
}

impl Render for Papyru2Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let experimental_text_size_px = px(f32::from(cx.theme().font_size) + 0.5);
        let background_rgb_hex = self.ui_color_config.background_rgb_hex;
        let foreground_rgb_hex = self.ui_color_config.foreground_rgb_hex;

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
            .capture_action(cx.listener(Self::on_move_up_action))
            .child(
                crate::app::apply_req_editor_shared_text_size(
                    Input::new(&self.input_state)
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
}
