//! Updates from the GitHub releases, the way DiffLab's updater works: every few hours the app
//! reads `latest.json` from the newest release, downloads a newer build in the background,
//! checks its minisign signature against the key built into the app, and installs it when the
//! app quits (or right away from Preferences → Updates, which then restarts the app).
//!
//! What gets replaced: the AppImage file on Linux, `cluster-desk.exe` on Windows, the
//! `Cluster Desk.app` bundle on macOS. Builds made outside CI (no `CLUSTER_DESK_VERSION`) never
//! update themselves.

use eframe::egui;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The release this binary is (set by CI), else the crate version.
pub const VERSION: &str = match option_env!("CLUSTER_DESK_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
const RELEASE_BUILD: bool = option_env!("CLUSTER_DESK_VERSION").is_some();
const MANIFEST_URL: &str = "https://github.com/DiverOfDark/cluster-desk/releases/latest/download/latest.json";
/// minisign public key; the secret key signs the packages in CI (MINISIGN_SECRET_KEY).
const PUBLIC_KEY: &str = "RWRQj//TJJ/8pyT8uYhbZ1KyBRZ6dnaTuau1bhik9nts2x7z+bzF4Ta5";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Names next to the installed app: the downloaded update and the replaced copy. Each running
/// copy of the app uses its own (the process id is added), so one copy never deletes another's.
const STAGED: &str = ".cluster-desk-update";
const OLD: &str = ".cluster-desk-old";
/// An update another copy downloaded but never installed is deleted after this long.
const STALE: Duration = Duration::from_secs(2 * 24 * 3600);

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// Updates can't happen for this copy; the text says why.
    Off(String),
    Idle,
    Checking,
    UpToDate,
    Downloading(String),
    /// Downloaded and checked; installs on quit.
    Ready(String),
    /// Installed; the next start runs it.
    Installed(String),
    Failed(String),
}

#[derive(Deserialize)]
struct Manifest {
    version: String,
    platforms: HashMap<String, Asset>,
}

#[derive(Deserialize)]
struct Asset {
    url: String,
    /// Contents of the `.minisig` file.
    signature: String,
}

/// The installed copy that an update replaces.
#[derive(Clone, Debug)]
enum Target {
    AppImage(PathBuf),
    Exe(PathBuf),
    Bundle(PathBuf),
}

struct Shared {
    state: State,
    checked_at: Option<Instant>,
}

#[derive(Clone)]
pub struct Updater {
    shared: Arc<Mutex<Shared>>,
    target: Option<Target>,
    started: Arc<Mutex<bool>>,
}

impl Default for Updater {
    fn default() -> Self {
        let (state, target) = if !RELEASE_BUILD {
            (State::Off("Development build: updates are off.".into()), None)
        } else {
            match target() {
                Ok(t) => (State::Idle, Some(t)),
                Err(e) => (State::Off(e), None),
            }
        };
        Updater { shared: Arc::new(Mutex::new(Shared { state, checked_at: None })), target, started: Arc::new(Mutex::new(false)) }
    }
}

impl Updater {
    pub fn state(&self) -> State {
        self.shared.lock().unwrap().state.clone()
    }

    fn set(&self, s: State) {
        self.shared.lock().unwrap().state = s;
    }

    /// Start the background checks (once). Not called in screenshot mode.
    pub fn start(&self, ctx: &egui::Context) {
        {
            let mut started = self.started.lock().unwrap();
            if *started || self.target.is_none() {
                return;
            }
            *started = true;
        }
        if let Some(t) = &self.target {
            cleanup(t);
        }
        let me = self.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(10));
            loop {
                let due = {
                    let s = me.shared.lock().unwrap();
                    s.checked_at.map(|t| t.elapsed() >= CHECK_EVERY).unwrap_or(true)
                        && !matches!(s.state, State::Ready(_) | State::Installed(_))
                };
                if due && crate::settings::get().auto_update {
                    me.check_once(&ctx);
                }
                std::thread::sleep(Duration::from_secs(60));
            }
        });
    }

    /// "Check now" in Preferences.
    pub fn check_now(&self, ctx: &egui::Context) {
        if self.target.is_none() {
            return;
        }
        let me = self.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || me.check_once(&ctx));
    }

    fn check_once(&self, ctx: &egui::Context) {
        let Some(target) = self.target.clone() else { return };
        {
            let mut s = self.shared.lock().unwrap();
            if matches!(s.state, State::Checking | State::Downloading(_) | State::Ready(_) | State::Installed(_)) {
                return;
            }
            s.state = State::Checking;
            s.checked_at = Some(Instant::now());
        }
        ctx.request_repaint();
        let r = self.fetch_and_stage(&target, ctx);
        self.set(match r {
            Ok(Some(v)) => State::Ready(v),
            Ok(None) => State::UpToDate,
            Err(e) => State::Failed(e),
        });
        ctx.request_repaint();
    }

    /// Ok(None): already the newest. Ok(Some(version)): downloaded and staged.
    fn fetch_and_stage(&self, target: &Target, ctx: &egui::Context) -> Result<Option<String>, String> {
        // CLUSTER_DESK_UPDATE_URL: another latest.json, for testing the updater. Packages are
        // still checked against the built-in key.
        let url = std::env::var("CLUSTER_DESK_UPDATE_URL").unwrap_or_else(|_| MANIFEST_URL.to_string());
        let body = crate::settings::fetch(&url)?;
        let m: Manifest = serde_json::from_slice(&body).map_err(|e| format!("latest.json: {e}"))?;
        if !newer(&m.version, VERSION) {
            return Ok(None);
        }
        let key = platform_key().ok_or("No updates are built for this platform.")?;
        let asset = m.platforms.get(key).ok_or_else(|| format!("Release {} has no package for {key}.", m.version))?;
        self.set(State::Downloading(m.version.clone()));
        ctx.request_repaint();
        let data = crate::settings::fetch(&asset.url)?;
        verify(&data, &asset.signature, &format!("cluster-desk {} {key}", m.version))?;
        stage(target, &data)?;
        Ok(Some(m.version))
    }

    /// Put a downloaded update in place of the running copy. Returns the version installed.
    pub fn install(&self) -> Result<Option<String>, String> {
        let State::Ready(v) = self.state() else { return Ok(None) };
        let Some(t) = &self.target else { return Ok(None) };
        if !staged_path(t).exists() {
            // Removed behind our back: download it again on the next check.
            let mut s = self.shared.lock().unwrap();
            s.state = State::Idle;
            s.checked_at = None;
            return Err("The downloaded update is gone; it will be downloaded again.".into());
        }
        install(t)?;
        self.set(State::Installed(v.clone()));
        Ok(Some(v))
    }

    /// "Restart to update": install, start the new copy, close this one.
    pub fn restart(&self, ctx: &egui::Context) -> Result<(), String> {
        if matches!(self.state(), State::Ready(_)) {
            self.install()?;
        }
        let Some(t) = &self.target else { return Ok(()) };
        let spawned = match t {
            Target::AppImage(p) | Target::Exe(p) => std::process::Command::new(p).spawn(),
            Target::Bundle(p) => std::process::Command::new("open").arg("-n").arg(p).spawn(),
        };
        spawned.map_err(|e| format!("The update is installed, but the new version did not start: {e}"))?;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        Ok(())
    }
}

/// x.y.z numerically; a missing part counts as 0.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> { v.trim().trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    let (mut a, mut b) = (parse(candidate), parse(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

fn platform_key() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-x86_64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("macos", _) => Some("macos"),
        _ => None,
    }
}

/// The signature must be ours and its trusted comment must name this exact version and
/// platform, so an older signed package can't be served as a newer one.
fn verify(data: &[u8], signature: &str, expected_comment: &str) -> Result<(), String> {
    let pk = minisign_verify::PublicKey::from_base64(PUBLIC_KEY).map_err(|e| format!("public key: {e}"))?;
    let sig = minisign_verify::Signature::decode(signature).map_err(|e| format!("signature: {e}"))?;
    pk.verify(data, &sig, false).map_err(|e| format!("The download failed its signature check ({e}); not installing it."))?;
    if sig.trusted_comment().trim() != expected_comment {
        return Err(format!("The download is signed for \"{}\", expected \"{expected_comment}\"; not installing it.", sig.trusted_comment()));
    }
    Ok(())
}

fn target() -> Result<Target, String> {
    if cfg!(target_os = "linux") {
        return match std::env::var_os("APPIMAGE") {
            Some(p) if !p.is_empty() => Ok(Target::AppImage(PathBuf::from(p))),
            _ => Err("Updates install only into the AppImage from the releases page.".into()),
        };
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe = exe.canonicalize().unwrap_or(exe);
    if cfg!(windows) {
        return Ok(Target::Exe(exe));
    }
    if cfg!(target_os = "macos") {
        let bundle = exe.ancestors().nth(3).map(Path::to_path_buf).filter(|b| b.extension().map(|e| e == "app").unwrap_or(false));
        let Some(bundle) = bundle else {
            return Err("Updates install only into Cluster Desk.app from the releases page.".into());
        };
        let s = bundle.display().to_string();
        if s.starts_with("/Volumes/") || s.contains("/AppTranslocation/") {
            return Err("Move Cluster Desk to the Applications folder to get updates.".into());
        }
        return Ok(Target::Bundle(bundle));
    }
    Err("No updates are built for this platform.".into())
}

fn dir_of(p: &Path) -> PathBuf {
    p.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}

/// Where the downloaded update waits, next to the installed copy so the final move is a rename.
fn staged_path(t: &Target) -> PathBuf {
    let id = std::process::id();
    match t {
        Target::AppImage(p) => dir_of(p).join(format!("{STAGED}-{id}.AppImage")),
        Target::Exe(p) => dir_of(p).join(format!("{STAGED}-{id}.exe")),
        Target::Bundle(p) => dir_of(p).join(format!("{STAGED}-{id}")),
    }
}

fn old_path(t: &Target) -> PathBuf {
    let id = std::process::id();
    match t {
        Target::AppImage(p) => dir_of(p).join(format!("{OLD}-{id}.AppImage")),
        Target::Exe(p) => dir_of(p).join(format!("{OLD}-{id}.exe")),
        Target::Bundle(p) => dir_of(p).join(format!("{OLD}-{id}.app")),
    }
}

fn remove(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

/// Leftovers of earlier updates next to the installed copy. Replaced copies go now (Windows
/// can't delete an exe that is still running, so one in use by another copy of the app stays
/// and is tried again next time). Downloads go only once stale, since another running copy may
/// be waiting to install its own.
fn cleanup(t: &Target) {
    let Ok(entries) = std::fs::read_dir(dir_of(match t {
        Target::AppImage(p) | Target::Exe(p) | Target::Bundle(p) => p,
    })) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let stale = || {
            e.metadata().and_then(|m| m.modified()).ok().and_then(|m| m.elapsed().ok()).map(|age| age >= STALE).unwrap_or(false)
        };
        if name.starts_with(OLD) || (name.starts_with(STAGED) && stale()) {
            remove(&e.path());
        }
    }
}

fn stage(t: &Target, data: &[u8]) -> Result<(), String> {
    let staged = staged_path(t);
    remove(&staged);
    let cant = |e: std::io::Error| format!("Could not save the update next to {}: {e}", dir_of(&staged).display());
    match t {
        Target::AppImage(_) => {
            std::fs::write(&staged, data).map_err(cant)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).map_err(cant)?;
            }
        }
        Target::Exe(_) => {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("update zip: {e}"))?;
            let name = zip
                .file_names()
                .filter_map(|n| n.ok().map(|n| n.to_string()))
                .find(|n| n.rsplit(['/', '\\']).next() == Some("cluster-desk.exe"))
                .ok_or("The update zip has no cluster-desk.exe.")?;
            let mut f = zip.by_name(&name).map_err(|e| format!("update zip: {e}"))?;
            let mut out = std::fs::File::create(&staged).map_err(cant)?;
            std::io::copy(&mut f, &mut out).map_err(cant)?;
        }
        Target::Bundle(_) => {
            std::fs::create_dir_all(&staged).map_err(cant)?;
            tar::Archive::new(flate2::read::GzDecoder::new(data)).unpack(&staged).map_err(cant)?;
            if !staged.join("Cluster Desk.app").is_dir() {
                remove(&staged);
                return Err("The update has no Cluster Desk.app.".into());
            }
        }
    }
    Ok(())
}

fn install(t: &Target) -> Result<(), String> {
    let staged = staged_path(t);
    let old = old_path(t);
    remove(&old);
    let fail = |e: std::io::Error| format!("Could not install the update: {e}");
    match t {
        // The running AppImage stays mounted, so the file can be replaced in one rename.
        Target::AppImage(p) => std::fs::rename(&staged, p).map_err(fail)?,
        // A running exe can be renamed but not overwritten.
        Target::Exe(p) => {
            std::fs::rename(p, &old).map_err(fail)?;
            if let Err(e) = std::fs::rename(&staged, p) {
                let _ = std::fs::rename(&old, p);
                return Err(fail(e));
            }
        }
        Target::Bundle(p) => {
            std::fs::rename(p, &old).map_err(fail)?;
            if let Err(e) = std::fs::rename(staged.join("Cluster Desk.app"), p) {
                let _ = std::fs::rename(&old, p);
                return Err(fail(e));
            }
            remove(&old);
            remove(&staged);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("v0.2.0", "0.1.99"));
        assert!(newer("0.1.1", "0.1"));
        assert!(!newer("0.1.9", "0.1.9"));
        assert!(!newer("0.1.8", "0.1.9"));
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cluster-desk-update-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn appimage_is_replaced() {
        let d = tmp("appimage");
        let t = Target::AppImage(d.join("Cluster.AppImage"));
        std::fs::write(d.join("Cluster.AppImage"), b"old").unwrap();
        stage(&t, b"new").unwrap();
        install(&t).unwrap();
        assert_eq!(std::fs::read(d.join("Cluster.AppImage")).unwrap(), b"new");
        assert!(!staged_path(&t).exists());
    }

    #[test]
    fn exe_is_taken_from_the_zip() {
        use std::io::Write;
        let d = tmp("exe");
        let t = Target::Exe(d.join("cluster-desk.exe"));
        std::fs::write(d.join("cluster-desk.exe"), b"old").unwrap();
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            z.start_file("cluster-desk/README.md", o).unwrap();
            z.write_all(b"readme").unwrap();
            z.start_file("cluster-desk/cluster-desk.exe", o).unwrap();
            z.write_all(b"new").unwrap();
            z.finish().unwrap();
        }
        stage(&t, buf.get_ref()).unwrap();
        install(&t).unwrap();
        assert_eq!(std::fs::read(d.join("cluster-desk.exe")).unwrap(), b"new");
        assert_eq!(std::fs::read(old_path(&t)).unwrap(), b"old");
        cleanup(&t);
        assert!(!old_path(&t).exists());
    }

    #[test]
    fn bundle_is_swapped() {
        let d = tmp("bundle");
        let app = d.join("Cluster Desk.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::write(app.join("Contents/MacOS/cluster-desk"), b"old").unwrap();
        let src = tmp("bundle-src");
        std::fs::create_dir_all(src.join("Cluster Desk.app/Contents/MacOS")).unwrap();
        std::fs::write(src.join("Cluster Desk.app/Contents/MacOS/cluster-desk"), b"new").unwrap();
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut tb = tar::Builder::new(&mut gz);
            tb.append_dir_all("Cluster Desk.app", src.join("Cluster Desk.app")).unwrap();
            tb.finish().unwrap();
        }
        let data = gz.finish().unwrap();
        let t = Target::Bundle(app.clone());
        stage(&t, &data).unwrap();
        install(&t).unwrap();
        assert_eq!(std::fs::read(app.join("Contents/MacOS/cluster-desk")).unwrap(), b"new");
        assert!(!old_path(&t).exists() && !staged_path(&t).exists());
    }

    #[test]
    fn cleanup_keeps_another_copys_download() {
        let d = tmp("cleanup");
        let t = Target::AppImage(d.join("Cluster.AppImage"));
        let theirs = d.join(format!("{STAGED}-1.AppImage"));
        let replaced = d.join(format!("{OLD}-1.AppImage"));
        std::fs::write(&theirs, b"pending").unwrap();
        std::fs::write(&replaced, b"old").unwrap();
        cleanup(&t);
        assert!(theirs.exists());
        assert!(!replaced.exists());
    }

    // Made with: echo -n "cluster desk" | minisign -S -s minisign.key -m - -t "cluster-desk 0.1.7 linux-x86_64"
    const SIG: &str = include_str!("../assets/test-signature.minisig");

    #[test]
    fn signature_checks() {
        verify(b"cluster desk", SIG, "cluster-desk 0.1.7 linux-x86_64").unwrap();
        assert!(verify(b"cluster desk!", SIG, "cluster-desk 0.1.7 linux-x86_64").is_err());
        assert!(verify(b"cluster desk", SIG, "cluster-desk 0.1.8 linux-x86_64").is_err());
    }
}
