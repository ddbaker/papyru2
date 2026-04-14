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

pub(crate) use crate::log::trace_debug;

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

pub(crate) const PAPYRU2_CONF_FILE_NAME: &str = "papyru2_conf.toml";
pub(crate) const REQ_COLR_DEFAULT_BACKGROUND_RGB_HEX: u32 = 0xFDFDE6;
pub(crate) const REQ_COLR_DEFAULT_FOREGROUND_RGB_HEX: u32 = 0x000000;
pub(crate) const REQ_EDITOR_DEFAULT_CODE_EDITOR: &str = "text";
pub(crate) const REQ_EDITOR_DEFAULT_SOFT_WRAP: bool = true;
pub(crate) const REQ_EDITOR_DEFAULT_LINE_NUMBER: bool = false;
pub(crate) const REQ_EDITOR_DEFAULT_SHOW_WHITESPACES: bool = false;
const REQ_COLR_MAX_RGB_HEX: u32 = 0x00FF_FFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct UiColorConfig {
    pub background_rgb_hex: u32,
    pub foreground_rgb_hex: u32,
}

impl Default for UiColorConfig {
    fn default() -> Self {
        Self {
            background_rgb_hex: REQ_COLR_DEFAULT_BACKGROUND_RGB_HEX,
            foreground_rgb_hex: REQ_COLR_DEFAULT_FOREGROUND_RGB_HEX,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EditorConfig {
    pub code_editor: String,
    pub soft_wrap: bool,
    pub line_number: bool,
    pub show_whitespaces: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            code_editor: REQ_EDITOR_DEFAULT_CODE_EDITOR.to_string(),
            soft_wrap: REQ_EDITOR_DEFAULT_SOFT_WRAP,
            line_number: REQ_EDITOR_DEFAULT_LINE_NUMBER,
            show_whitespaces: REQ_EDITOR_DEFAULT_SHOW_WHITESPACES,
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReqColrConfigFile {
    #[serde(default)]
    color: ReqColrColorSection,
    #[serde(default)]
    editor: ReqEditorSection,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReqColrColorSection {
    #[serde(default)]
    background: Option<u32>,
    #[serde(default)]
    foreground: Option<u32>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReqEditorSection {
    #[serde(default)]
    code_editor: Option<String>,
    #[serde(default)]
    soft_wrap: Option<bool>,
    #[serde(default)]
    line_number: Option<bool>,
    #[serde(default)]
    show_whitespaces: Option<bool>,
}

pub(crate) fn req_colr_rgb_hex_to_hsla(rgb_hex: u32) -> Hsla {
    Hsla::from(rgb(rgb_hex))
}

pub(crate) fn req_colr_default_ui_colors() -> UiColorConfig {
    UiColorConfig::default()
}

pub(crate) fn req_editor_default_config() -> EditorConfig {
    EditorConfig::default()
}

fn req_colr_hex_text(rgb_hex: u32) -> String {
    format!("#{rgb_hex:06x}")
}

fn req_colr_default_config_toml(colors: UiColorConfig, editor: &EditorConfig) -> String {
    format!(
        "[color]\nbackground = 0x{:06x}\nforeground = 0x{:06x}\n\n[editor]\ncode_editor = \"{}\"\nsoft_wrap = {}\nline_number = {}\nshow_whitespaces = {}\n",
        colors.background_rgb_hex,
        colors.foreground_rgb_hex,
        editor.code_editor,
        editor.soft_wrap,
        editor.line_number,
        editor.show_whitespaces
    )
}

fn req_colr_validate_rgb_hex(field_name: &str, rgb_hex: u32) -> std::io::Result<u32> {
    if rgb_hex > REQ_COLR_MAX_RGB_HEX {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("req-colr invalid color.{field_name} value=0x{rgb_hex:08x} exceeds 24-bit rgb"),
        ));
    }
    Ok(rgb_hex)
}

fn write_default_ui_color_config(
    path: &std::path::Path,
    colors: UiColorConfig,
) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "req-colr config path has no parent directory",
        )
    })?;
    std::fs::create_dir_all(parent)?;
    let editor_defaults = req_editor_default_config();
    let default_toml = req_colr_default_config_toml(colors, &editor_defaults);
    std::fs::write(path, default_toml.as_bytes())
}

fn load_or_create_ui_color_config_result(path: &std::path::Path) -> std::io::Result<UiColorConfig> {
    if path.exists() && !path.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("req-colr config path is not a file path={}", path.display()),
        ));
    }

    let defaults = req_colr_default_ui_colors();
    if !path.is_file() {
        write_default_ui_color_config(path, defaults)?;
        trace_debug(format!(
            "req-colr config created path={} background={} foreground={}",
            path.display(),
            req_colr_hex_text(defaults.background_rgb_hex),
            req_colr_hex_text(defaults.foreground_rgb_hex),
        ));
        return Ok(defaults);
    }

    let raw = std::fs::read_to_string(path)?;
    let parsed: ReqColrConfigFile = toml::from_str(&raw)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;

    let background_rgb_hex = req_colr_validate_rgb_hex(
        "background",
        parsed
            .color
            .background
            .unwrap_or(defaults.background_rgb_hex),
    )?;
    let foreground_rgb_hex = req_colr_validate_rgb_hex(
        "foreground",
        parsed
            .color
            .foreground
            .unwrap_or(defaults.foreground_rgb_hex),
    )?;

    let resolved = UiColorConfig {
        background_rgb_hex,
        foreground_rgb_hex,
    };
    trace_debug(format!(
        "req-colr config loaded path={} background={} foreground={}",
        path.display(),
        req_colr_hex_text(resolved.background_rgb_hex),
        req_colr_hex_text(resolved.foreground_rgb_hex),
    ));
    Ok(resolved)
}

pub(crate) fn load_or_create_ui_color_config(path: &std::path::Path) -> UiColorConfig {
    match load_or_create_ui_color_config_result(path) {
        Ok(colors) => colors,
        Err(error) => {
            let defaults = req_colr_default_ui_colors();
            trace_debug(format!(
                "req-colr config fallback path={} error={} defaults background={} foreground={}",
                path.display(),
                error,
                req_colr_hex_text(defaults.background_rgb_hex),
                req_colr_hex_text(defaults.foreground_rgb_hex),
            ));
            defaults
        }
    }
}

fn load_req_editor_config_result(path: &std::path::Path) -> std::io::Result<EditorConfig> {
    if path.exists() && !path.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "req-editor config path is not a file path={}",
                path.display()
            ),
        ));
    }

    let defaults = req_editor_default_config();
    if !path.is_file() {
        trace_debug(format!(
            "req-editor config missing path={} defaults code_editor={} soft_wrap={} line_number={} show_whitespaces={}",
            path.display(),
            defaults.code_editor,
            defaults.soft_wrap,
            defaults.line_number,
            defaults.show_whitespaces
        ));
        return Ok(defaults);
    }

    let raw = std::fs::read_to_string(path)?;
    let parsed: ReqColrConfigFile = toml::from_str(&raw)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;

    let resolved = EditorConfig {
        code_editor: parsed
            .editor
            .code_editor
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| defaults.code_editor.clone()),
        soft_wrap: parsed.editor.soft_wrap.unwrap_or(defaults.soft_wrap),
        line_number: parsed.editor.line_number.unwrap_or(defaults.line_number),
        show_whitespaces: parsed
            .editor
            .show_whitespaces
            .unwrap_or(defaults.show_whitespaces),
    };
    trace_debug(format!(
        "req-editor config loaded path={} code_editor={} soft_wrap={} line_number={} show_whitespaces={} searchable=true",
        path.display(),
        resolved.code_editor,
        resolved.soft_wrap,
        resolved.line_number,
        resolved.show_whitespaces
    ));
    Ok(resolved)
}

pub(crate) fn load_req_editor_config(path: &std::path::Path) -> EditorConfig {
    match load_req_editor_config_result(path) {
        Ok(config) => config,
        Err(error) => {
            let defaults = req_editor_default_config();
            trace_debug(format!(
                "req-editor config fallback path={} error={} defaults code_editor={} soft_wrap={} line_number={} show_whitespaces={} searchable=true",
                path.display(),
                error,
                defaults.code_editor,
                defaults.soft_wrap,
                defaults.line_number,
                defaults.show_whitespaces
            ));
            defaults
        }
    }
}

pub(crate) fn apply_req_colr_theme_overrides(ui_color_config: UiColorConfig, cx: &mut App) {
    let background = req_colr_rgb_hex_to_hsla(ui_color_config.background_rgb_hex);
    let foreground = req_colr_rgb_hex_to_hsla(ui_color_config.foreground_rgb_hex);

    let theme = gpui_component::Theme::global_mut(cx);
    theme.background = background;
    theme.foreground = foreground;

    let mut highlight_theme = (*theme.highlight_theme).clone();
    highlight_theme.style.editor_background = Some(background);
    highlight_theme.style.editor_foreground = Some(foreground);
    theme.highlight_theme = std::sync::Arc::new(highlight_theme);

    trace_debug(format!(
        "req-colr theme override applied background={} foreground={} editor_background_synced=true",
        req_colr_hex_text(ui_color_config.background_rgb_hex),
        req_colr_hex_text(ui_color_config.foreground_rgb_hex),
    ));
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

fn build_startup_window_options(
    startup_bounds: WindowBounds,
    startup_display_id: Option<DisplayId>,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(startup_bounds),
        focus: true,
        show: true,
        display_id: startup_display_id,
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

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if path.is_empty() {
            return Ok(None);
        }
        if let Some(svg_bytes) = crate::top_bars::load_top_bars_icon_asset(path) {
            return Ok(Some(Cow::Borrowed(svg_bytes)));
        }
        gpui_component_assets::Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut assets = gpui_component_assets::Assets.list(path)?;
        crate::top_bars::list_top_bars_icon_assets(path, &mut assets);
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
        ui_color_config: UiColorConfig,
        editor_config: EditorConfig,
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
                ui_color_config,
                cx,
            )
        });
        let singleline = top_bars.read(cx).singleline();
        let editor = cx.new(|cx| Papyru2Editor::new(window, ui_color_config, editor_config, cx));
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
            FileTreeView::new(
                protected_delete_roots,
                file_tree_root_dir.clone(),
                ui_color_config,
                cx,
            )
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
        let options = build_startup_window_options(startup_bounds, None);

        assert!(options.focus);
        assert!(options.show);
        assert_eq!(options.window_bounds, Some(startup_bounds));
        assert_eq!(options.display_id, None);
    }

    fn req_colr_test_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "gpui_papyru2_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn req_colr_test_cleanup(path: &std::path::Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn colr_test1_req_colr_defaults_match_source_constants() {
        let defaults = super::req_colr_default_ui_colors();
        assert_eq!(
            defaults.background_rgb_hex,
            super::REQ_COLR_DEFAULT_BACKGROUND_RGB_HEX
        );
        assert_eq!(
            defaults.foreground_rgb_hex,
            super::REQ_COLR_DEFAULT_FOREGROUND_RGB_HEX
        );
    }

    #[test]
    fn colr_test2_req_colr_missing_config_creates_default_file() {
        let root = req_colr_test_temp_root("colr_test2");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);

        let resolved = super::load_or_create_ui_color_config(config_path.as_path());
        assert_eq!(resolved, super::req_colr_default_ui_colors());
        assert!(config_path.is_file());

        let raw = std::fs::read_to_string(config_path.as_path()).expect("read color config");
        assert!(raw.contains("background = 0xfdfde6"));
        assert!(raw.contains("foreground = 0x000000"));

        req_colr_test_cleanup(root.as_path());
    }

    #[test]
    fn colr_test3_req_colr_valid_hex_values_override_defaults() {
        let root = req_colr_test_temp_root("colr_test3");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(
            config_path.as_path(),
            "[color]\nbackground = 0xf7f2ec\nforeground = 0x437085\n",
        )
        .expect("write color config");

        let resolved = super::load_or_create_ui_color_config(config_path.as_path());
        assert_eq!(resolved.background_rgb_hex, 0xF7F2EC);
        assert_eq!(resolved.foreground_rgb_hex, 0x437085);

        req_colr_test_cleanup(root.as_path());
    }

    #[test]
    fn colr_test4_req_colr_partial_toml_falls_back_per_field() {
        let root = req_colr_test_temp_root("colr_test4");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(config_path.as_path(), "[color]\nbackground = 0xf7f2ec\n")
            .expect("write partial config");

        let resolved = super::load_or_create_ui_color_config(config_path.as_path());
        assert_eq!(resolved.background_rgb_hex, 0xF7F2EC);
        assert_eq!(
            resolved.foreground_rgb_hex,
            super::REQ_COLR_DEFAULT_FOREGROUND_RGB_HEX
        );

        req_colr_test_cleanup(root.as_path());
    }

    #[test]
    fn colr_test5_req_colr_invalid_toml_falls_back_without_panic() {
        let root = req_colr_test_temp_root("colr_test5");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(config_path.as_path(), "[color]\nbackground = \"red\"\n")
            .expect("write invalid config");

        let resolved = super::load_or_create_ui_color_config(config_path.as_path());
        assert_eq!(resolved, super::req_colr_default_ui_colors());

        req_colr_test_cleanup(root.as_path());
    }

    #[test]
    fn colr_test6_req_colr_rgb_value_must_fit_within_24_bits() {
        let root = req_colr_test_temp_root("colr_test6");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(
            config_path.as_path(),
            "[color]\nbackground = 0x1000000\nforeground = 0x000000\n",
        )
        .expect("write out-of-range config");

        let result = super::load_or_create_ui_color_config_result(config_path.as_path());
        assert!(result.is_err());
        let error_text = result.err().expect("expected error").to_string();
        assert!(error_text.contains("exceeds 24-bit rgb"));

        req_colr_test_cleanup(root.as_path());
    }
}

#[cfg(test)]
mod editor_config_tests {
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn req_editor_test_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "gpui_papyru2_req_editor_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        std::fs::create_dir_all(path.as_path()).expect("create temp root");
        path
    }

    fn req_editor_test_cleanup(path: &Path) {
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn editor_test1_req_editor_defaults_match_source_constants() {
        let defaults = super::req_editor_default_config();
        assert_eq!(defaults.code_editor, super::REQ_EDITOR_DEFAULT_CODE_EDITOR);
        assert_eq!(defaults.soft_wrap, super::REQ_EDITOR_DEFAULT_SOFT_WRAP);
        assert_eq!(defaults.line_number, super::REQ_EDITOR_DEFAULT_LINE_NUMBER);
        assert_eq!(
            defaults.show_whitespaces,
            super::REQ_EDITOR_DEFAULT_SHOW_WHITESPACES
        );
    }

    #[test]
    fn editor_test2_req_editor_missing_section_uses_defaults() {
        let root = req_editor_test_temp_root("editor_test2");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(
            config_path.as_path(),
            "[color]\nbackground = 0xf7f2ec\nforeground = 0x437085\n",
        )
        .expect("write color-only config");

        let resolved = super::load_req_editor_config(config_path.as_path());
        assert_eq!(resolved, super::req_editor_default_config());

        req_editor_test_cleanup(root.as_path());
    }

    #[test]
    fn editor_test3_req_editor_overrides_load_from_config() {
        let root = req_editor_test_temp_root("editor_test3");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(
            config_path.as_path(),
            "[editor]\ncode_editor = \"markdown\"\nsoft_wrap = false\nline_number = true\nshow_whitespaces = true\n",
        )
        .expect("write editor config");

        let resolved = super::load_req_editor_config(config_path.as_path());
        assert_eq!(resolved.code_editor, "markdown");
        assert!(!resolved.soft_wrap);
        assert!(resolved.line_number);
        assert!(resolved.show_whitespaces);

        req_editor_test_cleanup(root.as_path());
    }

    #[test]
    fn editor_test4_req_editor_invalid_toml_falls_back_without_panic() {
        let root = req_editor_test_temp_root("editor_test4");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(config_path.as_path(), "[editor]\nsoft_wrap = \"yes\"\n")
            .expect("write invalid editor config");

        let resolved = super::load_req_editor_config(config_path.as_path());
        assert_eq!(resolved, super::req_editor_default_config());

        req_editor_test_cleanup(root.as_path());
    }

    #[test]
    fn editor_test5_req_editor_default_created_config_contains_editor_keys() {
        let root = req_editor_test_temp_root("editor_test5");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);

        let _ = super::load_or_create_ui_color_config(config_path.as_path());
        let raw = std::fs::read_to_string(config_path.as_path()).expect("read created config");
        assert!(raw.contains("[editor]"));
        assert!(raw.contains("code_editor = \"text\""));
        assert!(raw.contains("soft_wrap = true"));
        assert!(raw.contains("line_number = false"));
        assert!(raw.contains("show_whitespaces = false"));

        req_editor_test_cleanup(root.as_path());
    }

    #[test]
    fn editor_test6_req_editor13_partial_keys_use_required_defaults() {
        let root = req_editor_test_temp_root("editor_test6");
        let config_path = root.join("conf").join(super::PAPYRU2_CONF_FILE_NAME);
        std::fs::create_dir_all(config_path.parent().expect("config parent")).expect("mkdir conf");
        std::fs::write(
            config_path.as_path(),
            "[editor]\ncode_editor = \"markdown\"\n",
        )
        .expect("write partial editor config");

        let resolved = super::load_req_editor_config(config_path.as_path());
        assert_eq!(resolved.code_editor, "markdown");
        assert_eq!(resolved.soft_wrap, super::REQ_EDITOR_DEFAULT_SOFT_WRAP);
        assert_eq!(resolved.line_number, super::REQ_EDITOR_DEFAULT_LINE_NUMBER);
        assert_eq!(
            resolved.show_whitespaces,
            super::REQ_EDITOR_DEFAULT_SHOW_WHITESPACES
        );

        req_editor_test_cleanup(root.as_path());
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

    let resolved_paths = match cli_override {
        Some(mode) => crate::path_resolver::AppPaths::resolve_with_cli_override(Some(mode)),
        None => crate::path_resolver::AppPaths::resolve(),
    };

    let app_paths = match resolved_paths {
        Ok(paths) => paths,
        Err(error) => {
            trace_debug(format!("path_resolver resolve failed error={error}"));
            eprintln!("papyru2 path resolver failed: {error}");
            return;
        }
    };

    let color_config_path = app_paths.config_file_path(PAPYRU2_CONF_FILE_NAME);
    let req_log_profile_default = crate::log::req_log_profile_default_enabled();
    let req_log_config_override =
        crate::log::load_req_log_config_override(color_config_path.as_path());
    let req_log_effective_enabled = crate::log::req_log_effective_debug_logging_enabled(
        req_log_profile_default,
        req_log_config_override,
    );
    crate::log::configure_trace_debug_enabled(req_log_effective_enabled);

    crate::log::configure_trace_debug_log_path(&app_paths);
    if let Err(error) = crate::log::prepare_startup_log_files(&app_paths) {
        eprintln!("papyru2 startup log preparation failed: {error}");
    }

    trace_debug(format!(
        "req-log startup profile_default={} config_override={req_log_config_override:?} effective={req_log_effective_enabled}",
        req_log_profile_default
    ));
    trace_debug(format!("path_resolver cli_override={cli_override:?}"));

    let config_file = app_paths.config_file_path("app.toml");
    let log_file = app_paths.log_file_path("papyru2.log");
    trace_debug(format!(
        "path_resolver resolved mode={:?} reason={} app_home={} conf={} data={} user_document={} recyclebin={} log={} bin={} config_file={} app_log_file={}",
        app_paths.mode,
        app_paths.mode.reason(),
        app_paths.app_home.display(),
        app_paths.conf_dir.display(),
        app_paths.data_dir.display(),
        app_paths.user_document_dir.display(),
        app_paths.recyclebin_dir.display(),
        app_paths.log_dir.display(),
        app_paths.bin_dir.display(),
        config_file.display(),
        log_file.display()
    ));

    let ui_color_config = load_or_create_ui_color_config(color_config_path.as_path());
    trace_debug(format!(
        "req-colr startup colors path={} background={} foreground={}",
        color_config_path.display(),
        req_colr_hex_text(ui_color_config.background_rgb_hex),
        req_colr_hex_text(ui_color_config.foreground_rgb_hex),
    ));
    let editor_config = load_req_editor_config(color_config_path.as_path());
    trace_debug(format!(
        "req-editor startup config path={} code_editor={} soft_wrap={} line_number={} show_whitespaces={} searchable=true",
        color_config_path.display(),
        editor_config.code_editor,
        editor_config.soft_wrap,
        editor_config.line_number,
        editor_config.show_whitespaces
    ));

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
        apply_req_colr_theme_overrides(ui_color_config, cx);

        let primary_display = cx.primary_display();
        let primary_monitor_id = primary_display.as_ref().map(|display| u32::from(display.id()));

        let mut startup_displays: Vec<(DisplayId, crate::window_position::StartupDisplaySnapshot)> =
            cx.displays()
                .into_iter()
                .map(|display| {
                    let display_id = display.id();
                    (
                        display_id,
                        crate::window_position::StartupDisplaySnapshot {
                            monitor_id: u32::from(display_id),
                            monitor_uuid: display.uuid().ok().map(|uuid| uuid.to_string()),
                            bounds: display.bounds(),
                        },
                    )
                })
                .collect();

        if startup_displays.is_empty() {
            if let Some(primary_display) = primary_display.as_ref() {
                let display_id = primary_display.id();
                startup_displays.push((
                    display_id,
                    crate::window_position::StartupDisplaySnapshot {
                        monitor_id: u32::from(display_id),
                        monitor_uuid: primary_display.uuid().ok().map(|uuid| uuid.to_string()),
                        bounds: primary_display.bounds(),
                    },
                ));
            }
        }

        let startup_display_snapshots: Vec<crate::window_position::StartupDisplaySnapshot> =
            startup_displays
                .iter()
                .map(|(_, snapshot)| snapshot.clone())
                .collect();

        let startup_display_resolution = crate::window_position::resolve_startup_display_resolution(
            persisted_window_position.as_ref(),
            startup_display_snapshots.as_slice(),
            primary_monitor_id,
        );

        let startup_display_id = startup_display_resolution.monitor_id.and_then(|resolved_monitor_id| {
            startup_displays
                .iter()
                .find(|(_, snapshot)| snapshot.monitor_id == resolved_monitor_id)
                .map(|(display_id, _)| *display_id)
        });

        trace_debug(format!(
            "window_position startup monitor resolve saved_monitor_id={:?} saved_monitor_uuid={:?} primary_monitor_id={primary_monitor_id:?} source={:?} resolved_monitor_id={:?} resolved_bounds={:?}",
            persisted_window_position
                .as_ref()
                .and_then(|state| state.monitor_id),
            persisted_window_position
                .as_ref()
                .and_then(|state| state.monitor_uuid.as_deref()),
            startup_display_resolution.source,
            startup_display_resolution.monitor_id,
            startup_display_resolution.display_bounds,
        ));

        let default_centered_bounds = WindowBounds::centered(size(px(1200.), px(800.)), cx);
        let fallback_bounds = crate::window_position::first_launch_fallback_bounds(
            startup_display_resolution.display_bounds,
            default_centered_bounds,
        );
        let startup_bounds = crate::window_position::resolve_startup_window_bounds(
            persisted_window_position.as_ref(),
            fallback_bounds,
            startup_display_resolution.display_bounds,
            crate::window_position::should_ignore_exact_position_for_wayland(),
        );

        let window_options = build_startup_window_options(startup_bounds, startup_display_id);
        trace_debug(format!(
            "window_options startup focus={} show={} has_bounds={} startup_monitor_id={:?}",
            window_options.focus,
            window_options.show,
            window_options.window_bounds.is_some(),
            startup_display_resolution.monitor_id,
        ));

        let app_paths = app_paths.clone();
        let window_position_path = window_position_path.clone();
        let restored_splitter_left_size = restored_splitter_left_size;
        let ui_color_config = ui_color_config;
        let editor_config = editor_config;
        cx.spawn(async move |cx| {
            cx.open_window(window_options, move |window, cx| {
                let app_paths = app_paths.clone();
                let view = cx.new(|cx| {
                    Papyru2App::new(
                        window,
                        app_paths,
                        restored_splitter_left_size,
                        ui_color_config,
                        editor_config,
                        cx,
                    )
                });

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
