use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use chrono::Local;
use gpui::*;
use gpui_component::{
    Root,
    resizable::{ResizablePanelEvent, ResizableState, h_resizable, resizable_panel},
    v_flex,
};
use gpui_component_assets::Assets;

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

fn should_restore_singleline_focus_after_new_file(
    singleline_was_focused: bool,
    editor_was_focused: bool,
) -> bool {
    singleline_was_focused && !editor_was_focused
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

fn persisted_splitter_sizes(
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
    top_bars: Entity<TopBars>,
    singleline: Entity<crate::singleline_input::SingleLineInput>,
    editor: Entity<Papyru2Editor>,
    file_tree: Entity<FileTreeView>,
    layout_split_state: Entity<ResizableState>,
    split_left_panel_size: Pixels,
    last_window_width: Pixels,
    layout_split_subscription: Subscription,
    file_workflow: crate::file_update_handler::SinglelineCreateFileWorkflow,
    editor_autosave: crate::file_update_handler::EditorAutoSaveCoordinator,
    _subscriptions: Vec<Subscription>,
    app_paths: crate::path_resolver::AppPaths,
}

impl Papyru2App {
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
        let file_tree = cx.new(|cx| FileTreeView::new(cx));
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

        let mut subscriptions = vec![
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
                &top_bars,
                window,
                move |this, _, event: &crate::top_bars::TopBarsEvent, window, cx| match event {
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
                        trace_debug("app received EditorEvent::FocusGained");
                        this.ensure_new_file_flow("editor_focus", window, cx);
                    }
                    crate::editor::EditorEvent::UserBufferChanged { value } => {
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

        Self {
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
        }
    }

    fn capture_window_position_state(
        &self,
        window: &Window,
        cx: &App,
    ) -> crate::window_position::WindowPositionState {
        let actual_splitter_sizes = self.layout_split_state.read(cx).sizes().clone();
        let splitter_sizes =
            persisted_splitter_sizes(&actual_splitter_sizes, self.split_left_panel_size);
        if splitter_sizes != actual_splitter_sizes {
            trace_debug(format!(
                "window_position splitter persistence fallback left_size={} actual_sizes={:?}",
                f32::from(self.split_left_panel_size),
                actual_splitter_sizes
            ));
        }
        crate::window_position::WindowPositionState::from_window(window, cx)
            .with_splitter_sizes(&splitter_sizes)
    }

    fn apply_focus_target(
        &mut self,
        focus_target: crate::sl_editor_association::FocusTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match focus_target {
            crate::sl_editor_association::FocusTarget::Editor => {
                self.editor.update(cx, |editor, cx| {
                    editor.focus(window, cx);
                });
            }
            crate::sl_editor_association::FocusTarget::SingleLine => {
                self.singleline.update(cx, |singleline, cx| {
                    singleline.focus(window, cx);
                });
            }
        }
    }

    fn sync_current_editing_path_to_components(
        &mut self,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let autosave_path = path.clone();
        self.singleline.update(cx, |singleline, _| {
            singleline.set_current_editing_file_path(path.clone());
        });
        self.editor.update(cx, |editor, _| {
            editor.set_current_editing_file_path(path);
        });
        self.editor_autosave.on_edit_path_changed(autosave_path);

        let sl_path = self.singleline.read(cx).current_editing_file_path();
        let ed_path = self.editor.read(cx).current_editing_file_path();
        trace_debug(format!(
            "current_edit_path sync singleline={} editor={}",
            sl_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            ed_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
    }

    fn apply_forced_singleline_stem(
        &mut self,
        forced_stem: Option<String>,
        trace_label: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(forced_stem) = forced_stem else {
            trace_debug(format!(
                "{trace_label} force singleline stem update skipped (req-newf32)"
            ));
            return;
        };

        trace_debug(format!(
            "{trace_label} force singleline stem update='{}'",
            compact_text(&forced_stem)
        ));
        self.singleline.update(cx, |singleline, cx| {
            singleline.apply_text_and_cursor(
                forced_stem.clone(),
                forced_stem.chars().count(),
                window,
                cx,
            );
        });
    }

    fn ensure_new_file_flow(&mut self, trigger: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.file_workflow.state() != crate::file_update_handler::SinglelineFileState::Neutral {
            return;
        }

        let singleline_snapshot = self.singleline.read(cx).snapshot(cx);
        let singleline_was_focused = self.singleline.read(cx).is_focused(window, cx);
        let editor_was_focused = self.editor.read(cx).is_focused(window, cx);
        trace_debug(format!(
            "new_file_flow trigger={} state=NEUTRAL singleline='{}' singleline_focused={} editor_focused={}",
            trigger,
            compact_text(&singleline_snapshot.value),
            singleline_was_focused,
            editor_was_focused
        ));

        let now_local = Local::now();
        match self.file_workflow.try_create_from_neutral(
            &singleline_snapshot.value,
            self.app_paths.user_document_dir.as_path(),
            Instant::now(),
            now_local,
        ) {
            Ok(Some(path)) => {
                trace_debug(format!("new_file_flow created path={}", path.display()));
                self.sync_current_editing_path_to_components(Some(path.clone()), cx);
                self.apply_forced_singleline_stem(
                    crate::file_update_handler::forced_singleline_stem_after_create(
                        &singleline_snapshot.value,
                        path.as_path(),
                        now_local,
                    ),
                    "new_file_flow",
                    window,
                    cx,
                );
                self.editor.update(cx, |editor, cx| {
                    let _ = editor.open_file(path, window, cx);
                });

                if should_restore_singleline_focus_after_new_file(
                    singleline_was_focused,
                    editor_was_focused,
                ) {
                    let singleline_after = self.singleline.read(cx).snapshot(cx);
                    let restore_cursor_char = singleline_snapshot
                        .cursor_char
                        .min(singleline_after.value.chars().count());

                    trace_debug(format!(
                        "new_file_flow restore singleline focus cursor={} (rule-1)",
                        restore_cursor_char
                    ));
                    self.singleline.update(cx, |singleline, cx| {
                        singleline.apply_cursor(restore_cursor_char, window, cx);
                        singleline.focus(window, cx);
                    });
                } else {
                    trace_debug("new_file_flow no focus restore (rule-2)");
                }
            }
            Ok(None) => {
                trace_debug(format!(
                    "new_file_flow trigger={} skipped (state/throttle gate)",
                    trigger
                ));
            }
            Err(error) => {
                trace_debug(format!(
                    "new_file_flow trigger={} failed error={error}",
                    trigger
                ));
            }
        }
    }

    fn on_singleline_value_changed(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.file_workflow.state() {
            crate::file_update_handler::SinglelineFileState::Neutral => {
                self.ensure_new_file_flow("singleline_value_changed", window, cx);
            }
            crate::file_update_handler::SinglelineFileState::Edit => {
                let now_local = Local::now();
                match self.file_workflow.try_rename_in_edit(value, now_local) {
                    Ok(Some(path)) => {
                        trace_debug(format!(
                            "rename_flow success new_path={} value='{}'",
                            path.display(),
                            compact_text(value)
                        ));
                        self.sync_current_editing_path_to_components(Some(path.clone()), cx);
                        self.apply_forced_singleline_stem(
                            crate::file_update_handler::forced_singleline_stem_after_rename(
                                value,
                                path.as_path(),
                                now_local,
                            ),
                            "rename_flow",
                            window,
                            cx,
                        );
                    }
                    Ok(None) => {}
                    Err(error) => {
                        trace_debug(format!(
                            "rename_flow failed value='{}' error={error}",
                            compact_text(value)
                        ));
                    }
                }
            }
            crate::file_update_handler::SinglelineFileState::New => {}
        }
    }

    fn on_editor_user_buffer_changed(&mut self, value: &str, cx: &mut Context<Self>) {
        let snapshot = self.file_workflow.snapshot();
        let Some(current_path) = snapshot.current_edit_path.clone() else {
            trace_debug(format!(
                "autosave critical invalid path on user edit state={:?} text_len={}",
                snapshot.state,
                value.len()
            ));
            debug_assert!(
                false,
                "autosave invariant violation: current_edit_path must be present on editor user edit"
            );
            return;
        };

        if snapshot.state != crate::file_update_handler::SinglelineFileState::Edit {
            trace_debug(format!(
                "autosave critical invalid state on user edit state={:?} path={}",
                snapshot.state,
                current_path.display()
            ));
            debug_assert!(
                false,
                "autosave invariant violation: state must be EDIT on editor user edit"
            );
            return;
        }

        let editor_path = self.editor.read(cx).current_editing_file_path();
        if editor_path.as_ref() != Some(&current_path) {
            trace_debug(format!(
                "autosave critical path mismatch workflow={} editor={}",
                current_path.display(),
                editor_path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            ));
            debug_assert!(
                false,
                "autosave invariant violation: editor path and workflow path mismatch"
            );
        }

        trace_debug(format!(
            "autosave step-2 pin user edit path={} text_len={}",
            current_path.display(),
            value.len()
        ));

        self.editor_autosave.mark_user_edit(
            crate::file_update_handler::EditorAutoSavePayload {
                current_path,
                editor_text: value.to_string(),
            },
            Instant::now(),
        );
    }

    fn flush_editor_content_before_context_switch(
        &mut self,
        trigger: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let snapshot = self.file_workflow.snapshot();
        if snapshot.state != crate::file_update_handler::SinglelineFileState::Edit {
            trace_debug(format!(
                "autosave pre-switch trigger={} skipped state={:?}",
                trigger, snapshot.state
            ));
            return true;
        }

        let Some(current_path) = snapshot.current_edit_path.clone() else {
            trace_debug(format!(
                "autosave pre-switch trigger={} critical missing path state={:?}",
                trigger, snapshot.state
            ));
            debug_assert!(
                false,
                "autosave invariant violation: current_edit_path must be present for pre-switch flush"
            );
            return false;
        };

        let editor_snapshot = self.editor.read(cx).snapshot(cx);
        trace_debug(format!(
            "autosave pre-switch trigger={} raise path={} text_len={}",
            trigger,
            current_path.display(),
            editor_snapshot.value.len()
        ));

        let flush_result = self
            .file_workflow
            .flush_editor_content_in_edit(&editor_snapshot.value);
        self.editor_autosave.reset_cycle();

        match flush_result {
            Ok(true) => {
                trace_debug(format!(
                    "autosave pre-switch trigger={} consumed path={}",
                    trigger,
                    current_path.display()
                ));
                true
            }
            Ok(false) => {
                trace_debug(format!(
                    "autosave pre-switch trigger={} no-op by workflow gate path={}",
                    trigger,
                    current_path.display()
                ));
                true
            }
            Err(error) => {
                trace_debug(format!(
                    "autosave pre-switch trigger={} failed path={} error={error}",
                    trigger,
                    current_path.display()
                ));
                false
            }
        }
    }

    fn handle_plus_button(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush_editor_content_before_context_switch("req-aus6-plus", cx) {
            trace_debug("plus_button aborted (pre-switch autosave failed)");
            return;
        }

        if !self.file_workflow.transition_edit_to_neutral() {
            trace_debug("plus_button no-op (state is not EDIT)");
            return;
        }

        let previous_path = self.file_workflow.current_edit_path();
        trace_debug(format!(
            "plus_button transition EDIT -> NEUTRAL previous_path={}",
            previous_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string())
        ));
        self.sync_current_editing_path_to_components(None, cx);

        self.singleline.update(cx, |singleline, cx| {
            singleline.apply_text_and_cursor("", 0, window, cx);
            singleline.focus(window, cx);
        });

        self.editor.update(cx, |editor, cx| {
            editor.apply_text_and_cursor("", 0, 0, window, cx);
        });
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

        let Some(result) = crate::sl_editor_association::transfer_on_enter(
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
            if result.new_editor_text == editor_snapshot.value {
                editor.apply_cursor(
                    result.new_editor_cursor_line,
                    result.new_editor_cursor_char,
                    window,
                    cx,
                );
            } else {
                editor.apply_text_and_cursor(
                    result.new_editor_text.clone(),
                    result.new_editor_cursor_line,
                    result.new_editor_cursor_char,
                    window,
                    cx,
                );
            }
        });

        self.apply_focus_target(result.focus_target, window, cx);

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

    fn transfer_singleline_down(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let singleline_snapshot = self.singleline.read(cx).snapshot(cx);
        let editor_snapshot = self.editor.read(cx).snapshot(cx);

        trace_debug(format!(
            "transfer_down before sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
            compact_text(&singleline_snapshot.value),
            singleline_snapshot.cursor_char,
            compact_text(&editor_snapshot.value),
            editor_snapshot.cursor_line,
            editor_snapshot.cursor_char
        ));

        let result = crate::sl_editor_association::transfer_on_down(
            singleline_snapshot.cursor_char,
            &editor_snapshot.value,
        );

        trace_debug(format!(
            "transfer_down result ed_cursor=({}, {}) focus={:?}",
            result.new_editor_cursor_line, result.new_editor_cursor_char, result.focus_target
        ));

        self.editor.update(cx, |editor, cx| {
            editor.apply_cursor(
                result.new_editor_cursor_line,
                result.new_editor_cursor_char,
                window,
                cx,
            );
        });

        self.apply_focus_target(result.focus_target, window, cx);

        let sl_after = self.singleline.read(cx).snapshot(cx);
        let ed_after = self.editor.read(cx).snapshot(cx);
        trace_debug(format!(
            "transfer_down after sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
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

        if !crate::sl_editor_association::should_transfer_backspace(
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

        let Some(result) = crate::sl_editor_association::transfer_on_backspace(
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

        self.apply_focus_target(result.focus_target, window, cx);

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

    fn transfer_editor_up(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let editor_snapshot = self.editor.read(cx).snapshot(cx);
        let singleline_snapshot = self.singleline.read(cx).snapshot(cx);

        trace_debug(format!(
            "transfer_up before ed='{}' ed_cursor=({}, {}) sl='{}' sl_cursor={}",
            compact_text(&editor_snapshot.value),
            editor_snapshot.cursor_line,
            editor_snapshot.cursor_char,
            compact_text(&singleline_snapshot.value),
            singleline_snapshot.cursor_char
        ));

        let Some(result) = crate::sl_editor_association::transfer_on_up(
            editor_snapshot.cursor_line,
            editor_snapshot.cursor_char,
            &singleline_snapshot.value,
        ) else {
            trace_debug("transfer_up skipped (editor cursor not on line-1)");
            return;
        };

        trace_debug(format!(
            "transfer_up result sl_cursor={} focus={:?}",
            result.new_singleline_cursor_char, result.focus_target
        ));

        self.singleline.update(cx, |singleline, cx| {
            singleline.apply_cursor(result.new_singleline_cursor_char, window, cx);
        });

        self.apply_focus_target(result.focus_target, window, cx);

        let sl_after = self.singleline.read(cx).snapshot(cx);
        let ed_after = self.editor.read(cx).snapshot(cx);
        trace_debug(format!(
            "transfer_up after sl='{}' sl_cursor={} ed='{}' ed_cursor=({}, {})",
            compact_text(&sl_after.value),
            sl_after.cursor_char,
            compact_text(&ed_after.value),
            ed_after.cursor_line,
            ed_after.cursor_char
        ));
    }

    fn open_file(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if !self.flush_editor_content_before_context_switch("req-aus8-open-file", cx) {
            trace_debug(format!(
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
            trace_debug(format!("open_file failed path={}", path.display()));
            return;
        }

        self.file_workflow.set_edit_from_open_file(path.clone());
        self.sync_current_editing_path_to_components(Some(path), cx);
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
        DEFAULT_SPLIT_LEFT_PANEL_SIZE_PX, SPLITTER_PERSISTENCE_FALLBACK_RIGHT_PANEL_SIZE_PX,
        build_startup_window_options, persisted_splitter_sizes, should_recreate_layout_split_state,
        should_restore_singleline_focus_after_new_file,
    };
    use crate::file_update_handler::EditorAutoSaveCoordinator;
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
                "path_resolver resolved mode={:?} reason={} app_home={} conf={} data={} user_document={} log={} bin={} config_file={} app_log_file={}",
                paths.mode,
                paths.mode.reason(),
                paths.app_home.display(),
                paths.conf_dir.display(),
                paths.data_dir.display(),
                paths.user_document_dir.display(),
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

    let app = Application::new().with_assets(Assets);

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
