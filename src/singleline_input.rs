use std::path::PathBuf;

use gpui::*;
use gpui_component::input::InputEvent;
use gpui_component::input::{Input, InputState};

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
}

impl EventEmitter<SingleLineEvent> for SingleLineInput {}

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

        Self {
            sl_input_state,
            last_value,
            last_cursor,
            pending_programmatic_change_events: 0,
            current_editing_file_path: None,
            _subscriptions,
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
        div()
            .w_full()
            .on_key_down(cx.listener(Self::on_key_down))
            .child(Input::new(&self.sl_input_state).w_full())
    }
}
