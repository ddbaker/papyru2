use gpui::*;
use gpui_component::input::{Input, InputEvent, InputState};

pub struct SingleLineInput {
    sl_input_state: Entity<InputState>,
    display_text: SharedString,
    _sl_subscriptions: Vec<Subscription>,
}

impl SingleLineInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sl_input_state =
            cx.new(|cx| InputState::new(window, cx).placeholder("Type here..."));

        let _sl_subscriptions = vec![cx.subscribe_in(&sl_input_state, window, {
            let sl_input_state = sl_input_state.clone();
            move |this, _, ev: &InputEvent, _window, cx| {
                if let InputEvent::Change = ev {
                    let value = sl_input_state.read(cx).value();
                    this.display_text = format!("Hello, {}!", value).into();
                    cx.notify();
                }
            }
        })];

        Self {
            sl_input_state,
            display_text: SharedString::default(),
            _sl_subscriptions,
        }
    }
}

impl Render for SingleLineInput {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Input::new(&self.sl_input_state).w_full()
    }
}
