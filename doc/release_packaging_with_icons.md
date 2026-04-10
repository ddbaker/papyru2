# papyru2 release packaging (Windows / Linux / macOS)

This document defines repo-local commands to produce release artifacts with application icons wired in:

- Windows executable icon (`.ico`) is embedded by `build.rs` + `winres`.
- Linux/macOS bundle icons are provided via `Cargo.toml` `[package.metadata.bundle]`.

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

- Windows build: `target/x86_64-pc-windows-msvc/release/papyru2.exe`
- Bundle output root: `target/<triple>/release/bundle/` (format depends on platform/toolchain)

## verification checklist after packaging

1. Launch packaged app on target OS.
2. Confirm app icon in launcher/dock/taskbar/window switcher.
3. If stale icon is shown, clear OS icon cache or remove/re-pin old shortcuts and re-test.
