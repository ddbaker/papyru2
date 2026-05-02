use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ICON_SIZES: [u32; 7] = [16, 32, 64, 128, 256, 512, 1024];

struct BinaryIcon {
    binary_name: &'static str,
    file_description: &'static str,
    source_svg: &'static str,
    windows_ico: &'static str,
    macos_icns: &'static str,
    linux_png_prefix: &'static str,
}

const BINARY_ICONS: [BinaryIcon; 3] = [
    BinaryIcon {
        binary_name: "papyru2",
        file_description: "papyru2 desktop note-taking application",
        source_svg: "assets/icons/source/paper-duotone-line_number2-512px.svg",
        windows_ico: "assets/icons/windows/papyru2_app_icon.ico",
        macos_icns: "assets/icons/macos/papyru2_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2",
    },
    BinaryIcon {
        binary_name: "papyru2_pin_file",
        file_description: "papyru2 pinned-file helper",
        source_svg: "assets/icons/source/pin-ok-red.svg",
        windows_ico: "assets/icons/windows/papyru2_pin_file_app_icon.ico",
        macos_icns: "assets/icons/macos/papyru2_pin_file_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2_pin_file",
    },
    BinaryIcon {
        binary_name: "papyru2_textfile_import",
        file_description: "papyru2 text-file import helper",
        source_svg: "assets/icons/source/import-2-yg.svg",
        windows_ico: "assets/icons/windows/papyru2_textfile_import_app_icon.ico",
        macos_icns: "assets/icons/macos/papyru2_textfile_import_app_icon.icns",
        linux_png_prefix: "assets/icons/linux/papyru2_textfile_import",
    },
];

fn main() {
    println!("cargo:rerun-if-changed=tools/generate_app_icons/src/main.rs");
    for icon in BINARY_ICONS {
        println!("cargo:rerun-if-changed={}", icon.source_svg);
        println!("cargo:rerun-if-changed={}", icon.windows_ico);
        println!("cargo:rerun-if-changed={}", icon.macos_icns);
        for size in ICON_SIZES {
            println!(
                "cargo:rerun-if-changed={}_{}x{}.png",
                icon.linux_png_prefix, size, size
            );
        }
    }

    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("windows") => compile_windows_icon_resources(),
        Ok("linux") => ensure_linux_icon_assets(),
        Ok("macos") => ensure_macos_icon_assets(),
        _ => {}
    }
}

fn compile_windows_icon_resources() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR must be set"));
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let package_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());

    for icon in BINARY_ICONS {
        ensure_exists(icon.windows_ico);
        let rc_path = out_dir.join(format!("{}_app_icon.rc", icon.binary_name));
        let res_path = out_dir.join(format!("{}_app_icon.res", icon.binary_name));
        write_windows_resource_rc(&rc_path, &icon, &package_version);
        compile_resource_file(&target_env, &rc_path, &res_path);
        println!(
            "cargo:rustc-link-arg-bin={}={}",
            icon.binary_name,
            res_path.display()
        );
    }
}

fn ensure_linux_icon_assets() {
    for icon in BINARY_ICONS {
        for size in ICON_SIZES {
            ensure_exists(&format!("{}_{}x{}.png", icon.linux_png_prefix, size, size));
        }
    }
}

fn ensure_macos_icon_assets() {
    for icon in BINARY_ICONS {
        ensure_exists(icon.macos_icns);
    }
}

fn write_windows_resource_rc(rc_path: &Path, icon: &BinaryIcon, package_version: &str) {
    let canonical_icon = fs::canonicalize(icon.windows_ico)
        .unwrap_or_else(|error| panic!("failed to resolve {}: {error}", icon.windows_ico));
    let escaped_icon = canonical_icon
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let file_version = windows_version_quad(package_version);
    let version_string = rc_string(package_version);
    let product_name = rc_string("papyru2");
    let file_description = rc_string(icon.file_description);
    let internal_name = rc_string(icon.binary_name);
    let original_filename = rc_string(&format!("{}.exe", icon.binary_name));
    let company_name = rc_string("papyru2 contributors");
    let copyright = rc_string("Copyright (C) papyru2 contributors");
    fs::write(
        rc_path,
        format!(
            r#"1 ICON "{escaped_icon}"

1 VERSIONINFO
FILEVERSION {file_version}
PRODUCTVERSION {file_version}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "{company_name}\0"
            VALUE "FileDescription", "{file_description}\0"
            VALUE "FileVersion", "{version_string}\0"
            VALUE "InternalName", "{internal_name}\0"
            VALUE "OriginalFilename", "{original_filename}\0"
            VALUE "ProductName", "{product_name}\0"
            VALUE "ProductVersion", "{version_string}\0"
            VALUE "LegalCopyright", "{copyright}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
        ),
    )
    .unwrap_or_else(|error| panic!("failed to write {}: {error}", rc_path.display()));
}

fn windows_version_quad(version: &str) -> String {
    let mut parts = [0_u16; 4];
    for (index, raw_part) in version.split('.').take(4).enumerate() {
        let digits: String = raw_part
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        if !digits.is_empty() {
            let parsed = digits.parse::<u32>().unwrap_or(0).min(u16::MAX as u32);
            parts[index] = parsed as u16;
        }
    }
    format!("{},{},{},{}", parts[0], parts[1], parts[2], parts[3])
}

fn rc_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn compile_resource_file(target_env: &str, rc_path: &Path, res_path: &Path) {
    let mut command = if target_env == "gnu" {
        let mut command = Command::new("windres");
        command
            .arg(rc_path)
            .arg("-O")
            .arg("coff")
            .arg("-o")
            .arg(res_path);
        command
    } else {
        let mut command = Command::new("rc.exe");
        command.arg("/nologo").arg("/fo").arg(res_path).arg(rc_path);
        command
    };

    let output = command.output().unwrap_or_else(|error| {
        panic!(
            "failed to run resource compiler for {}: {error}",
            rc_path.display()
        )
    });
    if !output.status.success() {
        panic!(
            "resource compiler failed for {}\nstdout:\n{}\nstderr:\n{}",
            rc_path.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn ensure_exists(path: &str) {
    if !Path::new(path).exists() {
        panic!("required app icon asset is missing: {path}");
    }
}
