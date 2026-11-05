//! Preferences: where kubectl/helm come from, kubeconfig files, extra PATH entries for
//! credential plugins, HTTPS proxy and terminal options. Saved as JSON in the user's config
//! folder. Defaults and download rules follow the original (see MECHANICS.md, "Preferences").

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, RwLock};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct Settings {
    /// downloadKubectlBinaries (default true): fetch the kubectl matching each cluster.
    pub download_kubectl: bool,
    /// downloadMirror: "default" (https://dl.k8s.io/release) or "custom".
    pub download_mirror: String,
    pub custom_mirror: String,
    /// downloadBinariesPath: folder for downloaded tools ("" = default).
    pub binaries_dir: String,
    /// kubectlBinariesPath: kubectl to use when downloads are off ("" = kubectl on PATH).
    pub kubectl_path: String,
    /// helmBinariesPath ("" = helm on PATH).
    pub helm_path: String,
    /// Extra kubeconfig files merged with KUBECONFIG / ~/.kube/config.
    pub extra_kubeconfigs: Vec<String>,
    /// Extra folders searched for kubectl, helm and credential plugins (kubelogin, cloud CLIs).
    pub extra_path: Vec<String>,
    /// httpsProxy for kubectl and downloads.
    pub https_proxy: String,
    /// allowUntrustedCAs (default false): trust any certificate for the app's own downloads
    /// (corporate proxies that re-write certificates). Never affects cluster connections.
    pub allow_untrusted_cas: bool,
    /// terminalConfig.fontSize (default 12) and terminalCopyOnSelect (default false).
    pub terminal_font_size: f32,
    pub terminal_copy_on_select: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            download_kubectl: true,
            download_mirror: "default".into(),
            custom_mirror: String::new(),
            binaries_dir: String::new(),
            kubectl_path: String::new(),
            helm_path: String::new(),
            extra_kubeconfigs: vec![],
            extra_path: vec![],
            https_proxy: String::new(),
            allow_untrusted_cas: false,
            terminal_font_size: 12.0,
            terminal_copy_on_select: false,
        }
    }
}

pub const DEFAULT_MIRROR: &str = "https://dl.k8s.io/release";
/// The Helm version the original bundles (package.json config.bundledHelmVersion).
pub const HELM_VERSION: &str = "4.3.1";

/// kubectl-versions/index.ts: cluster minor version → kubectl release to download.
const KUBECTL_VERSIONS: &[(&str, &str)] = &[
    ("1.22", "1.22.17"),
    ("1.23", "1.23.17"),
    ("1.24", "1.24.17"),
    ("1.25", "1.25.16"),
    ("1.26", "1.26.15"),
    ("1.27", "1.27.16"),
    ("1.28", "1.28.15"),
    ("1.29", "1.29.15"),
    ("1.30", "1.30.14"),
    ("1.31", "1.31.14"),
    ("1.32", "1.32.13"),
    ("1.33", "1.33.13"),
    ("1.34", "1.34.12"),
    ("1.35", "1.35.9"),
    ("1.36", "1.36.5"),
    ("1.37", "1.37.1"),
];

fn store() -> &'static RwLock<Settings> {
    static S: OnceLock<RwLock<Settings>> = OnceLock::new();
    S.get_or_init(|| RwLock::new(load()))
}

/// context name → kubectl prepared for it (downloaded matching version).
fn resolved() -> &'static Mutex<HashMap<String, PathBuf>> {
    static R: OnceLock<Mutex<HashMap<String, PathBuf>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

/// context name → human readable kubectl status ("Using kubectl v1.31.14 (downloaded)").
fn statuses() -> &'static Mutex<HashMap<String, String>> {
    static R: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn get() -> Settings {
    store().read().map(|s| s.clone()).unwrap_or_default()
}

pub fn set(new: Settings) {
    if let Ok(mut s) = store().write() {
        *s = new.clone();
    }
    let _ = save(&new);
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn expand(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        home().join(rest)
    } else if p == "~" {
        home()
    } else {
        PathBuf::from(p)
    }
}

fn config_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    base.join("cluster-desk").join("settings.json")
}

pub fn default_binaries_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".local/share"));
    base.join("cluster-desk").join("binaries")
}

pub fn binaries_dir(s: &Settings) -> PathBuf {
    if s.binaries_dir.trim().is_empty() { default_binaries_dir() } else { expand(s.binaries_dir.trim()) }
}

fn load() -> Settings {
    std::fs::read_to_string(config_file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save(s: &Settings) -> Result<(), String> {
    let f = config_file();
    if let Some(d) = f.parent() {
        std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
    }
    std::fs::write(&f, serde_json::to_string_pretty(s).unwrap_or_default()).map_err(|e| e.to_string())
}

pub fn settings_file_path() -> String {
    config_file().display().to_string()
}

// ------------------------------------------------------------------ environment for kubectl

/// Folders added in front of PATH: the person's extra folders, then common places credential
/// plugins are installed to (only those that exist), then the folder of downloaded tools.
pub fn path_value() -> String {
    let s = get();
    let mut dirs: Vec<PathBuf> = s.extra_path.iter().map(|p| expand(p.trim())).filter(|p| !p.as_os_str().is_empty()).collect();
    for c in ["~/.krew/bin", "~/.local/bin", "~/go/bin", "~/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
        let p = expand(c);
        if p.is_dir() && !dirs.contains(&p) {
            dirs.push(p);
        }
    }
    let helm_dir = binaries_dir(&s).join("helm").join(HELM_VERSION);
    if helm_dir.is_dir() {
        dirs.push(helm_dir);
    }
    let mut out: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    if let Ok(p) = std::env::var("PATH") {
        out.push(p);
    }
    out.join(if cfg!(windows) { ";" } else { ":" })
}

/// KUBECONFIG value when extra files are configured (None = leave as is).
pub fn kubeconfig_value() -> Option<String> {
    let s = get();
    if s.extra_kubeconfigs.is_empty() {
        return None;
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut files: Vec<String> = match std::env::var("KUBECONFIG") {
        Ok(v) if !v.is_empty() => v.split(sep).map(|x| x.to_string()).collect(),
        _ => vec![home().join(".kube/config").display().to_string()],
    };
    for f in &s.extra_kubeconfigs {
        let p = expand(f.trim()).display().to_string();
        if !files.contains(&p) {
            files.push(p);
        }
    }
    Some(files.join(sep))
}

/// Environment every kubectl/helm call gets.
pub fn env_pairs() -> Vec<(String, String)> {
    let mut v = vec![("PATH".to_string(), path_value())];
    if let Some(k) = kubeconfig_value() {
        v.push(("KUBECONFIG".into(), k));
    }
    let proxy = get().https_proxy;
    if !proxy.trim().is_empty() {
        v.push(("HTTPS_PROXY".into(), proxy.trim().to_string()));
        v.push(("https_proxy".into(), proxy.trim().to_string()));
    }
    v
}

/// Which kubectl to run for a context: the downloaded matching one when prepared, otherwise
/// the configured path, otherwise `kubectl` from PATH.
pub fn kubectl_for(context: Option<&str>) -> String {
    let s = get();
    if s.download_kubectl {
        if let Some(c) = context {
            if let Some(p) = resolved().lock().ok().and_then(|m| m.get(c).cloned()) {
                return p.display().to_string();
            }
        }
    }
    if s.kubectl_path.trim().is_empty() { "kubectl".into() } else { expand(s.kubectl_path.trim()).display().to_string() }
}

pub fn helm_cmd() -> String {
    let s = get();
    if s.helm_path.trim().is_empty() { "helm".into() } else { expand(s.helm_path.trim()).display().to_string() }
}

pub fn kubectl_status(context: &str) -> Option<String> {
    statuses().lock().ok().and_then(|m| m.get(context).cloned())
}

fn set_status(context: &str, text: String) {
    if let Ok(mut m) = statuses().lock() {
        m.insert(context.to_string(), text);
    }
}

/// Find a program the way the shell would (absolute/relative path, or each PATH folder).
pub fn which(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.components().count() > 1 {
        return if p.is_file() { Some(p.to_path_buf()) } else { None };
    }
    let path = path_value();
    for dir in std::env::split_paths(&std::ffi::OsString::from(path)) {
        let cand = dir.join(program);
        if cand.is_file() {
            return Some(cand);
        }
        if cfg!(windows) && dir.join(format!("{program}.exe")).is_file() {
            return Some(dir.join(format!("{program}.exe")));
        }
    }
    None
}

pub fn command(program: &str) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    for (k, v) in env_pairs() {
        c.env(k, v);
    }
    c
}

// ------------------------------------------------------------------ versions and downloads

/// Kubectl constructor: the version map entry for the cluster's major.minor, else the
/// cluster's exact x.y.z.
pub fn matching_kubectl(server_version: &str) -> Option<String> {
    let v = server_version.trim().trim_start_matches('v');
    let mut parts = v.split(|c: char| c == '.' || c == '-' || c == '+');
    let (maj, min, pat) = (parts.next()?, parts.next()?, parts.next().unwrap_or("0"));
    let mm = format!("{maj}.{min}");
    Some(
        KUBECTL_VERSIONS
            .iter()
            .find(|(k, _)| *k == mm)
            .map(|(_, v)| v.to_string())
            .unwrap_or_else(|| format!("{maj}.{min}.{pat}")),
    )
}

fn platform() -> (&'static str, &'static str) {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "windows",
        _ => "linux",
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "amd64",
    };
    (os, arch)
}

/// Certificate check that accepts anything ("Allow untrusted Certificate Authorities").
#[derive(Debug)]
struct TrustAnything(std::sync::Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for TrustAnything {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn agent() -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(300));
    if get().allow_untrusted_cas {
        let provider = std::sync::Arc::new(rustls::crypto::ring::default_provider());
        if let Ok(cfg) = rustls::ClientConfig::builder_with_provider(provider.clone()).with_safe_default_protocol_versions() {
            let cfg = cfg
                .dangerous()
                .with_custom_certificate_verifier(std::sync::Arc::new(TrustAnything(provider)))
                .with_no_client_auth();
            b = b.tls_config(std::sync::Arc::new(cfg));
        }
    }
    let proxy = get().https_proxy;
    if !proxy.trim().is_empty() {
        if let Ok(p) = ureq::Proxy::new(proxy.trim()) {
            b = b.proxy(p);
        }
    }
    b.build()
}

fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let resp = agent().get(url).call().map_err(|e| format!("{url}: {e}"))?;
    let mut buf = Vec::new();
    resp.into_reader().take(512 * 1024 * 1024).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(data);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn make_executable(p: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755));
    }
    let _ = p;
}

/// Download kubectl `version` (x.y.z) unless present: `<mirror>/v<ver>/bin/<os>/<arch>/kubectl`,
/// checked against the published `.sha256`, stored in `<binaries>/kubectl/<ver>/kubectl`.
pub fn ensure_kubectl(version: &str) -> Result<PathBuf, String> {
    let s = get();
    let name = if cfg!(windows) { "kubectl.exe" } else { "kubectl" };
    let dir = binaries_dir(&s).join("kubectl").join(version);
    let target = dir.join(name);
    if target.is_file() {
        return Ok(target);
    }
    let mirror = if s.download_mirror == "custom" && !s.custom_mirror.trim().is_empty() {
        s.custom_mirror.trim().trim_end_matches('/').to_string()
    } else {
        DEFAULT_MIRROR.to_string()
    };
    let (os, arch) = platform();
    // Only versions with a checksum pinned in the app are downloaded (kubectl-checksums).
    let pinned_arch = if arch == "amd64" { "x64" } else { arch };
    let key = format!("{os}/{pinned_arch}");
    let Some(want) = crate::kubectl_checksums::PINNED.iter().find(|(v, p, _)| *v == version && *p == key).map(|x| x.2) else {
        return Err(format!("No verified checksum is pinned for kubectl v{version} on {os}/{arch}, so it was not downloaded"));
    };
    let url = format!("{mirror}/v{version}/bin/{os}/{arch}/{name}");
    let data = fetch(&url)?;
    let got = sha256_hex(&data);
    if want != got {
        return Err(format!("Checksum mismatch for kubectl {version} downloaded from {url}: expected sha256 {want}, got {got}"));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("{name}.download"));
    std::fs::write(&tmp, &data).map_err(|e| e.to_string())?;
    make_executable(&tmp);
    std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

/// Download Helm `HELM_VERSION` from get.helm.sh (tar.gz + .sha256sum) into
/// `<binaries>/helm/<ver>/helm` and use it from then on.
pub fn ensure_helm() -> Result<PathBuf, String> {
    let s = get();
    let name = if cfg!(windows) { "helm.exe" } else { "helm" };
    let dir = binaries_dir(&s).join("helm").join(HELM_VERSION);
    let target = dir.join(name);
    if target.is_file() {
        return Ok(target);
    }
    let (os, arch) = platform();
    let ext = if os == "windows" { "zip" } else { "tar.gz" };
    if ext == "zip" {
        return Err("Downloading Helm on Windows isn't supported yet; set the path to helm.exe instead.".into());
    }
    let url = format!("https://get.helm.sh/helm-v{HELM_VERSION}-{os}-{arch}.{ext}");
    let data = fetch(&url)?;
    let want = String::from_utf8_lossy(&fetch(&format!("{url}.sha256sum"))?).split_whitespace().next().unwrap_or("").to_lowercase();
    if want.is_empty() || want != sha256_hex(&data) {
        return Err(format!("checksum mismatch for {url}"));
    }
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(&data[..]));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    for entry in ar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?.to_path_buf();
        if path.file_name().map(|f| f == name).unwrap_or(false) {
            let tmp = dir.join(format!("{name}.download"));
            entry.unpack(&tmp).map_err(|e| e.to_string())?;
            make_executable(&tmp);
            std::fs::rename(&tmp, &target).map_err(|e| e.to_string())?;
            return Ok(target);
        }
    }
    Err("helm binary not found in the archive".into())
}

/// Ask the cluster for its version, then (if enabled) prepare the matching kubectl for it.
/// Runs on a background thread; progress is kept for the Preferences page and status bar.
pub fn prepare_for_context(context: String, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        let out = command(&kubectl_for(None))
            .args(["--context", &context, "version", "-o", "json", "--request-timeout=20s"])
            .output();
        let server = out.ok().and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok()).and_then(|v| {
            v.pointer("/serverVersion/gitVersion").and_then(|x| x.as_str()).map(|x| x.to_string())
        });
        let Some(server) = server else {
            set_status(&context, "Cluster version unknown (not reachable yet); using kubectl from PATH".into());
            ctx.request_repaint();
            return;
        };
        if !get().download_kubectl {
            set_status(&context, format!("Cluster {server}; using {}", kubectl_for(None)));
            ctx.request_repaint();
            return;
        }
        let Some(want) = matching_kubectl(&server) else { return };
        set_status(&context, format!("Cluster {server}; preparing kubectl v{want}…"));
        ctx.request_repaint();
        match ensure_kubectl(&want) {
            Ok(p) => {
                if let Ok(mut m) = resolved().lock() {
                    m.insert(context.clone(), p);
                }
                set_status(&context, format!("Cluster {server}; using kubectl v{want} (downloaded)"));
            }
            // Original: "Failed to prepare kubectl vX (reason) - using the bundled vY"; there is
            // no bundled kubectl here, so the fallback is the configured one / kubectl on PATH.
            Err(e) if e.starts_with("No verified checksum") => {
                set_status(&context, format!("Cluster {server}; {e} - using {}", kubectl_for(None)))
            }
            Err(e) => set_status(&context, format!("Failed to prepare kubectl v{want} ({e}) - using {}", kubectl_for(None))),
        }
        ctx.request_repaint();
    });
}
