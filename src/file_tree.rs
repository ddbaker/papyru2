use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

use gpui::*;
use gpui_component::{
    IconName, h_flex,
    list::ListItem,
    tree::{TreeItem, TreeState, tree},
};

pub enum FileTreeEvent {
    OpenFile(PathBuf),
    RecyclebinDeleteRequested(Vec<PathBuf>),
}

pub struct FileTreeView {
    tree_state: Entity<TreeState>,
    focus_handle: FocusHandle,
    workspace_root: PathBuf,
    root_items: Vec<TreeItem>,
    protected_delete_roots: Vec<PathBuf>,
    selected_item_ids: HashSet<String>,
    suppress_next_row_click_item_id: Option<String>,
}

impl EventEmitter<FileTreeEvent> for FileTreeView {}

impl FileTreeView {
    pub fn new(protected_delete_roots: Vec<PathBuf>, cx: &mut Context<Self>) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let focus_handle = cx.focus_handle().tab_stop(true);
        let workspace_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let mut this = Self {
            tree_state,
            focus_handle,
            workspace_root,
            root_items: Vec::new(),
            protected_delete_roots,
            selected_item_ids: HashSet::new(),
            suppress_next_row_click_item_id: None,
        };
        this.load_files(cx);
        this
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            cx.propagate();
            return;
        }

        let key = event.keystroke.key.as_str().to_ascii_lowercase();
        if key != "delete" {
            cx.propagate();
            return;
        }

        let requested = self.request_recyclebin_delete(cx);
        crate::app::trace_debug(format!("file_tree keydown delete requested={requested}"));
        if requested {
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }

    pub fn refresh_from_filesystem(&mut self, cx: &mut Context<Self>) {
        self.load_files(cx);
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus_handle.focus(window);
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle.contains_focused(window, cx)
    }

    pub fn apply_renamed_path(
        &mut self,
        old_path: &Path,
        new_path: &Path,
        cx: &mut Context<Self>,
    ) -> bool {
        let renamed = rename_tree_item_path(&mut self.root_items, old_path, new_path);
        if !renamed {
            crate::app::trace_debug(format!(
                "file_tree rename patch missed old_path={} new_path={}",
                old_path.display(),
                new_path.display()
            ));
            return false;
        }

        rewrite_selected_item_ids_for_rename(&mut self.selected_item_ids, old_path, new_path);
        if self.suppress_next_row_click_item_id.as_deref()
            == Some(old_path.to_string_lossy().as_ref())
        {
            self.suppress_next_row_click_item_id =
                Some(comparable_path(new_path).to_string_lossy().to_string());
        }
        crate::app::trace_debug(format!(
            "file_tree rename patch applied old_path={} new_path={}",
            old_path.display(),
            new_path.display()
        ));
        self.set_items_from_model(cx);
        true
    }

    pub fn request_recyclebin_delete(&mut self, cx: &mut Context<Self>) -> bool {
        let mut removed_protected_count = 0usize;
        let protected_delete_roots = self.protected_delete_roots.clone();
        self.selected_item_ids.retain(|id| {
            let keep = !is_delete_protected_path(Path::new(id), &protected_delete_roots);
            if !keep {
                removed_protected_count += 1;
            }
            keep
        });
        if removed_protected_count > 0 {
            crate::app::trace_debug(format!(
                "file_tree delete guard removed protected selections count={}",
                removed_protected_count
            ));
            cx.notify();
        }

        let selected_paths = self.selected_paths();
        crate::app::trace_debug(format!(
            "file_tree recyclebin delete requested selected_count={}",
            selected_paths.len()
        ));
        if selected_paths.is_empty() {
            return false;
        }

        cx.emit(FileTreeEvent::RecyclebinDeleteRequested(selected_paths));
        true
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        self.root_items = build_file_items(&self.workspace_root, &self.workspace_root);
        self.set_items_from_model(cx);
    }

    fn set_items_from_model(&mut self, cx: &mut Context<Self>) {
        let mut valid_item_ids = HashSet::new();
        collect_tree_item_ids(&self.root_items, &mut valid_item_ids);
        retain_existing_selections(&mut self.selected_item_ids, &valid_item_ids);
        let items = self.root_items.clone();
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        let mut selected_ids: Vec<_> = self.selected_item_ids.iter().cloned().collect();
        selected_ids.sort_unstable();
        selected_ids.into_iter().map(PathBuf::from).collect()
    }

    fn consume_suppressed_row_click(&mut self, item_id: &str) -> bool {
        if self.suppress_next_row_click_item_id.as_deref() == Some(item_id) {
            self.suppress_next_row_click_item_id = None;
            return true;
        }
        false
    }
}

impl Render for FileTreeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();

        div()
            .size_full()
            .track_focus(&self.focus_handle)
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(
                tree(
                    &self.tree_state,
                    move |ix, entry, _selected, _window, cx| {
                        view.update(cx, |this, cx| {
                            let item = entry.item();
                            let item_id = item.id.to_string();
                            let is_selected = this.selected_item_ids.contains(&item_id);

                            let icon = if !entry.is_folder() {
                                IconName::File
                            } else if entry.is_expanded() {
                                IconName::FolderOpen
                            } else {
                                IconName::Folder
                            };

                            ListItem::new(ix)
                                .selected(is_selected)
                                .w_full()
                                .py_0p5()
                                .px_2()
                                .pl(px(16.) * entry.depth() + px(8.))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            div()
                                                .w(px(22.))
                                                .text_center()
                                                .child(if is_selected { "[x]" } else { "[ ]" })
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener({
                                                        let item = item.clone();
                                                        let item_id = item_id.clone();
                                                        move |this, _, window, cx| {
                                                            cx.stop_propagation();
                                                            this.focus(window);
                                                            this.suppress_next_row_click_item_id =
                                                                Some(item_id.clone());
                                                            crate::app::trace_debug(format!(
                                                                "file_tree focus selector item={} focused={}",
                                                                item.id,
                                                                this.is_focused(window, cx)
                                                            ));
                                                            if is_delete_protected_path(
                                                                Path::new(item.id.as_ref()),
                                                                &this.protected_delete_roots,
                                                            ) {
                                                                let _ = this
                                                                    .selected_item_ids
                                                                    .remove(&item_id);
                                                                crate::app::trace_debug(format!(
                                                                    "file_tree selection guard blocked protected root item={}",
                                                                    item.id
                                                                ));
                                                                cx.notify();
                                                                return;
                                                            }

                                                            let is_now_selected =
                                                                toggle_item_selection(
                                                                    &mut this.selected_item_ids,
                                                                    &item_id,
                                                                );
                                                            crate::app::trace_debug(format!(
                                                                "file_tree selector toggle item={} selected_now={} total_selected={}",
                                                                item.id,
                                                                is_now_selected,
                                                                this.selected_item_ids.len()
                                                            ));
                                                            cx.notify();
                                                        }
                                                    }),
                                                )
                                        )
                                        .child(icon)
                                        .child(item.label.clone()),
                                )
                                .on_click(cx.listener({
                                    let item = item.clone();
                                    let item_id = item_id.clone();
                                    move |this, _, window, cx| {
                                        if this.consume_suppressed_row_click(&item_id) {
                                            crate::app::trace_debug(format!(
                                                "file_tree row click suppressed item={}",
                                                item.id
                                            ));
                                            return;
                                        }

                                        if item.is_folder() {
                                            this.focus(window);
                                            crate::app::trace_debug(format!(
                                                "file_tree row click folder item={} focused={} (expand/collapse handled by tree)",
                                                item.id,
                                                this.is_focused(window, cx)
                                            ));
                                            return;
                                        }

                                        crate::app::trace_debug(format!(
                                            "file_tree row click open file item={}",
                                            item.id
                                        ));
                                        cx.emit(FileTreeEvent::OpenFile(PathBuf::from(
                                            item.id.as_str(),
                                        )));
                                        cx.notify();
                                    }
                                }))
                        })
                    },
                )
                .text_sm()
                .p_1()
                .h_full(),
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

    sort_tree_items(&mut items);
    items
}

fn sort_tree_items(items: &mut [TreeItem]) {
    items.sort_by(|a, b| {
        b.is_folder()
            .cmp(&a.is_folder())
            .then(a.label.cmp(&b.label))
    });
}

fn rename_tree_item_path(items: &mut [TreeItem], old_path: &Path, new_path: &Path) -> bool {
    for index in 0..items.len() {
        if is_same_path(Path::new(items[index].id.as_ref()), old_path) {
            rewrite_tree_item_subtree_paths(&mut items[index], old_path, new_path);
            sort_tree_items(items);
            return true;
        }

        if rename_tree_item_path(&mut items[index].children, old_path, new_path) {
            sort_tree_items(items);
            return true;
        }
    }

    false
}

fn rewrite_tree_item_subtree_paths(item: &mut TreeItem, old_root: &Path, new_root: &Path) {
    if let Some(rebased_path) = rebase_tree_path(Path::new(item.id.as_ref()), old_root, new_root) {
        item.id = rebased_path.to_string_lossy().to_string().into();
        item.label = item_label_from_path(&rebased_path).into();
    }

    for child in item.children.iter_mut() {
        rewrite_tree_item_subtree_paths(child, old_root, new_root);
    }
}

fn rebase_tree_path(path: &Path, old_root: &Path, new_root: &Path) -> Option<PathBuf> {
    let comparable = comparable_path(path);
    let comparable_old_root = comparable_path(old_root);
    let comparable_new_root = comparable_path(new_root);
    if comparable == comparable_old_root {
        return Some(comparable_new_root);
    }

    comparable
        .strip_prefix(&comparable_old_root)
        .ok()
        .map(|relative| comparable_new_root.join(relative))
}

fn item_label_from_path(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

fn rewrite_selected_item_ids_for_rename(
    selected_item_ids: &mut HashSet<String>,
    old_path: &Path,
    new_path: &Path,
) {
    let mut updated = HashSet::new();
    for selected in selected_item_ids.drain() {
        let selected_path = PathBuf::from(&selected);
        let next = rebase_tree_path(selected_path.as_path(), old_path, new_path)
            .unwrap_or(selected_path)
            .to_string_lossy()
            .to_string();
        updated.insert(next);
    }
    *selected_item_ids = updated;
}

fn toggle_item_selection(selected_item_ids: &mut HashSet<String>, item_id: &str) -> bool {
    if selected_item_ids.contains(item_id) {
        selected_item_ids.remove(item_id);
        return false;
    }

    selected_item_ids.insert(item_id.to_string());
    true
}

fn retain_existing_selections(
    selected_item_ids: &mut HashSet<String>,
    valid_item_ids: &HashSet<String>,
) {
    selected_item_ids.retain(|id| valid_item_ids.contains(id));
}

fn collect_tree_item_ids(items: &[TreeItem], ids: &mut HashSet<String>) {
    for item in items {
        ids.insert(item.id.to_string());
        if item.is_folder() {
            collect_tree_item_ids(&item.children, ids);
        }
    }
}

fn recyclebin_target_path(source_path: &Path, recyclebin_dir: &Path) -> Option<PathBuf> {
    let file_name = source_path.file_name()?.to_string_lossy().to_string();

    for suffix in 1usize.. {
        let candidate_name = if suffix == 1 {
            file_name.clone()
        } else if source_path.is_dir() {
            format!("{file_name}_{suffix}")
        } else {
            let file_name_path = Path::new(file_name.as_str());
            let stem = file_name_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(file_name.as_str());
            match file_name_path.extension().and_then(|ext| ext.to_str()) {
                Some(ext) => format!("{stem}_{suffix}.{ext}"),
                None => format!("{stem}_{suffix}"),
            }
        };
        let candidate = recyclebin_dir.join(candidate_name);
        if !candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn comparable_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();
    if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }
    path.to_path_buf()
}

fn is_path_within(path: &Path, base: &Path) -> bool {
    comparable_path(path).starts_with(comparable_path(base))
}

fn is_same_path(lhs: &Path, rhs: &Path) -> bool {
    comparable_path(lhs) == comparable_path(rhs)
}

fn is_delete_protected_path(path: &Path, protected_delete_roots: &[PathBuf]) -> bool {
    protected_delete_roots
        .iter()
        .any(|protected| is_same_path(path, protected))
}

pub(crate) fn move_entries_to_recyclebin(
    source_paths: &[PathBuf],
    recyclebin_dir: &Path,
) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    fs::create_dir_all(recyclebin_dir)?;

    let mut moved = Vec::new();
    let mut seen_sources: HashSet<PathBuf> = HashSet::new();
    for source_path in source_paths {
        if !seen_sources.insert(source_path.clone()) {
            continue;
        }
        if !source_path.exists() {
            continue;
        }
        if is_path_within(source_path, recyclebin_dir) {
            crate::app::trace_debug(format!(
                "file_tree recyclebin move skipped source already in recyclebin source={} recyclebin={}",
                source_path.display(),
                recyclebin_dir.display()
            ));
            continue;
        }
        if is_path_within(recyclebin_dir, source_path) {
            crate::app::trace_debug(format!(
                "file_tree recyclebin move skipped source is ancestor of recyclebin source={} recyclebin={}",
                source_path.display(),
                recyclebin_dir.display()
            ));
            continue;
        }

        let Some(target) = recyclebin_target_path(source_path.as_path(), recyclebin_dir) else {
            continue;
        };
        match fs::rename(source_path, &target) {
            Ok(()) => moved.push((source_path.clone(), target)),
            Err(error) => {
                crate::app::trace_debug(format!(
                    "file_tree recyclebin move skipped rename error source={} target={} error={error}",
                    source_path.display(),
                    target.display()
                ));
            }
        }
    }

    Ok(moved)
}

impl crate::app::Papyru2App {
    pub(crate) fn refresh_file_tree(&mut self, reason: &str, cx: &mut Context<Self>) {
        crate::app::trace_debug(format!("file_tree refresh requested reason={reason}"));
        self.file_tree.update(cx, |file_tree, cx| {
            file_tree.refresh_from_filesystem(cx);
        });
    }

    pub(crate) fn on_file_tree_delete_requested(
        &mut self,
        paths: Vec<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        crate::app::trace_debug(format!(
            "file_tree delete request selected_count={} recyclebin={}",
            paths.len(),
            self.app_paths.recyclebin_dir.display()
        ));

        match move_entries_to_recyclebin(&paths, self.app_paths.recyclebin_dir.as_path()) {
            Ok(moved) => {
                crate::app::trace_debug(format!(
                    "file_tree delete move success moved_count={} selected_count={}",
                    moved.len(),
                    paths.len()
                ));
                self.refresh_file_tree("req-ftr3-delete", cx);
            }
            Err(error) => {
                crate::app::trace_debug(format!("file_tree delete move failed error={error}"));
            }
        }
    }

    pub(crate) fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush_editor_content_before_context_switch("req-aus8-open-file", cx) {
            crate::app::trace_debug(format!(
                "open_file aborted path={} (pre-switch autosave failed)",
                path.display()
            ));
            return;
        }

        let opened = self.editor.update(cx, {
            let path = path.clone();
            move |editor, cx| editor.open_file(path, window, cx)
        });

        if !opened {
            crate::app::trace_debug(format!("open_file failed path={}", path.display()));
            return;
        }

        self.file_workflow.set_edit_from_open_file(path.clone());
        self.sync_current_editing_path_to_components(Some(path), cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        TreeItem, build_file_items, collect_tree_item_ids, is_delete_protected_path,
        move_entries_to_recyclebin, rename_tree_item_path, retain_existing_selections,
        rewrite_selected_item_ids_for_rename, toggle_item_selection,
    };
    use std::{
        collections::HashSet,
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    fn new_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "gpui_papyru2_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn remove_temp_root(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn ftr_test1_refresh_reflects_create_and_delete_filesystem_changes() {
        let root = new_temp_root("ftr_test1");
        let file_a = root.join("a.txt");
        let file_b = root.join("b.txt");
        fs::write(&file_a, "a").expect("seed a");

        let initial_items = build_file_items(&root, &root);
        let mut initial_ids = HashSet::new();
        collect_tree_item_ids(&initial_items, &mut initial_ids);
        assert!(initial_ids.contains(file_a.to_string_lossy().as_ref()));

        fs::remove_file(&file_a).expect("delete a");
        fs::write(&file_b, "b").expect("seed b");

        let refreshed_items = build_file_items(&root, &root);
        let mut refreshed_ids = HashSet::new();
        collect_tree_item_ids(&refreshed_items, &mut refreshed_ids);
        assert!(!refreshed_ids.contains(file_a.to_string_lossy().as_ref()));
        assert!(refreshed_ids.contains(file_b.to_string_lossy().as_ref()));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test2_multi_selection_retains_only_existing_entries_after_refresh() {
        let mut selected = HashSet::new();
        let id_a = "C:/tmp/a.txt".to_string();
        let id_b = "C:/tmp/b.txt".to_string();

        assert!(toggle_item_selection(&mut selected, &id_a));
        assert!(toggle_item_selection(&mut selected, &id_b));
        assert_eq!(selected.len(), 2);

        let mut valid = HashSet::new();
        valid.insert(id_b.clone());
        retain_existing_selections(&mut selected, &valid);

        assert_eq!(selected.len(), 1);
        assert!(selected.contains(&id_b));
    }

    #[test]
    fn ftr_test3_delete_single_selection_moves_entry_to_recyclebin() {
        let root = new_temp_root("ftr_test3");
        let recyclebin_dir = root.join("recyclebin");
        let source = root.join("single.txt");
        fs::write(&source, "source").expect("seed source");

        let moved =
            move_entries_to_recyclebin(std::slice::from_ref(&source), recyclebin_dir.as_path())
                .expect("move to recyclebin");

        assert_eq!(moved.len(), 1);
        assert!(!source.exists());
        assert!(moved[0].1.exists());
        assert!(moved[0].1.starts_with(&recyclebin_dir));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test4_delete_multiple_selection_moves_all_entries_to_recyclebin() {
        let root = new_temp_root("ftr_test4");
        let recyclebin_dir = root.join("recyclebin");
        let source_file = root.join("multi_file.txt");
        let source_dir = root.join("multi_dir");
        fs::write(&source_file, "file").expect("seed file");
        fs::create_dir_all(&source_dir).expect("seed dir");
        fs::write(source_dir.join("inside.txt"), "inner").expect("seed inner");

        let moved = move_entries_to_recyclebin(
            &[source_file.clone(), source_dir.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("move multi to recyclebin");

        assert_eq!(moved.len(), 2);
        assert!(!source_file.exists());
        assert!(!source_dir.exists());
        assert!(moved.iter().all(|(_, target)| target.exists()));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test5_recyclebin_move_target_is_resolved_under_recyclebin_dir() {
        let root = new_temp_root("ftr_test5");
        let recyclebin_dir = root.join("recyclebin");
        let source = root.join("path_check.txt");
        fs::write(&source, "path").expect("seed source");

        let moved =
            move_entries_to_recyclebin(std::slice::from_ref(&source), recyclebin_dir.as_path())
                .expect("move path check");

        assert_eq!(moved.len(), 1);
        assert!(moved[0].1.starts_with(&recyclebin_dir));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test6_recyclebin_move_uses_collision_safe_suffix() {
        let root = new_temp_root("ftr_test6");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let source = root.join("collision.txt");
        fs::write(&source, "source").expect("seed source");
        fs::write(recyclebin_dir.join("collision.txt"), "existing").expect("seed existing");

        let moved =
            move_entries_to_recyclebin(std::slice::from_ref(&source), recyclebin_dir.as_path())
                .expect("move with collision");

        assert_eq!(moved.len(), 1);
        assert_eq!(
            moved[0].1.file_name().and_then(|name| name.to_str()),
            Some("collision_2.txt")
        );
        assert_eq!(
            fs::read_to_string(recyclebin_dir.join("collision.txt")).expect("read existing"),
            "existing"
        );
        assert_eq!(
            fs::read_to_string(&moved[0].1).expect("read moved"),
            "source"
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test7_delete_skips_ancestor_of_recyclebin_and_moves_valid_entries() {
        let root = new_temp_root("ftr_test7");
        let user_document_dir = root.join("user_document");
        let recyclebin_dir = user_document_dir.join("recyclebin");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let source_ancestor = user_document_dir.clone();
        let source_file = user_document_dir.join("note.txt");
        fs::write(&source_file, "note").expect("seed note");

        let moved = move_entries_to_recyclebin(
            &[source_ancestor.clone(), source_file.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("move with ancestor");

        assert_eq!(moved.len(), 1);
        assert!(moved[0].0.ends_with(Path::new("note.txt")));
        assert!(moved[0].1.starts_with(&recyclebin_dir));
        assert!(source_ancestor.exists());
        assert!(!source_file.exists());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test8_delete_guard_blocks_data_and_user_document_roots_only() {
        let root = new_temp_root("ftr_test8");
        let data_dir = root.join("data");
        let user_document_dir = data_dir.join("user_document");
        let sample_file = user_document_dir
            .join("2026")
            .join("03")
            .join("07")
            .join("note.txt");
        let protected = vec![data_dir.clone(), user_document_dir.clone()];

        assert!(is_delete_protected_path(data_dir.as_path(), &protected));
        assert!(is_delete_protected_path(
            user_document_dir.as_path(),
            &protected
        ));
        assert!(!is_delete_protected_path(sample_file.as_path(), &protected));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test9_rename_patch_updates_tree_item_and_selection_in_place() {
        let root = new_temp_root("ftr_test9");
        let old_path = root.join("alpha.txt");
        let new_path = root.join("beta.txt");
        let sibling_path = root.join("gamma.txt");

        let mut items = vec![
            TreeItem::new(old_path.to_string_lossy().to_string(), "alpha.txt"),
            TreeItem::new(sibling_path.to_string_lossy().to_string(), "gamma.txt"),
        ];
        let mut selected = HashSet::from([old_path.to_string_lossy().to_string()]);

        assert!(rename_tree_item_path(
            &mut items,
            old_path.as_path(),
            new_path.as_path()
        ));
        rewrite_selected_item_ids_for_rename(&mut selected, old_path.as_path(), new_path.as_path());

        let mut ids = HashSet::new();
        collect_tree_item_ids(&items, &mut ids);
        assert!(ids.contains(new_path.to_string_lossy().as_ref()));
        assert!(!ids.contains(old_path.to_string_lossy().as_ref()));
        assert!(selected.contains(new_path.to_string_lossy().as_ref()));
        assert_eq!(items[0].label.as_ref(), "beta.txt");
        remove_temp_root(root.as_path());
    }
}
