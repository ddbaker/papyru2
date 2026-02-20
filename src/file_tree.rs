use std::path::PathBuf;

use gpui::*;
use gpui_component::{
    IconName, h_flex,
    list::ListItem,
    tree::{TreeItem, TreeState, tree},
};

pub enum FileTreeEvent {
    OpenFile(PathBuf),
}

pub struct FileTreeView {
    tree_state: Entity<TreeState>,
    workspace_root: PathBuf,
}

impl EventEmitter<FileTreeEvent> for FileTreeView {}

impl FileTreeView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let this = Self {
            tree_state,
            workspace_root,
        };
        this.load_files(cx);
        this
    }

    fn load_files(&self, cx: &mut Context<Self>) {
        let items = build_file_items(&self.workspace_root, &self.workspace_root);
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
    }
}

impl Render for FileTreeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();

        tree(
            &self.tree_state,
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
                            move |_, _, _, cx| {
                                if item.is_folder() {
                                    return;
                                }

                                cx.emit(FileTreeEvent::OpenFile(PathBuf::from(item.id.as_str())));
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
