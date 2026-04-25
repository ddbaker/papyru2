use std::path::{Path, PathBuf};

pub(crate) const PAPYRU2_DEBUG_LOG_FILE_NAME: &str = "papyru2_debug.log";
pub(crate) const PAPYRU2_PIN_FILE_LOG_FILE_NAME: &str = "papyru2_pin_file.log";

static TRACE_DEBUG_LOG_PATH: std::sync::OnceLock<std::sync::Mutex<PathBuf>> =
    std::sync::OnceLock::new();
static TRACE_DEBUG_ENABLED: std::sync::OnceLock<std::sync::atomic::AtomicBool> =
    std::sync::OnceLock::new();

const REQ_BOOT_PROFILE_ENV_VAR: &str = "PAPYRU2_BOOT_PROFILE";
pub(crate) const PAPYRU2_BOOT_PROFILE_LOG_FILE_NAME: &str = "papyru2_boot_profile.log";
const REQ_RUNTIME_PROFILE_ENV_VAR: &str = "PAPYRU2_RUNTIME_PROFILE";
pub(crate) const PAPYRU2_RUNTIME_PROFILE_LOG_FILE_NAME: &str = "papyru2_runtime_profile.log";

static BOOT_PROFILE_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static BOOT_PROFILE_STATE: std::sync::OnceLock<std::sync::Mutex<BootProfileState>> =
    std::sync::OnceLock::new();
static RUNTIME_PROFILE_ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static RUNTIME_PROFILE_LOG_PATH: std::sync::OnceLock<std::sync::Mutex<PathBuf>> =
    std::sync::OnceLock::new();

#[derive(Debug)]
struct BootProfileState {
    start_instant: std::time::Instant,
    start_epoch_ms: u128,
    output_path: Option<PathBuf>,
    lines: Vec<String>,
    flushed: bool,
}

impl BootProfileState {
    fn new() -> Self {
        let start_epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);

        Self {
            start_instant: std::time::Instant::now(),
            start_epoch_ms,
            output_path: None,
            lines: Vec::new(),
            flushed: false,
        }
    }
}

fn req_profile_enabled_from_env_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn req_boot_profile_enabled_from_env_value(value: &str) -> bool {
    req_profile_enabled_from_env_value(value)
}

fn req_boot_profile_enabled_from_env() -> bool {
    std::env::var(REQ_BOOT_PROFILE_ENV_VAR)
        .map(|value| req_boot_profile_enabled_from_env_value(value.as_str()))
        .unwrap_or(false)
}

pub(crate) fn req_boot_profile_enabled() -> bool {
    *BOOT_PROFILE_ENABLED.get_or_init(req_boot_profile_enabled_from_env)
}

fn req_runtime_profile_enabled_from_env_value(value: &str) -> bool {
    req_profile_enabled_from_env_value(value)
}

fn req_runtime_profile_enabled_from_env() -> bool {
    std::env::var(REQ_RUNTIME_PROFILE_ENV_VAR)
        .map(|value| req_runtime_profile_enabled_from_env_value(value.as_str()))
        .unwrap_or(false)
}

pub(crate) fn req_runtime_profile_enabled() -> bool {
    *RUNTIME_PROFILE_ENABLED.get_or_init(req_runtime_profile_enabled_from_env)
}

fn boot_profile_output_path_from_app_paths(app_paths: &crate::path_resolver::AppPaths) -> PathBuf {
    app_paths.log_file_path(PAPYRU2_BOOT_PROFILE_LOG_FILE_NAME)
}

fn default_boot_profile_log_path() -> PathBuf {
    crate::path_resolver::AppPaths::resolve()
        .map(|app_paths| boot_profile_output_path_from_app_paths(&app_paths))
        .unwrap_or_else(|_| PathBuf::from(PAPYRU2_BOOT_PROFILE_LOG_FILE_NAME))
}

fn runtime_profile_output_path_from_app_paths(
    app_paths: &crate::path_resolver::AppPaths,
) -> PathBuf {
    app_paths.log_file_path(PAPYRU2_RUNTIME_PROFILE_LOG_FILE_NAME)
}

fn default_runtime_profile_log_path() -> PathBuf {
    crate::path_resolver::AppPaths::resolve()
        .map(|app_paths| runtime_profile_output_path_from_app_paths(&app_paths))
        .unwrap_or_else(|_| PathBuf::from(PAPYRU2_RUNTIME_PROFILE_LOG_FILE_NAME))
}

fn boot_profile_state_lock() -> &'static std::sync::Mutex<BootProfileState> {
    BOOT_PROFILE_STATE.get_or_init(|| std::sync::Mutex::new(BootProfileState::new()))
}

fn runtime_profile_log_path_lock() -> &'static std::sync::Mutex<PathBuf> {
    RUNTIME_PROFILE_LOG_PATH
        .get_or_init(|| std::sync::Mutex::new(default_runtime_profile_log_path()))
}

pub(crate) fn configure_boot_profile_log_path(app_paths: &crate::path_resolver::AppPaths) {
    if !req_boot_profile_enabled() {
        return;
    }

    if let Ok(mut state) = boot_profile_state_lock().lock() {
        state.output_path = Some(boot_profile_output_path_from_app_paths(app_paths));
    }
}

pub(crate) fn configure_runtime_profile_log_path(app_paths: &crate::path_resolver::AppPaths) {
    if !req_runtime_profile_enabled() {
        return;
    }

    if let Ok(mut output_path) = runtime_profile_log_path_lock().lock() {
        *output_path = runtime_profile_output_path_from_app_paths(app_paths);
    }
}

fn boot_profile_push_line(stage: &str, detail: String) {
    if !req_boot_profile_enabled() {
        return;
    }

    let Ok(mut state) = boot_profile_state_lock().lock() else {
        return;
    };

    if state.flushed {
        return;
    }

    let elapsed_ms = state.start_instant.elapsed().as_millis();
    if detail.is_empty() {
        state.lines.push(format!("{elapsed_ms:>6}ms {stage}"));
    } else {
        state
            .lines
            .push(format!("{elapsed_ms:>6}ms {stage} {detail}"));
    }
}

fn runtime_profile_epoch_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

fn runtime_profile_push_line(stage: &str, detail: String) {
    if !req_runtime_profile_enabled() {
        return;
    }

    let output_path = runtime_profile_log_path_lock()
        .lock()
        .map(|path| path.clone())
        .unwrap_or_else(|_| default_runtime_profile_log_path());
    let epoch_ms = runtime_profile_epoch_ms();
    let line = if detail.is_empty() {
        format!("[runtime-profile] epoch_ms={epoch_ms} stage={stage}\n")
    } else {
        format!("[runtime-profile] epoch_ms={epoch_ms} stage={stage} {detail}\n")
    };

    if let Some(parent) = output_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_path.as_path())
    {
        let _ = std::io::Write::write_all(&mut file, line.as_bytes());
    } else {
        eprintln!(
            "papyru2 runtime profile write failed path={}",
            output_path.display()
        );
        eprintln!("{line}");
    }
}

pub(crate) fn boot_profile_mark(stage: &str) {
    boot_profile_push_line(stage, String::new());
}

pub(crate) fn boot_profile_mark_detail(stage: &str, detail: String) {
    boot_profile_push_line(stage, detail);
}

pub(crate) fn runtime_profile_mark_detail(stage: &str, detail: String) {
    runtime_profile_push_line(stage, detail);
}

pub(crate) fn runtime_profile_mark_detail_lazy<F>(stage: &str, detail: F)
where
    F: FnOnce() -> String,
{
    if !req_runtime_profile_enabled() {
        return;
    }

    runtime_profile_push_line(stage, detail());
}

pub(crate) fn boot_profile_mark_timing(stage: &str, duration: std::time::Duration, detail: String) {
    if detail.is_empty() {
        boot_profile_push_line(stage, format!("duration_ms={}", duration.as_millis()));
    } else {
        boot_profile_push_line(
            stage,
            format!("duration_ms={} {detail}", duration.as_millis()),
        );
    }
}

pub(crate) fn runtime_profile_mark_timing(
    stage: &str,
    duration: std::time::Duration,
    detail: String,
) {
    if detail.is_empty() {
        runtime_profile_push_line(stage, format!("duration_ms={}", duration.as_millis()));
    } else {
        runtime_profile_push_line(
            stage,
            format!("duration_ms={} {detail}", duration.as_millis()),
        );
    }
}

pub(crate) fn runtime_profile_mark_timing_lazy<F>(
    stage: &str,
    duration: std::time::Duration,
    detail: F,
) where
    F: FnOnce() -> String,
{
    if !req_runtime_profile_enabled() {
        return;
    }

    runtime_profile_mark_timing(stage, duration, detail());
}

pub(crate) fn flush_boot_profile(reason: &str) {
    if !req_boot_profile_enabled() {
        return;
    }

    let Ok(mut state) = boot_profile_state_lock().lock() else {
        return;
    };

    if state.flushed {
        return;
    }

    let total_ms = state.start_instant.elapsed().as_millis();
    let output_path = state
        .output_path
        .clone()
        .unwrap_or_else(default_boot_profile_log_path);

    let mut output = String::new();
    output.push('\n');
    output.push_str(&format!(
        "[boot-profile] start_epoch_ms={} reason={} total_ms={}\n",
        state.start_epoch_ms, reason, total_ms
    ));
    for line in &state.lines {
        output.push_str(line.as_str());
        output.push('\n');
    }
    output.push_str("[boot-profile] end\n");

    state.flushed = true;
    drop(state);

    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(output_path.as_path())
    {
        let _ = std::io::Write::write_all(&mut file, output.as_bytes());
    } else {
        eprintln!(
            "papyru2 boot profile write failed path={} reason={reason}",
            output_path.display()
        );
        eprintln!("{output}");
    }
}

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
    prepare_debug_log: bool,
) -> std::io::Result<()> {
    let debug_log_path = debug_log_path_from_app_paths(app_paths);
    let pin_file_log_path = app_paths.log_file_path(PAPYRU2_PIN_FILE_LOG_FILE_NAME);
    if prepare_debug_log {
        rotate_startup_log_file(debug_log_path.as_path())?;
    }
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

pub(crate) fn trace_debug_lazy(message: impl FnOnce() -> String) {
    if !trace_debug_is_enabled() {
        return;
    }

    trace_debug(message());
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

        prepare_startup_log_files(&paths, true).expect("prepare startup logs");

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

        prepare_startup_log_files(&paths, true).expect("prepare startup logs from missing state");

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

    #[test]
    fn log_test10_req_aus_lag_lazy_trace_skips_closure_when_disabled() {
        let previous = trace_debug_is_enabled();
        configure_trace_debug_enabled(false);

        let evaluated = std::sync::atomic::AtomicUsize::new(0);
        trace_debug_lazy(|| {
            evaluated.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            "should not be evaluated".to_string()
        });

        assert_eq!(evaluated.load(std::sync::atomic::Ordering::SeqCst), 0);
        configure_trace_debug_enabled(previous);
    }

    #[test]
    fn log_test11_req_boot_profile_parser_accepts_truthy_tokens() {
        for value in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(req_boot_profile_enabled_from_env_value(value));
        }
    }

    #[test]
    fn log_test12_req_boot_profile_parser_rejects_non_truthy_tokens() {
        for value in ["", "0", "false", "no", "off", "unexpected"] {
            assert!(!req_boot_profile_enabled_from_env_value(value));
        }
    }

    #[test]
    fn log_test13_req_runtime_profile_parser_accepts_truthy_tokens() {
        for value in ["1", "true", "TRUE", " yes ", "On"] {
            assert!(req_runtime_profile_enabled_from_env_value(value));
        }
    }

    #[test]
    fn log_test14_req_runtime_profile_parser_rejects_non_truthy_tokens() {
        for value in ["", "0", "false", "no", "off", "unexpected"] {
            assert!(!req_runtime_profile_enabled_from_env_value(value));
        }
    }
}
