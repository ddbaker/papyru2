use gpui::*;
use gpui_component::{
    input::{Input, InputEvent, InputState},
    *,
};

pub struct SingleLineInput {
    sl_input_state: Entity<InputState>,
    display_text: SharedString,

    /// We need to keep the subscriptions alive with the SingleLineInput entity.
    ///
    /// So if the SingleLineInput entity is dropped, the subscriptions are also dropped.
    /// This is important to avoid memory leaks.
    _sl_subscriptions: Vec<Subscription>,
}

impl SingleLineInput {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let sl_input_state = cx.new(|cx| InputState::new(window, cx).placeholder("Enter your name"));

        let _sl_subscriptions = vec![cx.subscribe_in(&sl_input_state, window, {
            let sl_input_state = sl_input_state.clone();
            move |this, _, ev: &InputEvent, _window, cx| match ev {
                InputEvent::Change => {
                    let value = sl_input_state.read(cx).value();
                    this.display_text = format!("Hello, {}!", value).into();
                    cx.notify()
                }
                _ => {}
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
        v_flex()
            .p_5()
            .gap_2()
            .size_full()
            .items_center()
            //.justify_center()
            .child(Input::new(&self.sl_input_state))
            //.child(self.display_text.clone())
    }
}
