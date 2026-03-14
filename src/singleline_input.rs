use std::path::{Path, PathBuf};

use chrono::Local;
use gpui::*;
use gpui_component::input::InputEvent;
use gpui_component::input::{Input, InputState};

use gpui_component::ActiveTheme as _;
#[derive(Clone, Debug)]
pub enum SingleLineEvent {
    PressEnter,
    PressDown,
    ValueChanged { value: String, cursor_char: usize },
}

#[derive(Clone, Debug)]
pub struct SingleLineSnapshot {
    pub value: String,
    pub cursor_char: usize,
}

pub struct SingleLineInput {
    sl_input_state: Entity<InputState>,
    last_value: String,
    last_cursor: gpui_component::input::Position,
    pending_programmatic_change_events: usize,
    current_editing_file_path: Option<PathBuf>,
    _subscriptions: Vec<Subscription>,
    font_size_logged_once: bool,
}

impl EventEmitter<SingleLineEvent> for SingleLineInput {}

pub(crate) fn req_editor_singleline_font_size_policy() -> &'static str {
    crate::app::req_editor_shared_text_size_policy()
}

impl SingleLineInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sl_input_state = cx.new(|cx| InputState::new(window, cx).placeholder("Type here..."));
        let (last_value, last_cursor) = {
            let initial = sl_input_state.read(cx);
            (initial.value().to_string(), initial.cursor_position())
        };

        let _subscriptions = vec![cx.subscribe_in(&sl_input_state, window, {
            move |this, state, event: &InputEvent, _window, cx| {
                if let InputEvent::Change = event {
                    let state = state.read(cx);
                    let value = state.value().to_string();
                    let cursor = state.cursor_position();
                    if this.pending_programmatic_change_events > 0 {
                        this.pending_programmatic_change_events -= 1;
                        this.last_value = value;
                        this.last_cursor = cursor;
                        return;
                    }

                    this.last_value = value.clone();
                    this.last_cursor = cursor;
                    cx.emit(SingleLineEvent::ValueChanged {
                        value,
                        cursor_char: cursor.character as usize,
                    });
                }
            }
        })];

        crate::app::trace_debug(format!(
            "req-editor7 singleline_input font_size_policy={}",
            req_editor_singleline_font_size_policy()
        ));

        Self {
            sl_input_state,
            last_value,
            last_cursor,
            pending_programmatic_change_events: 0,
            current_editing_file_path: None,
            _subscriptions,
            font_size_logged_once: false,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            cx.propagate();
            return;
        }

        let key_raw = event.keystroke.key.as_str();
        let key = key_raw.to_ascii_lowercase();
        crate::app::trace_debug(format!("singleline keydown key={key}"));

        if key == "enter" || key == "return" {
            crate::app::trace_debug("singleline emit PressEnter");
            cx.emit(SingleLineEvent::PressEnter);
            cx.stop_propagation();
            return;
        }

        if key == "down" || key == "arrowdown" {
            let snapshot = self.snapshot(cx);
            crate::app::trace_debug(format!(
                "singleline down candidate cursor={} value='{}'",
                snapshot.cursor_char,
                crate::app::compact_text(&snapshot.value)
            ));
            crate::app::trace_debug("singleline emit PressDown");
            cx.emit(SingleLineEvent::PressDown);
            cx.stop_propagation();
            return;
        }

        cx.propagate();
    }

    pub fn snapshot(&self, cx: &App) -> SingleLineSnapshot {
        let state = self.sl_input_state.read(cx);
        let cursor = state.cursor_position();

        SingleLineSnapshot {
            value: state.value().to_string(),
            cursor_char: cursor.character as usize,
        }
    }

    pub fn apply_text_and_cursor(
        &mut self,
        text: impl Into<SharedString>,
        cursor_char: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text: SharedString = text.into();
        let text_owned = text.to_string();
        let cursor_char_u32 = cursor_char.min(u32::MAX as usize) as u32;

        self.pending_programmatic_change_events += 1;

        self.sl_input_state.update(cx, move |state, cx| {
            state.set_value(text.clone(), window, cx);
            state.set_cursor_position(
                gpui_component::input::Position {
                    line: 0,
                    character: cursor_char_u32,
                },
                window,
                cx,
            );
        });

        self.last_value = text_owned;
        self.last_cursor = gpui_component::input::Position {
            line: 0,
            character: cursor_char_u32,
        };
    }

    pub fn apply_text_value_only(
        &mut self,
        text: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let text: SharedString = text.into();
        let text_owned = text.to_string();

        self.pending_programmatic_change_events += 1;

        self.sl_input_state.update(cx, move |state, cx| {
            state.set_value(text.clone(), window, cx);
        });

        self.last_value = text_owned;
    }

    pub fn apply_cursor(
        &mut self,
        cursor_char: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sl_input_state.update(cx, move |state, cx| {
            state.set_cursor_position(
                gpui_component::input::Position {
                    line: 0,
                    character: cursor_char.min(u32::MAX as usize) as u32,
                },
                window,
                cx,
            );
        });

        self.last_cursor = gpui_component::input::Position {
            line: 0,
            character: cursor_char.min(u32::MAX as usize) as u32,
        };
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sl_input_state
            .update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.sl_input_state
            .read(cx)
            .focus_handle(cx)
            .is_focused(window)
    }

    pub fn set_current_editing_file_path(&mut self, path: Option<PathBuf>) {
        self.current_editing_file_path = path;
    }

    pub fn current_editing_file_path(&self) -> Option<PathBuf> {
        self.current_editing_file_path.clone()
    }
}

impl Render for SingleLineInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let experimental_text_size_px = px(f32::from(cx.theme().font_size) + 1.0);

        if !self.font_size_logged_once {
            crate::app::trace_debug(format!(
                "req-editor-font-size snapshot component=singleline_input policy={} input_size_variant=medium_default wrapper_text_size=text_sm experimental_text_size_plus_1px={:?} theme.font_size={:?} theme.mono_font_size={:?}",
                req_editor_singleline_font_size_policy(),
                experimental_text_size_px,
                cx.theme().font_size,
                cx.theme().mono_font_size,
            ));
            self.font_size_logged_once = true;
        }

        div()
            .w_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(
                crate::app::apply_req_editor_shared_text_size(
                    Input::new(&self.sl_input_state).w_full(),
                )
                .text_size(experimental_text_size_px),
            )
    }
}

pub(crate) fn singleline_stem_from_file_tree_selection(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy().to_string();
    if let Some(stem) = file_name.strip_suffix(".txt") {
        return Some(stem.to_string());
    }
    Some(file_name)
}

impl crate::app::Papyru2App {
    pub(crate) fn sync_singleline_from_file_tree_selection(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(stem) = singleline_stem_from_file_tree_selection(path) else {
            crate::app::trace_debug(format!(
                "file_tree selection sync skipped path={} (no filename)",
                path.display()
            ));
            return;
        };

        crate::app::trace_debug(format!(
            "file_tree selection sync path={} stem='{}'",
            path.display(),
            crate::app::compact_text(&stem)
        ));
        self.singleline.update(cx, |singleline, cx| {
            singleline.apply_text_value_only(stem.clone(), window, cx);
        });
    }

    pub(crate) fn on_singleline_value_changed(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.file_workflow.state() {
            crate::file_update_handler::SinglelineFileState::Neutral => {
                self.ensure_new_file_flow("singleline_value_changed", window, cx);
            }
            crate::file_update_handler::SinglelineFileState::Edit => {
                let now_local = Local::now();
                let previous_path = self.file_workflow.current_edit_path();
                match self.file_workflow.try_rename_in_edit(value, now_local) {
                    Ok(Some(path)) => {
                        crate::app::trace_debug(format!(
                            "rename_flow success new_path={} value='{}'",
                            path.display(),
                            crate::app::compact_text(value)
                        ));
                        self.sync_current_editing_path_to_components(Some(path.clone()), cx);
                        if crate::app::req_ftr14_rename_flow_uses_watcher_refresh_only() {
                            crate::app::trace_debug(format!(
                                "rename_flow watcher_refresh_only=true previous_path={} direct_tree_patch_skipped",
                                previous_path
                                    .as_ref()
                                    .map(|path| path.display().to_string())
                                    .unwrap_or_else(|| "<none>".to_string())
                            ));
                        }
                        self.apply_forced_singleline_stem(
                            crate::file_update_handler::forced_singleline_stem_after_rename(
                                value,
                                path.as_path(),
                                now_local,
                            ),
                            "rename_flow",
                            window,
                            cx,
                        );
                    }
                    Ok(None) => {}
                    Err(error) => {
                        crate::app::trace_debug(format!(
                            "rename_flow failed value='{}' error={error}",
                            crate::app::compact_text(value)
                        ));
                    }
                }
            }
            crate::file_update_handler::SinglelineFileState::New => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::singleline_stem_from_file_tree_selection;
    use std::path::Path;

    #[test]
    fn ftr_test10_req_ftr5_ascii_txt_selection_maps_to_singleline_stem() {
        let actual = singleline_stem_from_file_tree_selection(Path::new("C:/tmp/fileA.txt"));
        assert_eq!(actual.as_deref(), Some("fileA"));
    }

    #[test]
    fn ftr_test11_req_ftr5_multibyte_txt_selection_maps_to_singleline_stem() {
        let actual =
            singleline_stem_from_file_tree_selection(Path::new("C:/tmp/こんにちは 世界.txt"));
        assert_eq!(actual.as_deref(), Some("こんにちは 世界"));
    }
}
