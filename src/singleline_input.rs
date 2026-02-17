use gpui::*;
use gpui_component::input::{Input, InputState};

#[derive(Clone, Debug)]
pub enum SingleLineEvent {
    PressEnter,
}

#[derive(Clone, Debug)]
pub struct SingleLineSnapshot {
    pub value: String,
    pub cursor_char: usize,
}

pub struct SingleLineInput {
    sl_input_state: Entity<InputState>,
}

impl EventEmitter<SingleLineEvent> for SingleLineInput {}

impl SingleLineInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sl_input_state =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type here..."));

        Self { sl_input_state }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            cx.propagate();
            return;
        }

        let key = event.keystroke.key.as_str();
        crate::app::trace_debug(format!("singleline keydown key={key}"));

        if key == "enter" || key == "return" {
            crate::app::trace_debug("singleline emit PressEnter");
            cx.emit(SingleLineEvent::PressEnter);
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
        let cursor_char_u32 = cursor_char.min(u32::MAX as usize) as u32;

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
    }

    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sl_input_state
            .update(cx, |state, cx| state.focus(window, cx));
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

