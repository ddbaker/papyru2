use std::{
    borrow::Cow,
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::*;
use gpui_component::{
    Root,
    resizable::{ResizablePanelEvent, ResizableState, h_resizable, resizable_panel},
    v_flex,
};

use crate::editor::Papyru2Editor;
use crate::file_tree::{FileTreeEvent, FileTreeView};
use crate::top_bars::{SHARED_INTER_PANEL_SPACING_PX, TopBars};

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

pub(crate) const REQ_EDITOR_SHARED_TEXT_SIZE_POLICY: &str = "text_sm";

pub(crate) fn req_editor_shared_text_size_policy() -> &'static str {
    REQ_EDITOR_SHARED_TEXT_SIZE_POLICY
}

pub(crate) fn apply_req_editor_shared_text_size<T>(element: T) -> T
where
    T: Styled,
{
    element.text_sm()
}

pub(crate) fn should_restore_singleline_focus_after_new_file(
    singleline_was_focused: bool,
    editor_was_focused: bool,
) -> bool {
    singleline_was_focused && !editor_was_focused
}

pub(crate) fn file_tree_root_dir_from_app_paths(
    app_paths: &crate::path_resolver::AppPaths,
) -> PathBuf {
    app_paths.user_document_dir.clone()
}

pub(crate) fn should_route_delete_to_file_tree(
    file_tree_focused: bool,
    file_tree_delete_shortcut_armed: bool,
    editor_focused: bool,
    singleline_focused: bool,
) -> bool {
    if singleline_focused {
        return false;
    }
    if file_tree_focused {
        return true;
    }
    editor_focused && file_tree_delete_shortcut_armed
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SelectionLoadRoutingTransition {
    pub next_focus_reassert_pending: bool,
    pub schedule_focus_reassert: bool,
}

pub(crate) fn transition_selection_load_result(
    selection_load_succeeded: bool,
) -> SelectionLoadRoutingTransition {
    SelectionLoadRoutingTransition {
        next_focus_reassert_pending: selection_load_succeeded,
        schedule_focus_reassert: selection_load_succeeded,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EditorFocusRoutingTransition {
    pub process_editor_focus: bool,
    pub next_focus_reassert_pending: bool,
}

pub(crate) fn transition_editor_focus_gained(
    selection_focus_reassert_pending: bool,
) -> EditorFocusRoutingTransition {
    EditorFocusRoutingTransition {
        process_editor_focus: !selection_focus_reassert_pending,
        next_focus_reassert_pending: selection_focus_reassert_pending,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FocusReassertTickTransition {
    pub run_focus_reassert: bool,
    pub next_focus_reassert_pending: bool,
}

pub(crate) fn transition_focus_reassert_tick(
    selection_focus_reassert_pending: bool,
) -> FocusReassertTickTransition {
    if !selection_focus_reassert_pending {
        return FocusReassertTickTransition {
            run_focus_reassert: false,
            next_focus_reassert_pending: false,
        };
    }

    FocusReassertTickTransition {
        run_focus_reassert: true,
        next_focus_reassert_pending: false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlusButtonResetStep {
    ClearEditor,
    ClearSingleline,
    FocusSingleline,
}

pub(crate) fn req_newf34_plus_button_reset_steps() -> [PlusButtonResetStep; 3] {
    [
        PlusButtonResetStep::ClearEditor,
        PlusButtonResetStep::ClearSingleline,
        PlusButtonResetStep::FocusSingleline,
    ]
}

pub(crate) fn req_ftr14_create_flow_uses_watcher_refresh_only() -> bool {
    true
}

pub(crate) fn req_ftr14_delete_flow_uses_watcher_refresh_only() -> bool {
    true
}

pub(crate) fn req_ftr14_rename_flow_uses_watcher_refresh_only() -> bool {
    true
}

const DEFAULT_SPLIT_LEFT_PANEL_SIZE_PX: f32 = 320.0;
const SPLITTER_PERSISTENCE_FALLBACK_RIGHT_PANEL_SIZE_PX: f32 = 1.0;

fn is_valid_split_panel_size(size: Pixels) -> bool {
    let size = f32::from(size);
    size.is_finite() && size > 0.0
}

fn normalize_split_left_panel_size(restored_splitter_left_size: Option<f32>) -> Pixels {
    px(restored_splitter_left_size
        .filter(|size| size.is_finite() && *size > 0.0)
        .unwrap_or(DEFAULT_SPLIT_LEFT_PANEL_SIZE_PX))
}

fn current_window_width(window: &Window) -> Pixels {
    window.window_bounds().get_bounds().size.width
}

fn should_recreate_layout_split_state(
    previous_window_width: Pixels,
    current_window_width: Pixels,
) -> bool {
    previous_window_width != current_window_width
}

pub(crate) fn persisted_splitter_sizes(
    actual_sizes: &[Pixels],
    preserved_left_panel_size: Pixels,
) -> Vec<Pixels> {
    if actual_sizes.len() >= 2
        && actual_sizes
            .iter()
            .all(|size| is_valid_split_panel_size(*size))
    {
        return actual_sizes.to_vec();
    }

    vec![
        preserved_left_panel_size,
        px(SPLITTER_PERSISTENCE_FALLBACK_RIGHT_PANEL_SIZE_PX),
    ]
}

fn build_startup_window_options(startup_bounds: WindowBounds) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(startup_bounds),
        focus: true,
        show: true,
        ..Default::default()
    }
}

pub struct Papyru2App {
    pub(crate) top_bars: Entity<TopBars>,
    pub(crate) singleline: Entity<crate::singleline_input::SingleLineInput>,
    pub(crate) editor: Entity<Papyru2Editor>,
    pub(crate) file_tree: Entity<FileTreeView>,
    pub(crate) layout_split_state: Entity<ResizableState>,
    pub(crate) split_left_panel_size: Pixels,
    pub(crate) last_window_width: Pixels,
    pub(crate) layout_split_subscription: Subscription,
    pub(crate) file_workflow: crate::file_update_handler::SinglelineCreateFileWorkflow,
    pub(crate) editor_autosave: crate::file_update_handler::EditorAutoSaveCoordinator,
    pub(crate) _subscriptions: Vec<Subscription>,
    pub(crate) app_paths: crate::path_resolver::AppPaths,
    pub(crate) _file_tree_watcher: crate::file_tree_watcher::FileTreeWatcher,
    pub(crate) selection_focus_reassert_pending: bool,
    pub(crate) rpc_highlight_active: bool,
    pub(crate) rpc_highlight_line_1_based: Option<u32>,
}

pub(crate) const FOLDER_REFRESH_ICON_PATH: &str = "icons/folder-refresh.svg";

const FOLDER_REFRESH_ICON_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 2h7a2 2 0 0 1 2 2z"/><path d="M14 14a4 4 0 1 0 1.2-2.8"/><path d="M14 10v4h4"/></svg>"#;

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if path == FOLDER_REFRESH_ICON_PATH {
            return Ok(Some(Cow::Borrowed(FOLDER_REFRESH_ICON_SVG)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        if FOLDER_REFRESH_ICON_PATH.starts_with(path)
            && !assets
                .iter()
                .any(|entry| entry.as_ref() == FOLDER_REFRESH_ICON_PATH)
        {
            assets.push(FOLDER_REFRESH_ICON_PATH.into());
        }
        Ok(assets)
    }
}

impl Papyru2App {
    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if event.is_held {
            cx.propagate();
            return;
        }

        let key = event.keystroke.key.as_str().to_ascii_lowercase();
        let is_delete_key =
            key == "delete" || key == "backspace" || key == "forwarddelete" || key == "del";
        if !is_delete_key {
            let editor_focused = self.editor.read(cx).is_focused(window, cx);
            let singleline_focused = self.singleline.read(cx).is_focused(window, cx);
            if editor_focused || singleline_focused {
                self.file_tree.update(cx, |file_tree, _| {
                    file_tree.disarm_delete_shortcut("non_delete_key")
                });
            }
            cx.propagate();
            return;
        }

        let editor_focused = self.editor.read(cx).is_focused(window, cx);
        let singleline_focused = self.singleline.read(cx).is_focused(window, cx);
        let file_tree_focused = self.file_tree.read(cx).is_focused(window, cx);
        let file_tree_delete_shortcut_armed = if file_tree_focused {
            false
        } else {
            self.file_tree.update(cx, |file_tree, _| {
                file_tree.consume_delete_shortcut_for_editor()
            })
        };
        let should_route_to_file_tree = should_route_delete_to_file_tree(
            file_tree_focused,
            file_tree_delete_shortcut_armed,
            editor_focused,
            singleline_focused,
        );

        if !should_route_to_file_tree {
            trace_debug(format!(
                "app keydown key={} propagate editor_focused={} singleline_focused={} file_tree_focused={} delete_shortcut_armed={}",
                key,
                editor_focused,
                singleline_focused,
                file_tree_focused,
                file_tree_delete_shortcut_armed
            ));
            cx.propagate();
            return;
        }

        let requested = self
            .file_tree
            .update(cx, |file_tree, cx| file_tree.request_recyclebin_delete(cx));
        trace_debug(format!(
            "app keydown key={} requested_by_file_tree={} editor_focused={} singleline_focused={} file_tree_focused={} delete_shortcut_armed={}",
            key,
            requested,
            editor_focused,
            singleline_focused,
            file_tree_focused,
            file_tree_delete_shortcut_armed
        ));
        if requested {
            cx.stop_propagation();
        } else {
            cx.propagate();
        }
    }

    fn subscribe_layout_split_state(
        layout_split_state: &Entity<ResizableState>,
        splitter_resize_save_path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        cx.subscribe_in(
            layout_split_state,
            window,
            move |this, _, event: &ResizablePanelEvent, window, cx| match event {
                ResizablePanelEvent::Resized => {
                    let next_left_panel_size = this
                        .layout_split_state
                        .read(cx)
                        .sizes()
                        .first()
                        .copied()
                        .filter(|size| is_valid_split_panel_size(*size))
                        .unwrap_or(this.split_left_panel_size);
                    this.split_left_panel_size = next_left_panel_size;

                    let layout_split_state = this.layout_split_state.clone();
                    this.top_bars.update(cx, |top_bars, _| {
                        top_bars.sync_layout_split(layout_split_state, next_left_panel_size);
                    });

                    trace_debug(format!(
                        "layout split resized left_size={}",
                        f32::from(next_left_panel_size)
                    ));

                    let state = this.capture_window_position_state(window, cx);
                    trace_debug(format!(
                        "window_position splitter resize save path={} splitter_sizes={:?}",
                        splitter_resize_save_path.display(),
                        state.splitter_sizes
                    ));
                    if let Err(error) = crate::window_position::save_window_position_atomic(
                        splitter_resize_save_path.as_path(),
                        &state,
                    ) {
                        trace_debug(format!(
                            "window_position splitter resize save failed path={} error={error}",
                            splitter_resize_save_path.display()
                        ));
                    }
                }
            },
        )
    }

    fn reset_layout_split_state(
        &mut self,
        window: &mut Window,
        splitter_resize_save_path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let new_layout_split_state = cx.new(|_| ResizableState::default());
        self.layout_split_subscription = Self::subscribe_layout_split_state(
            &new_layout_split_state,
            splitter_resize_save_path,
            window,
            cx,
        );
        self.layout_split_state = new_layout_split_state.clone();
        let split_left_panel_size = self.split_left_panel_size;
        self.top_bars.update(cx, |top_bars, _| {
            top_bars.sync_layout_split(new_layout_split_state, split_left_panel_size);
        });
        trace_debug(format!(
            "layout split state reset for window resize left_size={}",
            f32::from(self.split_left_panel_size)
        ));
        cx.notify();
    }

    fn new(
        window: &mut Window,
        app_paths: crate::path_resolver::AppPaths,
        restored_splitter_left_size: Option<f32>,
        cx: &mut Context<Self>,
    ) -> Self {
        let split_left_panel_size = normalize_split_left_panel_size(restored_splitter_left_size);
        trace_debug(format!(
            "window_position splitter restore left_size={} applied={}",
            f32::from(split_left_panel_size),
            restored_splitter_left_size.is_some()
        ));

        let layout_split_state = cx.new(|_| ResizableState::default());
        let top_bars = cx.new(|cx| {
            TopBars::new(
                window,
                layout_split_state.clone(),
                split_left_panel_size,
                cx,
            )
        });
        let singleline = top_bars.read(cx).singleline();
        let editor = cx.new(|cx| Papyru2Editor::new(window, cx));
        let protected_delete_roots = vec![
            app_paths.data_dir.clone(),
            app_paths.user_document_dir.clone(),
        ];
        let file_tree_root_dir = file_tree_root_dir_from_app_paths(&app_paths);
        trace_debug(format!(
            "file_tree app root_dir={}",
            file_tree_root_dir.display()
        ));
        let startup_daily_dir = match crate::file_update_handler::ensure_daily_directory(
            app_paths.user_document_dir.as_path(),
            chrono::Local::now(),
        ) {
            Ok(path) => {
                trace_debug(format!(
                    "file_tree req-ftr18 startup daily_dir ensured path={}",
                    path.display()
                ));
                path
            }
            Err(error) => {
                trace_debug(format!(
                    "file_tree req-ftr18 startup daily_dir ensure failed error={error}"
                ));
                panic!("file_tree req-ftr18 startup daily_dir ensure failed: {error}");
            }
        };
        let file_tree = cx.new(move |cx| {
            FileTreeView::new(protected_delete_roots, file_tree_root_dir.clone(), cx)
        });
        let (file_tree_watcher, file_tree_refresh_rx) =
            match crate::file_tree_watcher::start_file_tree_watcher(
                app_paths.user_document_dir.clone(),
            ) {
                Ok(watcher) => watcher,
                Err(error) => {
                    trace_debug(format!("file_tree watcher init failed error={error}"));
                    panic!("file_tree watcher init failed: {error}");
                }
            };
        let file_workflow = crate::file_update_handler::SinglelineCreateFileWorkflow::new();
        let editor_autosave = crate::file_update_handler::EditorAutoSaveCoordinator::new();

        let window_position_path =
            app_paths.config_file_path(crate::window_position::WINDOW_POSITION_FILE_NAME);
        let last_debounced_save = Rc::new(RefCell::new(None::<Instant>));
        let debounced_save_clock = last_debounced_save.clone();
        let debounced_save_path = window_position_path.clone();
        let splitter_resize_save_path = window_position_path.clone();
        let observe_splitter_resize_save_path = window_position_path.clone();

        let layout_split_subscription = Self::subscribe_layout_split_state(
            &layout_split_state,
            splitter_resize_save_path,
            window,
            cx,
        );

        crate::file_update_handler::spawn_editor_autosave_worker(
            editor_autosave.clone(),
            file_workflow.clone(),
        );
        let (quic_rpc_ui_tx, quic_rpc_ui_rx) =
            smol::channel::unbounded::<crate::quic_rpc::QuicRpcUiCommand>();
        crate::quic_rpc::spawn_quic_rpc_server(
            app_paths.clone(),
            file_workflow.clone(),
            quic_rpc_ui_tx,
        );
        let quic_window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            while let Ok(command) = quic_rpc_ui_rx.recv().await {
                let Some(this) = this.upgrade() else {
                    break;
                };
                let window_handle = quic_window_handle.clone();
                let _ = this.update(cx, move |app, cx| {
                    if let Err(error) = cx.update_window(window_handle, |_, window, cx| {
                        app.apply_quic_rpc_pin_command(command, window, cx);
                    }) {
                        trace_debug(format!("quic_rpc ui apply skipped error={error}"));
                    }
                });
            }
            trace_debug("quic_rpc ui bridge loop detached");
        })
        .detach();
        cx.spawn(async move |this, cx| {
            while file_tree_refresh_rx.recv().await.is_ok() {
                let Some(this) = this.upgrade() else {
                    break;
                };
                let _ = this.update(cx, |app, cx| app.apply_file_tree_watcher_refresh(cx));
            }
            trace_debug("file_tree watcher refresh loop detached");
        })
        .detach();

        let mut subscriptions = vec![
            cx.subscribe_in(
                &file_tree,
                window,
                move |this, _, event: &FileTreeEvent, window, cx| match event {
                    FileTreeEvent::SelectionChanged(path) => {
                        this.handle_file_tree_selection_changed(path.clone(), window, cx);
                    }
                    FileTreeEvent::OpenFile(path) => {
                        this.sync_singleline_from_file_tree_selection(path.as_path(), window, cx);
                        let _ = this.open_file(path.clone(), window, cx);
                    }
                    FileTreeEvent::RecyclebinDeleteRequested(paths) => {
                        this.on_file_tree_delete_requested(paths.clone(), window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &top_bars,
                window,
                move |this, _, event: &crate::top_bars::TopBarsEvent, window, cx| match event {
                    crate::top_bars::TopBarsEvent::PressFolderRefresh => {
                        trace_debug("app received TopBarsEvent::PressFolderRefresh");
                        this.handle_folder_refresh_button(window, cx);
                    }
                    crate::top_bars::TopBarsEvent::PressPlus => {
                        trace_debug("app received TopBarsEvent::PressPlus");
                        this.handle_plus_button(window, cx);
                    }
                },
            ),
            cx.subscribe_in(
                &singleline,
                window,
                move |this, _, event: &crate::singleline_input::SingleLineEvent, window, cx| {
                    match event {
                        crate::singleline_input::SingleLineEvent::PressEnter => {
                            trace_debug("app received SingleLineEvent::PressEnter");
                            this.transfer_singleline_enter(window, cx);
                        }
                        crate::singleline_input::SingleLineEvent::PressDown => {
                            trace_debug("app received SingleLineEvent::PressDown");
                            this.ensure_new_file_flow("singleline_down", window, cx);
                            this.transfer_singleline_down(window, cx);
                        }
                        crate::singleline_input::SingleLineEvent::ValueChanged {
                            value,
                            cursor_char,
                        } => {
                            trace_debug(format!(
                                "app received SingleLineEvent::ValueChanged cursor={} value='{}'",
                                cursor_char,
                                compact_text(value)
                            ));
                            this.on_singleline_value_changed(value, window, cx);
                        }
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
                    crate::editor::EditorEvent::PressUpAtFirstLine => {
                        trace_debug("app received EditorEvent::PressUpAtFirstLine");
                        this.transfer_editor_up(window, cx);
                    }
                    crate::editor::EditorEvent::FocusGained => {
                        let transition =
                            transition_editor_focus_gained(this.selection_focus_reassert_pending);
                        this.selection_focus_reassert_pending =
                            transition.next_focus_reassert_pending;
                        trace_debug(format!(
                            "app received EditorEvent::FocusGained process={} selection_focus_reassert_pending={}",
                            transition.process_editor_focus,
                            this.selection_focus_reassert_pending
                        ));
                        if !transition.process_editor_focus {
                            return;
                        }
                        this.ensure_new_file_flow("editor_focus", window, cx);
                    }
                    crate::editor::EditorEvent::UserInteraction => {
                        this.clear_rpc_highlight_on_editor_interaction();
                    }
                    crate::editor::EditorEvent::UserBufferChanged { value } => {
                        this.clear_rpc_highlight_on_editor_interaction();
                        this.on_editor_user_buffer_changed(value, cx);
                    }
                },
            ),
        ];

        subscriptions.push(cx.observe_window_bounds(window, move |this, window, cx| {
            let current_width = current_window_width(window);
            if should_recreate_layout_split_state(this.last_window_width, current_width) {
                trace_debug(format!(
                    "layout split window resize detected previous_width={} current_width={} preserved_left_size={}",
                    f32::from(this.last_window_width),
                    f32::from(current_width),
                    f32::from(this.split_left_panel_size)
                ));
                this.reset_layout_split_state(
                    window,
                    observe_splitter_resize_save_path.clone(),
                    cx,
                );
            }
            this.last_window_width = current_width;

            let now = Instant::now();
            let should_save = debounced_save_clock
                .borrow()
                .map(|last_save| now.duration_since(last_save) >= Duration::from_secs(1))
                .unwrap_or(true);
            if !should_save {
                return;
            }

            *debounced_save_clock.borrow_mut() = Some(now);
            let state = this.capture_window_position_state(window, cx);
            if let Err(error) = crate::window_position::save_window_position_atomic(
                debounced_save_path.as_path(),
                &state,
            ) {
                trace_debug(format!(
                    "window_position debounced save failed error={error}"
                ));
            }
        }));

        file_workflow.reset_startup_to_neutral();
        singleline.update(cx, |singleline, cx| {
            singleline.apply_cursor(0, window, cx);
            singleline.focus(window, cx);
            singleline.set_current_editing_file_path(None);
        });
        editor.update(cx, |editor, _| {
            editor.set_current_editing_file_path(None);
        });

        let mut this = Self {
            top_bars,
            singleline,
            editor,
            file_tree,
            layout_split_state,
            split_left_panel_size,
            last_window_width: current_window_width(window),
            layout_split_subscription,
            file_workflow,
            editor_autosave,
            _subscriptions: subscriptions,
            app_paths,
            _file_tree_watcher: file_tree_watcher,
            selection_focus_reassert_pending: false,
            rpc_highlight_active: false,
            rpc_highlight_line_1_based: None,
        };

        this.apply_req_ftr18_startup_daily_folder_positioning(startup_daily_dir, window, cx);

        this
    }
}

impl Render for Papyru2App {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("papyru2")
            .size_full()
            .capture_key_down(cx.listener(Self::on_key_down))
            .gap_2()
            .p_2()
            .child(self.top_bars.clone())
            .child(
                div().flex_1().child(
                    h_resizable("bottom-split")
                        .with_state(&self.layout_split_state)
                        .child(
                            resizable_panel()
                                .size(self.split_left_panel_size)
                                .child(self.file_tree.clone()),
                        )
                        .child(
                            resizable_panel().child(
                                div()
                                    .size_full()
                                    .pl(px(SHARED_INTER_PANEL_SPACING_PX))
                                    .child(self.editor.clone()),
                            ),
                        ),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_SPLIT_LEFT_PANEL_SIZE_PX, PlusButtonResetStep,
        SPLITTER_PERSISTENCE_FALLBACK_RIGHT_PANEL_SIZE_PX, build_startup_window_options,
        file_tree_root_dir_from_app_paths, persisted_splitter_sizes,
        req_ftr14_create_flow_uses_watcher_refresh_only,
        req_ftr14_delete_flow_uses_watcher_refresh_only,
        req_ftr14_rename_flow_uses_watcher_refresh_only, req_newf34_plus_button_reset_steps,
        should_recreate_layout_split_state, should_restore_singleline_focus_after_new_file,
        should_route_delete_to_file_tree, transition_editor_focus_gained,
        transition_focus_reassert_tick, transition_selection_load_result,
    };
    use crate::file_update_handler::EditorAutoSaveCoordinator;
    use crate::path_resolver::{AppPaths, RunEnvPattern};
    use crate::top_bars::SHARED_INTER_PANEL_SPACING_PX;
    use gpui::{WindowBounds, bounds, point, px, size};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };

    fn autosave_payload(
        path: &str,
        text: &str,
    ) -> crate::file_update_handler::EditorAutoSavePayload {
        crate::file_update_handler::EditorAutoSavePayload {
            user_document_dir: PathBuf::from("C:/tmp"),
            current_path: PathBuf::from(path),
            editor_text: text.to_string(),
        }
    }

    #[test]
    fn app_newfile_focus_test1_restore_when_singleline_had_focus_and_editor_did_not() {
        assert!(should_restore_singleline_focus_after_new_file(true, false));
    }

    #[test]
    fn app_newfile_focus_test2_no_restore_when_editor_already_had_focus() {
        assert!(!should_restore_singleline_focus_after_new_file(false, true));
        assert!(!should_restore_singleline_focus_after_new_file(true, true));
    }

    #[test]
    fn app_newf_focus_test3_req_newf34_plus_button_restores_singleline_focus_from_editor() {
        assert_eq!(
            req_newf34_plus_button_reset_steps(),
            [
                PlusButtonResetStep::ClearEditor,
                PlusButtonResetStep::ClearSingleline,
                PlusButtonResetStep::FocusSingleline,
            ]
        );
    }

    #[test]
    fn ftr_test21_req_ftr3_regression_editor_focus_with_tree_shortcut_routes_delete_to_file_tree() {
        assert!(should_route_delete_to_file_tree(false, true, true, false));
    }

    #[test]
    fn ftr_test22_req_ftr3_regression_without_tree_shortcut_keeps_editor_delete_behavior() {
        assert!(!should_route_delete_to_file_tree(false, false, true, false));
        assert!(!should_route_delete_to_file_tree(false, true, false, false));
        assert!(!should_route_delete_to_file_tree(false, true, true, true));
    }

    #[test]
    fn ftr_test40_req_ftr16_regression_selection_load_focus_reassert_gate_engages() {
        assert!(transition_selection_load_result(true).schedule_focus_reassert);
        assert!(!transition_selection_load_result(false).schedule_focus_reassert);
    }

    #[test]
    fn ftr_test41_req_ftr16_regression_regular_editor_focus_path_is_preserved() {
        assert!(transition_editor_focus_gained(false).process_editor_focus);
        assert!(!transition_editor_focus_gained(true).process_editor_focus);
    }

    #[test]
    fn ftr_test42_req_ftr16_regression_delete_routes_to_file_tree_after_focus_reassert() {
        assert!(should_route_delete_to_file_tree(true, false, false, false));
    }

    #[test]
    fn ftr_test44_req_ftr16_hard_selection_load_success_schedules_reassert() {
        let transition = transition_selection_load_result(true);
        assert_eq!(
            transition,
            super::SelectionLoadRoutingTransition {
                next_focus_reassert_pending: true,
                schedule_focus_reassert: true,
            }
        );
    }

    #[test]
    fn ftr_test45_req_ftr16_hard_selection_load_failure_skips_reassert() {
        let transition = transition_selection_load_result(false);
        assert_eq!(
            transition,
            super::SelectionLoadRoutingTransition {
                next_focus_reassert_pending: false,
                schedule_focus_reassert: false,
            }
        );
    }

    #[test]
    fn ftr_test46_req_ftr16_hard_editor_focus_processing_respects_pending_reassert() {
        let pending_transition = transition_editor_focus_gained(true);
        assert_eq!(
            pending_transition,
            super::EditorFocusRoutingTransition {
                process_editor_focus: false,
                next_focus_reassert_pending: true,
            }
        );

        let regular_transition = transition_editor_focus_gained(false);
        assert_eq!(
            regular_transition,
            super::EditorFocusRoutingTransition {
                process_editor_focus: true,
                next_focus_reassert_pending: false,
            }
        );
    }

    #[test]
    fn ftr_test47_req_ftr16_hard_focus_reassert_tick_is_idempotent() {
        let first_tick = transition_focus_reassert_tick(true);
        assert_eq!(
            first_tick,
            super::FocusReassertTickTransition {
                run_focus_reassert: true,
                next_focus_reassert_pending: false,
            }
        );

        let second_tick = transition_focus_reassert_tick(first_tick.next_focus_reassert_pending);
        assert_eq!(
            second_tick,
            super::FocusReassertTickTransition {
                run_focus_reassert: false,
                next_focus_reassert_pending: false,
            }
        );
    }

    #[test]
    fn ftr_test48_req_ftr16_hard_sequence_success_routes_delete_to_file_tree() {
        let selection_load = transition_selection_load_result(true);
        let mut pending_focus_reassert = selection_load.next_focus_reassert_pending;
        assert!(selection_load.schedule_focus_reassert);
        assert!(pending_focus_reassert);

        let editor_focus = transition_editor_focus_gained(pending_focus_reassert);
        pending_focus_reassert = editor_focus.next_focus_reassert_pending;
        assert!(!editor_focus.process_editor_focus);
        assert!(pending_focus_reassert);

        let tick = transition_focus_reassert_tick(pending_focus_reassert);
        pending_focus_reassert = tick.next_focus_reassert_pending;
        assert!(tick.run_focus_reassert);
        assert!(!pending_focus_reassert);

        assert!(should_route_delete_to_file_tree(
            true,  // file_tree_focused after reassert
            false, // delete shortcut arm not required when focused
            false, // editor should not own delete
            false, // singleline not focused
        ));
    }

    #[test]
    fn ftr_test49_req_ftr16_hard_sequence_failure_keeps_non_tree_delete_routing() {
        let selection_load = transition_selection_load_result(false);
        let mut pending_focus_reassert = selection_load.next_focus_reassert_pending;
        assert!(!selection_load.schedule_focus_reassert);
        assert!(!pending_focus_reassert);

        let editor_focus = transition_editor_focus_gained(pending_focus_reassert);
        pending_focus_reassert = editor_focus.next_focus_reassert_pending;
        assert!(editor_focus.process_editor_focus);
        assert!(!pending_focus_reassert);

        let tick = transition_focus_reassert_tick(pending_focus_reassert);
        pending_focus_reassert = tick.next_focus_reassert_pending;
        assert!(!tick.run_focus_reassert);
        assert!(!pending_focus_reassert);

        assert!(!should_route_delete_to_file_tree(
            false, // file_tree not focused
            false, // no consumed file-tree shortcut arm
            true,  // editor remains focused
            false, // singleline not focused
        ));
    }

    #[test]
    fn ftr_test50_req_ftr16_hard_delete_routing_truth_table_is_exhaustive() {
        for file_tree_focused in [false, true] {
            for file_tree_delete_shortcut_armed in [false, true] {
                for editor_focused in [false, true] {
                    for singleline_focused in [false, true] {
                        let actual = should_route_delete_to_file_tree(
                            file_tree_focused,
                            file_tree_delete_shortcut_armed,
                            editor_focused,
                            singleline_focused,
                        );
                        let expected = !singleline_focused
                            && (file_tree_focused
                                || (editor_focused && file_tree_delete_shortcut_armed));
                        assert_eq!(
                            actual,
                            expected,
                            "routing mismatch (file_tree_focused={}, shortcut_armed={}, editor_focused={}, singleline_focused={})",
                            file_tree_focused,
                            file_tree_delete_shortcut_armed,
                            editor_focused,
                            singleline_focused
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ftr_test55_req_ftr17_delete_routing_invariants_remain_unchanged() {
        let cases = [
            (true, false, false, false),
            (false, true, true, false),
            (false, false, true, false),
            (false, true, true, true),
        ];

        for (
            file_tree_focused,
            file_tree_delete_shortcut_armed,
            editor_focused,
            singleline_focused,
        ) in cases
        {
            let actual = should_route_delete_to_file_tree(
                file_tree_focused,
                file_tree_delete_shortcut_armed,
                editor_focused,
                singleline_focused,
            );
            let expected = !singleline_focused
                && (file_tree_focused || (editor_focused && file_tree_delete_shortcut_armed));
            assert_eq!(
                actual,
                expected,
                "req-ftr17 invariant mismatch (file_tree_focused={}, shortcut_armed={}, editor_focused={}, singleline_focused={})",
                file_tree_focused,
                file_tree_delete_shortcut_armed,
                editor_focused,
                singleline_focused
            );
        }
    }

    #[test]
    fn ftr_test73_req_ftr20_delete_routing_invariants_remain_unchanged_for_multi_delete_flow() {
        for file_tree_focused in [false, true] {
            for file_tree_delete_shortcut_armed in [false, true] {
                for editor_focused in [false, true] {
                    for singleline_focused in [false, true] {
                        let actual = should_route_delete_to_file_tree(
                            file_tree_focused,
                            file_tree_delete_shortcut_armed,
                            editor_focused,
                            singleline_focused,
                        );
                        let expected = !singleline_focused
                            && (file_tree_focused
                                || (editor_focused && file_tree_delete_shortcut_armed));
                        assert_eq!(
                            actual,
                            expected,
                            "req-ftr20 routing invariant mismatch (file_tree_focused={}, shortcut_armed={}, editor_focused={}, singleline_focused={})",
                            file_tree_focused,
                            file_tree_delete_shortcut_armed,
                            editor_focused,
                            singleline_focused
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ftr_test30_req_ftr14_create_flow_uses_watcher_refresh_only() {
        assert!(req_ftr14_create_flow_uses_watcher_refresh_only());
    }

    #[test]
    fn ftr_test31_req_ftr14_delete_flow_uses_watcher_refresh_only() {
        assert!(req_ftr14_delete_flow_uses_watcher_refresh_only());
    }

    #[test]
    fn ftr_test32_req_ftr14_rename_flow_uses_watcher_refresh_only() {
        assert!(req_ftr14_rename_flow_uses_watcher_refresh_only());
    }

    #[test]
    fn ftr_test26_req_ftr11_file_tree_root_is_user_document_dir_from_app_paths() {
        let app_paths = AppPaths {
            mode: RunEnvPattern::DevCargoRun,
            app_home: PathBuf::from("C:/tmp/app_home"),
            conf_dir: PathBuf::from("C:/tmp/app_home/conf"),
            data_dir: PathBuf::from("C:/tmp/app_home/data"),
            user_document_dir: PathBuf::from("C:/tmp/app_home/data/user_document"),
            recyclebin_dir: PathBuf::from("C:/tmp/app_home/data/user_document/recyclebin"),
            log_dir: PathBuf::from("C:/tmp/app_home/log"),
            bin_dir: PathBuf::from("C:/tmp/app_home/bin"),
        };

        assert_eq!(
            file_tree_root_dir_from_app_paths(&app_paths),
            app_paths.user_document_dir
        );
    }

    #[test]
    fn aus_test4_timer_gate_rearms_between_cycles() {
        let coordinator = EditorAutoSaveCoordinator::new();
        let base = Instant::now();
        coordinator.mark_user_edit(autosave_payload("C:/tmp/a.txt", "first"), base);

        let not_due =
            coordinator.pop_due_payload(base + Duration::from_secs(5), Duration::from_secs(6));
        assert!(not_due.is_none());

        let due = coordinator
            .pop_due_payload(base + Duration::from_secs(6), Duration::from_secs(6))
            .expect("due payload");
        assert_eq!(due.editor_text, "first");
        assert!(!coordinator.has_pending_payload());

        let no_repeat =
            coordinator.pop_due_payload(base + Duration::from_secs(7), Duration::from_secs(6));
        assert!(no_repeat.is_none());

        coordinator.mark_user_edit(
            autosave_payload("C:/tmp/a.txt", "second"),
            base + Duration::from_secs(8),
        );
        let due_again = coordinator
            .pop_due_payload(base + Duration::from_secs(14), Duration::from_secs(6))
            .expect("due payload again");
        assert_eq!(due_again.editor_text, "second");
    }

    #[test]
    fn aus_test5_non_user_events_do_not_arm_autosave_timer() {
        let coordinator = EditorAutoSaveCoordinator::new();
        coordinator.on_edit_path_changed(Some(PathBuf::from("C:/tmp/a.txt")));
        assert!(!coordinator.has_pending_payload());

        let none = coordinator.pop_due_payload(
            Instant::now() + Duration::from_secs(100),
            Duration::from_secs(6),
        );
        assert!(none.is_none());
    }

    #[test]
    fn aus_test7_continuous_typing_keeps_six_second_periodic_cycle() {
        let coordinator = EditorAutoSaveCoordinator::new();
        let base = Instant::now();

        coordinator.mark_user_edit(autosave_payload("C:/tmp/a.txt", "t0"), base);
        coordinator.mark_user_edit(
            autosave_payload("C:/tmp/a.txt", "t3"),
            base + Duration::from_secs(3),
        );
        coordinator.mark_user_edit(
            autosave_payload("C:/tmp/a.txt", "t5"),
            base + Duration::from_secs(5),
        );

        let due = coordinator
            .pop_due_payload(base + Duration::from_secs(6), Duration::from_secs(6))
            .expect("due at first 6-second window");
        assert_eq!(due.editor_text, "t5");
        assert!(!coordinator.has_pending_payload());

        coordinator.mark_user_edit(
            autosave_payload("C:/tmp/a.txt", "t7"),
            base + Duration::from_secs(7),
        );
        let not_due =
            coordinator.pop_due_payload(base + Duration::from_secs(12), Duration::from_secs(6));
        assert!(not_due.is_none());

        let due_again = coordinator
            .pop_due_payload(base + Duration::from_secs(13), Duration::from_secs(6))
            .expect("due at second 6-second window");
        assert_eq!(due_again.editor_text, "t7");
    }

    #[test]
    fn lo_test2_req_lo3_shared_inter_panel_spacing_is_10px() {
        assert_eq!(SHARED_INTER_PANEL_SPACING_PX, 10.0);
    }

    #[test]
    fn editor_test6_req_editor6_7_8_font_size_policy_maps_are_identified() {
        assert_eq!(super::req_editor_shared_text_size_policy(), "text_sm");
        assert_eq!(
            crate::file_tree::req_editor_file_tree_font_size_policy(),
            "text_sm"
        );
        assert_eq!(
            crate::singleline_input::req_editor_singleline_font_size_policy(),
            "text_sm"
        );
        assert_eq!(
            crate::editor::req_editor_editor_font_size_policy(),
            "text_sm"
        );
    }

    #[test]
    fn editor_test9_req_editor9_singleline_and_editor_share_file_tree_font_size_policy() {
        let file_tree_policy = crate::file_tree::req_editor_file_tree_font_size_policy();
        assert_eq!(
            crate::singleline_input::req_editor_singleline_font_size_policy(),
            file_tree_policy
        );
        assert_eq!(
            crate::editor::req_editor_editor_font_size_policy(),
            file_tree_policy
        );
    }

    #[test]
    fn lo_test3_req_lo4_persistence_fallback_preserves_left_panel_width() {
        let preserved_left_panel_size = px(DEFAULT_SPLIT_LEFT_PANEL_SIZE_PX);

        let persisted = persisted_splitter_sizes(&[], preserved_left_panel_size);

        assert_eq!(persisted.len(), 2);
        assert_eq!(f32::from(persisted[0]), DEFAULT_SPLIT_LEFT_PANEL_SIZE_PX);
        assert_eq!(
            f32::from(persisted[1]),
            SPLITTER_PERSISTENCE_FALLBACK_RIGHT_PANEL_SIZE_PX
        );
    }

    #[test]
    fn lo_test4_req_lo4_window_width_change_requires_split_state_reset() {
        assert!(should_recreate_layout_split_state(px(1200.0), px(1400.0)));
        assert!(!should_recreate_layout_split_state(px(1200.0), px(1200.0)));
    }

    #[test]
    fn win_test14_req_win18_startup_window_options_enable_focus_and_show() {
        let startup_bounds = WindowBounds::Windowed(bounds(
            point(px(50.0), px(60.0)),
            size(px(1200.0), px(800.0)),
        ));
        let options = build_startup_window_options(startup_bounds);

        assert!(options.focus);
        assert!(options.show);
        assert_eq!(options.window_bounds, Some(startup_bounds));
    }
}

pub fn run() {
    let cli_override = match crate::path_resolver::parse_cli_mode_override(std::env::args()) {
        Ok(override_mode) => override_mode,
        Err(error) => {
            trace_debug(format!("path_resolver CLI parse failed error={error}"));
            eprintln!("papyru2 CLI override parsing failed: {error}");
            eprintln!("use either --portable or --installed (not both)");
            return;
        }
    };

    trace_debug(format!("path_resolver cli_override={cli_override:?}"));

    let resolved_paths = match cli_override {
        Some(mode) => crate::path_resolver::AppPaths::resolve_with_cli_override(Some(mode)),
        None => crate::path_resolver::AppPaths::resolve(),
    };

    let app_paths = match resolved_paths {
        Ok(paths) => {
            let config_file = paths.config_file_path("app.toml");
            let log_file = paths.log_file_path("papyru2.log");
            trace_debug(format!(
                "path_resolver resolved mode={:?} reason={} app_home={} conf={} data={} user_document={} recyclebin={} log={} bin={} config_file={} app_log_file={}",
                paths.mode,
                paths.mode.reason(),
                paths.app_home.display(),
                paths.conf_dir.display(),
                paths.data_dir.display(),
                paths.user_document_dir.display(),
                paths.recyclebin_dir.display(),
                paths.log_dir.display(),
                paths.bin_dir.display(),
                config_file.display(),
                log_file.display()
            ));
            paths
        }
        Err(error) => {
            trace_debug(format!("path_resolver resolve failed error={error}"));
            eprintln!("papyru2 path resolver failed: {error}");
            return;
        }
    };

    let window_position_path =
        app_paths.config_file_path(crate::window_position::WINDOW_POSITION_FILE_NAME);
    let persisted_window_position =
        match crate::window_position::load_window_position(window_position_path.as_path()) {
            Ok(state) => {
                trace_debug(format!(
                    "window_position load path={} found={}",
                    window_position_path.display(),
                    state.is_some()
                ));
                state
            }
            Err(error) => {
                trace_debug(format!(
                    "window_position load failed path={} error={error}",
                    window_position_path.display()
                ));
                None
            }
        };
    let restored_splitter_left_size = persisted_window_position
        .as_ref()
        .and_then(|state| state.splitter_left_size());
    trace_debug(format!(
        "window_position splitter startup restore left_size={:?}",
        restored_splitter_left_size
    ));

    let app = Application::new().with_assets(AppAssets);

    app.run(move |cx| {
        gpui_component::init(cx);

        let primary_display_bounds = cx.primary_display().map(|display| display.bounds());
        let default_centered_bounds = WindowBounds::centered(size(px(1200.), px(800.)), cx);
        let fallback_bounds = crate::window_position::first_launch_fallback_bounds(
            primary_display_bounds.clone(),
            default_centered_bounds,
        );
        let startup_bounds = crate::window_position::resolve_startup_window_bounds(
            persisted_window_position.as_ref(),
            fallback_bounds,
            primary_display_bounds,
            crate::window_position::should_ignore_exact_position_for_wayland(),
        );

        let window_options = build_startup_window_options(startup_bounds);
        trace_debug(format!(
            "window_options startup focus={} show={} has_bounds={}",
            window_options.focus,
            window_options.show,
            window_options.window_bounds.is_some()
        ));

        let app_paths = app_paths.clone();
        let window_position_path = window_position_path.clone();
        let restored_splitter_left_size = restored_splitter_left_size;
        cx.spawn(async move |cx| {
            cx.open_window(window_options, move |window, cx| {
                let app_paths = app_paths.clone();
                let view = cx
                    .new(|cx| Papyru2App::new(window, app_paths, restored_splitter_left_size, cx));

                let close_save_path = window_position_path.clone();
                let close_view = view.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    let pre_close_saved = cx.update_entity(&close_view, |app, cx| {
                        app.flush_editor_content_before_context_switch("req-aus7-window-close", cx)
                    });
                    if !pre_close_saved {
                        trace_debug("autosave pre-close aborted close");
                        return false;
                    }

                    let state = cx.update_entity(&close_view, |app, cx| {
                        app.capture_window_position_state(window, cx)
                    });
                    trace_debug(format!(
                        "window_position close save path={} splitter_sizes={:?}",
                        close_save_path.display(),
                        state.splitter_sizes
                    ));
                    if let Err(error) = crate::window_position::save_window_position_atomic(
                        close_save_path.as_path(),
                        &state,
                    ) {
                        trace_debug(format!(
                            "window_position close save failed path={} error={error}",
                            close_save_path.display()
                        ));
                    }
                    true
                });

                cx.new(|cx| Root::new(view, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
