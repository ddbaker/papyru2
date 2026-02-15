use gpui::*;
use gpui_component::{
    IconName, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
};

use crate::singleline_input::SingleLineInput;

pub struct TopBars {
    singleline: Entity<SingleLineInput>,
}

impl TopBars {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let singleline = cx.new(|cx| SingleLineInput::new(window, cx));
        Self { singleline }
    }

    fn render_round_button(
        &self,
        id: &'static str,
        icon: IconName,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        Button::new(id)
            .ghost()
            .xsmall()
            .icon(icon)
            .on_click(cx.listener(|_, _, _, _| {
                // Placeholder button (no-op)
            }))
    }
}

impl Render for TopBars {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_2()
            .items_center()
            .child(self.render_round_button("round-button1", IconName::Plus, cx))
            .child(self.render_round_button("round-button2", IconName::Search, cx))
            .child(div().flex_1().child(self.singleline.clone()))
    }
}
