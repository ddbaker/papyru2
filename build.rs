#[cfg(windows)]
fn main() {
    const ICON_PATH: &str = "assets/icons/windows/papyru2_app_icon.ico";
    const LINUX_512_ICON_PATH: &str = "assets/icons/linux/papyru2_512x512.png";
    const MAC_ICNS_PATH: &str = "assets/icons/macos/papyru2_app_icon.icns";

    println!("cargo:rerun-if-changed={ICON_PATH}");
    println!("cargo:rerun-if-changed={LINUX_512_ICON_PATH}");
    println!("cargo:rerun-if-changed={MAC_ICNS_PATH}");

    let mut resource = winres::WindowsResource::new();
    resource.set_icon(ICON_PATH);
    resource
        .compile()
        .expect("failed to compile Windows resource metadata");
}

#[cfg(not(windows))]
fn main() {
    const WINDOWS_ICON_PATH: &str = "assets/icons/windows/papyru2_app_icon.ico";
    const LINUX_512_ICON_PATH: &str = "assets/icons/linux/papyru2_512x512.png";
    const MAC_ICNS_PATH: &str = "assets/icons/macos/papyru2_app_icon.icns";

    println!("cargo:rerun-if-changed={WINDOWS_ICON_PATH}");
    println!("cargo:rerun-if-changed={LINUX_512_ICON_PATH}");
    println!("cargo:rerun-if-changed={MAC_ICNS_PATH}");

    #[cfg(target_os = "linux")]
    ensure_exists(LINUX_512_ICON_PATH);

    #[cfg(target_os = "macos")]
    ensure_exists(MAC_ICNS_PATH);
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn ensure_exists(path: &str) {
    if !std::path::Path::new(path).exists() {
        panic!("required app icon asset is missing: {path}");
    }
}
