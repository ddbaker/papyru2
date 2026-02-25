use std::{
    cell::RefCell,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

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
    _app_paths: crate::path_resolver::AppPaths,
}

impl Papyru2App {
    fn new(
        window: &mut Window,
        app_paths: crate::path_resolver::AppPaths,
        cx: &mut Context<Self>,
    ) -> Self {
        let layout_split_state = cx.new(|_| ResizableState::default());
        let top_bars = cx.new(|cx| TopBars::new(window, layout_split_state.clone(), cx));
        let singleline = top_bars.read(cx).singleline();
        let editor = cx.new(|cx| Papyru2Editor::new(window, cx));
        let file_tree = cx.new(|cx| FileTreeView::new(cx));

        let window_position_path =
            app_paths.config_file_path(crate::window_position::WINDOW_POSITION_FILE_NAME);
        let last_debounced_save = Rc::new(RefCell::new(None::<Instant>));
        let debounced_save_clock = last_debounced_save.clone();
        let debounced_save_path = window_position_path.clone();

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
                            this.transfer_singleline_down(window, cx);
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
                },
            ),
        ];

        subscriptions.push(cx.observe_window_bounds(window, move |_, window, _cx| {
            let now = Instant::now();
            let should_save = debounced_save_clock
                .borrow()
                .map(|last_save| now.duration_since(last_save) >= Duration::from_secs(1))
                .unwrap_or(true);
            if !should_save {
                return;
            }

            *debounced_save_clock.borrow_mut() = Some(now);
            let state = crate::window_position::WindowPositionState::from_window_bounds(
                window.window_bounds(),
                None,
                None,
                Some(window.scale_factor()),
            );
            if let Err(error) =
                crate::window_position::save_window_position_atomic(&debounced_save_path, &state)
            {
                trace_debug(format!("window_position debounced save failed error={error}"));
            }
        }));

        Self {
            top_bars,
            singleline,
            editor,
            file_tree,
            layout_split_state,
            _subscriptions: subscriptions,
            _app_paths: app_paths,
        }
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
                        .child(
                            resizable_panel()
                                .size(px(320.))
                                .child(self.file_tree.clone()),
                        )
                        .child(resizable_panel().child(self.editor.clone())),
                ),
            )
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
                "path_resolver resolved mode={:?} reason={} app_home={} conf={} data={} log={} bin={} config_file={} app_log_file={}",
                paths.mode,
                paths.mode.reason(),
                paths.app_home.display(),
                paths.conf_dir.display(),
                paths.data_dir.display(),
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
    let persisted_window_position = match crate::window_position::load_window_position(&window_position_path)
    {
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

    let app = Application::new().with_assets(Assets);

    app.run(move |cx| {
        gpui_component::init(cx);

        let fallback_bounds = WindowBounds::centered(size(px(1200.), px(800.)), cx);
        let startup_bounds = crate::window_position::resolve_startup_window_bounds(
            persisted_window_position.as_ref(),
            fallback_bounds,
            cx.primary_display().map(|display| display.bounds()),
            crate::window_position::should_ignore_exact_position_for_wayland(),
        );

        let window_options = WindowOptions {
            window_bounds: Some(startup_bounds),
            ..Default::default()
        };

        let app_paths = app_paths.clone();
        let window_position_path = window_position_path.clone();
        cx.spawn(async move |cx| {
            cx.open_window(window_options, move |window, cx| {
                let close_save_path = window_position_path.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    let state = crate::window_position::WindowPositionState::from_window(window, cx);
                    if let Err(error) =
                        crate::window_position::save_window_position_atomic(&close_save_path, &state)
                    {
                        trace_debug(format!(
                            "window_position close save failed path={} error={error}",
                            close_save_path.display()
                        ));
                    }
                    true
                });

                let app_paths = app_paths.clone();
                let view = cx.new(|cx| Papyru2App::new(window, app_paths, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })?;

            Ok::<_, anyhow::Error>(())
        })
        .detach();
    });
}
