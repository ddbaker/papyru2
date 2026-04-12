use std::env;
use std::ffi::OsString;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use zip::CompressionMethod;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

const APP_BINARY_NAME: &str = "papyru2";
const PIN_BINARY_NAME: &str = "papyru2_pin_file";
const TEXTFILE_IMPORT_BINARY_NAME: &str = "papyru2_textfile_import";
const PORTABLE_MARKER_FILE: &str = "papyru2.portable";
const CONFIG_FILE_NAME: &str = "papyru2_conf.toml";
const PORTABLE_BINARY_NAMES: [&str; 3] = [
    APP_BINARY_NAME,
    PIN_BINARY_NAME,
    TEXTFILE_IMPORT_BINARY_NAME,
];

fn main() {
    if let Err(error) = run() {
        eprintln!("release_portable_packager failed: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse(env::args_os().skip(1))?;
    let version = normalize_version(env!("CARGO_PKG_VERSION"));
    let artifact = package_portable_release(
        args.platform,
        &version,
        &args.bin_dir,
        &args.output_dir,
        &args.config_path,
    )?;

    println!("portable_archive={}", artifact.archive_stem);
    println!("portable_root={}", artifact.staged_root.display());
    println!("portable_zip={}", artifact.zip_path.display());
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Platform {
    Windows,
    Linux,
    Macos,
}

impl Platform {
    fn parse(raw: &str) -> Result<Self> {
        match raw {
            "windows" => Ok(Self::Windows),
            "linux" => Ok(Self::Linux),
            "macos" => Ok(Self::Macos),
            other => bail!("unsupported platform `{other}`; expected windows|linux|macos"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::Linux => "linux",
            Self::Macos => "macos",
        }
    }

    fn executable_name(self, binary_name: &str) -> String {
        match self {
            Self::Windows => format!("{binary_name}.exe"),
            Self::Linux | Self::Macos => binary_name.to_owned(),
        }
    }
}

#[derive(Debug)]
struct Args {
    platform: Platform,
    bin_dir: PathBuf,
    output_dir: PathBuf,
    config_path: PathBuf,
}

impl Args {
    fn parse(args: impl Iterator<Item = OsString>) -> Result<Self> {
        let mut platform = None;
        let mut bin_dir = None;
        let mut output_dir = None;
        let mut config_path = None;
        let mut args = args.peekable();

        while let Some(flag) = args.next() {
            let flag = flag
                .into_string()
                .map_err(|_| anyhow::anyhow!("arguments must be valid UTF-8"))?;

            match flag.as_str() {
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                "--platform" => {
                    let value = next_arg_value(&mut args, "--platform")?;
                    platform = Some(Platform::parse(&value)?);
                }
                "--bin-dir" => {
                    let value = next_arg_value(&mut args, "--bin-dir")?;
                    bin_dir = Some(PathBuf::from(value));
                }
                "--output-dir" => {
                    let value = next_arg_value(&mut args, "--output-dir")?;
                    output_dir = Some(PathBuf::from(value));
                }
                "--config-path" => {
                    let value = next_arg_value(&mut args, "--config-path")?;
                    config_path = Some(PathBuf::from(value));
                }
                other => bail!("unknown argument `{other}`"),
            }
        }

        Ok(Self {
            platform: platform.context("missing required `--platform`")?,
            bin_dir: bin_dir.context("missing required `--bin-dir`")?,
            output_dir: output_dir.context("missing required `--output-dir`")?,
            config_path: config_path.context("missing required `--config-path`")?,
        })
    }
}

fn print_usage() {
    println!(
        "release_portable_packager --platform <windows|linux|macos> --bin-dir <dir> --output-dir <dir> --config-path <path>"
    );
}

fn next_arg_value(args: &mut impl Iterator<Item = OsString>, flag: &str) -> Result<String> {
    args.next()
        .context(format!("missing value after `{flag}`"))?
        .into_string()
        .map_err(|_| anyhow::anyhow!("arguments must be valid UTF-8"))
}

#[derive(Debug)]
struct ArtifactLayout {
    archive_stem: String,
    staged_root: PathBuf,
    zip_path: PathBuf,
}

fn package_portable_release(
    platform: Platform,
    version: &str,
    bin_dir: &Path,
    output_dir: &Path,
    config_path: &Path,
) -> Result<ArtifactLayout> {
    let archive_stem = format!("papyru2-{}-{version}", platform.as_str());
    let staged_root = output_dir.join(&archive_stem);
    let zip_path = output_dir.join(format!("{archive_stem}.zip"));

    ensure_path_exists(bin_dir, "binary directory")?;
    ensure_path_exists(config_path, "config file")?;
    prepare_output_root(output_dir)?;
    recreate_dir(&staged_root)?;
    if zip_path.exists() {
        fs::remove_file(&zip_path).with_context(|| {
            format!(
                "failed to remove existing archive at {}",
                zip_path.display()
            )
        })?;
    }

    let staged_bin_dir = staged_root.join("bin");
    let staged_conf_dir = staged_root.join("conf");
    fs::create_dir_all(&staged_bin_dir)
        .with_context(|| format!("failed to create {}", staged_bin_dir.display()))?;
    fs::create_dir_all(&staged_conf_dir)
        .with_context(|| format!("failed to create {}", staged_conf_dir.display()))?;

    let marker_path = staged_root.join(PORTABLE_MARKER_FILE);
    File::create(&marker_path)
        .with_context(|| format!("failed to create {}", marker_path.display()))?;

    for binary_name in PORTABLE_BINARY_NAMES {
        let source = bin_dir.join(platform.executable_name(binary_name));
        ensure_path_exists(&source, "release binary")?;
        let destination =
            staged_bin_dir.join(source.file_name().context("binary file name missing")?);
        fs::copy(&source, &destination).with_context(|| {
            format!(
                "failed to copy release binary from {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }

    let staged_config_path = staged_conf_dir.join(CONFIG_FILE_NAME);
    fs::copy(config_path, &staged_config_path).with_context(|| {
        format!(
            "failed to copy config file from {} to {}",
            config_path.display(),
            staged_config_path.display()
        )
    })?;

    write_portable_zip(platform, &archive_stem, &staged_root, &zip_path)?;

    Ok(ArtifactLayout {
        archive_stem,
        staged_root,
        zip_path,
    })
}

fn normalize_version(version: &str) -> String {
    let mut normalized = String::with_capacity(version.len());
    let mut previous_was_separator = false;

    for ch in version.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
            previous_was_separator = false;
        } else if !previous_was_separator {
            normalized.push('_');
            previous_was_separator = true;
        }
    }

    normalized.trim_matches('_').to_owned()
}

fn ensure_path_exists(path: &Path, label: &str) -> Result<()> {
    if path.exists() {
        Ok(())
    } else {
        bail!("{label} does not exist: {}", path.display())
    }
}

fn prepare_output_root(output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        if !output_dir.is_dir() {
            bail!("output path is not a directory: {}", output_dir.display());
        }
        return Ok(());
    }

    fs::create_dir_all(output_dir)
        .with_context(|| format!("failed to create output directory {}", output_dir.display()))
}

fn recreate_dir(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path)
            .with_context(|| format!("failed to remove existing directory {}", path.display()))?;
    }
    fs::create_dir_all(path)
        .with_context(|| format!("failed to create directory {}", path.display()))
}

fn write_portable_zip(
    platform: Platform,
    archive_stem: &str,
    staged_root: &Path,
    zip_path: &Path,
) -> Result<()> {
    let zip_file = File::create(zip_path)
        .with_context(|| format!("failed to create {}", zip_path.display()))?;
    let mut zip = ZipWriter::new(zip_file);

    let dir_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);
    let file_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let executable_options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o755);

    zip.add_directory(format!("{archive_stem}/"), dir_options)
        .context("failed to add zip root directory")?;
    zip.add_directory(format!("{archive_stem}/bin/"), dir_options)
        .context("failed to add zip bin directory")?;
    zip.add_directory(format!("{archive_stem}/conf/"), dir_options)
        .context("failed to add zip conf directory")?;

    add_file_to_zip(
        &mut zip,
        &staged_root.join(PORTABLE_MARKER_FILE),
        &format!("{archive_stem}/{PORTABLE_MARKER_FILE}"),
        file_options,
    )?;
    for binary_name in PORTABLE_BINARY_NAMES {
        add_file_to_zip(
            &mut zip,
            &staged_root
                .join("bin")
                .join(platform.executable_name(binary_name)),
            &format!(
                "{archive_stem}/bin/{}",
                platform.executable_name(binary_name)
            ),
            executable_options,
        )?;
    }
    add_file_to_zip(
        &mut zip,
        &staged_root.join("conf").join(CONFIG_FILE_NAME),
        &format!("{archive_stem}/conf/{CONFIG_FILE_NAME}"),
        file_options,
    )?;

    zip.finish().context("failed to finalize zip archive")?;
    Ok(())
}

fn add_file_to_zip(
    zip: &mut ZipWriter<File>,
    source_path: &Path,
    zip_path: &str,
    options: SimpleFileOptions,
) -> Result<()> {
    let mut source_file = File::open(source_path)
        .with_context(|| format!("failed to open source file {}", source_path.display()))?;
    let mut buffer = Vec::new();
    source_file
        .read_to_end(&mut buffer)
        .with_context(|| format!("failed to read {}", source_path.display()))?;

    zip.start_file(zip_path, options)
        .with_context(|| format!("failed to add zip entry {zip_path}"))?;
    zip.write_all(&buffer)
        .with_context(|| format!("failed to write zip entry {zip_path}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::ZipArchive;

    #[test]
    fn normalize_version_rewrites_semver_for_archive_names() {
        assert_eq!(normalize_version("0.12.0"), "0_12_0");
        assert_eq!(normalize_version("1.2.3-beta.1"), "1_2_3_beta_1");
    }

    #[test]
    fn package_portable_release_creates_linux_layout_and_zip() -> Result<()> {
        assert_packaging_round_trip(Platform::Linux, "1_2_3")
    }

    #[test]
    fn package_portable_release_creates_windows_zip_with_exe_suffixes() -> Result<()> {
        assert_packaging_round_trip(Platform::Windows, "2_0_0")
    }

    fn assert_packaging_round_trip(platform: Platform, version: &str) -> Result<()> {
        let temp_root = unique_temp_dir("portable_packager");
        let bin_dir = temp_root.join("bin-input");
        let out_dir = temp_root.join("dist");
        let config_path = temp_root.join(CONFIG_FILE_NAME);
        fs::create_dir_all(&bin_dir)?;
        fs::write(&config_path, "[debug]\n")?;

        let app_binary = platform.executable_name(APP_BINARY_NAME);
        let pin_binary = platform.executable_name(PIN_BINARY_NAME);
        let import_binary = platform.executable_name(TEXTFILE_IMPORT_BINARY_NAME);
        fs::write(bin_dir.join(&app_binary), b"main-binary")?;
        fs::write(bin_dir.join(&pin_binary), b"pin-binary")?;
        fs::write(bin_dir.join(&import_binary), b"import-binary")?;

        let artifact =
            package_portable_release(platform, version, &bin_dir, &out_dir, &config_path)?;

        let root_name = format!("papyru2-{}-{version}", platform.as_str());
        assert_eq!(artifact.archive_stem, root_name);
        assert!(artifact.staged_root.is_dir());
        assert!(artifact.zip_path.is_file());
        assert_eq!(
            fs::metadata(artifact.staged_root.join(PORTABLE_MARKER_FILE))?.len(),
            0
        );

        let zip_file = File::open(&artifact.zip_path)?;
        let mut archive = ZipArchive::new(zip_file)?;
        assert!(
            archive
                .by_name(&format!("{root_name}/{PORTABLE_MARKER_FILE}"))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!("{root_name}/bin/{app_binary}"))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!("{root_name}/bin/{pin_binary}"))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!("{root_name}/bin/{import_binary}"))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!("{root_name}/conf/{CONFIG_FILE_NAME}"))
                .is_ok()
        );

        fs::remove_dir_all(&temp_root)?;
        Ok(())
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "papyru2_{label}_{}_{}",
            std::process::id(),
            timestamp
        ));
        fs::create_dir_all(&path).expect("failed to create temp directory");
        path
    }
}
