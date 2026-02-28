use std::env;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const APP_NAME: &str = "papyru2";
pub const APP_HOME_ENV: &str = "PAPYRU2_HOME";
pub const PORTABLE_MARKER_FILE: &str = "papyru2.portable";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliRunModeOverride {
    Portable,
    Installed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnvPattern {
    EnvOverride,
    Portable,
    DevCargoRun,
    Installed,
}

impl RunEnvPattern {
    pub fn reason(self) -> &'static str {
        match self {
            Self::EnvOverride => "env_override",
            Self::Portable => "portable_marker_or_layout",
            Self::DevCargoRun => "cargo_target_layout",
            Self::Installed => "installed_home_fallback",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub mode: RunEnvPattern,
    pub app_home: PathBuf,
    pub conf_dir: PathBuf,
    pub data_dir: PathBuf,
    pub user_document_dir: PathBuf,
    pub log_dir: PathBuf,
    pub bin_dir: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> io::Result<Self> {
        Self::resolve_with_cli_override(None)
    }

    pub fn resolve_with_cli_override(override_mode: Option<CliRunModeOverride>) -> io::Result<Self> {
        let env_home = env::var_os(APP_HOME_ENV).map(PathBuf::from);
        let exe_path = current_exe_path()?;
        Self::resolve_from_inputs(env_home, exe_path, os_home_dir(), override_mode)
    }

    pub(crate) fn resolve_from_inputs(
        env_home: Option<PathBuf>,
        exe_path: PathBuf,
        user_home: Option<PathBuf>,
        override_mode: Option<CliRunModeOverride>,
    ) -> io::Result<Self> {
        let exe_dir = exe_path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "failed to resolve executable directory",
            )
        })?;

        if let Some(mode) = override_mode {
            match mode {
                CliRunModeOverride::Portable => {
                    return Self::build_paths(
                        RunEnvPattern::Portable,
                        forced_portable_app_home(exe_dir)?,
                    );
                }
                CliRunModeOverride::Installed => {
                    return Self::build_paths(
                        RunEnvPattern::Installed,
                        installed_app_home(user_home)?,
                    );
                }
            }
        }

        if let Some(home) = env_home {
            return Self::build_paths(RunEnvPattern::EnvOverride, home);
        }

        if let Some(home) = detect_portable_app_home(exe_dir) {
            return Self::build_paths(RunEnvPattern::Portable, home);
        }

        if let Some(home) = detect_dev_app_home(exe_dir) {
            return Self::build_paths(RunEnvPattern::DevCargoRun, home);
        }

        Self::build_paths(RunEnvPattern::Installed, installed_app_home(user_home)?)
    }

    fn build_paths(mode: RunEnvPattern, app_home: PathBuf) -> io::Result<Self> {
        let paths = Self::from_home(mode, app_home);
        paths.ensure_dirs()?;
        Ok(paths)
    }

    pub fn ensure_dirs(&self) -> io::Result<()> {
        fs::create_dir_all(&self.app_home)?;
        fs::create_dir_all(&self.conf_dir)?;
        fs::create_dir_all(&self.data_dir)?;
        fs::create_dir_all(&self.user_document_dir)?;
        fs::create_dir_all(&self.log_dir)?;
        fs::create_dir_all(&self.bin_dir)?;
        Ok(())
    }

    pub fn config_file_path(&self, file_name: impl AsRef<Path>) -> PathBuf {
        self.conf_dir.join(file_name)
    }

    pub fn log_file_path(&self, file_name: impl AsRef<Path>) -> PathBuf {
        self.log_dir.join(file_name)
    }

    fn from_home(mode: RunEnvPattern, app_home: PathBuf) -> Self {
        let data_dir = app_home.join("data");
        Self {
            conf_dir: app_home.join("conf"),
            user_document_dir: data_dir.join("user_document"),
            data_dir,
            log_dir: app_home.join("log"),
            bin_dir: app_home.join("bin"),
            app_home,
            mode,
        }
    }
}

fn current_exe_path() -> io::Result<PathBuf> {
    let exe_path = env::current_exe()?;
    match fs::canonicalize(&exe_path) {
        Ok(path) => Ok(path),
        Err(_) => Ok(exe_path),
    }
}

pub fn parse_cli_mode_override<I, S>(args: I) -> io::Result<Option<CliRunModeOverride>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut force_portable = false;
    let mut force_installed = false;

    for arg in args.into_iter().skip(1) {
        match arg.as_ref() {
            "--portable" => force_portable = true,
            "--installed" => force_installed = true,
            _ => {}
        }
    }

    if force_portable && force_installed {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "conflicting CLI overrides: --portable and --installed",
        ));
    }

    if force_portable {
        return Ok(Some(CliRunModeOverride::Portable));
    }
    if force_installed {
        return Ok(Some(CliRunModeOverride::Installed));
    }
    Ok(None)
}

fn detect_portable_app_home(exe_dir: &Path) -> Option<PathBuf> {
    let exe_dir_name = exe_dir.file_name()?.to_string_lossy().to_ascii_lowercase();
    if exe_dir_name != "bin" {
        return None;
    }

    let app_home = exe_dir.parent()?.to_path_buf();
    if app_home.join(PORTABLE_MARKER_FILE).is_file() {
        return Some(app_home);
    }

    // Markerless fallback still requires a strong, deterministic layout signal.
    let has_layout = app_home.join("conf").is_dir()
        && app_home.join("data").is_dir()
        && app_home.join("log").is_dir();
    if has_layout {
        Some(app_home)
    } else {
        None
    }
}

fn forced_portable_app_home(exe_dir: &Path) -> io::Result<PathBuf> {
    exe_dir.parent().map(Path::to_path_buf).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "failed to resolve portable APP_HOME from executable directory",
        )
    })
}

fn detect_dev_app_home(exe_dir: &Path) -> Option<PathBuf> {
    let build_kind = exe_dir.file_name()?.to_string_lossy().to_ascii_lowercase();
    if build_kind != "debug" && build_kind != "release" {
        return None;
    }

    let target_dir = exe_dir.parent()?;
    if target_dir.file_name()? != OsStr::new("target") {
        return None;
    }

    let repo_root = target_dir.parent()?.to_path_buf();
    if repo_root.join("Cargo.toml").is_file() {
        Some(repo_root)
    } else {
        None
    }
}

fn installed_app_home(user_home: Option<PathBuf>) -> io::Result<PathBuf> {
    let home = user_home.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "failed to resolve user home directory for installed fallback",
        )
    })?;
    Ok(home.join(format!(".{APP_NAME}")))
}

fn os_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        env::var_os("HOME").map(PathBuf::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn new_temp_root(name: &str) -> PathBuf {
        let mut path = env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!("gpui_papyru2_{name}_{}_{}", std::process::id(), stamp));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn write_empty_file(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, b"").expect("write file");
    }

    fn remove_temp_root(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn path_test1_env_override_takes_highest_priority() {
        let root = new_temp_root("path_test1");
        let env_home = root.join("env_home");
        let exe_path = root.join("portable").join("bin").join("papyru2.exe");
        let user_home = root.join("user_home");

        let result =
            AppPaths::resolve_from_inputs(Some(env_home.clone()), exe_path, Some(user_home), None)
                .unwrap();

        assert_eq!(result.mode, RunEnvPattern::EnvOverride);
        assert_eq!(result.app_home, env_home);
        assert!(result.conf_dir.is_dir());
        assert!(result.data_dir.is_dir());
        assert!(result.log_dir.is_dir());
        assert!(result.bin_dir.is_dir());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test2_portable_marker_resolves_to_parent_of_bin() {
        let root = new_temp_root("path_test2");
        let app_home = root.join("portable");
        let exe_path = app_home.join("bin").join("papyru2.exe");
        write_empty_file(app_home.join(PORTABLE_MARKER_FILE).as_path());

        let result =
            AppPaths::resolve_from_inputs(None, exe_path, Some(root.join("user_home")), None)
                .unwrap();

        assert_eq!(result.mode, RunEnvPattern::Portable);
        assert_eq!(result.app_home, app_home);
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test3_invalid_portable_layout_does_not_trigger() {
        let root = new_temp_root("path_test3");
        let exe_path = root.join("portable").join("bin").join("papyru2.exe");
        fs::create_dir_all(exe_path.parent().expect("exe parent")).expect("create bin");

        let user_home = root.join("user_home");
        let result =
            AppPaths::resolve_from_inputs(None, exe_path, Some(user_home.clone()), None).unwrap();

        assert_eq!(result.mode, RunEnvPattern::Installed);
        assert_eq!(result.app_home, user_home.join(format!(".{APP_NAME}")));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test4_dev_detects_debug_and_release_layouts() {
        let root = new_temp_root("path_test4");
        let repo_root = root.join("repo");
        write_empty_file(repo_root.join("Cargo.toml").as_path());

        let debug_exe = repo_root.join("target").join("debug").join("papyru2.exe");
        let release_exe = repo_root.join("target").join("release").join("papyru2.exe");

        let debug_result =
            AppPaths::resolve_from_inputs(None, debug_exe, Some(root.join("user_home")), None)
                .unwrap();
        let release_result =
            AppPaths::resolve_from_inputs(None, release_exe, Some(root.join("user_home")), None)
                .unwrap();

        assert_eq!(debug_result.mode, RunEnvPattern::DevCargoRun);
        assert_eq!(debug_result.app_home, repo_root);
        assert_eq!(release_result.mode, RunEnvPattern::DevCargoRun);
        assert_eq!(release_result.app_home, root.join("repo"));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test5_dev_layout_without_cargo_toml_falls_back_to_installed() {
        let root = new_temp_root("path_test5");
        let fake_repo = root.join("fake_repo");
        let exe_path = fake_repo.join("target").join("debug").join("papyru2.exe");
        let user_home = root.join("user_home");

        let result =
            AppPaths::resolve_from_inputs(None, exe_path, Some(user_home.clone()), None).unwrap();

        assert_eq!(result.mode, RunEnvPattern::Installed);
        assert_eq!(result.app_home, user_home.join(format!(".{APP_NAME}")));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test6_installed_fallback_uses_user_home_dot_appname() {
        let root = new_temp_root("path_test6");
        let exe_path = root.join("other").join("layout").join("papyru2.exe");
        let user_home = root.join("user_home");

        let result =
            AppPaths::resolve_from_inputs(None, exe_path, Some(user_home.clone()), None).unwrap();

        assert_eq!(result.mode, RunEnvPattern::Installed);
        assert_eq!(result.app_home, user_home.join(format!(".{APP_NAME}")));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test7_ensure_dirs_is_idempotent() {
        let root = new_temp_root("path_test7");
        let app_home = root.join("idempotent_home");
        let paths = AppPaths::from_home(RunEnvPattern::Installed, app_home.clone());

        paths.ensure_dirs().expect("first ensure_dirs");
        paths.ensure_dirs().expect("second ensure_dirs");

        assert!(paths.app_home.is_dir());
        assert!(paths.conf_dir.is_dir());
        assert!(paths.data_dir.is_dir());
        assert!(paths.user_document_dir.is_dir());
        assert!(paths.log_dir.is_dir());
        assert!(paths.bin_dir.is_dir());
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test8_priority_is_deterministic_when_env_and_portable_both_match() {
        let root = new_temp_root("path_test8");
        let app_home = root.join("portable");
        let exe_path = app_home.join("bin").join("papyru2.exe");
        let env_home = root.join("env_home");

        write_empty_file(app_home.join(PORTABLE_MARKER_FILE).as_path());

        let result = AppPaths::resolve_from_inputs(
            Some(env_home.clone()),
            exe_path,
            Some(root.join("user_home")),
            None,
        )
        .unwrap();

        assert_eq!(result.mode, RunEnvPattern::EnvOverride);
        assert_eq!(result.app_home, env_home);
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test9_config_file_path_resolves_under_conf_dir() {
        let root = new_temp_root("path_test9");
        let paths = AppPaths::from_home(RunEnvPattern::Installed, root.join("app_home"));

        let config_path = paths.config_file_path("app.toml");

        assert_eq!(config_path, paths.conf_dir.join("app.toml"));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test10_log_file_path_resolves_under_log_dir() {
        let root = new_temp_root("path_test10");
        let paths = AppPaths::from_home(RunEnvPattern::Installed, root.join("app_home"));

        let log_path = paths.log_file_path("papyru2.log");

        assert_eq!(log_path, paths.log_dir.join("papyru2.log"));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test11_cli_portable_override_selects_portable_mode() {
        let root = new_temp_root("path_test11");
        let exe_path = root.join("portable").join("bin").join("papyru2.exe");
        let user_home = root.join("user_home");

        let cli_override =
            parse_cli_mode_override(["papyru2.exe", "--portable"]).expect("parse override");
        let result =
            AppPaths::resolve_from_inputs(None, exe_path, Some(user_home), cli_override).unwrap();

        assert_eq!(result.mode, RunEnvPattern::Portable);
        assert_eq!(result.app_home, root.join("portable"));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test12_cli_installed_override_selects_installed_mode() {
        let root = new_temp_root("path_test12");
        let exe_path = root.join("portable").join("bin").join("papyru2.exe");
        let user_home = root.join("user_home");

        let cli_override =
            parse_cli_mode_override(["papyru2.exe", "--installed"]).expect("parse override");
        let result = AppPaths::resolve_from_inputs(
            None,
            exe_path,
            Some(user_home.clone()),
            cli_override,
        )
        .unwrap();

        assert_eq!(result.mode, RunEnvPattern::Installed);
        assert_eq!(result.app_home, user_home.join(format!(".{APP_NAME}")));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test13_cli_override_precedes_env_override() {
        let root = new_temp_root("path_test13");
        let env_home = root.join("env_home");
        let exe_path = root.join("portable").join("bin").join("papyru2.exe");
        let user_home = root.join("user_home");
        let cli_override =
            parse_cli_mode_override(["papyru2.exe", "--portable"]).expect("parse override");

        let result =
            AppPaths::resolve_from_inputs(Some(env_home), exe_path, Some(user_home), cli_override)
                .unwrap();

        assert_eq!(result.mode, RunEnvPattern::Portable);
        assert_eq!(result.app_home, root.join("portable"));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test14_user_document_dir_resolves_under_data_dir() {
        let root = new_temp_root("path_test14");
        let paths = AppPaths::from_home(RunEnvPattern::Installed, root.join("app_home"));

        assert_eq!(paths.user_document_dir, paths.data_dir.join("user_document"));
        remove_temp_root(root.as_path());
    }

    #[test]
    fn path_test15_ensure_dirs_creates_user_document_dir_and_is_idempotent() {
        let root = new_temp_root("path_test15");
        let app_home = root.join("idempotent_user_document_home");
        let paths = AppPaths::from_home(RunEnvPattern::Installed, app_home);

        paths.ensure_dirs().expect("first ensure_dirs");
        paths.ensure_dirs().expect("second ensure_dirs");

        assert!(paths.user_document_dir.is_dir());
        remove_temp_root(root.as_path());
    }
}
