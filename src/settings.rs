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
    /// Check GitHub releases for a newer Cluster Desk and download it (default true).
    pub auto_update: bool,
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
            auto_update: true,
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
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
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

/// PATH for kubectl, helm and credential plugins: the person's extra folders and the folder of
/// downloaded tools, then the inherited PATH, then common places credential plugins are
/// installed to (only those that exist). The common places come last so they only fill in for
/// a short desktop-launch PATH: a plugin found on the inherited PATH wins, as in a terminal,
/// instead of an older copy of it in one of those folders.
pub fn path_value() -> String {
    let s = get();
    let sep = if cfg!(windows) { ";" } else { ":" };
    let mut dirs: Vec<PathBuf> = s.extra_path.iter().map(|p| expand(p.trim())).filter(|p| !p.as_os_str().is_empty()).collect();
    let helm_dir = binaries_dir(&s).join("helm").join(HELM_VERSION);
    if helm_dir.is_dir() {
        dirs.push(helm_dir);
    }
    let inherited: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    let mut out: Vec<String> = dirs.iter().map(|d| d.display().to_string()).collect();
    if let Ok(p) = std::env::var("PATH") {
        if !p.is_empty() {
            out.push(p);
        }
    }
    for c in ["~/.krew/bin", "~/.local/bin", "~/go/bin", "~/bin", "/usr/local/bin", "/opt/homebrew/bin"] {
        let p = expand(c);
        if p.is_dir() && !dirs.contains(&p) && !inherited.contains(&p) {
            out.push(p.display().to_string());
            dirs.push(p);
        }
    }
    out.join(sep)
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
/// the configured path, otherwise `kubectl` from PATH, otherwise the downloaded stand-in for
/// the kubectl the original bundles (see `ensure_fallback_kubectl`).
pub fn kubectl_for(context: Option<&str>) -> String {
    let s = get();
    if s.download_kubectl {
        if let Some(c) = context {
            if let Some(p) = resolved().lock().ok().and_then(|m| m.get(c).cloned()) {
                return p.display().to_string();
            }
        }
    }
    if !s.kubectl_path.trim().is_empty() {
        return expand(s.kubectl_path.trim()).display().to_string();
    }
    if which("kubectl").is_none() {
        let fallback = fallback_kubectl_path(&s);
        if fallback.is_file() {
            return fallback.display().to_string();
        }
    }
    "kubectl".into()
}

/// The kubectl used before a cluster's version is known, when none is installed: the newest
/// one in the version map (the original bundles one with the app instead).
fn fallback_kubectl_version() -> &'static str {
    KUBECTL_VERSIONS.last().map(|(_, v)| *v).unwrap_or("1.37.1")
}

fn fallback_kubectl_path(s: &Settings) -> PathBuf {
    let name = if cfg!(windows) { "kubectl.exe" } else { "kubectl" };
    binaries_dir(s).join("kubectl").join(fallback_kubectl_version()).join(name)
}

/// Without a configured kubectl or one on PATH, download the fallback one so contexts can be
/// listed and cluster versions found (needs "Download kubectl binaries" on). Blocking.
pub fn ensure_fallback_kubectl() -> Result<(), String> {
    // One download at a time; the others wait for it and then find the file.
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    let s = get();
    if !s.kubectl_path.trim().is_empty() || which("kubectl").is_some() || fallback_kubectl_path(&s).is_file() {
        return Ok(());
    }
    if !s.download_kubectl {
        return Err("kubectl was not found on PATH. Install it, set its path in Preferences → Kubernetes, or turn on kubectl downloads there.".into());
    }
    ensure_kubectl(fallback_kubectl_version())
        .map(|_| ())
        .map_err(|e| format!("kubectl was not found on PATH, and downloading kubectl v{} failed: {e}", fallback_kubectl_version()))
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

/// `command` for a program run against one context: also points KUBECONFIG at the context's
/// adapted kubeconfig, if one was made (see `adapt_exec_version`).
pub fn command_for(program: &str, context: Option<&str>) -> std::process::Command {
    let mut c = std::process::Command::new(program);
    for (k, v) in env_pairs_for(context) {
        c.env(k, v);
    }
    c
}

/// `env_pairs` for programs run against one context (see `command_for`).
pub fn env_pairs_for(context: Option<&str>) -> Vec<(String, String)> {
    let mut v = env_pairs();
    if let Some(o) = context.and_then(exec_override) {
        v.retain(|(k, _)| k != "KUBECONFIG");
        v.push(("KUBECONFIG".into(), o.path.display().to_string()));
    }
    v
}

/// `kubectl --context <context> <args>` with output captured; when kubectl refuses the
/// context's sign-in plugin over its API version, adapts (`adapt_exec_version`) and runs again.
pub fn kubectl_output(context: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    let run = || {
        command_for(&kubectl_for(Some(context)), Some(context))
            .args(["--context", context])
            .args(args)
            .stdin(std::process::Stdio::null())
            .output()
    };
    let out = run()?;
    if !out.status.success() && adapt_exec_version(context, &String::from_utf8_lossy(&out.stderr)) {
        return run();
    }
    Ok(out)
}

// ------------------------------------------------------------------ sign-in plugin API version

/// The exec credential versions kubectl still accepts.
const EXEC_VERSIONS: &[&str] = &["client.authentication.k8s.io/v1", "client.authentication.k8s.io/v1beta1"];

#[derive(Clone)]
struct ExecOverride {
    path: PathBuf,
    /// The kubeconfig's version and the one the plugin answers with.
    configured: String,
    answered: String,
}

/// context name → private kubeconfig copy whose exec apiVersion is the one the plugin answers.
fn exec_overrides() -> &'static Mutex<HashMap<String, ExecOverride>> {
    static R: OnceLock<Mutex<HashMap<String, ExecOverride>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn exec_override(context: &str) -> Option<ExecOverride> {
    exec_overrides().lock().ok().and_then(|m| m.get(context).cloned())
}

/// The kubeconfig's and the plugin's version from kubectl's "exec plugin is configured to use
/// API version A, plugin returned version B".
pub fn exec_version_mismatch(stderr: &str) -> Option<(String, String)> {
    let version = |t: &str| -> String { t.chars().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '/' | '-')).collect() };
    let rest = stderr.split("exec plugin is configured to use API version ").nth(1)?;
    let (configured, rest) = rest.split_once(", plugin returned version ")?;
    let (configured, answered) = (version(configured), version(rest));
    (!configured.is_empty() && !answered.is_empty()).then_some((configured, answered))
}

/// Set the exec apiVersion of every user signing in with `configured` to `answered`.
/// Returns how many users were changed.
fn patch_exec_version(kubeconfig: &mut serde_json::Value, configured: &str, answered: &str) -> usize {
    let mut changed = 0;
    if let Some(users) = kubeconfig.get_mut("users").and_then(|u| u.as_array_mut()) {
        for u in users {
            if let Some(exec) = u.pointer_mut("/user/exec").and_then(|e| e.as_object_mut()) {
                if exec.get("apiVersion").and_then(|v| v.as_str()) == Some(configured) {
                    exec.insert("apiVersion".into(), answered.into());
                    changed += 1;
                }
            }
        }
    }
    changed
}

/// Note for Preferences when a context runs with an adapted kubeconfig.
pub fn exec_version_note(context: &str) -> Option<String> {
    exec_override(context).map(|o| {
        format!(
            "Its sign-in plugin answers with {} although the kubeconfig asks for {}, so Cluster Desk uses {} for it. \
             To fix it for good, set apiVersion: {} in the kubeconfig user's exec section, or update the plugin.",
            o.answered, o.configured, o.answered, o.answered
        )
    })
}

/// kubectl refuses a sign-in plugin that answers with another API version than the kubeconfig
/// asks for (older kubelogin, aws-iam-authenticator and other plugins answer v1beta1 whatever
/// they are asked). When `stderr` is that refusal and the plugin's version is still supported,
/// write a private copy of the context's kubeconfig asking for the plugin's version, and use
/// it for this context from now on (`command_for`). Returns true when kubectl should be run
/// again. The kubeconfig itself is not changed.
pub fn adapt_exec_version(context: &str, stderr: &str) -> bool {
    let Some((configured, answered)) = exec_version_mismatch(stderr) else { return false };
    if configured == answered || !EXEC_VERSIONS.contains(&answered.as_str()) {
        return false;
    }
    // Parallel calls fail together; the first one writes the copy, the others just retry.
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE.lock().unwrap_or_else(|e| e.into_inner());
    if exec_override(context).is_some() {
        return true;
    }
    match write_exec_override(context, &configured, &answered) {
        Ok(path) => {
            if let Ok(mut m) = exec_overrides().lock() {
                m.insert(context.to_string(), ExecOverride { path, configured, answered });
            }
            true
        }
        Err(_) => false,
    }
}

fn exec_override_dir() -> PathBuf {
    // The session's private runtime folder when there is one, else next to the settings.
    match std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d).join("cluster-desk").join("kubeconfigs"),
        None => config_file().parent().map(|p| p.to_path_buf()).unwrap_or_else(home).join("kubeconfigs"),
    }
}

fn write_exec_override(context: &str, configured: &str, answered: &str) -> Result<PathBuf, String> {
    // Only the context's own entries, with certificate files inlined so the copy stands alone.
    let out = command(&kubectl_for(Some(context)))
        .args(["config", "view", "--raw", "--minify", "--flatten", "--context", context, "-o", "json"])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let mut v: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    if patch_exec_version(&mut v, configured, answered) == 0 {
        return Err(format!("no user of {context} signs in with {configured}"));
    }
    let dir = exec_override_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let safe: String = context.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).take(60).collect();
    let path = dir.join(format!("{safe}-{}.json", &sha256_hex(context.as_bytes())[..8]));
    let text = serde_json::to_vec_pretty(&v).map_err(|e| e.to_string())?;
    // The copy holds the same credentials as the kubeconfig: readable by this user only.
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
        opts.mode(0o600);
    }
    use std::io::Write;
    opts.open(&path).and_then(|mut f| f.write_all(&text)).map_err(|e| e.to_string())?;
    Ok(path)
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

pub fn fetch(url: &str) -> Result<Vec<u8>, String> {
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
        let _ = ensure_fallback_kubectl();
        let out = kubectl_output(&context, &["version", "-o", "json", "--request-timeout=20s"]);
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

#[cfg(test)]
mod tests {
    use super::*;

    // kubectl's message from issue #5.
    const REFUSAL: &str = r#"E1106 13:11:22.131045  137607 memcache.go:265] "Unhandled Error" err="couldn't get current server API group list: Get \"https://haproxy.k8s-control.dev.example.com/api?timeout=30s\": getting credentials: exec plugin is configured to use API version client.authentication.k8s.io/v1, plugin returned version client.authentication.k8s.io/v1beta1""#;

    #[test]
    fn reads_versions_from_kubectl_refusal() {
        assert_eq!(
            exec_version_mismatch(REFUSAL),
            Some(("client.authentication.k8s.io/v1".into(), "client.authentication.k8s.io/v1beta1".into()))
        );
        assert_eq!(exec_version_mismatch("error: You must be logged in to the server (Unauthorized)"), None);
    }

    #[test]
    fn unsupported_answer_is_not_adapted() {
        let old = REFUSAL.replace("returned version client.authentication.k8s.io/v1beta1", "returned version client.authentication.k8s.io/v1alpha1");
        assert!(!adapt_exec_version("never-used", &old));
        assert!(!adapt_exec_version("never-used", "some other error"));
    }

    #[test]
    fn patches_only_users_with_the_refused_version() {
        let mut v = serde_json::json!({
            "users": [
                {"name": "a", "user": {"exec": {"apiVersion": "client.authentication.k8s.io/v1", "command": "kubelogin"}}},
                {"name": "b", "user": {"exec": {"apiVersion": "client.authentication.k8s.io/v1beta1", "command": "aws"}}},
                {"name": "c", "user": {"token": "x"}}
            ]
        });
        assert_eq!(patch_exec_version(&mut v, "client.authentication.k8s.io/v1", "client.authentication.k8s.io/v1beta1"), 1);
        assert_eq!(v.pointer("/users/0/user/exec/apiVersion").unwrap(), "client.authentication.k8s.io/v1beta1");
        assert_eq!(v.pointer("/users/0/user/exec/command").unwrap(), "kubelogin");
        assert_eq!(v.pointer("/users/1/user/exec/apiVersion").unwrap(), "client.authentication.k8s.io/v1beta1");
        assert_eq!(v.pointer("/users/2/user/token").unwrap(), "x");
    }
}
