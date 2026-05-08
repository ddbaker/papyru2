# papyru2 release packaging (Windows / Linux / macOS)

This document defines repo-local commands and GitHub workflow entry points for release artifacts with application icons wired in.

The repository currently builds four release binaries:

- `papyru2[.exe]`
- `papyru2_pin_file[.exe]`
- `papyru2_textfile_import[.exe]`
- `release_portable_packager[.exe]`

The portable release archive ships the three runtime/user-facing papyru2 binaries and the platform-specific Harper language-server binary in `bin/`. `release_portable_packager[.exe]` is a build-time helper used to assemble the archive and is not copied into the portable zip.

- Dedicated Windows executable icons (`.ico`) are embedded per binary by `build.rs` with per-bin resource linker arguments.
- Linux release packages include `.desktop` entries bound to hicolor PNG icons.
- macOS release packages include `.app` bundles with `Info.plist` icon metadata and `.icns` resources.
- Portable GitHub release archives are assembled by `src/bin/release_portable_packager.rs`.

## GitHub portable release workflow

GitHub Actions workflow file: `.github/workflows/release-portable.yml`

- Tag-driven release: push a tag matching `v*` such as `v0.12.0`.
- Manual release: run the workflow with `workflow_dispatch` and provide an existing git tag in `release_tag`.
- Icon generation step: `cargo run --release --manifest-path tools/generate_app_icons/Cargo.toml`
- Build step: `cargo build --release --bin papyru2 --bin papyru2_pin_file --bin papyru2_textfile_import --bin release_portable_packager`
- Packaging step: `cargo run --release --bin release_portable_packager -- --platform <windows|linux|macos> --bin-dir target/release --output-dir dist --config-path conf/papyru2_conf.toml`
- Published assets: one `.zip` per platform attached to the matching GitHub Release:
  - `papyru2-windows-x_y_z.zip`
  - `papyru2-linux-x_y_z.zip`
  - `papyru2-macos-x_y_z.zip`

Each archive contains:

```text
papyru2-<platform>-x_y_z/
  papyru2.portable
  bin/
    papyru2[.exe]
    papyru2_pin_file[.exe]
    papyru2_textfile_import[.exe]
    harper-ls[.exe]
  conf/
    papyru2_conf.toml
```

Platform-specific icon handling differs by OS:

- Windows archives do not include `icons/windows/*.ico` sidecars; executable icons are embedded in the `.exe` resources at build time.
- Linux archives include `share/applications/*.desktop` and `share/icons/hicolor/<size>x<size>/apps/*.png`.
- macOS archives include `apps/*.app/Contents/Info.plist`, `apps/*.app/Contents/Resources/*.icns`, and app-local executable copies in `apps/*.app/Contents/MacOS/`.

## icon source mapping

- `papyru2[.exe]`: `assets/icons/source/paper-duotone-line_number2-512px.svg`
- `papyru2_pin_file[.exe]`: `assets/icons/source/pin-ok-red.svg`
- `papyru2_textfile_import[.exe]`: `assets/icons/source/import-2-yg.svg`

## generated icon assets used by build and packaging

- Windows: `assets/icons/windows/papyru2_app_icon.ico`
- Windows: `assets/icons/windows/papyru2_pin_file_app_icon.ico`
- Windows: `assets/icons/windows/papyru2_textfile_import_app_icon.ico`
- macOS: `assets/icons/macos/papyru2_app_icon.icns`
- macOS: `assets/icons/macos/papyru2_pin_file_app_icon.icns`
- macOS: `assets/icons/macos/papyru2_textfile_import_app_icon.icns`
- Linux primary icon set: `assets/icons/linux/papyru2_<size>x<size>.png`
- Linux pin-file icon set: `assets/icons/linux/papyru2_pin_file_<size>x<size>.png`
- Linux textfile-import icon set: `assets/icons/linux/papyru2_textfile_import_<size>x<size>.png`
- Linux sizes: 16, 32, 64, 128, 256, 512, 1024.

## prerequisites

1. Install bundle tool once:

```bash
cargo install cargo-bundle --locked
```

2. Run packaging natively on each target OS (recommended).

## repo commands (Cargo aliases)

Aliases are defined in `.cargo/config.toml`.

- Windows release binaries:

```bash
cargo release-win
```

- Generic bundle for current host OS:

```bash
cargo bundle-release
```

Use this only on Linux/macOS. On Windows, `cargo bundle-release` invokes
experimental MSI packaging and is not the recommended workflow for this
project.

- Linux bundle:

```bash
cargo bundle-linux
```

- macOS Apple Silicon bundle:

```bash
cargo bundle-macos-arm64
```

- macOS Intel bundle:

```bash
cargo bundle-macos-x64
```

## expected outputs

- Native release binary output root: `target/release/`
- Windows release binary output root when using `cargo release-win`: `target/x86_64-pc-windows-msvc/release/`
- Release binaries:
  - `papyru2[.exe]`
  - `papyru2_pin_file[.exe]`
  - `papyru2_textfile_import[.exe]`
  - `release_portable_packager[.exe]`
- Linux/macOS bundle output root: `target/<triple>/release/bundle/` (format depends on platform/toolchain)
- Portable release zip output root: `dist/papyru2-<platform>-x_y_z.zip`

## local portable zip packaging

Build the release binaries and the packaging helper:

```bash
cargo run --release --manifest-path tools/generate_app_icons/Cargo.toml
cargo build --release --bin papyru2 --bin papyru2_pin_file --bin papyru2_textfile_import --bin release_portable_packager
```

Create a portable release zip for the current host platform by passing the matching platform token:

```bash
cargo run --release --bin release_portable_packager -- --platform windows --bin-dir target/release --output-dir dist --config-path conf/papyru2_conf.toml
```

Swap `windows` for `linux` or `macos` when packaging on those hosts.

## verification checklist after packaging

1. Launch packaged app on target OS.
2. Confirm `papyru2`, `papyru2_pin_file`, and `papyru2_textfile_import` show their assigned icons in launcher/dock/taskbar/window switcher where that platform exposes those binaries as applications.
3. If stale icon is shown, clear OS icon cache or remove/re-pin old shortcuts and re-test.
4. Confirm the portable zip contains `papyru2.portable`, `bin/papyru2[.exe]`, `bin/papyru2_pin_file[.exe]`, `bin/papyru2_textfile_import[.exe]`, `bin/harper-ls[.exe]`, and `conf/papyru2_conf.toml`.
5. Confirm the portable zip does not contain `release_portable_packager[.exe]`; it is only the archive assembly helper.
6. Confirm Windows `.exe` resources use the per-binary `.ico` files; sidecar copies are not present under `icons/windows/`.
7. Confirm Linux `.desktop` entries reference matching hicolor icon names under `share/icons/hicolor/`.
8. Confirm macOS `.app` bundles contain `Info.plist`, `Contents/MacOS/<binary>`, and matching `Contents/Resources/*.icns`.
