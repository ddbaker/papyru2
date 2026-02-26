use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use gpui::{App, Bounds, Pixels, Window, WindowBounds, bounds, point, px, size};
use serde::{Deserialize, Serialize};

pub const WINDOW_POSITION_FILE_NAME: &str = "window_position.toml";
pub const FIRST_LAUNCH_DISPLAY_RATIO: f32 = 0.7;
const MIN_WINDOW_DIMENSION: f32 = 120.0;
const MAX_ABS_COORDINATE: f32 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PersistedWindowMode {
    Windowed,
    Maximized,
    Fullscreen,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowPositionState {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub window_mode: PersistedWindowMode,
    pub monitor_id: Option<u32>,
    pub monitor_uuid: Option<String>,
    pub dpi_scale: Option<f32>,
}

impl WindowPositionState {
    pub fn from_window(window: &Window, cx: &App) -> Self {
        let display = window.display(cx);
        let monitor_id = display.as_ref().map(|display| u32::from(display.id()));
        let monitor_uuid = display
            .as_ref()
            .and_then(|display| display.uuid().ok())
            .map(|uuid| uuid.to_string());

        Self::from_window_bounds(
            window.window_bounds(),
            monitor_id,
            monitor_uuid,
            Some(window.scale_factor()),
        )
    }

    pub fn from_window_bounds(
        window_bounds: WindowBounds,
        monitor_id: Option<u32>,
        monitor_uuid: Option<String>,
        dpi_scale: Option<f32>,
    ) -> Self {
        let restore_bounds = window_bounds.get_bounds();
        Self {
            x: f32::from(restore_bounds.origin.x),
            y: f32::from(restore_bounds.origin.y),
            width: f32::from(restore_bounds.size.width),
            height: f32::from(restore_bounds.size.height),
            window_mode: mode_from_window_bounds(window_bounds),
            monitor_id,
            monitor_uuid,
            dpi_scale,
        }
    }

    pub fn to_window_bounds(&self) -> Option<WindowBounds> {
        if !is_valid_coordinate(self.x)
            || !is_valid_coordinate(self.y)
            || !is_valid_dimension(self.width)
            || !is_valid_dimension(self.height)
        {
            return None;
        }

        let restore_bounds = bounds(
            point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        );
        Some(window_bounds_from_parts(self.window_mode, restore_bounds))
    }
}

pub fn load_window_position(path: &Path) -> io::Result<Option<WindowPositionState>> {
    if !path.is_file() {
        return Ok(None);
    }

    let raw = fs::read_to_string(path)?;
    let state: WindowPositionState = toml::from_str(&raw)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    Ok(Some(state))
}

pub fn save_window_position_atomic(path: &Path, state: &WindowPositionState) -> io::Result<()> {
    let serialized = toml::to_string_pretty(state)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    write_atomic(path, serialized.as_bytes())
}

pub fn resolve_startup_window_bounds(
    persisted: Option<&WindowPositionState>,
    fallback: WindowBounds,
    display_bounds: Option<Bounds<Pixels>>,
    ignore_exact_position: bool,
) -> WindowBounds {
    let Some(persisted) = persisted else {
        return fallback;
    };

    let Some(raw_bounds) = persisted.to_window_bounds() else {
        return fallback;
    };

    sanitize_window_bounds(raw_bounds, fallback, display_bounds, ignore_exact_position)
}

pub fn first_launch_fallback_bounds(
    primary_display_bounds: Option<Bounds<Pixels>>,
    default_centered_bounds: WindowBounds,
) -> WindowBounds {
    let Some(display_bounds) = primary_display_bounds else {
        return default_centered_bounds;
    };

    let display_x = f32::from(display_bounds.origin.x);
    let display_y = f32::from(display_bounds.origin.y);
    let display_w = f32::from(display_bounds.size.width).max(MIN_WINDOW_DIMENSION);
    let display_h = f32::from(display_bounds.size.height).max(MIN_WINDOW_DIMENSION);

    let width = (display_w * FIRST_LAUNCH_DISPLAY_RATIO)
        .max(MIN_WINDOW_DIMENSION)
        .min(display_w);
    let height = (display_h * FIRST_LAUNCH_DISPLAY_RATIO)
        .max(MIN_WINDOW_DIMENSION)
        .min(display_h);

    let x = display_x + ((display_w - width) / 2.0);
    let y = display_y + ((display_h - height) / 2.0);

    WindowBounds::Windowed(bounds(point(px(x), px(y)), size(px(width), px(height))))
}

pub fn should_ignore_exact_position_for_wayland() -> bool {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            return true;
        }
        std::env::var("XDG_SESSION_TYPE")
            .map(|value| value.eq_ignore_ascii_case("wayland"))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic_with_replace(path, bytes, replace_target_with_temp)
}

fn write_atomic_with_replace<F>(path: &Path, bytes: &[u8], replace_fn: F) -> io::Result<()>
where
    F: Fn(&Path, &Path) -> io::Result<()>,
{
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "window position path has no parent directory",
        )
    })?;
    fs::create_dir_all(parent)?;

    let temp_path = temp_path_for_atomic_write(path)?;
    if temp_path.is_file() {
        fs::remove_file(&temp_path)?;
    }
    let mut temp_file = fs::File::create(&temp_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("atomic write stage failed (create temp): {error}"),
        )
    })?;
    std::io::Write::write_all(&mut temp_file, bytes).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("atomic write stage failed (write temp): {error}"),
        )
    })?;
    temp_file.sync_all().map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("atomic write stage failed (sync temp): {error}"),
        )
    })?;
    drop(temp_file);

    if let Err(replace_error) = replace_fn(&temp_path, path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("atomic write stage failed (replace target): {error}"),
        )
    }) {
        if let Err(cleanup_error) = cleanup_temp_file(&temp_path) {
            return Err(io::Error::new(
                replace_error.kind(),
                format!(
                    "{replace_error}; cleanup temp failed: {cleanup_error}"
                ),
            ));
        }

        return Err(replace_error);
    }

    Ok(())
}

fn cleanup_temp_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn replace_target_with_temp(temp_path: &Path, target_path: &Path) -> io::Result<()> {
    // Safety invariant: never delete the existing target before a replacement operation succeeds.
    // On replace failure, caller keeps the last-good target file intact.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        use std::ptr::{null, null_mut};

        use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

        if !target_path.exists() {
            return fs::rename(temp_path, target_path);
        }

        let mut target_wide = target_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>();
        let mut temp_wide = temp_path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<u16>>();

        let result = unsafe {
            ReplaceFileW(
                target_wide.as_mut_ptr(),
                temp_wide.as_mut_ptr(),
                null(),
                0,
                null_mut(),
                null_mut(),
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        return Ok(());
    }

    #[cfg(not(target_os = "windows"))]
    {
        fs::rename(temp_path, target_path)
    }
}

fn temp_path_for_atomic_write(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "window position path has no parent directory",
        )
    })?;
    let file_name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "window position path has no file name",
        )
    })?;
    Ok(parent.join(format!("{}.tmp", file_name.to_string_lossy())))
}

fn sanitize_window_bounds(
    raw_bounds: WindowBounds,
    fallback: WindowBounds,
    display_bounds: Option<Bounds<Pixels>>,
    ignore_exact_position: bool,
) -> WindowBounds {
    let mode = mode_from_window_bounds(raw_bounds);
    let mut restore = raw_bounds.get_bounds();

    if ignore_exact_position && matches!(mode, PersistedWindowMode::Windowed) {
        restore.origin = fallback.get_bounds().origin;
    }

    if let Some(display) = display_bounds {
        if !intersects(restore, display) {
            return fallback;
        }

        let display_x = f32::from(display.origin.x);
        let display_y = f32::from(display.origin.y);
        let display_w = f32::from(display.size.width);
        let display_h = f32::from(display.size.height);

        let mut x = f32::from(restore.origin.x);
        let mut y = f32::from(restore.origin.y);
        let mut width = f32::from(restore.size.width);
        let mut height = f32::from(restore.size.height);

        width = width.max(MIN_WINDOW_DIMENSION).min(display_w.max(MIN_WINDOW_DIMENSION));
        height = height
            .max(MIN_WINDOW_DIMENSION)
            .min(display_h.max(MIN_WINDOW_DIMENSION));

        let max_x = display_x + (display_w - width).max(0.0);
        let max_y = display_y + (display_h - height).max(0.0);
        x = x.clamp(display_x, max_x);
        y = y.clamp(display_y, max_y);

        restore = bounds(point(px(x), px(y)), size(px(width), px(height)));
    }

    window_bounds_from_parts(mode, restore)
}

fn intersects(lhs: Bounds<Pixels>, rhs: Bounds<Pixels>) -> bool {
    let lhs_left = f32::from(lhs.origin.x);
    let lhs_top = f32::from(lhs.origin.y);
    let lhs_right = lhs_left + f32::from(lhs.size.width);
    let lhs_bottom = lhs_top + f32::from(lhs.size.height);

    let rhs_left = f32::from(rhs.origin.x);
    let rhs_top = f32::from(rhs.origin.y);
    let rhs_right = rhs_left + f32::from(rhs.size.width);
    let rhs_bottom = rhs_top + f32::from(rhs.size.height);

    lhs_left < rhs_right && lhs_right > rhs_left && lhs_top < rhs_bottom && lhs_bottom > rhs_top
}

fn mode_from_window_bounds(window_bounds: WindowBounds) -> PersistedWindowMode {
    match window_bounds {
        WindowBounds::Windowed(_) => PersistedWindowMode::Windowed,
        WindowBounds::Maximized(_) => PersistedWindowMode::Maximized,
        WindowBounds::Fullscreen(_) => PersistedWindowMode::Fullscreen,
    }
}

fn window_bounds_from_parts(mode: PersistedWindowMode, restore_bounds: Bounds<Pixels>) -> WindowBounds {
    match mode {
        PersistedWindowMode::Windowed => WindowBounds::Windowed(restore_bounds),
        PersistedWindowMode::Maximized => WindowBounds::Maximized(restore_bounds),
        PersistedWindowMode::Fullscreen => WindowBounds::Fullscreen(restore_bounds),
    }
}

fn is_valid_dimension(value: f32) -> bool {
    value.is_finite() && value >= MIN_WINDOW_DIMENSION
}

fn is_valid_coordinate(value: f32) -> bool {
    value.is_finite() && value.abs() <= MAX_ABS_COORDINATE
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn new_temp_root(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        path.push(format!(
            "gpui_papyru2_window_pos_{name}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn remove_temp_root(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    fn windowed(x: f32, y: f32, width: f32, height: f32) -> WindowBounds {
        WindowBounds::Windowed(bounds(
            point(px(x), px(y)),
            size(px(width), px(height)),
        ))
    }

    fn display_bounds(width: f32, height: f32) -> Bounds<Pixels> {
        bounds(point(px(0.0), px(0.0)), size(px(width), px(height)))
    }

    #[test]
    fn win_test1_first_run_without_file_uses_centered_default() {
        let root = new_temp_root("win_test1");
        let path = root.join("conf").join(WINDOW_POSITION_FILE_NAME);
        let fallback = windowed(100.0, 120.0, 1200.0, 800.0);

        let loaded = load_window_position(&path).expect("load state");
        let resolved =
            resolve_startup_window_bounds(loaded.as_ref(), fallback, Some(display_bounds(3000.0, 2000.0)), false);

        assert!(loaded.is_none());
        assert_eq!(resolved, fallback);
        remove_temp_root(&root);
    }

    #[test]
    fn win_test2_startup_applies_previously_saved_window_bounds() {
        let root = new_temp_root("win_test2");
        let path = root.join("conf").join(WINDOW_POSITION_FILE_NAME);
        let fallback = windowed(10.0, 20.0, 1200.0, 800.0);
        let saved = WindowPositionState {
            x: 300.0,
            y: 200.0,
            width: 900.0,
            height: 700.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: Some(1),
            monitor_uuid: Some("display-uuid".to_string()),
            dpi_scale: Some(1.5),
        };
        save_window_position_atomic(&path, &saved).expect("save state");

        let loaded = load_window_position(&path).expect("load state");
        let resolved =
            resolve_startup_window_bounds(loaded.as_ref(), fallback, Some(display_bounds(3000.0, 2000.0)), false);

        assert_eq!(resolved, windowed(300.0, 200.0, 900.0, 700.0));
        remove_temp_root(&root);
    }

    #[test]
    fn win_test3_close_save_writes_window_position_toml_under_conf_dir() {
        let root = new_temp_root("win_test3");
        let conf_dir = root.join("conf");
        let path = conf_dir.join(WINDOW_POSITION_FILE_NAME);
        let state = WindowPositionState {
            x: 10.0,
            y: 10.0,
            width: 1200.0,
            height: 800.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: None,
            monitor_uuid: None,
            dpi_scale: Some(1.0),
        };

        save_window_position_atomic(&path, &state).expect("save state");

        assert!(path.is_file());
        remove_temp_root(&root);
    }

    #[test]
    fn win_test4_serde_toml_round_trip_is_consistent() {
        let root = new_temp_root("win_test4");
        let path = root.join("conf").join(WINDOW_POSITION_FILE_NAME);
        let state = WindowPositionState {
            x: 20.0,
            y: 40.0,
            width: 1440.0,
            height: 900.0,
            window_mode: PersistedWindowMode::Maximized,
            monitor_id: Some(3),
            monitor_uuid: Some("monitor-3".to_string()),
            dpi_scale: Some(2.0),
        };

        save_window_position_atomic(&path, &state).expect("save state");
        let loaded = load_window_position(&path).expect("load state");

        assert_eq!(loaded, Some(state));
        remove_temp_root(&root);
    }

    #[test]
    fn win_test5_maximized_and_fullscreen_restore_bounds_round_trip() {
        let maximized = WindowPositionState::from_window_bounds(
            WindowBounds::Maximized(bounds(
                point(px(12.0), px(30.0)),
                size(px(1200.0), px(800.0)),
            )),
            Some(7),
            None,
            Some(1.0),
        );
        let fullscreen = WindowPositionState::from_window_bounds(
            WindowBounds::Fullscreen(bounds(
                point(px(24.0), px(40.0)),
                size(px(1280.0), px(900.0)),
            )),
            Some(8),
            None,
            Some(1.0),
        );

        assert_eq!(
            maximized.to_window_bounds(),
            Some(WindowBounds::Maximized(bounds(
                point(px(12.0), px(30.0)),
                size(px(1200.0), px(800.0)),
            )))
        );
        assert_eq!(
            fullscreen.to_window_bounds(),
            Some(WindowBounds::Fullscreen(bounds(
                point(px(24.0), px(40.0)),
                size(px(1280.0), px(900.0)),
            )))
        );
    }

    #[test]
    fn win_test6_minimized_state_is_not_accepted_for_startup_restore() {
        let root = new_temp_root("win_test6");
        let path = root.join("conf").join(WINDOW_POSITION_FILE_NAME);
        let raw = r#"
x = 10.0
y = 20.0
width = 1200.0
height = 800.0
window_mode = "minimized"
"#;
        fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        fs::write(&path, raw).expect("write invalid state");

        let loaded = load_window_position(&path);
        assert!(loaded.is_err());
        remove_temp_root(&root);
    }

    #[test]
    fn win_test7_off_screen_saved_position_falls_back_to_default() {
        let fallback = windowed(100.0, 120.0, 1200.0, 800.0);
        let state = WindowPositionState {
            x: 9000.0,
            y: 9000.0,
            width: 600.0,
            height: 400.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: None,
            monitor_uuid: None,
            dpi_scale: Some(1.0),
        };

        let resolved = resolve_startup_window_bounds(
            Some(&state),
            fallback,
            Some(display_bounds(1920.0, 1080.0)),
            false,
        );

        assert_eq!(resolved, fallback);
    }

    #[test]
    fn win_test8_clamp_validation_prevents_invalid_monitor_coordinates() {
        let fallback = windowed(0.0, 0.0, 1200.0, 800.0);
        let state = WindowPositionState {
            x: 1800.0,
            y: 950.0,
            width: 600.0,
            height: 400.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: None,
            monitor_uuid: None,
            dpi_scale: Some(1.0),
        };

        let resolved = resolve_startup_window_bounds(
            Some(&state),
            fallback,
            Some(display_bounds(1920.0, 1080.0)),
            false,
        );

        assert_eq!(resolved, windowed(1320.0, 680.0, 600.0, 400.0));
    }

    #[test]
    fn win_test9_replace_failure_preserves_existing_valid_file() {
        let root = new_temp_root("win_test9");
        let path = root.join("conf").join(WINDOW_POSITION_FILE_NAME);
        let old = WindowPositionState {
            x: 10.0,
            y: 20.0,
            width: 1200.0,
            height: 800.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: Some(1),
            monitor_uuid: Some("old".to_string()),
            dpi_scale: Some(1.0),
        };
        let new = WindowPositionState {
            monitor_uuid: Some("new".to_string()),
            ..old.clone()
        };

        save_window_position_atomic(&path, &old).expect("save old");
        let new_bytes = toml::to_string_pretty(&new).expect("serialize new");
        let result = write_atomic_with_replace(&path, new_bytes.as_bytes(), |_temp, _target| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "forced replace failure",
            ))
        });
        assert!(result.is_err());

        let loaded = load_window_position(&path).expect("load old state");
        assert_eq!(loaded, Some(old));
        remove_temp_root(&root);
    }

    #[test]
    fn win_test9_success_path_replaces_existing_content() {
        let root = new_temp_root("win_test9_success");
        let path = root.join("conf").join(WINDOW_POSITION_FILE_NAME);
        let old = WindowPositionState {
            x: 10.0,
            y: 20.0,
            width: 1200.0,
            height: 800.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: Some(1),
            monitor_uuid: Some("old".to_string()),
            dpi_scale: Some(1.0),
        };
        let new = WindowPositionState {
            x: 33.0,
            y: 44.0,
            width: 900.0,
            height: 700.0,
            window_mode: PersistedWindowMode::Maximized,
            monitor_id: Some(2),
            monitor_uuid: Some("new".to_string()),
            dpi_scale: Some(2.0),
        };

        save_window_position_atomic(&path, &old).expect("save old");
        save_window_position_atomic(&path, &new).expect("save new");

        let loaded = load_window_position(&path).expect("load new state");
        assert_eq!(loaded, Some(new));
        remove_temp_root(&root);
    }

    #[test]
    fn win_test9_cleanup_failure_is_non_destructive() {
        let root = new_temp_root("win_test9_cleanup");
        let path = root.join("conf").join(WINDOW_POSITION_FILE_NAME);
        let old = WindowPositionState {
            x: 10.0,
            y: 20.0,
            width: 1200.0,
            height: 800.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: Some(1),
            monitor_uuid: Some("old".to_string()),
            dpi_scale: Some(1.0),
        };
        let new = WindowPositionState {
            monitor_uuid: Some("new".to_string()),
            ..old.clone()
        };

        save_window_position_atomic(&path, &old).expect("save old");
        let new_bytes = toml::to_string_pretty(&new).expect("serialize new");
        let result = write_atomic_with_replace(&path, new_bytes.as_bytes(), |temp, _target| {
            fs::remove_file(temp)?;
            fs::create_dir_all(temp)?;
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "forced replace failure",
            ))
        });
        assert!(result.is_err());
        let error_text = result.err().expect("error").to_string();
        assert!(error_text.contains("replace target"));
        assert!(error_text.contains("cleanup temp failed"));

        let loaded = load_window_position(&path).expect("load old state");
        assert_eq!(loaded, Some(old));
        remove_temp_root(&root);
    }

    #[test]
    fn win_test10_wayland_position_ignore_path_is_non_fatal() {
        let fallback = windowed(300.0, 200.0, 1200.0, 800.0);
        let state = WindowPositionState {
            x: 20.0,
            y: 30.0,
            width: 700.0,
            height: 500.0,
            window_mode: PersistedWindowMode::Windowed,
            monitor_id: Some(1),
            monitor_uuid: None,
            dpi_scale: Some(1.0),
        };

        let resolved = resolve_startup_window_bounds(
            Some(&state),
            fallback,
            Some(display_bounds(1920.0, 1080.0)),
            true,
        );

        assert_eq!(resolved, windowed(300.0, 200.0, 700.0, 500.0));
    }

    #[test]
    fn win_test11_first_launch_without_persisted_geometry_uses_seventy_percent_and_centered() {
        let default_bounds = windowed(0.0, 0.0, 1200.0, 800.0);
        let fallback = first_launch_fallback_bounds(Some(display_bounds(2000.0, 1000.0)), default_bounds);
        let resolved = resolve_startup_window_bounds(
            None,
            fallback,
            Some(display_bounds(2000.0, 1000.0)),
            false,
        );

        assert_eq!(resolved, windowed(300.0, 150.0, 1400.0, 700.0));
    }
}
