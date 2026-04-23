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
const ICON_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];

struct PortableIcon {
    binary_name: &'static str,
    display_name: &'static str,
    bundle_identifier: &'static str,
    windows_ico: &'static str,
    macos_icns: &'static str,
    linux_png_prefix: &'static str,
}

const PORTABLE_ICONS: [PortableIcon; 3] = [
    PortableIcon {
        binary_name: APP_BINARY_NAME,
        display_name: "papyru2",
        bundle_identifier: "com.papyru2.app",
        windows_ico: "assets/icons/windows/papyru2_app_icon.ico",
        macos_icns: "assets/icons/macos/papyru2_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2",
    },
    PortableIcon {
        binary_name: PIN_BINARY_NAME,
        display_name: "papyru2 Pin File",
        bundle_identifier: "com.papyru2.pin-file",
        windows_ico: "assets/icons/windows/papyru2_pin_file_app_icon.ico",
        macos_icns: "assets/icons/macos/papyru2_pin_file_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2_pin_file",
    },
    PortableIcon {
        binary_name: TEXTFILE_IMPORT_BINARY_NAME,
        display_name: "papyru2 Text File Import",
        bundle_identifier: "com.papyru2.textfile-import",
        windows_ico: "assets/icons/windows/papyru2_textfile_import_app_icon.ico",
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
        Platform::Windows => stage_windows_icons(staged_root),
        Platform::Linux => stage_linux_icons(staged_root),
        Platform::Macos => stage_macos_app_bundles(version, staged_root),
    }
}

fn stage_windows_icons(staged_root: &Path) -> Result<()> {
    let staged_icon_dir = staged_root.join("icons").join("windows");
    fs::create_dir_all(&staged_icon_dir)
        .with_context(|| format!("failed to create {}", staged_icon_dir.display()))?;

    for icon in PORTABLE_ICONS {
        let source = Path::new(icon.windows_ico);
        ensure_path_exists(source, "Windows icon")?;
        let destination =
            staged_icon_dir.join(source.file_name().context("icon file name missing")?);
        fs::copy(source, &destination).with_context(|| {
            format!(
                "failed to copy Windows icon from {} to {}",
                source.display(),
                destination.display()
            )
        })?;
    }

    Ok(())
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
        ["bin", file_name] => PORTABLE_BINARY_NAMES
            .iter()
            .any(|binary_name| platform.executable_name(binary_name) == *file_name),
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
        assert_platform_icon_metadata(&mut archive, platform, &root_name)?;

        fs::remove_dir_all(&temp_root)?;
        Ok(())
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
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/icons/windows/papyru2_pin_file_app_icon.ico"
                        ))
                        .is_ok()
                );
                assert!(
                    archive
                        .by_name(&format!(
                            "{root_name}/icons/windows/papyru2_textfile_import_app_icon.ico"
                        ))
                        .is_ok()
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
}
