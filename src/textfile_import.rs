use crate::path_resolver;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

pub const BINARY_NAME: &str = "papyru2_textfile_import";
pub const LOG_FILE_NAME: &str = "papyru2_textfile_import.log";

const LOG_SOURCE_PREFIX: &str = "source_dir=";
const TEXT_SAMPLE_BYTES: usize = 8192;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportArgs {
    pub src_dir: PathBuf,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportSummary {
    pub src_dir: PathBuf,
    pub discovered_text_files: usize,
    pub copied_files: usize,
    pub skipped_non_text_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliAction {
    Help,
    Run(ImportArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ImportCandidate {
    source_path: PathBuf,
    modified_at: DateTime<Local>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiscoveryResult {
    candidates: Vec<ImportCandidate>,
    skipped_non_text_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LogPreparation {
    canonical_src_dir: PathBuf,
}

pub fn run_cli_with_app_paths<I, T>(
    args: I,
    app_paths: &path_resolver::AppPaths,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    match parse_args(args) {
        Ok(CliAction::Help) => {
            let _ = writeln!(stdout, "{}", usage_text());
            0
        }
        Ok(CliAction::Run(args)) => match import_text_files(args, app_paths, stdout) {
            Ok(summary) => {
                let _ = writeln!(
                    stdout,
                    "copied {} text file(s); skipped {} non-text file(s).",
                    summary.copied_files, summary.skipped_non_text_files
                );
                0
            }
            Err(error) => {
                let _ = writeln!(stderr, "{BINARY_NAME}: {error:#}");
                1
            }
        },
        Err(message) => {
            let _ = writeln!(stderr, "{BINARY_NAME}: {message}");
            let _ = writeln!(stderr);
            let _ = writeln!(stderr, "{}", usage_text());
            2
        }
    }
}

fn parse_args<I, T>(args: I) -> std::result::Result<CliAction, String>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString>,
{
    let mut iter = args.into_iter().map(Into::into);
    let _program_name = iter.next();
    let mut src_dir: Option<PathBuf> = None;
    let mut force = false;

    while let Some(arg) = iter.next() {
        match arg.to_string_lossy().as_ref() {
            "-h" | "--help" => return Ok(CliAction::Help),
            "--force" => force = true,
            "--src" => {
                let value = iter
                    .next()
                    .ok_or_else(|| "missing value for `--src`".to_string())?;
                let path = PathBuf::from(&value);
                if path.as_os_str().is_empty() {
                    return Err("missing value for `--src`".to_string());
                }
                if src_dir.replace(path).is_some() {
                    return Err("`--src` must be specified only once".to_string());
                }
            }
            "--dest" => return Err("`--dest` is not supported".to_string()),
            unknown => return Err(format!("unknown option `{unknown}`")),
        }
    }

    let src_dir =
        src_dir.ok_or_else(|| "missing required `--src <source-dir>` option".to_string())?;
    Ok(CliAction::Run(ImportArgs { src_dir, force }))
}

fn usage_text() -> &'static str {
    "usage: papyru2_textfile_import --src <source-dir> [--force]"
}

pub fn import_text_files(
    args: ImportArgs,
    app_paths: &path_resolver::AppPaths,
    stdout: &mut dyn Write,
) -> Result<ImportSummary> {
    app_paths
        .ensure_dirs()
        .context("failed to ensure resolver-managed application directories")?;

    let log_path = app_paths.log_file_path(LOG_FILE_NAME);
    let log_prep = prepare_log_file(&log_path, args.src_dir.as_path(), args.force)?;
    let mut log_file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&log_path)
        .with_context(|| format!("failed to create import log at {}", log_path.display()))?;
    append_log_line(
        &mut log_file,
        format!(
            "{LOG_SOURCE_PREFIX}{}",
            log_prep.canonical_src_dir.display()
        ),
    )?;

    let discovery = collect_text_file_candidates(log_prep.canonical_src_dir.as_path())
        .with_context(|| {
            format!(
                "failed to discover text files under {}",
                log_prep.canonical_src_dir.display()
            )
        })?;

    append_log_line(
        &mut log_file,
        format!(
            "scan discovered_text_files={} skipped_non_text_files={}",
            discovery.candidates.len(),
            discovery.skipped_non_text_files
        ),
    )?;

    if discovery.candidates.is_empty() {
        writeln!(
            stdout,
            "no text files found under {}",
            log_prep.canonical_src_dir.display()
        )
        .context("failed to write console output")?;
        append_log_line(&mut log_file, "scan completed without copy candidates")?;
        return Ok(ImportSummary {
            src_dir: log_prep.canonical_src_dir,
            discovered_text_files: 0,
            copied_files: 0,
            skipped_non_text_files: discovery.skipped_non_text_files,
        });
    }

    let mut copied_files = 0usize;
    let total_files = discovery.candidates.len();
    let mut seen_destinations = HashSet::new();
    for (index, candidate) in discovery.candidates.iter().enumerate() {
        let destination_dir = ensure_daily_directory_for_modified_time(
            app_paths.user_document_dir.as_path(),
            candidate.modified_at,
        )
        .with_context(|| {
            format!(
                "failed to create destination directory for {}",
                candidate.source_path.display()
            )
        })?;
        let file_name = candidate
            .source_path
            .file_name()
            .context("source file name missing")?;
        let destination_path =
            resolve_destination_path(destination_dir.as_path(), file_name, &seen_destinations);
        seen_destinations.insert(destination_path.clone());

        writeln!(
            stdout,
            "copy {}/{}: {} -> {}",
            index + 1,
            total_files,
            candidate.source_path.display(),
            destination_path.display()
        )
        .context("failed to write console progress output")?;
        append_log_line(
            &mut log_file,
            format!(
                "copy {}/{} source={} destination={}",
                index + 1,
                total_files,
                candidate.source_path.display(),
                destination_path.display()
            ),
        )?;

        fs::copy(&candidate.source_path, &destination_path).with_context(|| {
            format!(
                "failed to copy {} to {}",
                candidate.source_path.display(),
                destination_path.display()
            )
        })?;
        copied_files += 1;
    }

    append_log_line(
        &mut log_file,
        format!(
            "completed copied_files={} skipped_non_text_files={}",
            copied_files, discovery.skipped_non_text_files
        ),
    )?;

    Ok(ImportSummary {
        src_dir: log_prep.canonical_src_dir,
        discovered_text_files: total_files,
        copied_files,
        skipped_non_text_files: discovery.skipped_non_text_files,
    })
}

fn prepare_log_file(log_path: &Path, src_dir: &Path, force: bool) -> Result<LogPreparation> {
    let canonical_src_dir = canonical_source_dir(src_dir)?;

    if log_path.exists() {
        let logged_source_dir = read_logged_source_dir(log_path)?;
        if let Some(logged_source_dir) = logged_source_dir {
            if logged_source_dir == canonical_src_dir && !force {
                bail!(
                    "{} seemed to be already imported, avoid duplicated import. If you are really certain to proceed, specify --force option. exit",
                    canonical_src_dir.display()
                );
            }
        }

        fs::remove_file(log_path)
            .with_context(|| format!("failed to remove old log file {}", log_path.display()))?;
    }

    Ok(LogPreparation { canonical_src_dir })
}

fn canonical_source_dir(src_dir: &Path) -> Result<PathBuf> {
    if !src_dir.exists() {
        bail!("source directory does not exist: {}", src_dir.display());
    }
    if !src_dir.is_dir() {
        bail!("source path is not a directory: {}", src_dir.display());
    }
    fs::canonicalize(src_dir).with_context(|| {
        format!(
            "failed to canonicalize source directory {}",
            src_dir.display()
        )
    })
}

fn read_logged_source_dir(log_path: &Path) -> Result<Option<PathBuf>> {
    let file =
        File::open(log_path).with_context(|| format!("failed to open {}", log_path.display()))?;
    let mut reader = BufReader::new(file);
    let mut first_line = String::new();
    let bytes_read = reader
        .read_line(&mut first_line)
        .with_context(|| format!("failed to read {}", log_path.display()))?;
    if bytes_read == 0 {
        return Ok(None);
    }
    let trimmed = first_line.trim_end_matches(['\r', '\n']);
    Ok(trimmed.strip_prefix(LOG_SOURCE_PREFIX).map(PathBuf::from))
}

fn append_log_line(log_file: &mut File, message: impl AsRef<str>) -> io::Result<()> {
    writeln!(log_file, "{}", message.as_ref())
}

fn daily_directory_for_modified_time(
    user_document_dir: &Path,
    modified_at: DateTime<Local>,
) -> PathBuf {
    user_document_dir.join(modified_at.format("%Y/%m/%d").to_string())
}

fn ensure_daily_directory_for_modified_time(
    user_document_dir: &Path,
    modified_at: DateTime<Local>,
) -> io::Result<PathBuf> {
    let dir = daily_directory_for_modified_time(user_document_dir, modified_at);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn resolve_destination_path(
    destination_dir: &Path,
    source_file_name: &std::ffi::OsStr,
    reserved_destinations: &HashSet<PathBuf>,
) -> PathBuf {
    let mut suffix = 1usize;
    loop {
        let candidate = destination_dir.join(collision_safe_file_name(source_file_name, suffix));
        if reserved_destinations.contains(&candidate) || candidate.exists() {
            suffix += 1;
            continue;
        }
        return candidate;
    }
}

fn collision_safe_file_name(source_file_name: &std::ffi::OsStr, suffix: usize) -> OsString {
    if suffix == 1 {
        return source_file_name.to_os_string();
    }

    let file_name_path = Path::new(source_file_name);
    let stem = file_name_path.file_stem().unwrap_or(source_file_name);
    let extension = file_name_path.extension();

    let mut candidate = stem.to_os_string();
    candidate.push(format!("_{suffix}"));
    if let Some(extension) = extension {
        candidate.push(".");
        candidate.push(extension);
    }
    candidate
}

fn collect_text_file_candidates(root: &Path) -> Result<DiscoveryResult> {
    let mut dirs = vec![root.to_path_buf()];
    let mut candidates = Vec::new();
    let mut skipped_non_text_files = 0usize;

    while let Some(dir) = dirs.pop() {
        let mut entries = fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?
            .collect::<std::result::Result<Vec<_>, io::Error>>()
            .with_context(|| format!("failed to enumerate directory {}", dir.display()))?;
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            let file_type = entry
                .file_type()
                .with_context(|| format!("failed to inspect {}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                dirs.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }

            if is_probably_text_file(path.as_path())? {
                let modified_at = file_modified_at(path.as_path())?;
                candidates.push(ImportCandidate {
                    source_path: path,
                    modified_at,
                });
            } else {
                skipped_non_text_files += 1;
            }
        }
    }

    candidates.sort_by(|left, right| left.source_path.cmp(&right.source_path));
    Ok(DiscoveryResult {
        candidates,
        skipped_non_text_files,
    })
}

fn file_modified_at(path: &Path) -> Result<DateTime<Local>> {
    let metadata = fs::metadata(path)
        .with_context(|| format!("failed to read metadata {}", path.display()))?;
    let modified = metadata
        .modified()
        .with_context(|| format!("failed to read last modified time {}", path.display()))?;
    Ok(DateTime::<Local>::from(modified))
}

fn is_probably_text_file(path: &Path) -> Result<bool> {
    let mut file =
        File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut buffer = [0u8; TEXT_SAMPLE_BYTES];
    let bytes_read = file
        .read(&mut buffer)
        .with_context(|| format!("failed to read {}", path.display()))?;
    if bytes_read == 0 {
        return Ok(true);
    }

    let sample = &buffer[..bytes_read];
    if sample.contains(&0) {
        return Ok(false);
    }

    let suspicious_controls = sample
        .iter()
        .filter(|byte| {
            matches!(
                **byte,
                0x01..=0x08 | 0x0B | 0x0E..=0x1A | 0x1C..=0x1F | 0x7F
            )
        })
        .count();

    Ok(suspicious_controls * 100 <= bytes_read * 10)
}

#[cfg(test)]
mod tests {
    use super::{
        LOG_FILE_NAME, LOG_SOURCE_PREFIX, daily_directory_for_modified_time, run_cli_with_app_paths,
    };
    use crate::path_resolver;
    use chrono::{Local, TimeZone};
    use filetime::{FileTime, set_file_mtime};
    use std::env;
    use std::ffi::OsString;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use std::io::Write;

    #[test]
    fn tfim_test1_missing_src_prints_error_and_help() {
        let root = new_temp_root("tfim_test1");
        let app_paths = test_app_paths(root.as_path(), "missing_src");

        let (exit_code, stdout, stderr) = run_cli(&app_paths, vec![OsString::from("tfim")]);

        assert_eq!(exit_code, 2);
        assert!(stdout.is_empty());
        assert!(stderr.contains("missing required `--src <source-dir>` option"));
        assert!(stderr.contains("usage: papyru2_textfile_import --src <source-dir> [--force]"));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn tfim_test2_missing_source_directory_fails() {
        let root = new_temp_root("tfim_test2");
        let app_paths = test_app_paths(root.as_path(), "missing_dir");
        let missing_source = root.join("missing-source");

        let (exit_code, stdout, stderr) = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                missing_source.as_os_str().to_os_string(),
            ],
        );

        assert_eq!(exit_code, 1);
        assert!(stdout.is_empty());
        assert!(stderr.contains("source directory does not exist"));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn tfim_test3_recursive_import_uses_mtime_date_folder_and_logs_source_path() {
        let root = new_temp_root("tfim_test3");
        let app_paths = test_app_paths(root.as_path(), "mtime_copy");
        let source_root = root.join("source");
        let nested_dir = source_root.join("alpha").join("beta");
        fs::create_dir_all(&nested_dir).expect("create nested source tree");

        let text_path = nested_dir.join("note.rs");
        fs::write(&text_path, "fn imported() {}\n").expect("write source text file");
        let modified_at = Local
            .with_ymd_and_hms(2026, 1, 2, 13, 45, 11)
            .single()
            .expect("valid local datetime");
        set_file_mtime(
            &text_path,
            FileTime::from_unix_time(modified_at.timestamp(), 0),
        )
        .expect("set file mtime");

        let (exit_code, stdout, stderr) = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                source_root.as_os_str().to_os_string(),
            ],
        );

        assert_eq!(exit_code, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("copy 1/1:"));
        assert!(stdout.contains("copied 1 text file(s); skipped 0 non-text file(s)."));

        let expected_dir =
            daily_directory_for_modified_time(app_paths.user_document_dir.as_path(), modified_at);
        let imported_path = expected_dir.join("note.rs");
        assert_eq!(
            fs::read_to_string(&imported_path).expect("read imported file"),
            "fn imported() {}\n"
        );

        let log_path = app_paths.log_dir.join(LOG_FILE_NAME);
        let log_text = fs::read_to_string(&log_path).expect("read import log");
        let canonical_source = fs::canonicalize(&source_root).expect("canonical source root");
        assert!(
            log_text
                .lines()
                .next()
                .unwrap_or_default()
                .starts_with(LOG_SOURCE_PREFIX)
        );
        assert_eq!(
            log_text.lines().next().unwrap_or_default(),
            format!("{LOG_SOURCE_PREFIX}{}", canonical_source.display())
        );

        remove_temp_root(root.as_path());
    }

    #[test]
    fn tfim_test4_text_detection_uses_file_content_not_suffix() {
        let root = new_temp_root("tfim_test4");
        let app_paths = test_app_paths(root.as_path(), "type_detect");
        let source_root = root.join("source");
        fs::create_dir_all(&source_root).expect("create source tree");

        let text_path = source_root.join("script.rs");
        fs::write(&text_path, "let meaning = 42;\n").expect("write text source");
        let binary_path = source_root.join("looks_like_text.txt");
        fs::write(&binary_path, b"\x00\x01\x02png").expect("write binary source");

        let modified_at = Local
            .with_ymd_and_hms(2026, 2, 3, 9, 30, 0)
            .single()
            .expect("valid local datetime");
        let modified_file_time = FileTime::from_unix_time(modified_at.timestamp(), 0);
        set_file_mtime(&text_path, modified_file_time).expect("set text mtime");
        set_file_mtime(&binary_path, modified_file_time).expect("set binary mtime");

        let (exit_code, stdout, stderr) = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                source_root.as_os_str().to_os_string(),
            ],
        );

        assert_eq!(exit_code, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("copy 1/1:"));
        assert!(stdout.contains("skipped 1 non-text file(s)"));

        let expected_dir =
            daily_directory_for_modified_time(app_paths.user_document_dir.as_path(), modified_at);
        assert!(expected_dir.join("script.rs").is_file());
        assert!(!expected_dir.join("looks_like_text.txt").exists());

        remove_temp_root(root.as_path());
    }

    #[test]
    fn tfim_test5_duplicate_import_without_force_is_rejected() {
        let root = new_temp_root("tfim_test5");
        let app_paths = test_app_paths(root.as_path(), "duplicate_guard");
        let source_root = root.join("source");
        fs::create_dir_all(&source_root).expect("create source tree");

        let text_path = source_root.join("memo.md");
        fs::write(&text_path, "hello\n").expect("write text source");
        let modified_at = Local
            .with_ymd_and_hms(2026, 3, 4, 10, 0, 0)
            .single()
            .expect("valid local datetime");
        set_file_mtime(
            &text_path,
            FileTime::from_unix_time(modified_at.timestamp(), 0),
        )
        .expect("set file mtime");

        let first_run = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                source_root.as_os_str().to_os_string(),
            ],
        );
        assert_eq!(first_run.0, 0);

        let second_run = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                source_root.as_os_str().to_os_string(),
            ],
        );
        assert_eq!(second_run.0, 1);
        assert!(second_run.2.contains("already imported"));
        assert!(second_run.2.contains("--force"));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn tfim_test6_force_rerun_creates_suffix_and_recreates_log() {
        let root = new_temp_root("tfim_test6");
        let app_paths = test_app_paths(root.as_path(), "force_rerun");
        let source_root = root.join("source");
        fs::create_dir_all(&source_root).expect("create source tree");

        let text_path = source_root.join("draft.md");
        fs::write(&text_path, "first\n").expect("write initial source");
        let modified_at = Local
            .with_ymd_and_hms(2026, 4, 5, 14, 0, 0)
            .single()
            .expect("valid local datetime");
        set_file_mtime(
            &text_path,
            FileTime::from_unix_time(modified_at.timestamp(), 0),
        )
        .expect("set file mtime");

        let first_run = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                source_root.as_os_str().to_os_string(),
            ],
        );
        assert_eq!(first_run.0, 0);

        let log_path = app_paths.log_dir.join(LOG_FILE_NAME);
        fs::write(&text_path, "second\n").expect("update source content");
        set_file_mtime(
            &text_path,
            FileTime::from_unix_time(modified_at.timestamp(), 0),
        )
        .expect("restore file mtime");
        fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .expect("open import log for append")
            .write_all(b"stale-marker\n")
            .expect("append stale marker");

        let second_run = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                source_root.as_os_str().to_os_string(),
                OsString::from("--force"),
            ],
        );
        assert_eq!(second_run.0, 0);
        assert!(second_run.2.is_empty());

        let expected_dir =
            daily_directory_for_modified_time(app_paths.user_document_dir.as_path(), modified_at);
        assert_eq!(
            fs::read_to_string(expected_dir.join("draft.md")).expect("read first imported file"),
            "first\n"
        );
        assert_eq!(
            fs::read_to_string(expected_dir.join("draft_2.md"))
                .expect("read suffixed imported file"),
            "second\n"
        );

        let log_text = fs::read_to_string(&log_path).expect("read recreated import log");
        let canonical_source = fs::canonicalize(&source_root).expect("canonical source root");
        assert_eq!(
            log_text.lines().next().unwrap_or_default(),
            format!("{LOG_SOURCE_PREFIX}{}", canonical_source.display())
        );
        assert!(!log_text.contains("stale-marker"));

        remove_temp_root(root.as_path());
    }

    #[test]
    fn tfim_test7_destination_conflict_uses_next_available_suffix() {
        let root = new_temp_root("tfim_test7");
        let app_paths = test_app_paths(root.as_path(), "destination_conflict");
        let source_root = root.join("source");
        fs::create_dir_all(&source_root).expect("create source tree");

        let text_path = source_root.join("draft.md");
        fs::write(&text_path, "import-me\n").expect("write source text");
        let modified_at = Local
            .with_ymd_and_hms(2026, 4, 6, 8, 30, 0)
            .single()
            .expect("valid local datetime");
        set_file_mtime(
            &text_path,
            FileTime::from_unix_time(modified_at.timestamp(), 0),
        )
        .expect("set source mtime");

        let expected_dir =
            daily_directory_for_modified_time(app_paths.user_document_dir.as_path(), modified_at);
        fs::create_dir_all(&expected_dir).expect("create destination daily dir");
        fs::write(expected_dir.join("draft.md"), "existing-1\n")
            .expect("write existing destination file");
        fs::write(expected_dir.join("draft_2.md"), "existing-2\n")
            .expect("write second existing destination file");

        let run = run_cli(
            &app_paths,
            vec![
                OsString::from("tfim"),
                OsString::from("--src"),
                source_root.as_os_str().to_os_string(),
            ],
        );
        assert_eq!(run.0, 0);
        assert!(run.2.is_empty());
        assert!(run.1.contains("draft_3.md"));

        assert_eq!(
            fs::read_to_string(expected_dir.join("draft.md"))
                .expect("read existing destination file"),
            "existing-1\n"
        );
        assert_eq!(
            fs::read_to_string(expected_dir.join("draft_2.md"))
                .expect("read second existing destination file"),
            "existing-2\n"
        );
        assert_eq!(
            fs::read_to_string(expected_dir.join("draft_3.md"))
                .expect("read third destination file"),
            "import-me\n"
        );

        remove_temp_root(root.as_path());
    }

    fn run_cli(app_paths: &path_resolver::AppPaths, args: Vec<OsString>) -> (i32, String, String) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit_code = run_cli_with_app_paths(args, app_paths, &mut stdout, &mut stderr);
        (
            exit_code,
            String::from_utf8(stdout).expect("stdout should be utf8"),
            String::from_utf8(stderr).expect("stderr should be utf8"),
        )
    }

    fn test_app_paths(root: &Path, suffix: &str) -> path_resolver::AppPaths {
        let app_home = root.join(format!("app_home_{suffix}"));
        let paths = path_resolver::AppPaths {
            mode: path_resolver::RunEnvPattern::Installed,
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
        paths.ensure_dirs().expect("ensure test app dirs");
        paths
    }

    fn new_temp_root(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time after epoch")
            .as_nanos();
        let path =
            env::temp_dir().join(format!("papyru2_{label}_{}_{}", std::process::id(), nanos));
        fs::create_dir_all(&path).expect("create temp root");
        path
    }

    fn remove_temp_root(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
