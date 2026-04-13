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

use gpui_component::ActiveTheme as _;
pub enum FileTreeEvent {
    SelectionChanged(PathBuf),
    OpenFile(PathBuf),
    RecyclebinDeleteRequested(Vec<PathBuf>),
}

pub(crate) fn should_restore_selection_after_watcher_refresh(
    selected_count: usize,
    current_edit_path: Option<&Path>,
) -> bool {
    selected_count == 0 && current_edit_path.is_some()
}

pub(crate) fn should_apply_req_newf38_tree_selection(
    forced_singleline_stem: Option<&str>,
) -> bool {
    forced_singleline_stem.is_some_and(|stem| stem.starts_with("notitle-"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ReqFtr23DailyDirPlan {
    RefreshOnly { ensure_error: String },
    RefreshAndPosition { daily_dir: PathBuf },
}

pub(crate) fn req_ftr23_daily_dir_plan(
    daily_dir_result: io::Result<PathBuf>,
) -> ReqFtr23DailyDirPlan {
    match daily_dir_result {
        Ok(daily_dir) => ReqFtr23DailyDirPlan::RefreshAndPosition { daily_dir },
        Err(error) => ReqFtr23DailyDirPlan::RefreshOnly {
            ensure_error: error.to_string(),
        },
    }
}

pub(crate) fn req_editor_file_tree_font_size_policy() -> &'static str {
    crate::app::req_editor_shared_text_size_policy()
}

pub struct FileTreeView {
    tree_state: Entity<TreeState>,
    focus_handle: FocusHandle,
    tree_root_dir: PathBuf,
    root_items: Vec<TreeItem>,
    directory_item_ids: HashSet<String>,
    protected_delete_roots: Vec<PathBuf>,
    selected_item_ids: HashSet<String>,
    delete_shortcut_armed: bool,
    selection_anchor_item_id: Option<String>,
    visible_item_ids: Vec<String>,
    font_size_logged_once: bool,
    ui_color_config: crate::app::UiColorConfig,
}

impl EventEmitter<FileTreeEvent> for FileTreeView {}

impl FileTreeView {
    pub fn new(
        protected_delete_roots: Vec<PathBuf>,
        tree_root_dir: PathBuf,
        ui_color_config: crate::app::UiColorConfig,
        cx: &mut Context<Self>,
    ) -> Self {
        let tree_state = cx.new(|cx| TreeState::new(cx));
        let focus_handle = cx.focus_handle().tab_stop(true);

        let mut this = Self {
            tree_state,
            focus_handle,
            tree_root_dir,
            root_items: Vec::new(),
            directory_item_ids: HashSet::new(),
            protected_delete_roots,
            selected_item_ids: HashSet::new(),
            delete_shortcut_armed: false,
            selection_anchor_item_id: None,
            visible_item_ids: Vec::new(),
            font_size_logged_once: false,
            ui_color_config,
        };
        crate::log::trace_debug(format!(
            "file_tree init root_dir={}",
            this.tree_root_dir.display()
        ));
        crate::log::trace_debug(format!(
            "req-editor6 file_tree font_size_policy={}",
            req_editor_file_tree_font_size_policy()
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
                crate::log::trace_debug(format!(
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

    pub fn apply_req_ftr18_startup_daily_folder_position(
        &mut self,
        daily_dir: &Path,
        cx: &mut Context<Self>,
    ) -> Option<(usize, usize)> {
        let Some((expanded_count, target_index, last_index)) =
            req_ftr18_expand_and_resolve_top_index(
                &mut self.root_items,
                self.tree_root_dir.as_path(),
                daily_dir,
            )
        else {
            crate::log::trace_debug(format!(
                "file_tree req-ftr18 startup positioning skipped daily_dir={} root_dir={}",
                daily_dir.display(),
                self.tree_root_dir.display()
            ));
            return None;
        };

        self.set_items_from_model(cx);

        crate::log::trace_debug(format!(
            "file_tree req-ftr18 startup positioning prepared daily_dir={} expanded_count={} target_index={} last_index={}",
            daily_dir.display(),
            expanded_count,
            target_index,
            last_index
        ));

        Some((target_index, last_index))
    }

    pub fn selection_count(&self) -> usize {
        self.selected_item_ids.len()
    }

    pub fn focus(&self, window: &mut Window) {
        self.focus_handle.focus(window);
    }

    pub fn is_focused(&self, window: &Window, cx: &App) -> bool {
        self.focus_handle.contains_focused(window, cx)
    }

    pub fn restore_selection_for_path(&mut self, path: &Path, cx: &mut Context<Self>) -> bool {
        let item_id = path.to_string_lossy().to_string();
        self.rebuild_visible_item_ids();
        let Some(selected_index) = find_visible_index(&self.visible_item_ids, item_id.as_str())
        else {
            return false;
        };

        replace_single_selection(&mut self.selected_item_ids, item_id.as_str());
        self.selection_anchor_item_id = Some(item_id.clone());
        self.delete_shortcut_armed = false;
        self.tree_state.update(cx, |state, cx| {
            state.set_selected_index(Some(selected_index), cx);
        });
        crate::log::trace_debug(format!(
            "file_tree watcher selection restore item={} index={} delete_shortcut_armed={}",
            item_id, selected_index, self.delete_shortcut_armed
        ));
        cx.notify();
        true
    }

    pub fn clear_selection_for_req_ftr17_case3(&mut self, cx: &mut Context<Self>) {
        self.selected_item_ids.clear();
        self.selection_anchor_item_id = None;
        self.disarm_delete_shortcut("req_ftr17_case3_clear_selection");
        self.tree_state.update(cx, |state, cx| {
            state.set_selected_index(None, cx);
        });
        crate::log::trace_debug("file_tree req-ftr17 case3_reset_neutral clear_tree_selection");
        cx.notify();
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
            crate::log::trace_debug(format!(
                "file_tree delete guard removed protected selections count={}",
                removed_protected_count
            ));
            cx.notify();
        }

        let selected_paths = self.selected_paths();
        crate::log::trace_debug(format!(
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
        crate::log::trace_debug(format!(
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
        crate::log::trace_debug(format!(
            "file_tree delete shortcut disarmed reason={reason} selected_count={}",
            self.selected_item_ids.len()
        ));
    }

    fn load_files(&mut self, cx: &mut Context<Self>) {
        let previous_items = self.root_items.clone();
        let expanded_folder_item_ids = expanded_folder_item_ids(&previous_items);

        let mut refreshed_items = build_file_items(&self.tree_root_dir, &self.tree_root_dir);
        let mut directory_item_ids = HashSet::new();
        collect_directory_item_ids_from_tree(&refreshed_items, &mut directory_item_ids);

        let expanded_restored_count =
            apply_expanded_folder_item_ids(&mut refreshed_items, &expanded_folder_item_ids);
        let req_ftr19_daily_dirs = req_ftr19_first_file_daily_dirs(
            &previous_items,
            &refreshed_items,
            self.tree_root_dir.as_path(),
        );
        let req_ftr19_opened_folder_count = apply_req_ftr19_first_file_auto_open(
            &mut refreshed_items,
            self.tree_root_dir.as_path(),
            &req_ftr19_daily_dirs,
        );
        let req_ftr19_daily_dir_count = req_ftr19_daily_dirs.len();
        req_ftr18_append_scroll_padding_items(&mut refreshed_items);
        self.root_items = refreshed_items;
        self.directory_item_ids = directory_item_ids;

        if req_ftr19_daily_dir_count > 0 {
            let mut daily_dirs: Vec<String> = req_ftr19_daily_dirs.iter().cloned().collect();
            daily_dirs.sort();
            crate::log::trace_debug(format!(
                "file_tree req-ftr19 first_file_auto_open daily_dirs={} opened_folder_count={}",
                daily_dirs.join(","),
                req_ftr19_opened_folder_count
            ));
        }

        crate::log::trace_debug(format!(
            "file_tree load root_dir={} top_level_count={} expanded_snapshot_count={} expanded_restored_count={} req_ftr19_daily_dir_count={} req_ftr19_opened_folder_count={} directory_item_count={}",
            self.tree_root_dir.display(),
            self.root_items.len(),
            expanded_folder_item_ids.len(),
            expanded_restored_count,
            req_ftr19_daily_dir_count,
            req_ftr19_opened_folder_count,
            self.directory_item_ids.len()
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
        req_ftr20_selected_paths_in_visible_order(&self.selected_item_ids, &self.visible_item_ids)
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
        if is_req_ftr18_scroll_padding_item_id(item_id.as_str()) {
            return;
        }

        self.apply_single_selection_by_id(item_id.as_str(), "enter_key", cx);
        crate::log::trace_debug(format!(
            "file_tree enter select item={} folder={} total_selected={}",
            item_id,
            is_folder,
            self.selected_item_ids.len()
        ));
        if is_folder {
            return;
        }

        crate::log::trace_debug(format!("file_tree enter open file item={item_id}"));
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
            crate::log::trace_debug(format!(
                "file_tree keydown {key} shift_range=true current_index={} next_index={} selected_count={}",
                current_index,
                next_index,
                self.selected_item_ids.len()
            ));
        } else if let Some(item_id) = self.visible_item_ids.get(next_index).cloned() {
            self.apply_single_selection_by_id(item_id.as_str(), "arrow_key", cx);
            crate::log::trace_debug(format!(
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
        if is_req_ftr18_scroll_padding_item_id(item.id.as_ref()) {
            return;
        }

        self.focus(window);
        self.rebuild_visible_item_ids();

        let modifiers = event.modifiers();
        if modifiers.shift {
            self.apply_shift_range_selection_to_index(
                row_index,
                Some(row_index),
                "shift_click",
                cx,
            );
            if self.selected_item_ids.len() > 1 {
                self.tree_state
                    .update(cx, |state, cx| state.set_selected_index(None, cx));
            }
            crate::log::trace_debug(format!(
                "file_tree row click shift_range=true item={} index={} selected_count={}",
                item.id,
                row_index,
                self.selected_item_ids.len()
            ));
            return;
        }

        if modifiers.secondary() && !item.is_folder() {
            let selected_now = toggle_item_selection(&mut self.selected_item_ids, item.id.as_ref());
            self.delete_shortcut_armed = !self.selected_item_ids.is_empty();
            self.selection_anchor_item_id = Some(item.id.to_string());
            if self.selected_item_ids.len() > 1 {
                self.tree_state
                    .update(cx, |state, cx| state.set_selected_index(None, cx));
            }
            crate::log::trace_debug(format!(
                "file_tree row click secondary_toggle=true item={} selected_now={} index={} selected_count={} delete_shortcut_armed={}",
                item.id,
                selected_now,
                row_index,
                self.selected_item_ids.len(),
                self.delete_shortcut_armed
            ));
            cx.notify();
            return;
        }

        self.apply_single_selection_by_id(item.id.as_ref(), "row_click", cx);
        crate::log::trace_debug(format!(
            "file_tree row click select item={} folder={} index={} focused={}",
            item.id,
            item.is_folder(),
            row_index,
            self.is_focused(window, cx)
        ));
        if item.is_folder() {
            return;
        }

        crate::log::trace_debug(format!(
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
        let item_id = entry.item().id.to_string();
        let is_folder = self.directory_item_ids.contains(&item_id) || entry.is_folder();
        Some((selected_index, item_id, is_folder))
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
        crate::log::trace_debug(format!(
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
        crate::log::trace_debug(format!(
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
        let background_rgb_hex = self.ui_color_config.background_rgb_hex;
        let foreground_rgb_hex = self.ui_color_config.foreground_rgb_hex;

        if !self.font_size_logged_once {
            crate::log::trace_debug(format!(
                "req-editor-font-size snapshot component=file_tree policy={} tree_text_size=text_sm theme.font_size={:?} theme.mono_font_size={:?} req_colr_background=#{:06x} req_colr_foreground=#{:06x}",
                req_editor_file_tree_font_size_policy(),
                cx.theme().font_size,
                cx.theme().mono_font_size,
                background_rgb_hex,
                foreground_rgb_hex,
            ));
            self.font_size_logged_once = true;
        }

        self.rebuild_visible_item_ids();
        let view = cx.entity();

        let tree_view = crate::app::apply_req_editor_shared_text_size(tree(
            &self.tree_state,
            move |ix, entry, tree_selected, _window, cx| {
                view.update(cx, |this, cx| {
                    let item = entry.item();
                    let item_id = item.id.to_string();

                    if is_req_ftr18_scroll_padding_item_id(item_id.as_str()) {
                        return ListItem::new(ix)
                            .w_full()
                            .py_0p5()
                            .px_2()
                            .text_color(crate::app::req_colr_rgb_hex_to_hsla(
                                this.ui_color_config.foreground_rgb_hex,
                            ))
                            .child(" ");
                    }

                    let is_selected = this.selected_item_ids.contains(&item_id);
                    let is_folder = this.directory_item_ids.contains(&item_id) || entry.is_folder();

                    let icon = if !is_folder {
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
                        .selected(use_native_tree_selection_highlight(
                            tree_selected,
                            is_selected,
                        ))
                        .w_full()
                        .py_0p5()
                        .px_2()
                        .pl(px(16.) * entry.depth() + px(8.))
                        .text_color(crate::app::req_colr_rgb_hex_to_hsla(
                            this.ui_color_config.foreground_rgb_hex,
                        ))
                        .child(row_content)
                        .on_click(cx.listener({
                            let item = item.clone();
                            move |this, event, window, cx| {
                                this.on_row_click(&item, ix, event, window, cx);
                            }
                        }));
                    if let Some(color) = selected_row_highlight_color(tree_selected, is_selected) {
                        row.bg(color)
                    } else {
                        row
                    }
                })
            },
        ))
        .p_1()
        .h_full()
        .bg(crate::app::req_colr_rgb_hex_to_hsla(background_rgb_hex))
        .text_color(crate::app::req_colr_rgb_hex_to_hsla(foreground_rgb_hex));

        div()
            .size_full()
            .bg(crate::app::req_colr_rgb_hex_to_hsla(background_rgb_hex))
            .text_color(crate::app::req_colr_rgb_hex_to_hsla(foreground_rgb_hex))
            .track_focus(&self.focus_handle)
            .capture_key_down(cx.listener(Self::on_key_down))
            .child(tree_view)
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

fn collect_directory_item_ids_from_tree(items: &[TreeItem], directory_item_ids: &mut HashSet<String>) {
    for item in items {
        if Path::new(item.id.as_ref()).is_dir() {
            directory_item_ids.insert(item.id.to_string());
        }
        collect_directory_item_ids_from_tree(&item.children, directory_item_ids);
    }
}



fn sort_tree_items(items: &mut [TreeItem]) {
    items.sort_by(|a, b| {
        b.is_folder()
            .cmp(&a.is_folder())
            .then(a.label.cmp(&b.label))
    });
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

fn collect_visible_item_ids(items: &[TreeItem], ids: &mut Vec<String>) {
    for item in items {
        if is_req_ftr18_scroll_padding_item_id(item.id.as_ref()) {
            continue;
        }
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

fn collect_expanded_folder_item_ids(items: &[TreeItem], ids: &mut HashSet<String>) {
    for item in items {
        if item.is_folder() {
            if item.is_expanded() {
                ids.insert(item.id.to_string());
            }
            collect_expanded_folder_item_ids(&item.children, ids);
        }
    }
}

fn expanded_folder_item_ids(items: &[TreeItem]) -> HashSet<String> {
    let mut ids = HashSet::new();
    collect_expanded_folder_item_ids(items, &mut ids);
    ids
}

fn apply_expanded_folder_item_ids(
    items: &mut [TreeItem],
    expanded_folder_item_ids: &HashSet<String>,
) -> usize {
    let comparable_expanded_folder_item_ids: HashSet<String> = expanded_folder_item_ids
        .iter()
        .map(|item_id| {
            comparable_path(Path::new(item_id))
                .to_string_lossy()
                .to_string()
        })
        .collect();

    apply_expanded_folder_item_ids_with_comparable(
        items,
        expanded_folder_item_ids,
        &comparable_expanded_folder_item_ids,
    )
}

fn apply_expanded_folder_item_ids_with_comparable(
    items: &mut [TreeItem],
    expanded_folder_item_ids: &HashSet<String>,
    comparable_expanded_folder_item_ids: &HashSet<String>,
) -> usize {
    let mut restored = 0usize;

    for item in items {
        if !item.is_folder() {
            continue;
        }

        let item_id = item.id.as_ref();
        let comparable_item_id = comparable_path(Path::new(item_id))
            .to_string_lossy()
            .to_string();

        if (expanded_folder_item_ids.contains(item_id)
            || comparable_expanded_folder_item_ids.contains(comparable_item_id.as_str()))
            && !item.is_expanded()
        {
            *item = item.clone().expanded(true);
            restored += 1;
        }

        restored += apply_expanded_folder_item_ids_with_comparable(
            &mut item.children,
            expanded_folder_item_ids,
            comparable_expanded_folder_item_ids,
        );
    }

    restored
}

fn req_ftr19_is_ascii_digit_component(value: &str, expected_width: usize) -> bool {
    value.len() == expected_width && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn req_ftr19_is_yyyy_mm_dd_directory(tree_root_dir: &Path, directory: &Path) -> bool {
    let comparable_root = comparable_path(tree_root_dir);
    let comparable_directory = comparable_path(directory);

    if !comparable_directory.starts_with(comparable_root.as_path())
        || comparable_directory == comparable_root
    {
        return false;
    }

    let Ok(relative) = comparable_directory.strip_prefix(comparable_root.as_path()) else {
        return false;
    };

    let mut components = relative.components();
    let Some(std::path::Component::Normal(year)) = components.next() else {
        return false;
    };
    let Some(std::path::Component::Normal(month)) = components.next() else {
        return false;
    };
    let Some(std::path::Component::Normal(day)) = components.next() else {
        return false;
    };
    if components.next().is_some() {
        return false;
    }

    let Some(year) = year.to_str() else {
        return false;
    };
    let Some(month) = month.to_str() else {
        return false;
    };
    let Some(day) = day.to_str() else {
        return false;
    };

    req_ftr19_is_ascii_digit_component(year, 4)
        && req_ftr19_is_ascii_digit_component(month, 2)
        && req_ftr19_is_ascii_digit_component(day, 2)
}

fn req_ftr19_collect_file_counts_by_daily_dir(
    items: &[TreeItem],
    tree_root_dir: &Path,
    file_counts_by_daily_dir: &mut std::collections::HashMap<String, usize>,
) {
    for item in items {
        if item.is_folder() {
            req_ftr19_collect_file_counts_by_daily_dir(
                &item.children,
                tree_root_dir,
                file_counts_by_daily_dir,
            );
            continue;
        }

        let item_id = item.id.as_ref();
        if is_req_ftr18_scroll_padding_item_id(item_id) {
            continue;
        }

        let item_path = Path::new(item_id);
        let Some(parent_dir) = item_path.parent() else {
            continue;
        };
        if !req_ftr19_is_yyyy_mm_dd_directory(tree_root_dir, parent_dir) {
            continue;
        }

        let daily_dir_id = comparable_path(parent_dir).to_string_lossy().to_string();
        *file_counts_by_daily_dir.entry(daily_dir_id).or_insert(0) += 1;
    }
}

fn req_ftr19_first_file_daily_dirs(
    previous_items: &[TreeItem],
    refreshed_items: &[TreeItem],
    tree_root_dir: &Path,
) -> HashSet<String> {
    let mut previous_counts = std::collections::HashMap::new();
    req_ftr19_collect_file_counts_by_daily_dir(previous_items, tree_root_dir, &mut previous_counts);

    let mut refreshed_counts = std::collections::HashMap::new();
    req_ftr19_collect_file_counts_by_daily_dir(
        refreshed_items,
        tree_root_dir,
        &mut refreshed_counts,
    );

    let mut triggered_daily_dirs = HashSet::new();
    for (daily_dir, refreshed_count) in refreshed_counts {
        if refreshed_count == 0 {
            continue;
        }
        let previous_count = previous_counts.get(&daily_dir).copied().unwrap_or(0);
        if previous_count == 0 {
            triggered_daily_dirs.insert(daily_dir);
        }
    }

    triggered_daily_dirs
}

fn apply_req_ftr19_first_file_auto_open(
    items: &mut [TreeItem],
    tree_root_dir: &Path,
    triggered_daily_dirs: &HashSet<String>,
) -> usize {
    let mut opened_folder_count = 0usize;

    for daily_dir in triggered_daily_dirs {
        let daily_dir_path = Path::new(daily_dir);
        if !req_ftr19_is_yyyy_mm_dd_directory(tree_root_dir, daily_dir_path) {
            continue;
        }

        let Some(expanded_ids) =
            req_ftr18_daily_folder_chain_item_ids(tree_root_dir, daily_dir_path)
        else {
            crate::log::trace_debug(format!(
                "file_tree req-ftr19 first_file_auto_open skipped_chain daily_dir={} root_dir={}",
                daily_dir_path.display(),
                tree_root_dir.display()
            ));
            continue;
        };
        opened_folder_count += apply_expanded_folder_item_ids(items, &expanded_ids);
    }

    opened_folder_count
}

fn req_ftr18_daily_folder_chain_item_ids(
    tree_root_dir: &Path,
    daily_dir: &Path,
) -> Option<HashSet<String>> {
    let comparable_root = comparable_path(tree_root_dir);
    let comparable_daily = comparable_path(daily_dir);

    if !comparable_daily.starts_with(comparable_root.as_path())
        || comparable_daily == comparable_root
    {
        return None;
    }

    let mut item_ids = HashSet::new();

    let mut cursor = Some(comparable_daily.as_path());
    while let Some(path) = cursor {
        if path == comparable_root.as_path() {
            break;
        }
        if !path.starts_with(comparable_root.as_path()) {
            return None;
        }
        item_ids.insert(path.to_string_lossy().to_string());
        cursor = path.parent();
    }

    if daily_dir.starts_with(tree_root_dir) && daily_dir != tree_root_dir {
        let mut raw_cursor = Some(daily_dir);
        while let Some(path) = raw_cursor {
            if path == tree_root_dir {
                break;
            }
            if !path.starts_with(tree_root_dir) {
                break;
            }
            item_ids.insert(path.to_string_lossy().to_string());
            raw_cursor = path.parent();
        }
    }

    if item_ids.is_empty() {
        return None;
    }

    Some(item_ids)
}

fn req_ftr18_expand_and_resolve_top_index(
    items: &mut [TreeItem],
    tree_root_dir: &Path,
    daily_dir: &Path,
) -> Option<(usize, usize, usize)> {
    let expanded_ids = req_ftr18_daily_folder_chain_item_ids(tree_root_dir, daily_dir)?;
    let expanded_count = apply_expanded_folder_item_ids(items, &expanded_ids);

    let mut visible_item_ids = Vec::new();
    collect_visible_item_ids_including_padding(items, &mut visible_item_ids);

    let target_item_id = daily_dir.to_string_lossy().to_string();
    let target_index = find_visible_index(&visible_item_ids, target_item_id.as_str())?;
    let last_index = visible_item_ids.len().checked_sub(1)?;

    Some((expanded_count, target_index, last_index))
}

fn find_visible_index(visible_item_ids: &[String], item_id: &str) -> Option<usize> {
    visible_item_ids
        .iter()
        .position(|visible_item_id| visible_item_id == item_id)
}

fn req_ftr20_selected_paths_in_visible_order(
    selected_item_ids: &HashSet<String>,
    visible_item_ids: &[String],
) -> Vec<PathBuf> {
    let mut selected_ids: Vec<String> = selected_item_ids.iter().cloned().collect();
    selected_ids.sort_by(|left, right| {
        let left_index = find_visible_index(visible_item_ids, left).unwrap_or(usize::MAX);
        let right_index = find_visible_index(visible_item_ids, right).unwrap_or(usize::MAX);
        left_index.cmp(&right_index).then_with(|| left.cmp(right))
    });

    selected_ids.into_iter().map(PathBuf::from).collect()
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

const REQ_FTR18_SCROLL_PADDING_ROW_COUNT: usize = 128;
const REQ_FTR18_SCROLL_PADDING_ID_PREFIX: &str = "__req_ftr18_scroll_padding__";

fn is_req_ftr18_scroll_padding_item_id(item_id: &str) -> bool {
    item_id.starts_with(REQ_FTR18_SCROLL_PADDING_ID_PREFIX)
}

fn req_ftr18_append_scroll_padding_items(items: &mut Vec<TreeItem>) {
    if items
        .iter()
        .any(|item| is_req_ftr18_scroll_padding_item_id(item.id.as_ref()))
    {
        return;
    }

    for ix in 0..REQ_FTR18_SCROLL_PADDING_ROW_COUNT {
        items.push(
            TreeItem::new(format!("{REQ_FTR18_SCROLL_PADDING_ID_PREFIX}:{ix}"), "").disabled(true),
        );
    }
}

fn collect_visible_item_ids_including_padding(items: &[TreeItem], ids: &mut Vec<String>) {
    for item in items {
        ids.push(item.id.to_string());
        if item.is_folder() && item.is_expanded() {
            collect_visible_item_ids_including_padding(&item.children, ids);
        }
    }
}

fn use_native_tree_selection_highlight(_tree_selected: bool, _is_selected: bool) -> bool {
    false
}

fn selected_row_highlight_color(tree_selected: bool, is_selected: bool) -> Option<Hsla> {
    if !(tree_selected || is_selected) {
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
            crate::log::trace_debug(format!(
                "file_tree recyclebin move skipped source already in recyclebin source={} recyclebin={}",
                source_path.display(),
                recyclebin_dir.display()
            ));
            continue;
        }
        if is_path_within(recyclebin_dir, source_path) {
            crate::log::trace_debug(format!(
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
                crate::log::trace_debug(format!(
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
            crate::log::trace_debug(format!(
                "file_tree permanent delete skipped recyclebin root source={} recyclebin={}",
                source_path.display(),
                recyclebin_dir.display()
            ));
            continue;
        }

        if is_path_within(source_path, recyclebin_dir) {
            match remove_path_permanently(source_path) {
                Ok(()) => {
                    crate::log::trace_debug(format!(
                        "file_tree permanent delete success source={} recyclebin={}",
                        source_path.display(),
                        recyclebin_dir.display()
                    ));
                    outcome.permanently_deleted.push(source_path.clone());
                }
                Err(error) => {
                    crate::log::trace_debug(format!(
                        "file_tree permanent delete failed source={} error={error}",
                        source_path.display()
                    ));
                }
            }
            continue;
        }

        if is_path_within(recyclebin_dir, source_path) {
            crate::log::trace_debug(format!(
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
                crate::log::trace_debug(format!(
                    "file_tree recyclebin move success source={} target={}",
                    source_path.display(),
                    target.display()
                ));
                outcome
                    .moved_to_recyclebin
                    .push((source_path.clone(), target));
            }
            Err(error) => {
                crate::log::trace_debug(format!(
                    "file_tree recyclebin move skipped rename error source={} target={} error={error}",
                    source_path.display(),
                    target.display()
                ));
            }
        }
    }

    Ok(outcome)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReqFtr17PostDeleteDecision {
    SelectNext(PathBuf),
    SelectPrevious(PathBuf),
    ResetToNeutral,
}

fn req_ftr17_sort_key(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub(crate) fn req_ftr17_post_delete_decision_from_remaining_files(
    deleted_source: &Path,
    remaining_files: &[PathBuf],
) -> ReqFtr17PostDeleteDecision {
    let deleted_parent = deleted_source.parent().map(Path::to_path_buf);
    let deleted_key = req_ftr17_sort_key(deleted_source);
    let mut previous: Option<PathBuf> = None;

    for candidate in remaining_files {
        if !candidate.is_file() {
            continue;
        }
        if let Some(parent) = deleted_parent.as_deref()
            && candidate.parent() != Some(parent)
        {
            continue;
        }

        let candidate_key = req_ftr17_sort_key(candidate.as_path());
        if candidate_key > deleted_key {
            return ReqFtr17PostDeleteDecision::SelectNext(candidate.clone());
        }
        if candidate_key < deleted_key {
            previous = Some(candidate.clone());
        }
    }

    if let Some(path) = previous {
        ReqFtr17PostDeleteDecision::SelectPrevious(path)
    } else {
        ReqFtr17PostDeleteDecision::ResetToNeutral
    }
}

fn req_ftr17_sorted_remaining_files_in_parent(parent_dir: &Path) -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(parent_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            files.push(path);
        }
    }
    files.sort_by_key(|path| req_ftr17_sort_key(path.as_path()));
    Ok(files)
}

fn req_ftr17_post_delete_decision_from_filesystem(
    deleted_source: &Path,
) -> io::Result<ReqFtr17PostDeleteDecision> {
    let Some(parent_dir) = deleted_source.parent() else {
        return Ok(ReqFtr17PostDeleteDecision::ResetToNeutral);
    };
    let remaining_files = req_ftr17_sorted_remaining_files_in_parent(parent_dir)?;
    Ok(req_ftr17_post_delete_decision_from_remaining_files(
        deleted_source,
        &remaining_files,
    ))
}

fn req_ftr20_anchor_deleted_file_source_path(outcome: &FileTreeDeleteOutcome) -> Option<PathBuf> {
    outcome
        .moved_to_recyclebin
        .iter()
        .rev()
        .find(|(_, moved_target)| moved_target.is_file())
        .map(|(deleted_source, _)| deleted_source.clone())
}

fn req_ftr17_post_delete_decision_for_outcome(
    outcome: &FileTreeDeleteOutcome,
) -> io::Result<Option<(PathBuf, ReqFtr17PostDeleteDecision)>> {
    if !outcome.permanently_deleted.is_empty() {
        return Ok(None);
    }

    let Some(deleted_anchor_source) = req_ftr20_anchor_deleted_file_source_path(outcome) else {
        return Ok(None);
    };

    let decision = req_ftr17_post_delete_decision_from_filesystem(deleted_anchor_source.as_path())?;
    Ok(Some((deleted_anchor_source, decision)))
}

fn req_ftr17_deleted_paths_contain_current_edit(
    deleted_source_paths: &[PathBuf],
    current_edit_path: Option<&Path>,
) -> bool {
    let Some(current_edit_path) = current_edit_path else {
        return false;
    };
    deleted_source_paths
        .iter()
        .any(|source| is_same_path(source.as_path(), current_edit_path))
}

impl crate::app::Papyru2App {
    pub(crate) fn handle_file_tree_selection_changed(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_singleline_from_file_tree_selection(path.as_path(), window, cx);
        crate::log::trace_debug(format!(
            "file_tree selection load_editor requested path={}",
            path.display()
        ));
        let loaded = self.open_file(path.clone(), window, cx);
        crate::log::trace_debug(format!(
            "file_tree selection load_editor result path={} loaded={}",
            path.display(),
            loaded
        ));
        let transition = crate::app::transition_selection_load_result(loaded);
        self.selection_focus_reassert_pending = transition.next_focus_reassert_pending;
        if transition.schedule_focus_reassert {
            crate::log::trace_debug(format!(
                "file_tree selection focus_reassert scheduled path={} pending={}",
                path.display(),
                self.selection_focus_reassert_pending
            ));
            cx.defer_in(window, move |this, window, cx| {
                let tick_transition = crate::app::transition_focus_reassert_tick(
                    this.selection_focus_reassert_pending,
                );
                this.selection_focus_reassert_pending = tick_transition.next_focus_reassert_pending;
                if !tick_transition.run_focus_reassert {
                    crate::log::trace_debug("file_tree selection focus_reassert skipped pending=false");
                    return;
                }
                this.file_tree.update(cx, |file_tree, _| {
                    file_tree.focus(window);
                });
                let file_tree_focused = this.file_tree.read(cx).is_focused(window, cx);
                let editor_focused = this.editor.read(cx).is_focused(window, cx);
                crate::log::trace_debug(format!(
                    "file_tree selection focus_reassert done file_tree_focused={} editor_focused={} pending={}",
                    file_tree_focused,
                    editor_focused,
                    this.selection_focus_reassert_pending
                ));
            });
        }
        if loaded {
            crate::log::trace_debug(format!(
                "file_tree selection promoted_to_edit path={}",
                path.display()
            ));
        }
    }

    fn apply_req_ftr17_case3_reset_to_neutral(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let transitioned = self.file_workflow.transition_edit_to_neutral();
        self.sync_current_editing_path_to_components(None, cx);
        self.selection_focus_reassert_pending = false;
        self.file_tree.update(cx, |file_tree, cx| {
            file_tree.clear_selection_for_req_ftr17_case3(cx);
        });

        for step in crate::app::req_newf34_plus_button_reset_steps() {
            match step {
                crate::app::PlusButtonResetStep::ClearEditor => {
                    self.editor.update(cx, |editor, cx| {
                        editor.apply_text_and_cursor("", 0, 0, window, cx);
                    });
                }
                crate::app::PlusButtonResetStep::ClearSingleline => {
                    self.singleline.update(cx, |singleline, cx| {
                        singleline.apply_text_and_cursor("", 0, window, cx);
                    });
                }
                crate::app::PlusButtonResetStep::FocusSingleline => {
                    self.singleline.update(cx, |singleline, cx| {
                        singleline.focus(window, cx);
                    });
                }
            }
        }

        crate::log::trace_debug(format!(
            "file_tree req-ftr17 case3_reset_neutral transition_to_neutral={}",
            transitioned
        ));
        cx.defer_in(window, move |this, window, cx| {
            this.singleline.update(cx, |singleline, cx| {
                singleline.apply_cursor(0, window, cx);
                singleline.focus(window, cx);
            });
            let singleline_focused = this.singleline.read(cx).is_focused(window, cx);
            let editor_focused = this.editor.read(cx).is_focused(window, cx);
            crate::log::trace_debug(format!(
                "file_tree req-ftr17 case3_reset_neutral deferred_focus singleline_focused={} editor_focused={}",
                singleline_focused,
                editor_focused
            ));
        });
    }

    pub(crate) fn apply_file_tree_watcher_refresh(&mut self, cx: &mut Context<Self>) {
        let current_edit_path = self.file_workflow.current_edit_path();
        let mut restored_selection = false;
        self.file_tree.update(cx, |file_tree, cx| {
            file_tree.refresh_from_filesystem(cx);

            if should_restore_selection_after_watcher_refresh(
                file_tree.selection_count(),
                current_edit_path.as_deref(),
            ) && let Some(path) = current_edit_path.as_deref()
            {
                restored_selection = file_tree.restore_selection_for_path(path, cx);
            }
        });
        crate::log::trace_debug(format!(
            "file_tree watcher refresh applied current_edit_path_present={} restored_selection={}",
            current_edit_path.is_some(),
            restored_selection
        ));
    }

    pub(crate) fn select_created_file_in_tree_after_new_file(
        &mut self,
        created_path: &Path,
        cx: &mut Context<Self>,
    ) -> bool {
        let restored_selection = self.file_tree.update(cx, |file_tree, cx| {
            file_tree.refresh_from_filesystem(cx);
            file_tree.restore_selection_for_path(created_path, cx)
        });
        crate::log::trace_debug(format!(
            "file_tree req-newf38 create_select target={} restored_selection={}",
            created_path.display(),
            restored_selection
        ));
        restored_selection
    }

    pub(crate) fn handle_folder_refresh_button(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::log::trace_debug("folder_refresh_button click received");
        let daily_dir_plan =
            req_ftr23_daily_dir_plan(crate::file_update_handler::ensure_daily_directory(
                self.app_paths.user_document_dir.as_path(),
                chrono::Local::now(),
            ));
        self.apply_file_tree_watcher_refresh(cx);

        match daily_dir_plan {
            ReqFtr23DailyDirPlan::RefreshAndPosition { daily_dir } => {
                crate::log::trace_debug(format!(
                    "file_tree req-ftr23 refresh daily_dir ensured path={}",
                    daily_dir.display()
                ));
                self.apply_req_ftr18_startup_daily_folder_positioning(daily_dir, window, cx);
            }
            ReqFtr23DailyDirPlan::RefreshOnly { ensure_error } => {
                crate::log::trace_debug(format!(
                    "file_tree req-ftr23 refresh daily_dir ensure failed error={ensure_error}"
                ));
                crate::log::trace_debug(
                    "file_tree req-ftr23 refresh skipped req-ftr18 positioning (daily_dir unavailable)",
                );
            }
        }
    }

    pub(crate) fn apply_req_ftr18_startup_daily_folder_positioning(
        &mut self,
        daily_dir: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let immediate_plan = self.file_tree.update(cx, |file_tree, cx| {
            file_tree.apply_req_ftr18_startup_daily_folder_position(daily_dir.as_path(), cx)
        });
        let Some((target_index, last_index)) = immediate_plan else {
            crate::log::trace_debug(format!(
                "file_tree req-ftr18 startup immediate prepared=false daily_dir={}",
                daily_dir.display()
            ));
            return;
        };

        self.file_tree.update(cx, |file_tree, cx| {
            file_tree.tree_state.update(cx, |state, cx| {
                state.set_selected_index(Some(last_index), cx);
                state.scroll_to_item(last_index, gpui::ScrollStrategy::Bottom);
            });
        });
        crate::log::trace_debug(format!(
            "file_tree req-ftr18 startup immediate primed_bottom=true target_index={} last_index={} daily_dir={}",
            target_index,
            last_index,
            daily_dir.display()
        ));

        let daily_dir_next_frame = daily_dir.clone();
        cx.on_next_frame(window, move |this, window, cx| {
            let next_frame_plan = this.file_tree.update(cx, |file_tree, cx| {
                file_tree.apply_req_ftr18_startup_daily_folder_position(
                    daily_dir_next_frame.as_path(),
                    cx,
                )
            });

            if let Some((target_index, _)) = next_frame_plan {
                this.file_tree.update(cx, |file_tree, cx| {
                    file_tree.tree_state.update(cx, |state, cx| {
                        state.set_selected_index(Some(target_index), cx);
                        state.scroll_to_item(target_index, gpui::ScrollStrategy::Top);
                    });
                });
            }

            crate::log::trace_debug(format!(
                "file_tree req-ftr18 startup next_frame_1 prepared={} daily_dir={}",
                next_frame_plan.is_some(),
                daily_dir_next_frame.display()
            ));

            cx.on_next_frame(window, move |this, _window, cx| {
                let second_next_frame_plan = this.file_tree.update(cx, |file_tree, cx| {
                    file_tree.apply_req_ftr18_startup_daily_folder_position(daily_dir.as_path(), cx)
                });

                if let Some((target_index, _)) = second_next_frame_plan {
                    this.file_tree.update(cx, |file_tree, cx| {
                        file_tree.tree_state.update(cx, |state, cx| {
                            state.set_selected_index(Some(target_index), cx);
                            state.scroll_to_item(target_index, gpui::ScrollStrategy::Top);
                        });
                    });
                }

                crate::log::trace_debug(format!(
                    "file_tree req-ftr18 startup next_frame_2 prepared={} daily_dir={}",
                    second_next_frame_plan.is_some(),
                    daily_dir.display()
                ));
            });
        });
    }

    pub(crate) fn on_file_tree_delete_requested(
        &mut self,
        paths: Vec<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::log::trace_debug(format!(
            "file_tree delete request selected_count={} recyclebin={}",
            paths.len(),
            self.app_paths.recyclebin_dir.display()
        ));

        match delete_entries_for_file_tree(&paths, self.app_paths.recyclebin_dir.as_path()) {
            Ok(outcome) => {
                crate::log::trace_debug(format!(
                    "file_tree delete success moved_count={} permanently_deleted_count={} selected_count={}",
                    outcome.moved_to_recyclebin.len(),
                    outcome.permanently_deleted.len(),
                    paths.len()
                ));

                match req_ftr17_post_delete_decision_for_outcome(&outcome) {
                    Ok(Some((deleted_anchor_source, decision))) => {
                        crate::log::trace_debug(format!(
                            "file_tree req-ftr20 anchor_selected_bottom_file={} moved_count={} permanently_deleted_count={}",
                            deleted_anchor_source.display(),
                            outcome.moved_to_recyclebin.len(),
                            outcome.permanently_deleted.len()
                        ));

                        let moved_sources: Vec<PathBuf> = outcome
                            .moved_to_recyclebin
                            .iter()
                            .map(|(source, _)| source.clone())
                            .collect();
                        let deleted_current_edit_path =
                            req_ftr17_deleted_paths_contain_current_edit(
                                &moved_sources,
                                self.file_workflow.current_edit_path().as_deref(),
                            );

                        match decision {
                            ReqFtr17PostDeleteDecision::SelectNext(path) => {
                                if deleted_current_edit_path {
                                    let transitioned =
                                        self.file_workflow.transition_edit_to_neutral();
                                    self.sync_current_editing_path_to_components(None, cx);
                                    crate::log::trace_debug(format!(
                                        "file_tree req-ftr17 case1_next deleted_current_edit_path transition_to_neutral={}",
                                        transitioned
                                    ));
                                }
                                let restored_selection =
                                    self.file_tree.update(cx, |file_tree, cx| {
                                        file_tree.restore_selection_for_path(path.as_path(), cx)
                                    });
                                crate::log::trace_debug(format!(
                                    "file_tree req-ftr17 case1_next target={} restored_selection={}",
                                    path.display(),
                                    restored_selection
                                ));
                                self.handle_file_tree_selection_changed(path, window, cx);
                            }
                            ReqFtr17PostDeleteDecision::SelectPrevious(path) => {
                                if deleted_current_edit_path {
                                    let transitioned =
                                        self.file_workflow.transition_edit_to_neutral();
                                    self.sync_current_editing_path_to_components(None, cx);
                                    crate::log::trace_debug(format!(
                                        "file_tree req-ftr17 case2_prev deleted_current_edit_path transition_to_neutral={}",
                                        transitioned
                                    ));
                                }
                                let restored_selection =
                                    self.file_tree.update(cx, |file_tree, cx| {
                                        file_tree.restore_selection_for_path(path.as_path(), cx)
                                    });
                                crate::log::trace_debug(format!(
                                    "file_tree req-ftr17 case2_prev target={} restored_selection={}",
                                    path.display(),
                                    restored_selection
                                ));
                                self.handle_file_tree_selection_changed(path, window, cx);
                            }
                            ReqFtr17PostDeleteDecision::ResetToNeutral => {
                                crate::log::trace_debug(
                                    "file_tree req-ftr17 case3_reset_neutral no_remaining_file=true",
                                );
                                self.apply_req_ftr17_case3_reset_to_neutral(window, cx);
                            }
                        }
                    }
                    Ok(None) => {
                        crate::log::trace_debug(format!(
                            "file_tree req-ftr17 skipped moved_count={} permanently_deleted_count={}",
                            outcome.moved_to_recyclebin.len(),
                            outcome.permanently_deleted.len()
                        ));
                    }
                    Err(error) => {
                        crate::log::trace_debug(format!(
                            "file_tree req-ftr17 decision failed error={error}"
                        ));
                    }
                }

                if crate::app::req_ftr14_delete_flow_uses_watcher_refresh_only() {
                    crate::log::trace_debug(
                        "file_tree delete success watcher_refresh_only=true direct_refresh_skipped",
                    );
                }
            }
            Err(error) => {
                crate::log::trace_debug(format!("file_tree delete move failed error={error}"));
            }
        }
    }

    pub(crate) fn open_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.flush_editor_content_before_context_switch("req-aus8-open-file", cx) {
            crate::log::trace_debug(format!(
                "open_file aborted path={} (pre-switch autosave failed)",
                path.display()
            ));
            return false;
        }

        let opened = self.editor.update(cx, {
            let path = path.clone();
            move |editor, cx| editor.open_file(path, window, cx)
        });

        if !opened {
            crate::log::trace_debug(format!("open_file failed path={}", path.display()));
            return false;
        }

        self.file_workflow.set_edit_from_open_file(path.clone());
        self.sync_current_editing_path_to_components(Some(path), cx);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ReqFtr17PostDeleteDecision, ReqFtr23DailyDirPlan, TreeItem, apply_expanded_folder_item_ids,
        build_file_items, collect_tree_item_ids, collect_visible_item_ids,
        delete_entries_for_file_tree, expanded_folder_item_ids, find_visible_index,
        is_delete_protected_path, move_entries_to_recyclebin, replace_single_selection,
        req_ftr17_post_delete_decision_from_filesystem,
        req_ftr17_post_delete_decision_from_remaining_files, req_ftr17_sort_key,
        req_ftr23_daily_dir_plan, retain_existing_selections, select_range_items,
        selected_row_highlight_color, should_restore_selection_after_watcher_refresh,
        toggle_item_selection, use_checkbox_selection_markers,
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
            selected_row_highlight_color(false, true),
            Some(hsla(0.58, 0.65, 0.88, 1.0))
        );
        assert_eq!(
            selected_row_highlight_color(true, false),
            Some(hsla(0.58, 0.65, 0.88, 1.0))
        );
        assert_eq!(
            selected_row_highlight_color(true, true),
            Some(hsla(0.58, 0.65, 0.88, 1.0))
        );
        assert_eq!(selected_row_highlight_color(false, false), None);
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

    #[test]
    fn ftr_test33_req_ftr14_watcher_refresh_can_restore_selection_for_current_edit_path() {
        let path = PathBuf::from("C:/tmp/current.txt");
        assert!(should_restore_selection_after_watcher_refresh(
            0,
            Some(path.as_path())
        ));
        assert!(!should_restore_selection_after_watcher_refresh(
            1,
            Some(path.as_path())
        ));
        assert!(!should_restore_selection_after_watcher_refresh(0, None));
    }

    #[test]
    fn ftr_test34_req_ftr15_refresh_preserves_nested_expanded_folders() {
        let previous_items = vec![
            TreeItem::new("/root/2026", "2026")
                .expanded(true)
                .children([TreeItem::new("/root/2026/03", "03")
                    .expanded(true)
                    .children([TreeItem::new("/root/2026/03/09", "09")
                        .expanded(true)
                        .child(TreeItem::new("/root/2026/03/09/fileA.txt", "fileA.txt"))])]),
            TreeItem::new("/root/recyclebin", "recyclebin"),
        ];
        let expanded_ids = expanded_folder_item_ids(&previous_items);

        let mut refreshed_items = vec![
            TreeItem::new("/root/2026", "2026").children([TreeItem::new("/root/2026/03", "03")
                .children([TreeItem::new("/root/2026/03/09", "09")
                    .child(TreeItem::new("/root/2026/03/09/fileA.txt", "fileA.txt"))])]),
            TreeItem::new("/root/recyclebin", "recyclebin"),
        ];
        let restored_count = apply_expanded_folder_item_ids(&mut refreshed_items, &expanded_ids);
        let restored_ids = expanded_folder_item_ids(&refreshed_items);

        assert_eq!(restored_count, 3);
        assert!(restored_ids.contains("/root/2026"));
        assert!(restored_ids.contains("/root/2026/03"));
        assert!(restored_ids.contains("/root/2026/03/09"));
    }

    #[test]
    fn ftr_test35_req_ftr15_refresh_drops_expansion_for_removed_folders_only() {
        let expanded_ids = HashSet::from([
            "/root/2026".to_string(),
            "/root/2026/03".to_string(),
            "/root/2026/03/removed".to_string(),
        ]);
        let mut refreshed_items = vec![
            TreeItem::new("/root/2026", "2026").children([TreeItem::new("/root/2026/03", "03")
                .child(TreeItem::new("/root/2026/03/fileA.txt", "fileA.txt"))]),
        ];

        let restored_count = apply_expanded_folder_item_ids(&mut refreshed_items, &expanded_ids);
        let restored_ids = expanded_folder_item_ids(&refreshed_items);

        assert_eq!(restored_count, 2);
        assert!(restored_ids.contains("/root/2026"));
        assert!(restored_ids.contains("/root/2026/03"));
        assert!(!restored_ids.contains("/root/2026/03/removed"));
    }

    #[test]
    fn ftr_test36_req_ftr15_expansion_restore_and_selection_restore_do_not_conflict() {
        let current_path = PathBuf::from("/root/2026/03/09/fileB.txt");
        let expanded_ids = HashSet::from([
            "/root/2026".to_string(),
            "/root/2026/03".to_string(),
            "/root/2026/03/09".to_string(),
        ]);
        let mut refreshed_items = vec![TreeItem::new("/root/2026", "2026").children([
            TreeItem::new("/root/2026/03", "03").children([
                TreeItem::new("/root/2026/03/09", "09").children([
                    TreeItem::new("/root/2026/03/09/fileA.txt", "fileA.txt"),
                    TreeItem::new("/root/2026/03/09/fileB.txt", "fileB.txt"),
                ]),
            ]),
        ])];

        apply_expanded_folder_item_ids(&mut refreshed_items, &expanded_ids);
        let mut visible_item_ids = Vec::new();
        collect_visible_item_ids(&refreshed_items, &mut visible_item_ids);

        assert!(
            find_visible_index(&visible_item_ids, current_path.to_string_lossy().as_ref())
                .is_some()
        );
        assert!(should_restore_selection_after_watcher_refresh(
            0,
            Some(current_path.as_path())
        ));
    }

    #[test]
    fn ftr_test51_req_ftr17_case1_delete_middle_reselects_next_file() {
        let root = new_temp_root("ftr_test51");
        let dir = root.join("2026").join("03").join("12");
        fs::create_dir_all(&dir).expect("create dir");
        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");

        fs::remove_file(&file_b).expect("remove B");

        let decision = req_ftr17_post_delete_decision_from_filesystem(file_b.as_path())
            .expect("resolve post-delete decision");

        assert_eq!(decision, ReqFtr17PostDeleteDecision::SelectNext(file_c));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test52_req_ftr17_case2_delete_bottom_reselects_previous_file() {
        let root = new_temp_root("ftr_test52");
        let dir = root.join("2026").join("03").join("12");
        fs::create_dir_all(&dir).expect("create dir");
        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");

        fs::remove_file(&file_c).expect("remove C");

        let decision = req_ftr17_post_delete_decision_from_filesystem(file_c.as_path())
            .expect("resolve post-delete decision");

        assert_eq!(decision, ReqFtr17PostDeleteDecision::SelectPrevious(file_b));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test53_req_ftr17_case3_delete_last_file_resets_to_neutral() {
        let root = new_temp_root("ftr_test53");
        let dir = root.join("2026").join("03").join("12");
        fs::create_dir_all(&dir).expect("create dir");
        let file_a = dir.join("fileA.txt");
        fs::write(&file_a, "A").expect("seed A");

        fs::remove_file(&file_a).expect("remove A");

        let decision = req_ftr17_post_delete_decision_from_filesystem(file_a.as_path())
            .expect("resolve post-delete decision");

        assert_eq!(decision, ReqFtr17PostDeleteDecision::ResetToNeutral);
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test54_req_ftr17_reselection_ignores_folder_and_cross_directory_candidates() {
        let root = new_temp_root("ftr_test54");
        let dir = root.join("2026").join("03").join("12");
        fs::create_dir_all(&dir).expect("create dir");
        let other_dir = root.join("2026").join("03").join("13");
        fs::create_dir_all(&other_dir).expect("create other dir");

        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        let folder_d = dir.join("folderD");
        let cross_dir_file = other_dir.join("fileD.txt");

        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");
        fs::create_dir_all(&folder_d).expect("seed folder");
        fs::write(&cross_dir_file, "D").expect("seed cross-dir file");

        fs::remove_file(&file_b).expect("remove B");

        let mut candidates = vec![
            folder_d.clone(),
            file_a.clone(),
            cross_dir_file.clone(),
            file_c.clone(),
        ];
        candidates.sort_by_key(|path| req_ftr17_sort_key(path.as_path()));

        let decision =
            req_ftr17_post_delete_decision_from_remaining_files(file_b.as_path(), &candidates);

        assert_eq!(decision, ReqFtr17PostDeleteDecision::SelectNext(file_c));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test57_req_ftr18_startup_expands_yyyy_mm_dd_folders() {
        let root = new_temp_root("ftr_test57");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("13");
        let file = day.join("fileA.txt");
        let recyclebin = root.join("recyclebin");

        let mut items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").children([
                        TreeItem::new(day.to_string_lossy().to_string(), "13").child(
                            TreeItem::new(file.to_string_lossy().to_string(), "fileA.txt"),
                        ),
                    ]),
                ]),
                TreeItem::new(recyclebin.to_string_lossy().to_string(), "recyclebin"),
            ];

        let (expanded_count, _, _) = super::req_ftr18_expand_and_resolve_top_index(
            &mut items,
            root.as_path(),
            day.as_path(),
        )
        .expect("resolve req-ftr18 startup target");
        let expanded_ids = expanded_folder_item_ids(&items);

        assert_eq!(expanded_count, 3);
        assert!(expanded_ids.contains(year.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(month.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(day.to_string_lossy().as_ref()));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test58_req_ftr18_startup_resolves_dd_visible_index_for_top_scroll() {
        let root = new_temp_root("ftr_test58");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("13");
        let file = day.join("fileA.txt");
        let recyclebin = root.join("recyclebin");

        let mut items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").children([
                        TreeItem::new(day.to_string_lossy().to_string(), "13").child(
                            TreeItem::new(file.to_string_lossy().to_string(), "fileA.txt"),
                        ),
                    ]),
                ]),
                TreeItem::new(recyclebin.to_string_lossy().to_string(), "recyclebin"),
            ];

        let (_, target_index, _) = super::req_ftr18_expand_and_resolve_top_index(
            &mut items,
            root.as_path(),
            day.as_path(),
        )
        .expect("resolve req-ftr18 startup target");

        let mut visible_ids = Vec::new();
        collect_visible_item_ids(&items, &mut visible_ids);

        assert_eq!(
            find_visible_index(&visible_ids, day.to_string_lossy().as_ref()),
            Some(2)
        );
        assert_eq!(
            visible_ids.get(target_index),
            Some(&day.to_string_lossy().to_string())
        );

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test59_req_ftr18_startup_targets_dd_folder_without_forced_file_selection() {
        let root = new_temp_root("ftr_test59");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("13");
        let file = day.join("fileA.txt");
        let recyclebin = root.join("recyclebin");

        let mut items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").children([
                        TreeItem::new(day.to_string_lossy().to_string(), "13").child(
                            TreeItem::new(file.to_string_lossy().to_string(), "fileA.txt"),
                        ),
                    ]),
                ]),
                TreeItem::new(recyclebin.to_string_lossy().to_string(), "recyclebin"),
            ];

        let (_, target_index, _) = super::req_ftr18_expand_and_resolve_top_index(
            &mut items,
            root.as_path(),
            day.as_path(),
        )
        .expect("resolve req-ftr18 startup target");

        let mut visible_ids = Vec::new();
        collect_visible_item_ids(&items, &mut visible_ids);
        let target_item_id = visible_ids
            .get(target_index)
            .expect("target item should exist");

        assert_eq!(target_item_id, &day.to_string_lossy().to_string());
        assert!(!target_item_id.ends_with(".txt"));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test60_req_ftr19_detects_first_file_transition_for_yyyy_mm_dd() {
        let root = new_temp_root("ftr_test60");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("14");
        let file_a = day.join("fileA.txt");

        let previous_items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026")
                    .children([TreeItem::new(month.to_string_lossy().to_string(), "03")
                        .child(TreeItem::new(day.to_string_lossy().to_string(), "14"))]),
            ];
        let refreshed_items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").child(
                        TreeItem::new(day.to_string_lossy().to_string(), "14").child(
                            TreeItem::new(file_a.to_string_lossy().to_string(), "fileA.txt"),
                        ),
                    ),
                ]),
            ];

        let triggered_daily_dirs = super::req_ftr19_first_file_daily_dirs(
            &previous_items,
            &refreshed_items,
            root.as_path(),
        );
        let day_id = super::comparable_path(day.as_path())
            .to_string_lossy()
            .to_string();

        assert_eq!(triggered_daily_dirs.len(), 1);
        assert!(triggered_daily_dirs.contains(day_id.as_str()));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test61_req_ftr19_ignores_non_first_file_transition_for_yyyy_mm_dd() {
        let root = new_temp_root("ftr_test61");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("14");
        let file_a = day.join("fileA.txt");
        let file_b = day.join("fileB.txt");

        let previous_items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").child(
                        TreeItem::new(day.to_string_lossy().to_string(), "14").child(
                            TreeItem::new(file_a.to_string_lossy().to_string(), "fileA.txt"),
                        ),
                    ),
                ]),
            ];
        let refreshed_items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").child(
                        TreeItem::new(day.to_string_lossy().to_string(), "14").children([
                            TreeItem::new(file_a.to_string_lossy().to_string(), "fileA.txt"),
                            TreeItem::new(file_b.to_string_lossy().to_string(), "fileB.txt"),
                        ]),
                    ),
                ]),
            ];

        let triggered_daily_dirs = super::req_ftr19_first_file_daily_dirs(
            &previous_items,
            &refreshed_items,
            root.as_path(),
        );

        assert!(triggered_daily_dirs.is_empty());

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test62_req_ftr19_ignores_non_date_directory_parents() {
        let root = new_temp_root("ftr_test62");
        let notes_dir = root.join("notes");
        let file_a = notes_dir.join("fileA.txt");

        let previous_items = vec![
            TreeItem::new(notes_dir.to_string_lossy().to_string(), "notes"),
            TreeItem::new(
                root.join("recyclebin").to_string_lossy().to_string(),
                "recyclebin",
            ),
        ];
        let refreshed_items = vec![
            TreeItem::new(notes_dir.to_string_lossy().to_string(), "notes").child(TreeItem::new(
                file_a.to_string_lossy().to_string(),
                "fileA.txt",
            )),
            TreeItem::new(
                root.join("recyclebin").to_string_lossy().to_string(),
                "recyclebin",
            ),
        ];

        let triggered_daily_dirs = super::req_ftr19_first_file_daily_dirs(
            &previous_items,
            &refreshed_items,
            root.as_path(),
        );

        assert!(triggered_daily_dirs.is_empty());

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test63_req_ftr19_first_file_trigger_expands_yyyy_mm_dd_chain() {
        let root = new_temp_root("ftr_test63");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("14");
        let file_a = day.join("fileA.txt");

        let previous_items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026")
                    .children([TreeItem::new(month.to_string_lossy().to_string(), "03")
                        .child(TreeItem::new(day.to_string_lossy().to_string(), "14"))]),
            ];
        let mut refreshed_items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").child(
                        TreeItem::new(day.to_string_lossy().to_string(), "14").child(
                            TreeItem::new(file_a.to_string_lossy().to_string(), "fileA.txt"),
                        ),
                    ),
                ]),
            ];

        let triggered_daily_dirs = super::req_ftr19_first_file_daily_dirs(
            &previous_items,
            &refreshed_items,
            root.as_path(),
        );
        let opened_folder_count = super::apply_req_ftr19_first_file_auto_open(
            &mut refreshed_items,
            root.as_path(),
            &triggered_daily_dirs,
        );
        let expanded_ids = expanded_folder_item_ids(&refreshed_items);

        assert_eq!(opened_folder_count, 3);
        assert!(expanded_ids.contains(year.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(month.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(day.to_string_lossy().as_ref()));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test64_req_ftr19_first_file_trigger_is_idempotent_when_chain_already_open() {
        let root = new_temp_root("ftr_test64");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("14");
        let file_a = day.join("fileA.txt");

        let previous_items = vec![
            TreeItem::new(year.to_string_lossy().to_string(), "2026")
                .expanded(true)
                .children([TreeItem::new(month.to_string_lossy().to_string(), "03")
                    .expanded(true)
                    .child(TreeItem::new(day.to_string_lossy().to_string(), "14").expanded(true))]),
        ];
        let mut refreshed_items = vec![
            TreeItem::new(year.to_string_lossy().to_string(), "2026")
                .expanded(true)
                .children([TreeItem::new(month.to_string_lossy().to_string(), "03")
                    .expanded(true)
                    .child(
                        TreeItem::new(day.to_string_lossy().to_string(), "14")
                            .expanded(true)
                            .child(TreeItem::new(
                                file_a.to_string_lossy().to_string(),
                                "fileA.txt",
                            )),
                    )]),
        ];

        let triggered_daily_dirs = super::req_ftr19_first_file_daily_dirs(
            &previous_items,
            &refreshed_items,
            root.as_path(),
        );
        let opened_folder_count = super::apply_req_ftr19_first_file_auto_open(
            &mut refreshed_items,
            root.as_path(),
            &triggered_daily_dirs,
        );
        let expanded_ids = expanded_folder_item_ids(&refreshed_items);

        assert_eq!(triggered_daily_dirs.len(), 1);
        assert_eq!(opened_folder_count, 0);
        assert!(expanded_ids.contains(year.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(month.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(day.to_string_lossy().as_ref()));

        remove_temp_root(root.as_path());
    }

    #[cfg(windows)]
    #[test]
    fn ftr_test65_req_ftr19_windows_prefixed_root_still_expands_daily_chain() {
        let root = new_temp_root("ftr_test65");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("14");
        let file_a = day.join("fileA.txt");

        let root_prefixed = PathBuf::from(format!(r"\\?\{}", root.display()));

        let mut refreshed_items =
            vec![
                TreeItem::new(year.to_string_lossy().to_string(), "2026").children([
                    TreeItem::new(month.to_string_lossy().to_string(), "03").child(
                        TreeItem::new(day.to_string_lossy().to_string(), "14").child(
                            TreeItem::new(file_a.to_string_lossy().to_string(), "fileA.txt"),
                        ),
                    ),
                ]),
            ];

        let triggered_daily_dirs = HashSet::from([day.to_string_lossy().to_string()]);
        let opened_folder_count = super::apply_req_ftr19_first_file_auto_open(
            &mut refreshed_items,
            root_prefixed.as_path(),
            &triggered_daily_dirs,
        );
        let expanded_ids = expanded_folder_item_ids(&refreshed_items);

        assert_eq!(opened_folder_count, 3);
        assert!(expanded_ids.contains(year.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(month.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(day.to_string_lossy().as_ref()));

        remove_temp_root(root.as_path());
    }

    #[cfg(windows)]
    #[test]
    fn ftr_test66_req_ftr19_nonprefixed_daily_dir_matches_prefixed_tree_item_ids() {
        let root = new_temp_root("ftr_test66");
        let year = root.join("2026");
        let month = year.join("03");
        let day = month.join("14");
        let file_a = day.join("fileA.txt");

        let root_prefixed = PathBuf::from(format!(r"\\?\{}", root.display()));
        let year_prefixed = PathBuf::from(format!(r"\\?\{}", year.display()));
        let month_prefixed = PathBuf::from(format!(r"\\?\{}", month.display()));
        let day_prefixed = PathBuf::from(format!(r"\\?\{}", day.display()));
        let file_a_prefixed = PathBuf::from(format!(r"\\?\{}", file_a.display()));

        let mut refreshed_items = vec![
            TreeItem::new(year_prefixed.to_string_lossy().to_string(), "2026").children([
                TreeItem::new(month_prefixed.to_string_lossy().to_string(), "03").child(
                    TreeItem::new(day_prefixed.to_string_lossy().to_string(), "14").child(
                        TreeItem::new(file_a_prefixed.to_string_lossy().to_string(), "fileA.txt"),
                    ),
                ),
            ]),
        ];

        let triggered_daily_dirs = HashSet::from([day.to_string_lossy().to_string()]);
        let opened_folder_count = super::apply_req_ftr19_first_file_auto_open(
            &mut refreshed_items,
            root_prefixed.as_path(),
            &triggered_daily_dirs,
        );
        let expanded_ids = expanded_folder_item_ids(&refreshed_items);

        assert_eq!(opened_folder_count, 3);
        assert!(expanded_ids.contains(year_prefixed.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(month_prefixed.to_string_lossy().as_ref()));
        assert!(expanded_ids.contains(day_prefixed.to_string_lossy().as_ref()));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test67_req_ftr20_case1_multi_delete_reselects_next_from_bottom_anchor() {
        let root = new_temp_root("ftr_test67");
        let dir = root.join("2026").join("03").join("12");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&dir).expect("create dir");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        let file_d = dir.join("fileD.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");
        fs::write(&file_d, "D").expect("seed D");

        let outcome = delete_entries_for_file_tree(
            &[file_b.clone(), file_c.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("delete B and C");

        let result = super::req_ftr17_post_delete_decision_for_outcome(&outcome)
            .expect("resolve post-delete decision")
            .expect("decision for moved files");

        assert_eq!(
            result,
            (
                file_c.clone(),
                ReqFtr17PostDeleteDecision::SelectNext(file_d.clone())
            )
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test68_req_ftr20_case2_multi_delete_reselects_previous_from_bottom_anchor() {
        let root = new_temp_root("ftr_test68");
        let dir = root.join("2026").join("03").join("12");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&dir).expect("create dir");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");

        let outcome = delete_entries_for_file_tree(
            &[file_b.clone(), file_c.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("delete B and C");

        let result = super::req_ftr17_post_delete_decision_for_outcome(&outcome)
            .expect("resolve post-delete decision")
            .expect("decision for moved files");

        assert_eq!(
            result,
            (
                file_c.clone(),
                ReqFtr17PostDeleteDecision::SelectPrevious(file_a.clone())
            )
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test69_req_ftr20_case3_multi_delete_last_files_resets_to_neutral() {
        let root = new_temp_root("ftr_test69");
        let dir = root.join("2026").join("03").join("12");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&dir).expect("create dir");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");

        let outcome = delete_entries_for_file_tree(
            &[file_a.clone(), file_b.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("delete A and B");

        let result = super::req_ftr17_post_delete_decision_for_outcome(&outcome)
            .expect("resolve post-delete decision")
            .expect("decision for moved files");

        assert_eq!(
            result,
            (file_b.clone(), ReqFtr17PostDeleteDecision::ResetToNeutral)
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test70_req_ftr20_anchor_uses_bottom_visible_order_not_selection_insertion_order() {
        let visible_item_ids = vec![
            "/root/fileA.txt".to_string(),
            "/root/fileB.txt".to_string(),
            "/root/fileC.txt".to_string(),
            "/root/fileD.txt".to_string(),
        ];

        let mut selected_item_ids = HashSet::new();
        selected_item_ids.insert("/root/fileC.txt".to_string());
        selected_item_ids.insert("/root/fileB.txt".to_string());

        let selected_paths =
            super::req_ftr20_selected_paths_in_visible_order(&selected_item_ids, &visible_item_ids);

        assert_eq!(
            selected_paths,
            vec![
                PathBuf::from("/root/fileB.txt"),
                PathBuf::from("/root/fileC.txt"),
            ]
        );
        assert_eq!(
            selected_paths.last().cloned(),
            Some(PathBuf::from("/root/fileC.txt"))
        );
    }

    #[test]
    fn ftr_test71_req_ftr20_multi_delete_reselection_ignores_folder_and_cross_directory_candidates()
    {
        let root = new_temp_root("ftr_test71");
        let dir = root.join("2026").join("03").join("12");
        let other_dir = root.join("2026").join("03").join("13");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&dir).expect("create dir");
        fs::create_dir_all(&other_dir).expect("create other dir");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        let folder_z = dir.join("zFolder");
        let cross_dir_file = other_dir.join("fileD.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");
        fs::create_dir_all(&folder_z).expect("seed folder");
        fs::write(&cross_dir_file, "D").expect("seed cross-dir file");

        let outcome = delete_entries_for_file_tree(
            &[file_b.clone(), file_c.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("delete B and C");

        let result = super::req_ftr17_post_delete_decision_for_outcome(&outcome)
            .expect("resolve post-delete decision")
            .expect("decision for moved files");

        assert_eq!(
            result,
            (
                file_c.clone(),
                ReqFtr17PostDeleteDecision::SelectPrevious(file_a.clone())
            )
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test72_req_ftr20_single_delete_behavior_still_uses_deleted_file_as_anchor() {
        let root = new_temp_root("ftr_test72");
        let dir = root.join("2026").join("03").join("12");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&dir).expect("create dir");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");

        let outcome =
            delete_entries_for_file_tree(std::slice::from_ref(&file_b), recyclebin_dir.as_path())
                .expect("delete B");

        let result = super::req_ftr17_post_delete_decision_for_outcome(&outcome)
            .expect("resolve post-delete decision")
            .expect("decision for moved file");

        assert_eq!(
            result,
            (
                file_b.clone(),
                ReqFtr17PostDeleteDecision::SelectNext(file_c.clone())
            )
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test74_req_ftr21_secondary_click_adds_selection_without_clearing_existing() {
        let mut selected_item_ids = HashSet::from(["/root/fileA.txt".to_string()]);

        let selected_now = toggle_item_selection(&mut selected_item_ids, "/root/fileC.txt");

        assert!(selected_now);
        assert_eq!(selected_item_ids.len(), 2);
        assert!(selected_item_ids.contains("/root/fileA.txt"));
        assert!(selected_item_ids.contains("/root/fileC.txt"));
    }

    #[test]
    fn ftr_test75_req_ftr21_secondary_click_toggles_selected_item_off_without_clearing_others() {
        let mut selected_item_ids =
            HashSet::from(["/root/fileA.txt".to_string(), "/root/fileC.txt".to_string()]);

        let selected_now = toggle_item_selection(&mut selected_item_ids, "/root/fileC.txt");

        assert!(!selected_now);
        assert_eq!(selected_item_ids.len(), 1);
        assert!(selected_item_ids.contains("/root/fileA.txt"));
        assert!(!selected_item_ids.contains("/root/fileC.txt"));
    }

    #[test]
    fn ftr_test76_req_ftr21_secondary_click_builds_non_contiguous_selection_set() {
        let mut selected_item_ids = HashSet::new();

        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileA.txt"
        ));
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileC.txt"
        ));
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileE.txt"
        ));

        assert_eq!(selected_item_ids.len(), 3);
        assert!(selected_item_ids.contains("/root/fileA.txt"));
        assert!(selected_item_ids.contains("/root/fileC.txt"));
        assert!(selected_item_ids.contains("/root/fileE.txt"));
        assert!(!selected_item_ids.contains("/root/fileB.txt"));
        assert!(!selected_item_ids.contains("/root/fileD.txt"));
    }

    #[test]
    fn ftr_test77_req_ftr21_plain_click_after_secondary_multi_select_collapses_to_single_file() {
        let mut selected_item_ids = HashSet::new();
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileA.txt"
        ));
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileC.txt"
        ));
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileE.txt"
        ));

        replace_single_selection(&mut selected_item_ids, "/root/fileB.txt");

        assert_eq!(selected_item_ids.len(), 1);
        assert!(selected_item_ids.contains("/root/fileB.txt"));
    }

    #[test]
    fn ftr_test78_req_ftr21_shift_range_after_secondary_click_uses_anchor_visible_order() {
        let visible_item_ids = vec![
            "/root/fileA.txt".to_string(),
            "/root/fileB.txt".to_string(),
            "/root/fileC.txt".to_string(),
            "/root/fileD.txt".to_string(),
            "/root/fileE.txt".to_string(),
        ];
        let mut selected_item_ids = HashSet::new();

        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileA.txt"
        ));
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileC.txt"
        ));

        let anchor_index = find_visible_index(&visible_item_ids, "/root/fileC.txt")
            .expect("anchor index for fileC");
        let target_index = find_visible_index(&visible_item_ids, "/root/fileE.txt")
            .expect("target index for fileE");
        select_range_items(
            &mut selected_item_ids,
            &visible_item_ids,
            anchor_index,
            target_index,
        );

        assert_eq!(selected_item_ids.len(), 3);
        assert!(selected_item_ids.contains("/root/fileC.txt"));
        assert!(selected_item_ids.contains("/root/fileD.txt"));
        assert!(selected_item_ids.contains("/root/fileE.txt"));
        assert!(!selected_item_ids.contains("/root/fileA.txt"));
    }

    #[test]
    fn ftr_test79_req_ftr21_delete_paths_from_secondary_multi_select_stay_compatible_with_req_ftr20_anchor_policy()
     {
        let visible_item_ids = vec![
            "/root/fileA.txt".to_string(),
            "/root/fileB.txt".to_string(),
            "/root/fileC.txt".to_string(),
            "/root/fileD.txt".to_string(),
        ];
        let mut selected_item_ids = HashSet::new();
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileC.txt"
        ));
        assert!(toggle_item_selection(
            &mut selected_item_ids,
            "/root/fileB.txt"
        ));

        let selected_paths =
            super::req_ftr20_selected_paths_in_visible_order(&selected_item_ids, &visible_item_ids);

        assert_eq!(
            selected_paths,
            vec![
                PathBuf::from("/root/fileB.txt"),
                PathBuf::from("/root/fileC.txt")
            ]
        );

        let root = new_temp_root("ftr_test79");
        let dir = root.join("2026").join("03").join("12");
        let recyclebin_dir = root.join("recyclebin");
        fs::create_dir_all(&dir).expect("create dir");
        fs::create_dir_all(&recyclebin_dir).expect("create recyclebin");

        let file_a = dir.join("fileA.txt");
        let file_b = dir.join("fileB.txt");
        let file_c = dir.join("fileC.txt");
        let file_d = dir.join("fileD.txt");
        fs::write(&file_a, "A").expect("seed A");
        fs::write(&file_b, "B").expect("seed B");
        fs::write(&file_c, "C").expect("seed C");
        fs::write(&file_d, "D").expect("seed D");

        let outcome = delete_entries_for_file_tree(
            &[file_b.clone(), file_c.clone()],
            recyclebin_dir.as_path(),
        )
        .expect("delete B and C");
        let result = super::req_ftr17_post_delete_decision_for_outcome(&outcome)
            .expect("resolve post-delete decision")
            .expect("decision for moved files");

        assert_eq!(
            result,
            (
                file_c.clone(),
                ReqFtr17PostDeleteDecision::SelectNext(file_d.clone())
            )
        );
        remove_temp_root(root.as_path());
    }

    #[test]
    fn ftr_test80_req_ftr22_multi_selected_rows_disable_native_tree_highlight() {
        assert!(!super::use_native_tree_selection_highlight(true, true));
        assert!(!super::use_native_tree_selection_highlight(false, true));
        assert!(!super::use_native_tree_selection_highlight(false, false));
        assert!(!super::use_native_tree_selection_highlight(true, false));
    }

    #[test]
    fn ftr_test84_req_ftr23_daily_dir_plan_success_positions_after_refresh() {
        let daily_dir = PathBuf::from("C:/tmp/user_document/2026/04/05");
        let plan = req_ftr23_daily_dir_plan(Ok(daily_dir.clone()));
        assert_eq!(plan, ReqFtr23DailyDirPlan::RefreshAndPosition { daily_dir });
    }

    #[test]
    fn ftr_test85_req_ftr23_daily_dir_plan_failure_refresh_only_without_panic() {
        let plan = req_ftr23_daily_dir_plan(Err(std::io::Error::other(
            "req-ftr23 ensure daily dir failed",
        )));
        if let ReqFtr23DailyDirPlan::RefreshOnly { ensure_error } = plan {
            assert!(ensure_error.contains("req-ftr23 ensure daily dir failed"));
            return;
        }
        panic!("expected RefreshOnly plan on ensure_daily_directory error");
    }

    #[test]
    fn ftr_test86_empty_directory_is_tracked_as_directory_item_id() {
        let root = new_temp_root("ftr_test86_empty_directory_is_tracked_as_directory_item_id");
        let empty_dir = root.join("recyclebin");
        fs::create_dir_all(&empty_dir).expect("create empty directory");

        let file_path = root.join("note.txt");
        fs::write(&file_path, "note").expect("create sibling file");

        let empty_dir_id = empty_dir.to_string_lossy().to_string();
        let file_id = file_path.to_string_lossy().to_string();

        let items = super::build_file_items(&root, &root);
        let mut directory_item_ids = HashSet::new();
        super::collect_directory_item_ids_from_tree(&items, &mut directory_item_ids);

        assert!(
            directory_item_ids.contains(&empty_dir_id),
            "empty directory should remain classified as a folder"
        );
        assert!(
            !directory_item_ids.contains(&file_id),
            "regular files must not be classified as folders"
        );

        remove_temp_root(&root);
    }

    #[test]
    fn newf_test42_req_newf38_empty_create_select_policy_uses_notitle_stem() {
        assert!(super::should_apply_req_newf38_tree_selection(Some(
            "notitle-20260413235959999"
        )));
        assert!(!super::should_apply_req_newf38_tree_selection(Some("filename")));
        assert!(!super::should_apply_req_newf38_tree_selection(None));
    }
}
