use gpui::*;
use gpui_component::{
    IconNamed, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    resizable::{ResizableState, h_resizable, resizable_panel},
};

use crate::singleline_input::SingleLineInput;

pub(crate) const SHARED_INTER_PANEL_SPACING_PX: f32 = 10.0;

pub(crate) const TOP_BARS_BUTTONS_ADJACENT_TO_SINGLELINE: bool = true;
pub(crate) const TOP_BARS_BUTTON_GROUP_LEFT_SHIFT_PX: f32 = 15.0;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum TopBarButtonSpec {
    RefreshFileTree,
    PlusResetToNeutral,
}

pub(crate) const TOP_BARS_BUTTON_ORDER: [TopBarButtonSpec; 2] = [
    TopBarButtonSpec::RefreshFileTree,
    TopBarButtonSpec::PlusResetToNeutral,
];

pub(crate) const FOLDER_REFRESH_ICON_PATH: &str = "icons/folder-refresh.svg";

const FOLDER_REFRESH_ICON_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M18 14.5C19.11 14.5 20.11 14.95 20.83 15.67L22 14.5V18.5H18L19.77 16.73C19.32 16.28 18.69 16 18 16C16.62 16 15.5 17.12 15.5 18.5C15.5 19.88 16.62 21 18 21C18.82 21 19.54 20.61 20 20H21.71C21.12 21.47 19.68 22.5 18 22.5C15.79 22.5 14 20.71 14 18.5C14 16.29 15.79 14.5 18 14.5M20 8H4V18H12L12 18.5C12 19 12.06 19.5 12.17 20H4C2.89 20 2 19.1 2 18L2 6C2 4.89 2.89 4 4 4H10L12 6H20C21.1 6 22 6.89 22 8V13C21.39 12.63 20.72 12.34 20 12.17V8Z" /></svg>"#;

pub(crate) const PLUS_THIN_ICON_PATH: &str = "icons/add-file.svg";

const PLUS_THIN_ICON_SVG: &[u8] = br##"<?xml version="1.0" encoding="utf-8"?><!-- Uploaded to: SVG Repo, www.svgrepo.com, Generator: SVG Repo Mixer Tools --><svg width="800px" height="800px" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg"><path d="M13.5 3H12H8C6.34315 3 5 4.34315 5 6V18C5 19.6569 6.34315 21 8 21H11M13.5 3L19 8.625M13.5 3V7.625C13.5 8.17728 13.9477 8.625 14.5 8.625H19M19 8.625V11.8125" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/><path d="M17 15V18M17 21V18M17 18H14M17 18H20" stroke="#000000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/></svg>"##;

pub(crate) fn load_top_bars_icon_asset(path: &str) -> Option<&'static [u8]> {
    match path {
        FOLDER_REFRESH_ICON_PATH => Some(FOLDER_REFRESH_ICON_SVG),
        PLUS_THIN_ICON_PATH => Some(PLUS_THIN_ICON_SVG),
        _ => None,
    }
}

pub(crate) fn list_top_bars_icon_assets(path: &str, assets: &mut Vec<SharedString>) {
    for custom_icon_path in [FOLDER_REFRESH_ICON_PATH, PLUS_THIN_ICON_PATH] {
        if custom_icon_path.starts_with(path)
            && !assets
                .iter()
                .any(|entry| entry.as_ref() == custom_icon_path)
        {
            assets.push(custom_icon_path.into());
        }
    }
}

#[derive(Clone, Debug)]
pub enum TopBarsEvent {
    PressFolderRefresh,
    PressPlus,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum TopBarsIconName {
    FolderRefresh,
    PlusThin,
}

impl IconNamed for TopBarsIconName {
    fn path(self) -> SharedString {
        match self {
            Self::FolderRefresh => FOLDER_REFRESH_ICON_PATH.into(),
            Self::PlusThin => PLUS_THIN_ICON_PATH.into(),
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
        ui_color_config: crate::app::UiColorConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let singleline = cx.new(|cx| SingleLineInput::new(window, ui_color_config, cx));
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
            .large()
            .icon(TopBarsIconName::PlusThin)
            .on_click(cx.listener(|_, _, _, cx| {
                cx.emit(TopBarsEvent::PressPlus);
            }))
    }

    fn render_refresh_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        Button::new("round-button2")
            .ghost()
            .large()
            .icon(TopBarsIconName::FolderRefresh)
            .on_click(cx.listener(|_, _, _, cx| {
                cx.emit(TopBarsEvent::PressFolderRefresh);
            }))
    }

    fn render_button_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let button_group = h_flex().w_full().gap_2().items_center();
        let button_group = if TOP_BARS_BUTTONS_ADJACENT_TO_SINGLELINE {
            button_group
                .justify_end()
                .pr(px(TOP_BARS_BUTTON_GROUP_LEFT_SHIFT_PX))
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
        FOLDER_REFRESH_ICON_PATH, PLUS_THIN_ICON_PATH, SHARED_INTER_PANEL_SPACING_PX,
        TOP_BARS_BUTTON_GROUP_LEFT_SHIFT_PX, TOP_BARS_BUTTON_ORDER,
        TOP_BARS_BUTTONS_ADJACENT_TO_SINGLELINE, TopBarButtonSpec, TopBarsEvent, TopBarsIconName,
    };

    #[test]
    fn lo_test1_req_lo2_singleline_left_spacing_is_10px() {
        assert_eq!(SHARED_INTER_PANEL_SPACING_PX, 10.0);
    }

    #[test]
    fn ftr_test86_req_ftr23_button_group_left_shift_is_15px() {
        assert_eq!(TOP_BARS_BUTTON_GROUP_LEFT_SHIFT_PX, 15.0);
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

    #[test]
    fn ftr_test87_req_ftr23_follow_icons_are_custom_svg_assets() {
        assert_eq!(
            <TopBarsIconName as gpui_component::IconNamed>::path(TopBarsIconName::FolderRefresh)
                .as_ref(),
            FOLDER_REFRESH_ICON_PATH
        );
        assert_eq!(
            <TopBarsIconName as gpui_component::IconNamed>::path(TopBarsIconName::PlusThin).as_ref(),
            PLUS_THIN_ICON_PATH
        );
    }
}
