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
    SelectionChanged(PathBuf),
    OpenFile(PathBuf),
    RecyclebinDeleteRequested(Vec<PathBuf>),
}

pub struct FileTreeView {
    tree_state: Entity<TreeState>,
    focus_handle: FocusHandle,
    tree_root_dir: PathBuf,
    root_items: Vec<TreeItem>,
    protected_delete_roots: Vec<PathBuf>,
    selected_item_ids: HashSet<String>,
    delete_shortcut_armed: bool,
    selection_anchor_item_id: Option<String>,
    visible_item_ids: Vec<String>,
}

impl EventEmitter<FileTreeEvent> for FileTreeView {}

impl FileTreeView {
    pub fn new(
        protected_delete_roots: Vec<PathBuf>,
        tree_root_dir: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let focus_handle = cx.focus_handle().tab_stop(true);

        let mut this = Self {
            tree_state,
            focus_handle,
            tree_root_dir,
            root_items: Vec::new(),
            protected_delete_roots,
            selected_item_ids: HashSet::new(),
            delete_shortcut_armed: false,
            selection_anchor_item_id: None,
            visible_item_ids: Vec::new(),
        };
        crate::app::trace_debug(format!(
            "file_tree init root_dir={}",
            this.tree_root_dir.display()
        ));
        this.load_files(cx);
        this
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            cx.propagate();
            return;
        }

        let key = event.keystroke.key.as_str().to_ascii_lowercase();
        let is_delete_key =
            key == "delete" || key == "backspace" || key == "forwarddelete" || key == "del";
        match key.as_str() {
            _ if is_delete_key => {
                let requested = self.request_recyclebin_delete(cx);
                crate::app::trace_debug(format!(
                    "file_tree keydown key={} requested={requested}",
                    key
                ));
                if requested {
                    cx.stop_propagation();
                } else {
                    cx.propagate();
                }
            }
            "up" | "down" => {
                let shift = event.keystroke.modifiers.shift;
                let handled = self.move_selection_with_arrow_key(key.as_str(), shift, cx);
                if handled {
                    cx.stop_propagation();
                } else {
                    cx.propagate();
                }
            }
            "enter" => {
                self.handle_enter_key(cx);
                cx.propagate();
            }
            _ => {
                cx.propagate();
            }
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
        if let Some(anchor_item_id) = self.selection_anchor_item_id.take() {
            let anchor_path = PathBuf::from(anchor_item_id);
            let rewritten_anchor = rebase_tree_path(anchor_path.as_path(), old_path, new_path)
                .unwrap_or(anchor_path)
                .to_string_lossy()
                .to_string();
            self.selection_anchor_item_id = Some(rewritten_anchor);
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
            self.disarm_delete_shortcut("delete_request_empty_selection");
            return false;
        }

        self.disarm_delete_shortcut("delete_request_emit");
        cx.emit(FileTreeEvent::RecyclebinDeleteRequested(selected_paths));
        true
    }

    pub fn consume_delete_shortcut_for_editor(&mut self) -> bool {
        if !self.delete_shortcut_armed || self.selected_item_ids.is_empty() {
            return false;
        }

        self.delete_shortcut_armed = false;
        crate::app::trace_debug(format!(
            "file_tree delete shortcut consumed_for_editor selected_count={}",
            self.selected_item_ids.len()
        ));
        true
    }

    pub fn disarm_delete_shortcut(&mut self, reason: &str) {
        if !self.delete_shortcut_armed {
            return;
        }

        self.delete_shortcut_armed = false;
        crate::app::trace_debug(format!(
            "file_tree delete shortcut disarmed reason={reason} selected_count={}",
            self.selected_item_ids.len()
        ));
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        self.root_items = build_file_items(&self.tree_root_dir, &self.tree_root_dir);
        crate::app::trace_debug(format!(
            "file_tree load root_dir={} top_level_count={}",
            self.tree_root_dir.display(),
            self.root_items.len()
        ));
        self.set_items_from_model(cx);
    }

    fn set_items_from_model(&mut self, cx: &mut Context<Self>) {
        let mut valid_item_ids = HashSet::new();
        collect_tree_item_ids(&self.root_items, &mut valid_item_ids);
        retain_existing_selections(&mut self.selected_item_ids, &valid_item_ids);
        if self.selected_item_ids.is_empty() {
            self.disarm_delete_shortcut("set_items_empty_selection");
        }
        if self
            .selection_anchor_item_id
            .as_ref()
            .is_some_and(|anchor| !valid_item_ids.contains(anchor))
        {
            self.selection_anchor_item_id = None;
        }
        let items = self.root_items.clone();
        self.tree_state.update(cx, |state, cx| {
            state.set_items(items, cx);
        });
        self.rebuild_visible_item_ids();
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        let mut selected_ids: Vec<_> = self.selected_item_ids.iter().cloned().collect();
        selected_ids.sort_unstable();
        selected_ids.into_iter().map(PathBuf::from).collect()
    }

    fn handle_enter_key(&mut self, cx: &mut Context<Self>) {
        self.rebuild_visible_item_ids();
        if self.visible_item_ids.is_empty() {
            return;
        }

        let has_selected_index = self.tree_state.read(cx).selected_index().is_some();
        if !has_selected_index {
            self.tree_state
                .update(cx, |state, cx| state.set_selected_index(Some(0), cx));
        }

        let Some((_, item_id, is_folder)) = self.current_tree_selection_snapshot(cx) else {
            return;
        };

        self.apply_single_selection_by_id(item_id.as_str(), "enter_key", cx);
        crate::app::trace_debug(format!(
            "file_tree enter select item={} folder={} total_selected={}",
            item_id,
            is_folder,
            self.selected_item_ids.len()
        ));
        if is_folder {
            return;
        }

        crate::app::trace_debug(format!("file_tree enter open file item={item_id}"));
        self.disarm_delete_shortcut("enter_open_file");
        cx.emit(FileTreeEvent::OpenFile(PathBuf::from(item_id)));
    }

    fn move_selection_with_arrow_key(
        &mut self,
        key: &str,
        shift_range: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        self.rebuild_visible_item_ids();
        if self.visible_item_ids.is_empty() {
            return false;
        }

        let len = self.visible_item_ids.len();
        let current_index = self.tree_state.read(cx).selected_index().unwrap_or(0);
        let next_index = if key == "up" {
            if current_index > 0 {
                current_index - 1
            } else {
                len.saturating_sub(1)
            }
        } else if current_index + 1 < len {
            current_index + 1
        } else {
            0
        };

        self.tree_state.update(cx, |state, cx| {
            state.set_selected_index(Some(next_index), cx);
            let strategy = if key == "up" {
                gpui::ScrollStrategy::Top
            } else {
                gpui::ScrollStrategy::Bottom
            };
            state.scroll_to_item(next_index, strategy);
        });

        if shift_range {
            self.apply_shift_range_selection_to_index(
                next_index,
                Some(current_index),
                "shift_arrow",
                cx,
            );
            crate::app::trace_debug(format!(
                "file_tree keydown {key} shift_range=true current_index={} next_index={} selected_count={}",
                current_index,
                next_index,
                self.selected_item_ids.len()
            ));
        } else if let Some(item_id) = self.visible_item_ids.get(next_index).cloned() {
            self.apply_single_selection_by_id(item_id.as_str(), "arrow_key", cx);
            crate::app::trace_debug(format!(
                "file_tree keydown {key} shift_range=false current_index={} next_index={} selected_item={}",
                current_index, next_index, item_id
            ));
        }

        true
    }

    fn on_row_click(
        &mut self,
        item: &TreeItem,
        row_index: usize,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus(window);
        self.rebuild_visible_item_ids();

        if event.modifiers().shift {
            self.apply_shift_range_selection_to_index(
                row_index,
                Some(row_index),
                "shift_click",
                cx,
            );
            crate::app::trace_debug(format!(
                "file_tree row click shift_range=true item={} index={} selected_count={}",
                item.id,
                row_index,
                self.selected_item_ids.len()
            ));
            return;
        }

        self.apply_single_selection_by_id(item.id.as_ref(), "row_click", cx);
        crate::app::trace_debug(format!(
            "file_tree row click select item={} folder={} index={} focused={}",
            item.id,
            item.is_folder(),
            row_index,
            self.is_focused(window, cx)
        ));
        if item.is_folder() {
            return;
        }

        crate::app::trace_debug(format!(
            "file_tree row click selection_changed item={} (open deferred to enter)",
            item.id
        ));
        cx.emit(FileTreeEvent::SelectionChanged(PathBuf::from(
            item.id.as_ref(),
        )));
    }

    fn current_tree_selection_snapshot(&self, cx: &App) -> Option<(usize, String, bool)> {
        let state = self.tree_state.read(cx);
        let selected_index = state.selected_index()?;
        let entry = state.selected_entry()?;
        Some((
            selected_index,
            entry.item().id.to_string(),
            entry.is_folder(),
        ))
    }

    fn rebuild_visible_item_ids(&mut self) {
        self.visible_item_ids.clear();
        collect_visible_item_ids(&self.root_items, &mut self.visible_item_ids);
    }

    fn apply_single_selection_by_id(
        &mut self,
        item_id: &str,
        reason: &str,
        cx: &mut Context<Self>,
    ) {
        replace_single_selection(&mut self.selected_item_ids, item_id);
        self.delete_shortcut_armed = true;
        self.selection_anchor_item_id = Some(item_id.to_string());
        crate::app::trace_debug(format!(
            "file_tree selection single reason={reason} item={} total_selected={} delete_shortcut_armed={}",
            item_id,
            self.selected_item_ids.len(),
            self.delete_shortcut_armed
        ));
        cx.notify();
    }

    fn apply_shift_range_selection_to_index(
        &mut self,
        target_index: usize,
        fallback_anchor_index: Option<usize>,
        reason: &str,
        cx: &mut Context<Self>,
    ) {
        if target_index >= self.visible_item_ids.len() {
            return;
        }

        let derived_anchor_item_id = self
            .selection_anchor_item_id
            .as_ref()
            .filter(|id| {
                self.visible_item_ids
                    .iter()
                    .any(|visible_id| visible_id == *id)
            })
            .cloned()
            .or_else(|| fallback_anchor_index.and_then(|ix| self.visible_item_ids.get(ix).cloned()))
            .unwrap_or_else(|| self.visible_item_ids[target_index].clone());

        let anchor_index =
            find_visible_index(&self.visible_item_ids, derived_anchor_item_id.as_str())
                .unwrap_or(target_index);
        select_range_items(
            &mut self.selected_item_ids,
            &self.visible_item_ids,
            anchor_index,
            target_index,
        );
        self.delete_shortcut_armed = !self.selected_item_ids.is_empty();
        self.selection_anchor_item_id = Some(derived_anchor_item_id.clone());
        crate::app::trace_debug(format!(
            "file_tree selection range reason={reason} anchor_item={} anchor_index={} target_index={} total_selected={} delete_shortcut_armed={}",
            derived_anchor_item_id,
            anchor_index,
            target_index,
            self.selected_item_ids.len(),
            self.delete_shortcut_armed
        ));
        cx.notify();
    }
}

impl Render for FileTreeView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.rebuild_visible_item_ids();
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
                            let row_content = if use_checkbox_selection_markers() {
                                h_flex()
                                    .gap_2()
                                    .child(if is_selected { "[x]" } else { "[ ]" })
                                    .child(icon)
                                    .child(item.label.clone())
                            } else {
                                h_flex().gap_2().child(icon).child(item.label.clone())
                            };

                            let row = ListItem::new(ix)
                                .selected(is_selected)
                                .w_full()
                                .py_0p5()
                                .px_2()
                                .pl(px(16.) * entry.depth() + px(8.))
                                .child(row_content)
                                .on_click(cx.listener({
                                    let item = item.clone();
                                    move |this, event, window, cx| {
                                        this.on_row_click(&item, ix, event, window, cx);
                                    }
                                }));
                            if let Some(color) = selected_row_highlight_color(is_selected) {
                                row.bg(color)
                            } else {
                                row
                            }
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

#[cfg(test)]
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

fn collect_visible_item_ids(items: &[TreeItem], ids: &mut Vec<String>) {
    for item in items {
        ids.push(item.id.to_string());
        if item.is_folder() && item.is_expanded() {
            collect_visible_item_ids(&item.children, ids);
        }
    }
}

fn collect_tree_item_ids(items: &[TreeItem], ids: &mut HashSet<String>) {
    for item in items {
        ids.insert(item.id.to_string());
        if item.is_folder() {
            collect_tree_item_ids(&item.children, ids);
        }
    }
}

fn find_visible_index(visible_item_ids: &[String], item_id: &str) -> Option<usize> {
    visible_item_ids
        .iter()
        .position(|visible_item_id| visible_item_id == item_id)
}

fn select_range_items(
    selected_item_ids: &mut HashSet<String>,
    visible_item_ids: &[String],
    start_index: usize,
    end_index: usize,
) {
    let (from, to) = if start_index <= end_index {
        (start_index, end_index)
    } else {
        (end_index, start_index)
    };

    selected_item_ids.clear();
    for item_id in visible_item_ids.iter().skip(from).take(to - from + 1) {
        selected_item_ids.insert(item_id.clone());
    }
}

fn replace_single_selection(selected_item_ids: &mut HashSet<String>, item_id: &str) {
    selected_item_ids.clear();
    selected_item_ids.insert(item_id.to_string());
}

fn selected_row_highlight_color(is_selected: bool) -> Option<Hsla> {
    if !is_selected {
        return None;
    }
    Some(hsla(0.58, 0.65, 0.88, 1.0))
}

fn use_checkbox_selection_markers() -> bool {
    false
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

#[cfg(test)]
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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct FileTreeDeleteOutcome {
    pub moved_to_recyclebin: Vec<(PathBuf, PathBuf)>,
    pub permanently_deleted: Vec<PathBuf>,
}

fn remove_path_permanently(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

pub(crate) fn delete_entries_for_file_tree(
    source_paths: &[PathBuf],
    recyclebin_dir: &Path,
) -> io::Result<FileTreeDeleteOutcome> {
    fs::create_dir_all(recyclebin_dir)?;

    let mut outcome = FileTreeDeleteOutcome::default();
    let mut seen_sources: HashSet<PathBuf> = HashSet::new();
    for source_path in source_paths {
        if !seen_sources.insert(source_path.clone()) {
            continue;
        }
        if !source_path.exists() {
            continue;
        }

        if is_same_path(source_path, recyclebin_dir) {
            crate::app::trace_debug(format!(
                "file_tree permanent delete skipped recyclebin root source={} recyclebin={}",
                source_path.display(),
                recyclebin_dir.display()
            ));
            continue;
        }

        if is_path_within(source_path, recyclebin_dir) {
            match remove_path_permanently(source_path) {
                Ok(()) => {
                    crate::app::trace_debug(format!(
                        "file_tree permanent delete success source={} recyclebin={}",
                        source_path.display(),
                        recyclebin_dir.display()
                    ));
                    outcome.permanently_deleted.push(source_path.clone());
                }
                Err(error) => {
                    crate::app::trace_debug(format!(
                        "file_tree permanent delete failed source={} error={error}",
                        source_path.display()
                    ));
                }
            }
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
            Ok(()) => {
                crate::app::trace_debug(format!(
                    "file_tree recyclebin move success source={} target={}",
                    source_path.display(),
                    target.display()
                ));
                outcome
                    .moved_to_recyclebin
                    .push((source_path.clone(), target));
            }
            Err(error) => {
                crate::app::trace_debug(format!(
                    "file_tree recyclebin move skipped rename error source={} target={} error={error}",
                    source_path.display(),
                    target.display()
                ));
            }
        }
    }

    Ok(outcome)
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

        match delete_entries_for_file_tree(&paths, self.app_paths.recyclebin_dir.as_path()) {
            Ok(outcome) => {
                crate::app::trace_debug(format!(
                    "file_tree delete success moved_count={} permanently_deleted_count={} selected_count={}",
                    outcome.moved_to_recyclebin.len(),
                    outcome.permanently_deleted.len(),
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
        TreeItem, build_file_items, collect_tree_item_ids, collect_visible_item_ids,
        delete_entries_for_file_tree, find_visible_index, is_delete_protected_path,
        move_entries_to_recyclebin, rename_tree_item_path, replace_single_selection,
        retain_existing_selections, rewrite_selected_item_ids_for_rename, select_range_items,
        selected_row_highlight_color, toggle_item_selection, use_checkbox_selection_markers,
    };
    use gpui::hsla;
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

    #[test]
    fn ftr_test12_req_ftr6_delete_under_recyclebin_removes_file_permanently() {
        let root = new_temp_root("ftr_test12");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");
        let target = recyclebin_dir.join("trash.txt");
        fs::write(&target, "trash").expect("seed recyclebin file");

        let outcome =
            delete_entries_for_file_tree(std::slice::from_ref(&target), recyclebin_dir.as_path())
                .expect("delete under recyclebin");

        assert_eq!(outcome.moved_to_recyclebin.len(), 0);
        assert_eq!(outcome.permanently_deleted.len(), 1);
        assert!(!target.exists());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test13_req_ftr6_delete_multiple_under_recyclebin_removes_all_permanently() {
        let root = new_temp_root("ftr_test13");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");
        let target_file = recyclebin_dir.join("trash_a.txt");
        let target_dir = recyclebin_dir.join("trash_dir");
        fs::write(&target_file, "a").expect("seed recyclebin file");
        fs::create_dir_all(&target_dir).expect("create recyclebin dir");
        fs::write(target_dir.join("inside.txt"), "b").expect("seed recyclebin dir file");

        let outcome = delete_entries_for_file_tree(
            &[target_file.clone(), target_dir.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("delete multiple under recyclebin");

        assert_eq!(outcome.moved_to_recyclebin.len(), 0);
        assert_eq!(outcome.permanently_deleted.len(), 2);
        assert!(!target_file.exists());
        assert!(!target_dir.exists());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test14_req_ftr6_non_recyclebin_delete_still_moves_to_recyclebin() {
        let root = new_temp_root("ftr_test14");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");
        let source = root.join("outside.txt");
        fs::write(&source, "outside").expect("seed outside file");

        let outcome =
            delete_entries_for_file_tree(std::slice::from_ref(&source), recyclebin_dir.as_path())
                .expect("delete outside recyclebin");

        assert_eq!(outcome.permanently_deleted.len(), 0);
        assert_eq!(outcome.moved_to_recyclebin.len(), 1);
        assert!(!source.exists());
        assert!(outcome.moved_to_recyclebin[0].1.exists());
        assert!(
            outcome.moved_to_recyclebin[0]
                .1
                .starts_with(&recyclebin_dir)
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test15_req_ftr7_checkbox_selection_markers_are_disabled() {
        assert!(
            !use_checkbox_selection_markers(),
            "req-ftr7 expects file-tree checkbox markers to be removed"
        );
    }

    #[test]
    fn ftr_test16_req_ftr8_single_selection_replaces_previous_selection() {
        let mut selected = HashSet::from([
            "C:/tmp/fileA.txt".to_string(),
            "C:/tmp/fileB.txt".to_string(),
        ]);
        replace_single_selection(&mut selected, "C:/tmp/fileC.txt");

        assert_eq!(selected.len(), 1);
        assert!(selected.contains("C:/tmp/fileC.txt"));
        assert!(!selected.contains("C:/tmp/fileA.txt"));
        assert!(!selected.contains("C:/tmp/fileB.txt"));
    }

    #[test]
    fn ftr_test17_req_ftr8_visible_order_supports_arrow_enter_navigation() {
        let root = TreeItem::new("/root", "root").expanded(true).children([
            TreeItem::new("/root/2026", "2026")
                .expanded(true)
                .children([
                    TreeItem::new("/root/2026/fileA.txt", "fileA.txt"),
                    TreeItem::new("/root/2026/fileB.txt", "fileB.txt"),
                ]),
            TreeItem::new("/root/recyclebin", "recyclebin"),
        ]);

        let mut visible = Vec::new();
        collect_visible_item_ids(&[root], &mut visible);

        assert_eq!(
            visible,
            vec![
                "/root".to_string(),
                "/root/2026".to_string(),
                "/root/2026/fileA.txt".to_string(),
                "/root/2026/fileB.txt".to_string(),
                "/root/recyclebin".to_string()
            ]
        );
        assert_eq!(find_visible_index(&visible, "/root/2026"), Some(1));
        assert_eq!(
            find_visible_index(&visible, "/root/2026/fileA.txt"),
            Some(2)
        );
    }

    #[test]
    fn ftr_test18_req_ftr9_shift_click_selects_contiguous_range() {
        let visible = vec![
            "/root/a.txt".to_string(),
            "/root/b.txt".to_string(),
            "/root/c.txt".to_string(),
            "/root/d.txt".to_string(),
        ];
        let mut selected = HashSet::new();

        select_range_items(&mut selected, &visible, 1, 3);

        assert_eq!(selected.len(), 3);
        assert!(selected.contains("/root/b.txt"));
        assert!(selected.contains("/root/c.txt"));
        assert!(selected.contains("/root/d.txt"));
        assert!(!selected.contains("/root/a.txt"));
    }

    #[test]
    fn ftr_test19_req_ftr9_shift_arrow_range_selection_supports_reverse_direction() {
        let visible = vec![
            "/root/a.txt".to_string(),
            "/root/b.txt".to_string(),
            "/root/c.txt".to_string(),
            "/root/d.txt".to_string(),
        ];
        let mut selected = HashSet::new();

        select_range_items(&mut selected, &visible, 3, 1);

        assert_eq!(selected.len(), 3);
        assert!(selected.contains("/root/b.txt"));
        assert!(selected.contains("/root/c.txt"));
        assert!(selected.contains("/root/d.txt"));
        assert!(!selected.contains("/root/a.txt"));
    }

    #[test]
    fn ftr_test20_req_ftr10_selected_rows_use_pale_blue_highlight() {
        assert_eq!(
            selected_row_highlight_color(true),
            Some(hsla(0.58, 0.65, 0.88, 1.0))
        );
        assert_eq!(selected_row_highlight_color(false), None);
    }

    #[test]
    fn ftr_test24_req_ftr11_rooted_tree_excludes_user_document_dir_row() {
        let root = new_temp_root("ftr_test24");
        let user_document_dir = root.join("user_document");
        fs::create_dir_all(user_document_dir.join("2026").join("03"))
            .expect("create date directory");
        fs::create_dir_all(user_document_dir.join("recyclebin")).expect("create recyclebin");

        let items = build_file_items(&user_document_dir, &user_document_dir);
        let mut ids = HashSet::new();
        collect_tree_item_ids(&items, &mut ids);

        assert!(!ids.contains(user_document_dir.to_string_lossy().as_ref()));
        assert!(ids.contains(user_document_dir.join("2026").to_string_lossy().as_ref()));
        assert!(
            ids.contains(
                user_document_dir
                    .join("recyclebin")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test25_req_ftr11_top_level_rows_are_direct_children_of_user_document_dir() {
        let root = new_temp_root("ftr_test25");
        let user_document_dir = root.join("user_document");
        fs::create_dir_all(user_document_dir.join("2026")).expect("create year directory");
        fs::create_dir_all(user_document_dir.join("recyclebin")).expect("create recyclebin");
        fs::create_dir_all(user_document_dir.join("2025")).expect("create another year directory");

        let items = build_file_items(&user_document_dir, &user_document_dir);
        let top_labels: Vec<String> = items.iter().map(|item| item.label.to_string()).collect();

        assert_eq!(
            top_labels,
            vec![
                "2025".to_string(),
                "2026".to_string(),
                "recyclebin".to_string()
            ]
        );
        assert!(!top_labels.contains(&"user_document".to_string()));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test23_req_ftr3_regression_delete_flow_uses_provided_recyclebin_dir() {
        let root = new_temp_root("ftr_test23");
        let recyclebin_dir = root.join("data").join("user_document").join("recyclebin");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");
        let source = root
            .join("data")
            .join("user_document")
            .join("2026")
            .join("03")
            .join("08")
            .join("target.txt");
        fs::create_dir_all(source.parent().expect("source parent")).expect("create source dirs");
        fs::write(&source, "target").expect("seed source");

        let outcome =
            delete_entries_for_file_tree(std::slice::from_ref(&source), recyclebin_dir.as_path())
                .expect("delete through file tree flow");

        assert_eq!(outcome.permanently_deleted.len(), 0);
        assert_eq!(outcome.moved_to_recyclebin.len(), 1);
        assert!(
            outcome.moved_to_recyclebin[0]
                .1
                .starts_with(&recyclebin_dir)
        );
        remove_temp_root(root.as_path());
    }
}
