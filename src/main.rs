use std::path::PathBuf;

use gpui::*;
use gpui_component::{
    ActiveTheme, IconName, Root, Sizable,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    list::ListItem,
    resizable::{h_resizable, resizable_panel},
    tree::{TreeItem, TreeState, tree},
    v_flex,
};
use gpui_component_assets::Assets;

mod singleline_input;
use singleline_input::SingleLineInput;

struct Papyru2App {
    singleline: Entity<SingleLineInput>,
    editor: Entity<InputState>,
    file_tree_state: Entity<TreeState>,
    workspace_root: PathBuf,
}

impl Papyru2App {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let singleline = cx.new(|cx| SingleLineInput::new(window, cx));

        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .code_editor("rust")
                .line_number(true)
                .soft_wrap(false)
                .placeholder("Test area (integrated editor)")
        });

        let file_tree_state = cx.new(|cx| TreeState::new(cx));
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let this = Self {
            singleline,
            editor,
            file_tree_state,
            workspace_root,
        };

        this.load_files(cx);
        this
    }

    fn load_files(&self, cx: &mut Context<Self>) {
        let items = build_file_items(&self.workspace_root, &self.workspace_root);
        self.file_tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => return,
        };

        let language = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("txt")
            .to_string();

        self.editor.update(cx, |state, cx| {
            state.set_highlighter(language, cx);
            state.set_value(content, window, cx);
        });
    }

    fn render_file_tree(&self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();

        tree(
            &self.file_tree_state,
            move |ix, entry, _selected, _window, cx| {
                view.update(cx, |_, cx| {
                    let item = entry.item();
                    let icon = if !entry.is_folder() {
                        IconName::File
                    } else if entry.is_expanded() {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    };

                    ListItem::new(ix)
                        .w_full()
                        .py_0p5()
                        .px_2()
                        .pl(px(16.) * entry.depth() + px(8.))
                        .child(h_flex().gap_2().child(icon).child(item.label.clone()))
                        .on_click(cx.listener({
                            let item = item.clone();
                            move |this, _, window, cx| {
                                if item.is_folder() {
                                    return;
                                }

                                this.open_file(PathBuf::from(item.id.as_str()), window, cx);
                                cx.notify();
                            }
                        }))
                })
            },
        )
        .text_sm()
        .p_1()
        .h_full()
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

impl Render for Papyru2App {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("papyru2")
            .size_full()
            .gap_2()
            .p_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(self.render_round_button("round-button1", IconName::Plus, cx))
                    .child(self.render_round_button("round-button2", IconName::Search, cx))
                    .child(div().flex_1().child(self.singleline.clone())),
            )
            .child(
                div().flex_1().child(
                    h_resizable("bottom-split")
                        .child(
                            resizable_panel()
                                .size(px(320.))
                                .child(self.render_file_tree(window, cx)),
                        )
                        .child(
                            resizable_panel().child(
                                Input::new(&self.editor)
                                    .h_full()
                                    .font_family(cx.theme().mono_font_family.clone())
                                    .text_size(cx.theme().mono_font_size),
                            ),
                        ),
                ),
            )
    }
}

fn build_file_items(root: &PathBuf, path: &PathBuf) -> Vec<TreeItem> {
    let mut items = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();

            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name == ".git")
            {
                continue;
            }

            let relative_path = path.strip_prefix(root).unwrap_or(&path);
            let file_name = relative_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("Unknown")
                .to_string();
            let id = path.to_string_lossy().to_string();

            if path.is_dir() {
                let children = build_file_items(root, &path);
                items.push(TreeItem::new(id, file_name).children(children));
            } else {
                items.push(TreeItem::new(id, file_name));
            }
        }
    }

    items.sort_by(|a, b| {
        b.is_folder()
            .cmp(&a.is_folder())
            .then(a.label.cmp(&b.label))
    });
    items
}

fn main() {
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
