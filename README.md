# papyru2 [icon](assets\icons\source\papyru2_app_icon_base.png)
A simple desktop note taking application built with Rust, `gpui`, and `gpui-component`.


## Build from source code

### Example: Windows

```bash
cargo release-win
```

See [doc/release_packaging_with_icons.md](doc/release_packaging_with_icons.md) for Linux/MacOS build.

> [!NOTE]
> Windows icon embedding is wired in `build.rs` and uses `assets/icons/windows/papyru2_app_icon.ico`.

### Run

```bash
cargo run --bin papyru2
```
