use std::path::{Path, PathBuf};

pub(crate) const PAPYRU2_DEBUG_LOG_FILE_NAME: &str = "papyru2_debug.log";
pub(crate) const PAPYRU2_PIN_FILE_LOG_FILE_NAME: &str = "papyru2_pin_file.log";

static TRACE_DEBUG_LOG_PATH: std::sync::OnceLock<std::sync::Mutex<PathBuf>> =
    std::sync::OnceLock::new();
static TRACE_DEBUG_ENABLED: std::sync::OnceLock<std::sync::atomic::AtomicBool> =
    std::sync::OnceLock::new();

pub(crate) fn req_log_profile_default_enabled() -> bool {
    cfg!(debug_assertions)
}

fn trace_debug_enabled_flag() -> &'static std::sync::atomic::AtomicBool {
    TRACE_DEBUG_ENABLED
        .get_or_init(|| std::sync::atomic::AtomicBool::new(req_log_profile_default_enabled()))
}

pub(crate) fn trace_debug_is_enabled() -> bool {
    trace_debug_enabled_flag().load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn configure_trace_debug_enabled(enabled: bool) {
    trace_debug_enabled_flag().store(enabled, std::sync::atomic::Ordering::Relaxed);
}

fn default_trace_debug_log_path() -> PathBuf {
    crate::path_resolver::AppPaths::resolve()
        .map(|app_paths| app_paths.log_file_path(PAPYRU2_DEBUG_LOG_FILE_NAME))
        .unwrap_or_else(|_| PathBuf::from(PAPYRU2_DEBUG_LOG_FILE_NAME))
}

fn trace_debug_log_path_lock() -> &'static std::sync::Mutex<PathBuf> {
    TRACE_DEBUG_LOG_PATH.get_or_init(|| std::sync::Mutex::new(default_trace_debug_log_path()))
}

pub(crate) fn trace_debug_log_file_path() -> PathBuf {
    trace_debug_log_path_lock()
        .lock()
        .map(|path| path.clone())
        .unwrap_or_else(|_| default_trace_debug_log_path())
}

pub(crate) fn debug_log_path_from_app_paths(app_paths: &crate::path_resolver::AppPaths) -> PathBuf {
    app_paths.log_file_path(PAPYRU2_DEBUG_LOG_FILE_NAME)
}

pub(crate) fn configure_trace_debug_log_path(app_paths: &crate::path_resolver::AppPaths) {
    if let Ok(mut path) = trace_debug_log_path_lock().lock() {
        *path = debug_log_path_from_app_paths(app_paths);
    }
}

fn backup_log_path(log_path: &Path) -> PathBuf {
    log_path.with_extension("log.bak")
}

fn rotate_startup_log_file(log_path: &Path) -> std::io::Result<()> {
    let backup_path = backup_log_path(log_path);
    if backup_path.exists() {
        std::fs::remove_file(backup_path.as_path())?;
    }
    if log_path.exists() {
        std::fs::rename(log_path, backup_path.as_path())?;
    }

    std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(log_path)?;

    Ok(())
}

pub(crate) fn prepare_startup_log_files(
    app_paths: &crate::path_resolver::AppPaths,
) -> std::io::Result<()> {
    let debug_log_path = debug_log_path_from_app_paths(app_paths);
    let pin_file_log_path = app_paths.log_file_path(PAPYRU2_PIN_FILE_LOG_FILE_NAME);
    rotate_startup_log_file(debug_log_path.as_path())?;
    rotate_startup_log_file(pin_file_log_path.as_path())?;
    Ok(())
}

pub(crate) fn trace_debug(message: impl AsRef<str>) {
    if !trace_debug_is_enabled() {
        return;
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let line = format!("[{now}] {}\n", message.as_ref());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(trace_debug_log_file_path())
    {
        let _ = std::io::Write::write_all(&mut file, line.as_bytes());
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReqLogConfigFile {
    #[serde(default)]
    debug: ReqLogDebugSection,
}

#[derive(Debug, Default, serde::Deserialize)]
struct ReqLogDebugSection {
    #[serde(default)]
    log: Option<bool>,
}

pub(crate) fn req_log_effective_debug_logging_enabled(
    profile_default_enabled: bool,
    config_override: Option<bool>,
) -> bool {
    config_override.unwrap_or(profile_default_enabled)
}

fn load_req_log_config_override_result(path: &Path) -> std::io::Result<Option<bool>> {
    if path.exists() && !path.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("req-log config path is not a file path={}", path.display()),
        ));
    }
    if !path.is_file() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(path)?;
    let parsed: ReqLogConfigFile = toml::from_str(&raw)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    Ok(parsed.debug.log)
}

pub(crate) fn load_req_log_config_override(path: &Path) -> Option<bool> {
    load_req_log_config_override_result(path).ok().flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_resolver::{AppPaths, RunEnvPattern};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn log_test_temp_root(suffix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("papyru2_log_{suffix}_{nanos}"));
        std::fs::create_dir_all(root.as_path()).expect("create log temp root");
        root
    }

    fn log_test_cleanup(root: &std::path::Path) {
        if root.exists() {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    fn log_test_resolved_app_paths(root: &std::path::Path, suffix: &str) -> AppPaths {
        let app_home = root.join(format!("app_home_{suffix}"));
        let paths = AppPaths {
            mode: RunEnvPattern::Installed,
            app_home: app_home.clone(),
            conf_dir: app_home.join("conf"),
            data_dir: app_home.join("data"),
            user_document_dir: app_home.join("data").join("user_document"),
            recyclebin_dir: app_home
                .join("data")
                .join("user_document")
                .join("recyclebin"),
            log_dir: app_home.join("log"),
            bin_dir: app_home.join("bin"),
        };
        paths.ensure_dirs().expect("ensure app dirs");
        paths
    }

    #[test]
    fn log_test1_req_log1_debug_log_filename_is_renamed() {
        assert_eq!(PAPYRU2_DEBUG_LOG_FILE_NAME, "papyru2_debug.log");
    }

    #[test]
    fn log_test2_req_log2_debug_log_path_resolves_under_log_dir() {
        let root = log_test_temp_root("log_test2");
        let paths = log_test_resolved_app_paths(root.as_path(), "log_test2");

        let expected = paths.log_dir.join(PAPYRU2_DEBUG_LOG_FILE_NAME);
        assert_eq!(debug_log_path_from_app_paths(&paths), expected);

        log_test_cleanup(root.as_path());
    }

    #[test]
    fn log_test3_req_log3_startup_rotation_replaces_existing_bak_and_recreates_logs() {
        let root = log_test_temp_root("log_test3");
        let paths = log_test_resolved_app_paths(root.as_path(), "log_test3");

        let debug_log = debug_log_path_from_app_paths(&paths);
        let pin_log = paths.log_file_path(PAPYRU2_PIN_FILE_LOG_FILE_NAME);
        let debug_bak = debug_log.with_extension("log.bak");
        let pin_bak = pin_log.with_extension("log.bak");

        std::fs::write(debug_log.as_path(), "debug-current").expect("write debug log");
        std::fs::write(pin_log.as_path(), "pin-current").expect("write pin log");
        std::fs::write(debug_bak.as_path(), "debug-stale-bak").expect("write stale debug bak");
        std::fs::write(pin_bak.as_path(), "pin-stale-bak").expect("write stale pin bak");

        prepare_startup_log_files(&paths).expect("prepare startup logs");

        assert_eq!(
            std::fs::read_to_string(debug_bak.as_path()).expect("read rotated debug bak"),
            "debug-current"
        );
        assert_eq!(
            std::fs::read_to_string(pin_bak.as_path()).expect("read rotated pin bak"),
            "pin-current"
        );
        assert_eq!(
            std::fs::metadata(debug_log.as_path())
                .expect("debug log metadata")
                .len(),
            0
        );
        assert_eq!(
            std::fs::metadata(pin_log.as_path())
                .expect("pin log metadata")
                .len(),
            0
        );

        log_test_cleanup(root.as_path());
    }

    #[test]
    fn log_test4_req_log3_startup_rotation_creates_logs_when_missing() {
        let root = log_test_temp_root("log_test4");
        let paths = log_test_resolved_app_paths(root.as_path(), "log_test4");

        let debug_log = debug_log_path_from_app_paths(&paths);
        let pin_log = paths.log_file_path(PAPYRU2_PIN_FILE_LOG_FILE_NAME);
        let debug_bak = debug_log.with_extension("log.bak");
        let pin_bak = pin_log.with_extension("log.bak");

        if debug_log.exists() {
            std::fs::remove_file(debug_log.as_path()).expect("remove existing debug log");
        }
        if pin_log.exists() {
            std::fs::remove_file(pin_log.as_path()).expect("remove existing pin log");
        }
        if debug_bak.exists() {
            std::fs::remove_file(debug_bak.as_path()).expect("remove existing debug bak");
        }
        if pin_bak.exists() {
            std::fs::remove_file(pin_bak.as_path()).expect("remove existing pin bak");
        }

        prepare_startup_log_files(&paths).expect("prepare startup logs from missing state");

        assert!(debug_log.exists());
        assert!(pin_log.exists());
        assert_eq!(
            std::fs::metadata(debug_log.as_path())
                .expect("debug log metadata")
                .len(),
            0
        );
        assert_eq!(
            std::fs::metadata(pin_log.as_path())
                .expect("pin log metadata")
                .len(),
            0
        );
        assert!(!debug_bak.exists());
        assert!(!pin_bak.exists());

        log_test_cleanup(root.as_path());
    }

    #[test]
    fn log_test5_req_log4_default_debug_profile_keeps_logging_enabled_by_default() {
        assert!(req_log_effective_debug_logging_enabled(true, None));
    }

    #[test]
    fn log_test6_req_log5_default_release_profile_disables_logging_by_default() {
        assert!(!req_log_effective_debug_logging_enabled(false, None));
    }

    #[test]
    fn log_test7_req_log6_debug_table_override_supersedes_profile_default() {
        assert!(req_log_effective_debug_logging_enabled(false, Some(true)));
        assert!(!req_log_effective_debug_logging_enabled(true, Some(false)));
    }

    #[test]
    fn log_test8_req_log6_loads_debug_table_log_true_from_config_file() {
        let root = log_test_temp_root("log_test8");
        let config_path = root.join(crate::app::PAPYRU2_CONF_FILE_NAME);
        std::fs::write(
            config_path.as_path(),
            "[debug]\nlog = true\n\n[color]\nbackground = 0xf7f2ec\nforeground = 0x437085\n",
        )
        .expect("write req-log test config");

        assert_eq!(
            load_req_log_config_override(config_path.as_path()),
            Some(true)
        );

        log_test_cleanup(root.as_path());
    }

    #[test]
    fn log_test9_req_log6_loads_debug_table_log_false_from_config_file() {
        let root = log_test_temp_root("log_test9");
        let config_path = root.join(crate::app::PAPYRU2_CONF_FILE_NAME);
        std::fs::write(
            config_path.as_path(),
            "[debug]\nlog = false\n\n[color]\nbackground = 0xf7f2ec\nforeground = 0x437085\n",
        )
        .expect("write req-log test config");

        assert_eq!(
            load_req_log_config_override(config_path.as_path()),
            Some(false)
        );

        log_test_cleanup(root.as_path());
    }
}
