// Embeds the app icon (and file description) into the Windows .exe, so Explorer, the
// taskbar and shortcuts show it. Other platforms take the icon from the package instead
// (AppImage, .app bundle); the running window gets it from assets/icon.png in main.rs.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Cluster Desk")
            .set("FileDescription", "Cluster Desk");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
