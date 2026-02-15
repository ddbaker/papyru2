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

pub struct Papyru2App {
    top_bars: Entity<TopBars>,
    editor: Entity<Papyru2Editor>,
    file_tree: Entity<FileTreeView>,
    layout_split_state: Entity<ResizableState>,
    _subscriptions: Vec<Subscription>,
}

impl Papyru2App {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let layout_split_state = cx.new(|_| ResizableState::default());
        let top_bars = cx.new(|cx| TopBars::new(window, layout_split_state.clone(), cx));
        let editor = cx.new(|cx| Papyru2Editor::new(window, cx));
        let file_tree = cx.new(|cx| FileTreeView::new(cx));

        let _subscriptions = vec![cx.subscribe_in(
            &file_tree,
            window,
            move |this, _, event: &FileTreeEvent, window, cx| match event {
                FileTreeEvent::OpenFile(path) => {
                    this.open_file(path.clone(), window, cx);
                }
            },
        )];

        Self {
            top_bars,
            editor,
            file_tree,
            layout_split_state,
            _subscriptions,
        }
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
