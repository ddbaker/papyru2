use std::path::PathBuf;

use gpui::*;
use gpui_component::{
    Root,
    resizable::{ResizableState, h_resizable, resizable_panel},
    v_flex,
};
use gpui_component_assets::Assets;

use crate::editor::Papyru2Editor;
use crate::file_tree::{FileTreeEvent, FileTreeView};
use crate::top_bars::TopBars;


pub(crate) fn trace_debug(message: impl AsRef<str>) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let line = format!("[{now}] {}\n", message.as_ref());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("debug_assoc_trace.log")
    {
        let _ = std::io::Write::write_all(&mut file, line.as_bytes());
    }
}

pub(crate) fn compact_text(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\n', "\\n")
}

pub struct Papyru2App {
    top_bars: Entity<TopBars>,
    singleline: Entity<crate::singleline_input::SingleLineInput>,
    editor: Entity<Papyru2Editor>,
    file_tree: Entity<FileTreeView>,
    layout_split_state: Entity<ResizableState>,
    _subscriptions: Vec<Subscription>,
}

impl Papyru2App {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let layout_split_state = cx.new(|_| ResizableState::default());
        let top_bars = cx.new(|cx| TopBars::new(window, layout_split_state.clone(), cx));
        let singleline = top_bars.read(cx).singleline();
        let editor = cx.new(|cx| Papyru2Editor::new(window, cx));
        let file_tree = cx.new(|cx| FileTreeView::new(cx));

        let _subscriptions = vec![
            cx.subscribe_in(
                &file_tree,
                window,
                move |this, _, event: &FileTreeEvent, window, cx| match event {
                    FileTreeEvent::OpenFile(path) => {
                        this.open_file(path.clone(), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &singleline,
                window,
                move |this,
                      _,
                      event: &crate::singleline_input::SingleLineEvent,
                      window,
                      cx| match event {
                    crate::singleline_input::SingleLineEvent::PressEnter => {
                        trace_debug("app received SingleLineEvent::PressEnter");
                        this.transfer_singleline_enter(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &editor,
                window,
                move |this, _, event: &crate::editor::EditorEvent, window, cx| match event {
                    crate::editor::EditorEvent::BackspaceAtLineHead => {
                        trace_debug("app received EditorEvent::BackspaceAtLineHead");
                        this.transfer_editor_backspace(window, cx);
                    }
                },
            ),
        ];

        Self {
            top_bars,
            singleline,
            editor,
            file_tree,
            layout_split_state,
            _subscriptions,
        }
    }

    fn transfer_singleline_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let singleline_snapshot = self.singleline.read(cx).snapshot(cx);
        let editor_snapshot = self.editor.read(cx).snapshot(cx);

        trace_debug(format!(
            "transfer_enter before sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
            compact_text(&singleline_snapshot.value),
            singleline_snapshot.cursor_char,
            compact_text(&editor_snapshot.value),
            editor_snapshot.cursor_line,
            editor_snapshot.cursor_char
        ));

        let Some(result) = crate::association::transfer_on_enter(
            &singleline_snapshot.value,
            singleline_snapshot.cursor_char,
            &editor_snapshot.value,
        ) else {
            trace_debug("transfer_enter skipped (no right side)");
            return;
        };

        trace_debug(format!(
            "transfer_enter result sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
            compact_text(&result.new_singleline_text),
            result.new_singleline_cursor_char,
            compact_text(&result.new_editor_text),
            result.new_editor_cursor_line,
            result.new_editor_cursor_char
        ));

        self.singleline.update(cx, |singleline, cx| {
            singleline.apply_text_and_cursor(
                result.new_singleline_text.clone(),
                result.new_singleline_cursor_char,
                window,
                cx,
            );
        });

        self.editor.update(cx, |editor, cx| {
            editor.apply_text_and_cursor(
                result.new_editor_text.clone(),
                result.new_editor_cursor_line,
                result.new_editor_cursor_char,
                window,
                cx,
            );
        });

        match result.focus_target {
            crate::association::FocusTarget::Editor => {
                self.editor.update(cx, |editor, cx| {
                    editor.focus(window, cx);
                });
            }
            crate::association::FocusTarget::SingleLine => {
                self.singleline.update(cx, |singleline, cx| {
                    singleline.focus(window, cx);
                });
            }
        }

        let sl_after = self.singleline.read(cx).snapshot(cx);
        let ed_after = self.editor.read(cx).snapshot(cx);
        trace_debug(format!(
            "transfer_enter after sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
            compact_text(&sl_after.value),
            sl_after.cursor_char,
            compact_text(&ed_after.value),
            ed_after.cursor_line,
            ed_after.cursor_char
        ));
    }

    fn transfer_editor_backspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor_snapshot = self.editor.read(cx).snapshot(cx);
        trace_debug(format!(
            "transfer_backspace before ed='{}' ed_cursor=({}, {})",
            compact_text(&editor_snapshot.value),
            editor_snapshot.cursor_line,
            editor_snapshot.cursor_char
        ));

        if !crate::association::should_transfer_backspace(
            editor_snapshot.cursor_line,
            editor_snapshot.cursor_char,
        ) {
            trace_debug("transfer_backspace skipped (cursor not at line-1 head)");
            return;
        }

        let singleline_snapshot = self.singleline.read(cx).snapshot(cx);
        trace_debug(format!(
            "transfer_backspace before sl='{}' sl_cursor={}",
            compact_text(&singleline_snapshot.value),
            singleline_snapshot.cursor_char
        ));

        let Some(result) = crate::association::transfer_on_backspace(
            &singleline_snapshot.value,
            singleline_snapshot.cursor_char,
            &editor_snapshot.value,
        ) else {
            trace_debug("transfer_backspace skipped (editor line-1 empty)");
            return;
        };

        trace_debug(format!(
            "transfer_backspace result sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
            compact_text(&result.new_singleline_text),
            result.new_singleline_cursor_char,
            compact_text(&result.new_editor_text),
            result.new_editor_cursor_line,
            result.new_editor_cursor_char
        ));

        self.singleline.update(cx, |singleline, cx| {
            singleline.apply_text_and_cursor(
                result.new_singleline_text.clone(),
                result.new_singleline_cursor_char,
                window,
                cx,
            );
        });

        self.editor.update(cx, |editor, cx| {
            editor.apply_text_and_cursor(
                result.new_editor_text.clone(),
                result.new_editor_cursor_line,
                result.new_editor_cursor_char,
                window,
                cx,
            );
        });

        match result.focus_target {
            crate::association::FocusTarget::Editor => {
                self.editor.update(cx, |editor, cx| {
                    editor.focus(window, cx);
                });
            }
            crate::association::FocusTarget::SingleLine => {
                self.singleline.update(cx, |singleline, cx| {
                    singleline.focus(window, cx);
                });
            }
        }

        let sl_after = self.singleline.read(cx).snapshot(cx);
        let ed_after = self.editor.read(cx).snapshot(cx);
        trace_debug(format!(
            "transfer_backspace after sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
            compact_text(&sl_after.value),
            sl_after.cursor_char,
            compact_text(&ed_after.value),
            ed_after.cursor_line,
            ed_after.cursor_char
        ));
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.editor.update(cx, move |editor, cx| {
            editor.open_file(path, window, cx);
        });
    }
}

impl Render for Papyru2App {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("papyru2")
            .size_full()
            .gap_2()
            .p_2()
            .child(self.top_bars.clone())
            .child(
                div().flex_1().child(
                    h_resizable("bottom-split")
                        .with_state(&self.layout_split_state)
                        .child(resizable_panel().size(px(320.)).child(self.file_tree.clone()))
                        .child(resizable_panel().child(self.editor.clone())),
                ),
            )
    }
}

pub fn run() {
    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1200.), px(800.)), cx)),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| Papyru2App::new(window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}

