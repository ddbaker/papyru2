use gpui::*;
use gpui_component::{
    IconName, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    resizable::{ResizableState, h_resizable, resizable_panel},
};

use crate::singleline_input::SingleLineInput;

pub struct TopBars {
    singleline: Entity<SingleLineInput>,
    layout_split_state: Entity<ResizableState>,
}

impl TopBars {
    pub fn new(
        window: &mut Window,
        layout_split_state: Entity<ResizableState>,
        cx: &mut Context<Self>,
    ) -> Self {
        let singleline = cx.new(|cx| SingleLineInput::new(window, cx));
        Self {
            singleline,
            layout_split_state,
        }
    }

    pub fn singleline(&self) -> Entity<SingleLineInput> {
        self.singleline.clone()
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
        div().h(px(32.)).w_full().child(
            h_resizable("top-split")
                .with_state(&self.layout_split_state)
                .child(
                    resizable_panel().size(px(320.)).child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(self.render_round_button("round-button1", IconName::Plus, cx))
                            .child(self.render_round_button("round-button2", IconName::Search, cx)),
                    ),
                )
                .child(resizable_panel().child(self.singleline.clone())),
        )
    }
}
