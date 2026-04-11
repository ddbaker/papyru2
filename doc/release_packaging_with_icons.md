# papyru2 release packaging (Windows / Linux / macOS)

This document defines repo-local commands and GitHub workflow entry points for release artifacts with application icons wired in:

- Windows executable icon (`.ico`) is embedded by `build.rs` + `winres`.
- Linux/macOS bundle icons are provided via `Cargo.toml` `[package.metadata.bundle]`.
- Portable GitHub release archives are assembled by `src/bin/release_portable_packager.rs`.

## GitHub portable release workflow

GitHub Actions workflow file: `.github/workflows/release-portable.yml`

- Tag-driven release: push a tag matching `v*` such as `v0.12.0`.
- Manual release: run the workflow with `workflow_dispatch` and provide an existing git tag in `release_tag`.
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
  conf/
    papyru2_conf.toml
```

## icon assets used by packaging

- Windows: `assets/icons/windows/papyru2_app_icon.ico`
- macOS: `assets/icons/macos/papyru2_app_icon.icns`
- Linux: `assets/icons/linux/papyru2_16x16.png`
- Linux: `assets/icons/linux/papyru2_32x32.png`
- Linux: `assets/icons/linux/papyru2_64x64.png`
- Linux: `assets/icons/linux/papyru2_128x128.png`
- Linux: `assets/icons/linux/papyru2_256x256.png`
- Linux: `assets/icons/linux/papyru2_512x512.png`
- Linux: `assets/icons/linux/papyru2_1024x1024.png`

## prerequisites

1. Install bundle tool once:

```bash
cargo install cargo-bundle --locked
```

2. Run packaging natively on each target OS (recommended).

## repo commands (Cargo aliases)

Aliases are defined in `.cargo/config.toml`.

- Windows exe build:

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

- Windows executable build: `target/x86_64-pc-windows-msvc/release/papyru2.exe`
- Linux/macOS bundle output root: `target/<triple>/release/bundle/` (format depends on platform/toolchain)
- Portable release zip output root: `dist/papyru2-<platform>-x_y_z.zip`

## local portable zip packaging

Build the release binaries and the packaging helper:

```bash
cargo build --release --bin papyru2 --bin papyru2_pin_file --bin release_portable_packager
```

Create a portable release zip for the current host platform by passing the matching platform token:

```bash
cargo run --release --bin release_portable_packager -- --platform windows --bin-dir target/release --output-dir dist --config-path conf/papyru2_conf.toml
```

Swap `windows` for `linux` or `macos` when packaging on those hosts.

## verification checklist after packaging

1. Launch packaged app on target OS.
2. Confirm app icon in launcher/dock/taskbar/window switcher.
3. If stale icon is shown, clear OS icon cache or remove/re-pin old shortcuts and re-test.
4. Confirm the portable zip contains `papyru2.portable`, `bin/`, and `conf/papyru2_conf.toml`.
