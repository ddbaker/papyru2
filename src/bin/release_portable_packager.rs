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
const LICENSE_FILE_NAME: &str = "LICENSE";
const THIRD_PARTY_NOTICES_FILE_NAME: &str = "THIRD_PARTY_NOTICES.md";
const LICENSES_DIR_NAME: &str = "licenses";
const HARPER_LICENSE_SOURCE: &str = "licenses/harper/LICENSE-Apache-2.0.txt";
const HARPER_LICENSE_ARCHIVE_PATH: &str = "harper/LICENSE-Apache-2.0.txt";
const PORTABLE_BINARY_NAMES: [&str; 3] = [
    APP_BINARY_NAME,
    PIN_BINARY_NAME,
    TEXTFILE_IMPORT_BINARY_NAME,
];
const ICON_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];

struct PortableIcon {
    binary_name: &'static str,
    display_name: &'static str,
    bundle_identifier: &'static str,
    macos_icns: &'static str,
    linux_png_prefix: &'static str,
}

const PORTABLE_ICONS: [PortableIcon; 3] = [
    PortableIcon {
        binary_name: APP_BINARY_NAME,
        display_name: "papyru2",
        bundle_identifier: "com.papyru2.app",
        macos_icns: "assets/icons/macos/papyru2_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2",
    },
    PortableIcon {
        binary_name: PIN_BINARY_NAME,
        display_name: "papyru2 Pin File",
        bundle_identifier: "com.papyru2.pin-file",
        macos_icns: "assets/icons/macos/papyru2_pin_file_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2_pin_file",
    },
    PortableIcon {
        binary_name: TEXTFILE_IMPORT_BINARY_NAME,
        display_name: "papyru2 Text File Import",
        bundle_identifier: "com.papyru2.textfile-import",
        macos_icns: "assets/icons/macos/papyru2_textfile_import_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2_textfile_import",
    },
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

    fn harper_executable_name(self) -> &'static str {
        match self {
            Self::Windows => "harper-ls.exe",
            Self::Linux | Self::Macos => "harper-ls",
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
    let staged_licenses_dir = staged_root.join(LICENSES_DIR_NAME);
    let staged_harper_licenses_dir = staged_licenses_dir.join("harper");
    fs::create_dir_all(&staged_bin_dir)
        .with_context(|| format!("failed to create {}", staged_bin_dir.display()))?;
    fs::create_dir_all(&staged_conf_dir)
        .with_context(|| format!("failed to create {}", staged_conf_dir.display()))?;
    fs::create_dir_all(&staged_harper_licenses_dir)
        .with_context(|| format!("failed to create {}", staged_harper_licenses_dir.display()))?;

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

    let harper_source = bin_dir.join(platform.harper_executable_name());
    ensure_path_exists(&harper_source, "harper-ls release binary")?;
    let harper_destination = staged_bin_dir.join(platform.harper_executable_name());
    fs::copy(&harper_source, &harper_destination).with_context(|| {
        format!(
            "failed to copy harper-ls binary from {} to {}",
            harper_source.display(),
            harper_destination.display()
        )
    })?;

    let staged_config_path = staged_conf_dir.join(CONFIG_FILE_NAME);
    fs::copy(config_path, &staged_config_path).with_context(|| {
        format!(
            "failed to copy config file from {} to {}",
            config_path.display(),
            staged_config_path.display()
        )
    })?;

    copy_license_file(
        Path::new(LICENSE_FILE_NAME),
        &staged_licenses_dir.join(LICENSE_FILE_NAME),
    )?;
    copy_license_file(
        Path::new(THIRD_PARTY_NOTICES_FILE_NAME),
        &staged_licenses_dir.join(THIRD_PARTY_NOTICES_FILE_NAME),
    )?;
    copy_license_file(
        Path::new(HARPER_LICENSE_SOURCE),
        &staged_licenses_dir.join(HARPER_LICENSE_ARCHIVE_PATH),
    )?;

    stage_platform_icon_metadata(platform, version, &staged_root)?;

    write_portable_zip(platform, &archive_stem, &staged_root, &zip_path)?;

    Ok(ArtifactLayout {
        archive_stem,
        staged_root,
        zip_path,
    })
}

fn stage_platform_icon_metadata(
    platform: Platform,
    version: &str,
    staged_root: &Path,
) -> Result<()> {
    match platform {
        Platform::Windows => Ok(()),
        Platform::Linux => stage_linux_icons(staged_root),
        Platform::Macos => stage_macos_app_bundles(version, staged_root),
    }
}

fn stage_linux_icons(staged_root: &Path) -> Result<()> {
    let applications_dir = staged_root.join("share").join("applications");
    fs::create_dir_all(&applications_dir)
        .with_context(|| format!("failed to create {}", applications_dir.display()))?;

    for icon in PORTABLE_ICONS {
        for size in ICON_SIZES {
            let source = linux_png_path(&icon, size);
            ensure_path_exists(&source, "Linux icon")?;
            let destination_dir = staged_root
                .join("share")
                .join("icons")
                .join("hicolor")
                .join(format!("{size}x{size}"))
                .join("apps");
            fs::create_dir_all(&destination_dir)
                .with_context(|| format!("failed to create {}", destination_dir.display()))?;
            let destination = destination_dir.join(format!("{}.png", icon.binary_name));
            fs::copy(&source, &destination).with_context(|| {
                format!(
                    "failed to copy Linux icon from {} to {}",
                    source.display(),
                    destination.display()
                )
            })?;
        }

        let desktop_path = applications_dir.join(format!("{}.desktop", icon.binary_name));
        fs::write(&desktop_path, linux_desktop_entry(&icon)).with_context(|| {
            format!(
                "failed to write Linux desktop entry {}",
                desktop_path.display()
            )
        })?;
    }

    Ok(())
}

fn stage_macos_app_bundles(version: &str, staged_root: &Path) -> Result<()> {
    let apps_dir = staged_root.join("apps");
    fs::create_dir_all(&apps_dir)
        .with_context(|| format!("failed to create {}", apps_dir.display()))?;

    for icon in PORTABLE_ICONS {
        let app_root = apps_dir.join(format!("{}.app", icon.binary_name));
        let contents_dir = app_root.join("Contents");
        let macos_dir = contents_dir.join("MacOS");
        let resources_dir = contents_dir.join("Resources");
        fs::create_dir_all(&macos_dir)
            .with_context(|| format!("failed to create {}", macos_dir.display()))?;
        fs::create_dir_all(&resources_dir)
            .with_context(|| format!("failed to create {}", resources_dir.display()))?;

        let binary_name = Platform::Macos.executable_name(icon.binary_name);
        let binary_source = staged_root.join("bin").join(&binary_name);
        ensure_path_exists(&binary_source, "macOS release binary")?;
        let binary_destination = macos_dir.join(&binary_name);
        fs::copy(&binary_source, &binary_destination).with_context(|| {
            format!(
                "failed to copy macOS app executable from {} to {}",
                binary_source.display(),
                binary_destination.display()
            )
        })?;

        let icon_source = Path::new(icon.macos_icns);
        ensure_path_exists(icon_source, "macOS icon")?;
        let icon_file_name = icon_source
            .file_name()
            .context("macOS icon file name missing")?;
        let icon_destination = resources_dir.join(icon_file_name);
        fs::copy(icon_source, &icon_destination).with_context(|| {
            format!(
                "failed to copy macOS icon from {} to {}",
                icon_source.display(),
                icon_destination.display()
            )
        })?;

        let plist_path = contents_dir.join("Info.plist");
        fs::write(
            &plist_path,
            macos_info_plist(&icon, version, icon_file_name.to_string_lossy().as_ref()),
        )
        .with_context(|| format!("failed to write {}", plist_path.display()))?;
    }

    Ok(())
}

fn linux_png_path(icon: &PortableIcon, size: u32) -> PathBuf {
    PathBuf::from(format!("{}_{}x{}.png", icon.linux_png_prefix, size, size))
}

fn linux_desktop_entry(icon: &PortableIcon) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nTerminal=false\nCategories=Utility;\n",
        icon.display_name, icon.binary_name, icon.binary_name
    )
}

fn macos_info_plist(icon: &PortableIcon, version: &str, icon_file_name: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDisplayName</key>
  <string>{display_name}</string>
  <key>CFBundleExecutable</key>
  <string>{binary_name}</string>
  <key>CFBundleIconFile</key>
  <string>{icon_file_name}</string>
  <key>CFBundleIdentifier</key>
  <string>{bundle_identifier}</string>
  <key>CFBundleName</key>
  <string>{display_name}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>{version}</string>
  <key>CFBundleVersion</key>
  <string>{version}</string>
</dict>
</plist>
"#,
        display_name = xml_escape(icon.display_name),
        binary_name = xml_escape(icon.binary_name),
        icon_file_name = xml_escape(icon_file_name),
        bundle_identifier = xml_escape(icon.bundle_identifier),
        version = xml_escape(&version.replace('_', "."))
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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

fn copy_license_file(source: &Path, destination: &Path) -> Result<()> {
    ensure_path_exists(source, "license file")?;
    fs::copy(source, destination).with_context(|| {
        format!(
            "failed to copy license file from {} to {}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
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

    add_staged_tree_to_zip(
        &mut zip,
        platform,
        archive_stem,
        staged_root,
        file_options,
        dir_options,
        executable_options,
    )?;

    zip.finish().context("failed to finalize zip archive")?;
    Ok(())
}

fn add_staged_tree_to_zip(
    zip: &mut ZipWriter<File>,
    platform: Platform,
    archive_stem: &str,
    staged_root: &Path,
    file_options: SimpleFileOptions,
    dir_options: SimpleFileOptions,
    executable_options: SimpleFileOptions,
) -> Result<()> {
    add_directory_children_to_zip(
        zip,
        platform,
        archive_stem,
        staged_root,
        staged_root,
        file_options,
        dir_options,
        executable_options,
    )
}

fn add_directory_children_to_zip(
    zip: &mut ZipWriter<File>,
    platform: Platform,
    archive_stem: &str,
    staged_root: &Path,
    current_dir: &Path,
    file_options: SimpleFileOptions,
    dir_options: SimpleFileOptions,
    executable_options: SimpleFileOptions,
) -> Result<()> {
    let mut entries = fs::read_dir(current_dir)
        .with_context(|| format!("failed to read {}", current_dir.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .with_context(|| format!("failed to read entry under {}", current_dir.display()))?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries {
        add_path_to_zip(
            zip,
            platform,
            archive_stem,
            staged_root,
            &entry.path(),
            file_options,
            dir_options,
            executable_options,
        )?;
    }

    Ok(())
}

fn add_path_to_zip(
    zip: &mut ZipWriter<File>,
    platform: Platform,
    archive_stem: &str,
    staged_root: &Path,
    path: &Path,
    file_options: SimpleFileOptions,
    dir_options: SimpleFileOptions,
    executable_options: SimpleFileOptions,
) -> Result<()> {
    let relative_path = path
        .strip_prefix(staged_root)
        .with_context(|| format!("failed to relativize {}", path.display()))?;
    let zip_path = zip_entry_path(archive_stem, relative_path)?;
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat {}", path.display()))?;

    if metadata.is_dir() {
        zip.add_directory(format!("{zip_path}/"), dir_options)
            .with_context(|| format!("failed to add zip directory {zip_path}"))?;
        add_directory_children_to_zip(
            zip,
            platform,
            archive_stem,
            staged_root,
            path,
            file_options,
            dir_options,
            executable_options,
        )?;
    } else {
        let options = if is_executable_zip_entry(platform, relative_path) {
            executable_options
        } else {
            file_options
        };
        add_file_to_zip(zip, path, &zip_path, options)?;
    }

    Ok(())
}

fn zip_entry_path(archive_stem: &str, relative_path: &Path) -> Result<String> {
    let mut parts = vec![archive_stem.to_owned()];
    for component in relative_path.components() {
        parts.push(
            component
                .as_os_str()
                .to_str()
                .context("zip paths must be UTF-8")?
                .to_owned(),
        );
    }
    Ok(parts.join("/"))
}

fn is_executable_zip_entry(platform: Platform, relative_path: &Path) -> bool {
    let components = relative_path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();

    match components.as_slice() {
        ["bin", file_name] => {
            PORTABLE_BINARY_NAMES
                .iter()
                .any(|binary_name| platform.executable_name(binary_name) == *file_name)
                || platform.harper_executable_name() == *file_name
        }
        ["apps", _, "Contents", "MacOS", file_name] => PORTABLE_BINARY_NAMES
            .iter()
            .any(|binary_name| platform.executable_name(binary_name) == *file_name),
        _ => false,
    }
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

    #[test]
    fn package_portable_release_creates_macos_app_bundles() -> Result<()> {
        assert_packaging_round_trip(Platform::Macos, "3_1_0")
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
        let harper_binary = platform.harper_executable_name();
        fs::write(bin_dir.join(&app_binary), b"main-binary")?;
        fs::write(bin_dir.join(&pin_binary), b"pin-binary")?;
        fs::write(bin_dir.join(&import_binary), b"import-binary")?;
        fs::write(bin_dir.join(harper_binary), b"harper-binary")?;

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
                .by_name(&format!("{root_name}/bin/{harper_binary}"))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!("{root_name}/conf/{CONFIG_FILE_NAME}"))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!(
                    "{root_name}/{LICENSES_DIR_NAME}/{LICENSE_FILE_NAME}"
                ))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!(
                    "{root_name}/{LICENSES_DIR_NAME}/{THIRD_PARTY_NOTICES_FILE_NAME}"
                ))
                .is_ok()
        );
        assert!(
            archive
                .by_name(&format!(
                    "{root_name}/{LICENSES_DIR_NAME}/{HARPER_LICENSE_ARCHIVE_PATH}"
                ))
                .is_ok()
        );
        assert_platform_icon_metadata(&mut archive, platform, &root_name)?;

        fs::remove_dir_all(&temp_root)?;
        Ok(())
    }

    #[test]
    fn spchk_pack_test1_windows_harper_executable_name_is_windows_binary() {
        assert_eq!(Platform::Windows.harper_executable_name(), "harper-ls.exe");
    }

    #[test]
    fn spchk_pack_test2_linux_harper_executable_name_is_unix_binary() {
        assert_eq!(Platform::Linux.harper_executable_name(), "harper-ls");
    }

    #[test]
    fn spchk_pack_test3_macos_harper_executable_name_is_unix_binary() {
        assert_eq!(Platform::Macos.harper_executable_name(), "harper-ls");
    }

    #[test]
    fn spchk_pack_test4_harper_zip_entries_are_executable() {
        assert!(is_executable_zip_entry(
            Platform::Windows,
            Path::new("bin/harper-ls.exe")
        ));
        assert!(is_executable_zip_entry(
            Platform::Linux,
            Path::new("bin/harper-ls")
        ));
        assert!(is_executable_zip_entry(
            Platform::Macos,
            Path::new("bin/harper-ls")
        ));
    }

    fn assert_platform_icon_metadata(
        archive: &mut ZipArchive<File>,
        platform: Platform,
        root_name: &str,
    ) -> Result<()> {
        match platform {
            Platform::Windows => {
                assert!(
                    archive
                        .by_name(&format!("{root_name}/icons/windows/papyru2_app_icon.ico"))
                        .is_err()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/icons/windows/papyru2_pin_file_app_icon.ico"
                        ))
                        .is_err()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/icons/windows/papyru2_textfile_import_app_icon.ico"
                        ))
                        .is_err()
                );
            }
            Platform::Linux => {
                assert!(
                    archive
                        .by_name(&format!("{root_name}/share/applications/papyru2.desktop"))
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/share/applications/papyru2_pin_file.desktop"
                        ))
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/share/applications/papyru2_textfile_import.desktop"
                        ))
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/share/icons/hicolor/512x512/apps/papyru2.png"
                        ))
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/share/icons/hicolor/512x512/apps/papyru2_pin_file.png"
                        ))
                        .is_ok()
                );
                assert!(archive
                    .by_name(&format!(
                        "{root_name}/share/icons/hicolor/512x512/apps/papyru2_textfile_import.png"
                    ))
                    .is_ok());
            }
            Platform::Macos => {
                assert!(
                    archive
                        .by_name(&format!("{root_name}/apps/papyru2.app/Contents/Info.plist"))
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/apps/papyru2_pin_file.app/Contents/Info.plist"
                        ))
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/apps/papyru2_textfile_import.app/Contents/Info.plist"
                        ))
                        .is_ok()
                );
                assert!(archive
                    .by_name(&format!(
                        "{root_name}/apps/papyru2_pin_file.app/Contents/Resources/papyru2_pin_file_app_icon.icns"
                    ))
                    .is_ok());
                assert!(archive
                    .by_name(&format!(
                        "{root_name}/apps/papyru2_textfile_import.app/Contents/MacOS/papyru2_textfile_import"
                    ))
                    .is_ok());
            }
        }

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

    // Opt-in QA for a real release: reads payloads/resources and never deletes files.
    // Supply a fresh PAPYRU2_RELEASE_OUTPUT for prepare, then the generated ZIP for verify.
    #[test]
    #[ignore = "requires built Windows release binaries and an explicit output path"]
    fn local_windows_release_prepare() -> Result<()> {
        let output =
            PathBuf::from(env::var_os("PAPYRU2_RELEASE_OUTPUT").context("output required")?);
        assert!(!output.exists(), "release output must be fresh");
        let mut harper = zip::ZipArchive::new(File::open(
            "assets/harper-ls/harper-ls-x86_64-pc-windows-msvc.zip",
        )?)?;
        let mut expected = Vec::new();
        harper
            .by_name("harper-ls.exe")?
            .read_to_end(&mut expected)?;
        let staged = Path::new("target/release/harper-ls.exe");
        if staged.exists() {
            assert_eq!(
                fs::read(staged)?,
                expected,
                "staged Harper differs from asset"
            );
        } else {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(staged)?;
            file.write_all(&expected)?;
        }
        println!("fresh_output={}", output.display());
        println!("harper_asset_bytes_verified={}", expected.len());
        Ok(())
    }

    #[test]
    #[ignore = "requires a real packaged Windows release ZIP"]
    fn local_windows_release_verify() -> Result<()> {
        let zip_path = PathBuf::from(env::var_os("PAPYRU2_RELEASE_ZIP").context("ZIP required")?);
        let root = format!(
            "papyru2-windows-{}",
            normalize_version(env!("CARGO_PKG_VERSION"))
        );
        let staged = zip_path.parent().context("ZIP parent")?.join(&root);
        let mut archive = zip::ZipArchive::new(File::open(&zip_path)?)?;
        let payloads = [
            ("bin/papyru2.exe", "target/release/papyru2.exe"),
            (
                "bin/papyru2_pin_file.exe",
                "target/release/papyru2_pin_file.exe",
            ),
            (
                "bin/papyru2_textfile_import.exe",
                "target/release/papyru2_textfile_import.exe",
            ),
            ("bin/harper-ls.exe", "target/release/harper-ls.exe"),
            ("conf/papyru2_conf.toml", "conf/papyru2_conf.toml"),
            ("licenses/LICENSE", "LICENSE"),
            ("licenses/THIRD_PARTY_NOTICES.md", "THIRD_PARTY_NOTICES.md"),
            (
                "licenses/harper/LICENSE-Apache-2.0.txt",
                HARPER_LICENSE_SOURCE,
            ),
        ];
        let mut actual = std::collections::BTreeSet::new();
        for index in 0..archive.len() {
            let entry = archive.by_index(index)?;
            if !entry.is_dir() {
                assert!(
                    actual.insert(entry.name().to_owned()),
                    "duplicate ZIP payload"
                );
            }
        }
        let mut expected = std::collections::BTreeSet::new();
        for (relative, source) in payloads {
            let name = format!("{root}/{relative}");
            expected.insert(name.clone());
            let mut bytes = Vec::new();
            archive.by_name(&name)?.read_to_end(&mut bytes)?;
            assert_eq!(bytes, fs::read(source)?, "ZIP payload differs: {relative}");
            assert_eq!(
                bytes,
                fs::read(staged.join(relative))?,
                "stage differs: {relative}"
            );
        }
        let marker = format!("{root}/{PORTABLE_MARKER_FILE}");
        expected.insert(marker.clone());
        assert_eq!(
            archive.by_name(&marker)?.size(),
            0,
            "portable marker must be empty"
        );
        assert_eq!(actual, expected, "unexpected or missing ZIP payloads");

        for (binary, icon) in [
            ("papyru2", "assets/icons/windows/papyru2_app_icon.ico"),
            (
                "papyru2_pin_file",
                "assets/icons/windows/papyru2_pin_file_app_icon.ico",
            ),
            (
                "papyru2_textfile_import",
                "assets/icons/windows/papyru2_textfile_import_app_icon.ico",
            ),
        ] {
            let image = fs::read(staged.join(format!("bin/{binary}.exe")))?;
            let ico = fs::read(icon)?;
            let mut expected_icons = Vec::new();
            for index in 0..read_u16(&ico, 4)? as usize {
                let entry = 6 + index * 16;
                let length = read_u32(&ico, entry + 8)? as usize;
                let offset = read_u32(&ico, entry + 12)? as usize;
                expected_icons.push(
                    ico.get(offset..offset + length)
                        .context("ICO image")?
                        .to_vec(),
                );
            }
            let mut actual_icons = pe_resource_payloads(&image, 3)?;
            actual_icons.sort();
            expected_icons.sort();
            assert_eq!(
                actual_icons, expected_icons,
                "embedded icon differs: {binary}"
            );
            let versions = pe_resource_payloads(&image, 16)?;
            assert_eq!(versions.len(), 1, "version resource missing: {binary}");
            let strings: String = char::decode_utf16(
                versions[0]
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]])),
            )
            .map(|ch| ch.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
            assert!(
                strings.contains(env!("CARGO_PKG_VERSION")),
                "version differs: {binary}"
            );
            println!(
                "verified_binary={binary} version={} icon_images={}",
                env!("CARGO_PKG_VERSION"),
                actual_icons.len()
            );
        }
        let hash = std::process::Command::new("certutil")
            .arg("-hashfile")
            .arg(&zip_path)
            .arg("SHA256")
            .output()?;
        assert!(hash.status.success(), "SHA256 command failed");
        println!("archive_size={}", fs::metadata(&zip_path)?.len());
        println!("{}", String::from_utf8_lossy(&hash.stdout));
        println!("verified_zip_payloads={}", actual.len());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires a freshly packaged release and a free application instance port"]
    fn local_windows_release_startup() -> Result<()> {
        use std::os::windows::process::CommandExt as _;
        use std::time::{Duration, Instant};
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, SW_HIDE,
            ShowWindow, WM_CLOSE,
        };
        use windows::core::BOOL;

        struct WindowProbe {
            pid: u32,
            hwnd: HWND,
        }
        unsafe extern "system" fn find_window(hwnd: HWND, parameter: LPARAM) -> BOOL {
            // EnumWindows visits synchronously while the stack probe remains alive.
            let probe = unsafe { &mut *(parameter.0 as *mut WindowProbe) };
            let mut pid = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid == probe.pid {
                let mut class = [0_u16; 256];
                let length = unsafe { GetClassNameW(hwnd, &mut class) };
                if String::from_utf16_lossy(&class[..length as usize]) == "Zed::Window" {
                    probe.hwnd = hwnd;
                    return BOOL(0);
                }
            }
            BOOL(1)
        }
        let endpoint = std::net::SocketAddr::from(([127, 0, 0, 1], 46927));
        assert!(
            std::net::TcpStream::connect_timeout(&endpoint, Duration::from_millis(100)).is_err(),
            "close the existing papyru2 instance before startup QA"
        );
        let zip_path = PathBuf::from(env::var_os("PAPYRU2_RELEASE_ZIP").context("ZIP required")?);
        let root = format!(
            "papyru2-windows-{}",
            normalize_version(env!("CARGO_PKG_VERSION"))
        );
        let stage = zip_path
            .parent()
            .context("ZIP parent")?
            .join(root)
            .canonicalize()?;
        let log_path = stage.join("log/papyru2_boot_profile.log");
        assert!(
            !log_path.exists(),
            "startup QA requires a fresh staging folder"
        );
        let mut child = std::process::Command::new(stage.join("bin/papyru2.exe"))
            .arg("--portable")
            .env("PAPYRU2_BOOT_PROFILE", "1")
            .current_dir(&stage)
            .creation_flags(0x0800_0000)
            .spawn()?;
        let deadline = Instant::now() + Duration::from_secs(60);
        let mut probe = WindowProbe {
            pid: child.id(),
            hwnd: HWND::default(),
        };
        loop {
            unsafe {
                let _ = EnumWindows(
                    Some(find_window),
                    LPARAM((&mut probe as *mut WindowProbe) as isize),
                );
            }
            if !probe.hwnd.is_invalid() {
                unsafe {
                    let _ = ShowWindow(probe.hwnd, SW_HIDE);
                }
            }
            let log = fs::read_to_string(&log_path).unwrap_or_default();
            if !probe.hwnd.is_invalid() && log.contains("file_tree.initial_load_total") {
                break;
            }
            if let Some(status) = child.try_wait()? {
                bail!("packaged app exited before readiness: {status}; log={log}");
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                bail!("startup timed out; log={log}");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        unsafe {
            PostMessageW(Some(probe.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0))?;
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill()?;
                child.wait()?;
                bail!("packaged app did not exit after native close request");
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        assert!(status.success(), "packaged app exit: {status}");
        let log = fs::read_to_string(&log_path)?;
        assert!(log.contains("startup.open_window") && log.contains("ok=true"));
        println!("startup_pid={} native_window=true exit={status}", probe.pid);
        for line in log.lines().filter(|line| {
            line.contains("single_instance")
                || line.contains("first_render")
                || line.contains("open_window")
                || line.contains("startup_initial_load")
        }) {
            println!("{line}");
        }
        Ok(())
    }

    fn read_u16(bytes: &[u8], offset: usize) -> Result<u16> {
        let value: [u8; 2] = bytes
            .get(offset..offset + 2)
            .context("u16 bounds")?
            .try_into()?;
        Ok(u16::from_le_bytes(value))
    }

    fn read_u32(bytes: &[u8], offset: usize) -> Result<u32> {
        let value: [u8; 4] = bytes
            .get(offset..offset + 4)
            .context("u32 bounds")?
            .try_into()?;
        Ok(u32::from_le_bytes(value))
    }

    fn pe_resource_payloads(image: &[u8], resource_type: u32) -> Result<Vec<Vec<u8>>> {
        let pe = read_u32(image, 0x3c)? as usize;
        assert_eq!(image.get(pe..pe + 4), Some(b"PE\0\0".as_slice()));
        let optional = pe + 24;
        let directories = match read_u16(image, optional)? {
            0x20b => optional + 112,
            0x10b => optional + 96,
            _ => bail!("unsupported PE optional header"),
        };
        let sections = optional + read_u16(image, pe + 20)? as usize;
        let map_rva = |rva: u32| -> Result<usize> {
            for index in 0..read_u16(image, pe + 6)? as usize {
                let section = sections + index * 40;
                let address = read_u32(image, section + 12)?;
                let size = read_u32(image, section + 8)?.max(read_u32(image, section + 16)?);
                if rva >= address && rva - address < size {
                    return Ok((read_u32(image, section + 20)? + rva - address) as usize);
                }
            }
            bail!("PE resource RVA outside sections")
        };
        let base = map_rva(read_u32(image, directories + 16)?)?;
        let entries = |directory: usize| -> Result<Vec<(u32, u32)>> {
            let count = read_u16(image, directory + 12)? as usize
                + read_u16(image, directory + 14)? as usize;
            (0..count)
                .map(|index| {
                    let offset = directory + 16 + index * 8;
                    Ok((read_u32(image, offset)?, read_u32(image, offset + 4)?))
                })
                .collect()
        };
        let (_, types) = entries(base)?
            .into_iter()
            .find(|(id, _)| *id == resource_type)
            .context("PE resource type missing")?;
        assert_ne!(types & 0x8000_0000, 0);
        let mut payloads = Vec::new();
        for (_, names) in entries(base + (types & 0x7fff_ffff) as usize)? {
            assert_ne!(names & 0x8000_0000, 0);
            for (_, data) in entries(base + (names & 0x7fff_ffff) as usize)? {
                assert_eq!(data & 0x8000_0000, 0);
                let data = base + data as usize;
                let start = map_rva(read_u32(image, data)?)?;
                let length = read_u32(image, data + 4)? as usize;
                payloads.push(
                    image
                        .get(start..start + length)
                        .context("PE resource bounds")?
                        .to_vec(),
                );
            }
        }
        Ok(payloads)
    }
}
