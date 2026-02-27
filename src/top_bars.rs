use gpui::*;
use gpui_component::{
    IconName, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    resizable::{ResizableState, h_resizable, resizable_panel},
};

use crate::singleline_input::SingleLineInput;

#[derive(Clone, Debug)]
pub enum TopBarsEvent {
    PressPlus,
}

pub struct TopBars {
    singleline: Entity<SingleLineInput>,
    layout_split_state: Entity<ResizableState>,
}

impl EventEmitter<TopBarsEvent> for TopBars {}

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

    fn render_plus_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("round-button1")
            .ghost()
            .xsmall()
            .icon(IconName::Plus)
            .on_click(cx.listener(|_, _, _, cx| {
                cx.emit(TopBarsEvent::PressPlus);
            }))
    }

    fn render_search_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("round-button2")
            .ghost()
            .xsmall()
            .icon(IconName::Search)
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
                            .child(self.render_plus_button(cx))
                            .child(self.render_search_button(cx)),
                    ),
                )
                .child(resizable_panel().child(self.singleline.clone())),
        )
    }
}
