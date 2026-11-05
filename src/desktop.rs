//! Linux: register a running AppImage with the desktop, so the window and taskbar show the
//! app icon. On Wayland the compositor ignores the icon a window sets for itself and looks
//! up the `.desktop` file whose name matches the window's app id (`cluster-desk`) instead.
//! Writes the entry and icon into the user's data folder; does nothing outside an AppImage
//! (a `cargo run` binary must not end up in the application menu).

#[cfg(target_os = "linux")]
pub fn integrate() {
    let Some(appimage) = std::env::var_os("APPIMAGE").filter(|p| !p.is_empty()) else { return };
    if let Err(e) = linux::install(std::path::Path::new(&appimage)) {
        eprintln!("desktop integration: {e}");
    }
}

#[cfg(not(target_os = "linux"))]
pub fn integrate() {}

#[cfg(target_os = "linux")]
mod linux {
    use std::path::{Path, PathBuf};

    const ICON: &str = include_str!("../assets/icon.svg");

    fn data_home() -> Option<PathBuf> {
        match std::env::var_os("XDG_DATA_HOME") {
            Some(d) if !d.is_empty() => Some(PathBuf::from(d)),
            _ => std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")),
        }
    }

    /// Quotes a path for the `Exec=` key (Desktop Entry spec, "The Exec key").
    fn quote(s: &str) -> String {
        let mut out = String::from("\"");
        for c in s.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        // `%` is a field code; a literal one is written `%%`. The .desktop parser also
        // unescapes backslashes once more, so double them.
        out.replace('%', "%%").replace('\\', "\\\\")
    }

    pub fn entry(appimage: &str) -> String {
        format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=Cluster Desk\n\
             Comment=Fast desktop Kubernetes UI\n\
             Exec={exec}\n\
             TryExec={appimage}\n\
             Icon=cluster-desk\n\
             Terminal=false\n\
             Categories=Development;\n\
             StartupWMClass=cluster-desk\n\
             X-Cluster-Desk-Managed=true\n",
            exec = quote(appimage),
        )
    }

    /// Writes the file only when its contents differ, so desktop caches are not poked on
    /// every start.
    fn put(path: &Path, contents: &str) -> std::io::Result<()> {
        if std::fs::read_to_string(path).ok().as_deref() == Some(contents) {
            return Ok(());
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, contents)
    }

    pub fn install(appimage: &Path) -> Result<(), String> {
        let appimage = appimage.canonicalize().unwrap_or_else(|_| appimage.to_path_buf());
        let appimage = appimage.to_str().ok_or("AppImage path is not UTF-8")?;
        let data = data_home().ok_or("no HOME")?;
        put(&data.join("icons/hicolor/scalable/apps/cluster-desk.svg"), ICON).map_err(|e| e.to_string())?;
        put(&data.join("applications/cluster-desk.desktop"), &entry(appimage)).map_err(|e| e.to_string())
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn exec_is_quoted() {
            let e = super::entry("/home/me/Apps/Cluster Desk $1 100%.AppImage");
            assert!(e.contains("Exec=\"/home/me/Apps/Cluster Desk \\\\$1 100%%.AppImage\"\n"), "{e}");
        }
    }
}
