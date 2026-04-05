use gpui::*;
use gpui_component::{
    IconName, IconNamed, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    resizable::{ResizableState, h_resizable, resizable_panel},
};

use crate::singleline_input::SingleLineInput;

pub(crate) const SHARED_INTER_PANEL_SPACING_PX: f32 = 10.0;

pub(crate) const TOP_BARS_BUTTONS_ADJACENT_TO_SINGLELINE: bool = true;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum TopBarButtonSpec {
    RefreshFileTree,
    PlusResetToNeutral,
}

pub(crate) const TOP_BARS_BUTTON_ORDER: [TopBarButtonSpec; 2] = [
    TopBarButtonSpec::RefreshFileTree,
    TopBarButtonSpec::PlusResetToNeutral,
];

#[derive(Clone, Debug)]
pub enum TopBarsEvent {
    PressFolderRefresh,
    PressPlus,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TopBarsIconName {
    FolderRefresh,
}

impl IconNamed for TopBarsIconName {
    fn path(self) -> SharedString {
        match self {
            Self::FolderRefresh => crate::app::FOLDER_REFRESH_ICON_PATH.into(),
        }
    }
}

pub struct TopBars {
    singleline: Entity<SingleLineInput>,
    layout_split_state: Entity<ResizableState>,
    left_panel_size: Pixels,
}

impl EventEmitter<TopBarsEvent> for TopBars {}

impl TopBars {
    pub fn new(
        window: &mut Window,
        layout_split_state: Entity<ResizableState>,
        left_panel_size: Pixels,
        cx: &mut Context<Self>,
    ) -> Self {
        let singleline = cx.new(|cx| SingleLineInput::new(window, cx));
        Self {
            singleline,
            layout_split_state,
            left_panel_size,
        }
    }

    pub fn singleline(&self) -> Entity<SingleLineInput> {
        self.singleline.clone()
    }

    pub fn sync_layout_split(
        &mut self,
        layout_split_state: Entity<ResizableState>,
        left_panel_size: Pixels,
    ) {
        self.layout_split_state = layout_split_state;
        self.left_panel_size = left_panel_size;
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

    fn render_refresh_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("round-button2")
            .ghost()
            .xsmall()
            .icon(TopBarsIconName::FolderRefresh)
            .on_click(cx.listener(|_, _, _, cx| {
                cx.emit(TopBarsEvent::PressFolderRefresh);
            }))
    }

    fn render_button_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let button_group = h_flex().w_full().gap_2().items_center();
        let button_group = if TOP_BARS_BUTTONS_ADJACENT_TO_SINGLELINE {
            button_group.justify_end()
        } else {
            button_group.justify_start()
        };

        match TOP_BARS_BUTTON_ORDER {
            [
                TopBarButtonSpec::RefreshFileTree,
                TopBarButtonSpec::PlusResetToNeutral,
            ] => button_group
                .child(self.render_refresh_button(cx))
                .child(self.render_plus_button(cx)),
            [
                TopBarButtonSpec::PlusResetToNeutral,
                TopBarButtonSpec::RefreshFileTree,
            ] => button_group
                .child(self.render_plus_button(cx))
                .child(self.render_refresh_button(cx)),
            _ => button_group
                .child(self.render_refresh_button(cx))
                .child(self.render_plus_button(cx)),
        }
    }
}

impl Render for TopBars {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().h(px(32.)).w_full().child(
            h_resizable("top-split")
                .with_state(&self.layout_split_state)
                .child(
                    resizable_panel()
                        .size(self.left_panel_size)
                        .child(self.render_button_group(cx)),
                )
                .child(
                    resizable_panel().child(
                        div()
                            .w_full()
                            .pl(px(SHARED_INTER_PANEL_SPACING_PX))
                            .child(self.singleline.clone()),
                    ),
                ),
        )
    }
}

impl crate::app::Papyru2App {
    pub(crate) fn handle_plus_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush_editor_content_before_context_switch("req-aus6-plus", cx) {
            crate::app::trace_debug("plus_button aborted (pre-switch autosave failed)");
            return;
        }

        let editor_was_focused = self.editor.read(cx).is_focused(window, cx);
        let singleline_was_focused = self.singleline.read(cx).is_focused(window, cx);
        crate::app::trace_debug(format!(
            "plus_button start editor_focused={} singleline_focused={}",
            editor_was_focused, singleline_was_focused
        ));

        if !self.file_workflow.transition_edit_to_neutral() {
            crate::app::trace_debug("plus_button no-op (state is not EDIT)");
            return;
        }

        let previous_path = self.file_workflow.current_edit_path();
        crate::app::trace_debug(format!(
            "plus_button transition EDIT -> NEUTRAL previous_path={}",
            previous_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
        self.sync_current_editing_path_to_components(None, cx);

        // req-newf34: enforce deterministic reset order so final focus/cursor lands on singleline.
        for step in crate::app::req_newf34_plus_button_reset_steps() {
            match step {
                crate::app::PlusButtonResetStep::ClearEditor => {
                    crate::app::trace_debug("plus_button req-newf34 step=clear_editor");
                    self.editor.update(cx, |editor, cx| {
                        editor.apply_text_and_cursor("", 0, 0, window, cx);
                    });
                }
                crate::app::PlusButtonResetStep::ClearSingleline => {
                    crate::app::trace_debug("plus_button req-newf34 step=clear_singleline");
                    self.singleline.update(cx, |singleline, cx| {
                        singleline.apply_text_and_cursor("", 0, window, cx);
                    });
                }
                crate::app::PlusButtonResetStep::FocusSingleline => {
                    crate::app::trace_debug("plus_button req-newf34 step=focus_singleline");
                    self.singleline.update(cx, |singleline, cx| {
                        singleline.focus(window, cx);
                    });
                }
            }
        }

        crate::app::trace_debug(
            "plus_button req-newf34 schedule deferred singleline focus reassert",
        );
        cx.defer_in(window, move |this, window, cx| {
            this.singleline.update(cx, |singleline, cx| {
                singleline.apply_cursor(0, window, cx);
                singleline.focus(window, cx);
            });

            let singleline_snapshot = this.singleline.read(cx).snapshot(cx);
            let singleline_focused = this.singleline.read(cx).is_focused(window, cx);
            let editor_focused = this.editor.read(cx).is_focused(window, cx);
            crate::app::trace_debug(format!(
                "plus_button req-newf34 deferred focus reassert done cursor={} singleline_focused={} editor_focused={} pre_editor_focused={} pre_singleline_focused={}",
                singleline_snapshot.cursor_char,
                singleline_focused,
                editor_focused,
                editor_was_focused,
                singleline_was_focused
            ));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SHARED_INTER_PANEL_SPACING_PX, TOP_BARS_BUTTON_ORDER,
        TOP_BARS_BUTTONS_ADJACENT_TO_SINGLELINE, TopBarButtonSpec, TopBarsEvent,
    };

    #[test]
    fn lo_test1_req_lo2_singleline_left_spacing_is_10px() {
        assert_eq!(SHARED_INTER_PANEL_SPACING_PX, 10.0);
    }

    #[test]
    fn lo_test5_req_lo5_buttons_are_adjacent_to_singleline() {
        assert!(TOP_BARS_BUTTONS_ADJACENT_TO_SINGLELINE);
    }

    #[test]
    fn ftr_test82_req_ftr23_button_order_is_refresh_then_plus() {
        assert_eq!(
            TOP_BARS_BUTTON_ORDER,
            [
                TopBarButtonSpec::RefreshFileTree,
                TopBarButtonSpec::PlusResetToNeutral,
            ]
        );
    }

    #[test]
    fn ftr_test83_req_ftr23_refresh_event_contract_is_present_and_plus_unchanged() {
        let refresh_event = TopBarsEvent::PressFolderRefresh;
        assert!(matches!(refresh_event, TopBarsEvent::PressFolderRefresh));

        let emitted_event = TopBarsEvent::PressPlus;
        assert!(matches!(emitted_event, TopBarsEvent::PressPlus));
        assert_eq!(
            TOP_BARS_BUTTON_ORDER[1],
            TopBarButtonSpec::PlusResetToNeutral
        );
    }
}
