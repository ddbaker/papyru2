use std::path::PathBuf;

use gpui::*;
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState},
};

use gpui_component::input::InputEvent;
#[derive(Clone, Debug)]
pub enum EditorEvent {
    BackspaceAtLineHead,
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
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<EditorEvent> for Papyru2Editor {}

impl Papyru2Editor {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input_state = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("rust")
                .line_number(true)
                .soft_wrap(false)
                .placeholder("Test area (integrated editor)")
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
                    crate::app::trace_debug(format!(
                        "editor InputEvent::Change cursor=({}, {}) value='{}'",
                        cursor.line,
                        cursor.character,
                        crate::app::compact_text(&value)
                    ));

                    if this.pending_programmatic_change_events > 0 {
                        this.pending_programmatic_change_events -= 1;
                        crate::app::trace_debug(format!(
                            "editor InputEvent::Change ignored as programmatic (remaining={})",
                            this.pending_programmatic_change_events
                        ));
                        this.last_value = value;
                        this.last_cursor = cursor;
                        return;
                    }

                    let is_noop_change = value == this.last_value;
                    let first_line_non_empty = value
                        .split('\n')
                        .next()
                        .is_some_and(|line| !line.is_empty());

                    if is_noop_change && cursor.line == 0 && cursor.character == 0 && first_line_non_empty
                    {
                        crate::app::trace_debug(format!(
                            "editor InputEvent::Change detected no-op backspace candidate at head (last_cursor=({}, {}))",
                            this.last_cursor.line,
                            this.last_cursor.character
                        ));
                        cx.emit(EditorEvent::BackspaceAtLineHead);
                    }

                    this.last_value = value;
                    this.last_cursor = cursor;
                }
                InputEvent::PressEnter { secondary } => {
                    crate::app::trace_debug(format!(
                        "editor InputEvent::PressEnter secondary={secondary}"
                    ));
                }
                InputEvent::Focus => {
                    crate::app::trace_debug("editor InputEvent::Focus");
                }
                InputEvent::Blur => {
                    crate::app::trace_debug("editor InputEvent::Blur");
                }
            }
        })];

        Self {
            input_state,
            last_value,
            last_cursor,
            pending_programmatic_change_events: 0,
            _subscriptions,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let key_raw = event.keystroke.key.as_str();
        let key = key_raw.to_ascii_lowercase();
        crate::app::trace_debug(format!(
            "editor keydown raw='{}' key='{}' held={} key_char={}",
            key_raw,
            key,
            event.is_held,
            event
                .keystroke
                .key_char
                .as_deref()
                .unwrap_or("<none>")
        ));

        if key == "backspace" || key == "delete" {
            let snapshot = self.snapshot(cx);
            crate::app::trace_debug(format!(
                "editor backspace candidate cursor=({}, {}) value='{}'",
                snapshot.cursor_line,
                snapshot.cursor_char,
                crate::app::compact_text(&snapshot.value)
            ));
        }

        cx.propagate();
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
        crate::app::trace_debug(format!(
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

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input_state
            .update(cx, |state, cx| state.focus(window, cx));
    }

    pub fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => return,
        };

        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_string();

        self.pending_programmatic_change_events += 1;
        crate::app::trace_debug(format!(
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
    }
}

impl Render for Papyru2Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(
                Input::new(&self.input_state)
                    .size_full()
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(cx.theme().mono_font_size),
            )
    }
}
