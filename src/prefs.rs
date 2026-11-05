//! Preferences page: kubectl and Helm, kubeconfig files and sign-in (credential plugins),
//! proxy and terminal options.

use crate::app::App;
use crate::settings::{self, Settings};
use crate::theme;
use eframe::egui;
use egui::{Color32, CornerRadius, RichText, Stroke};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Kubernetes,
    Authentication,
    Proxy,
    Terminal,
    Updates,
}

#[derive(Clone, Default)]
pub struct ToolInfo {
    pub path: Option<String>,
    pub version: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct ContextAuth {
    pub name: String,
    pub cluster: String,
    pub server: String,
    pub user: String,
    pub method: String,
    /// Program the sign-in runs (exec plugin), if any.
    pub program: Option<String>,
    pub program_found: Option<bool>,
    pub notes: Vec<String>,
}

#[derive(Default)]
pub struct PrefsState {
    pub section: Section,
    pub draft: Option<Settings>,
    pub kubectl: Arc<Mutex<Option<ToolInfo>>>,
    pub helm: Arc<Mutex<Option<ToolInfo>>>,
    pub contexts: Arc<Mutex<Option<Result<Vec<ContextAuth>, String>>>>,
    pub tests: Arc<Mutex<HashMap<String, (bool, String)>>>,
    pub busy: Arc<Mutex<Option<String>>>,
    pub new_path: String,
    pub new_kubeconfig: String,
    pub loaded: bool,
}

fn detect(program: String, args: &'static [&'static str], parse: fn(&[u8]) -> Option<String>) -> ToolInfo {
    let path = settings::which(&program).map(|p| p.display().to_string());
    if path.is_none() {
        return ToolInfo { path: None, version: None, error: Some(format!("{program} was not found")) };
    }
    match settings::command(&program).args(args).output() {
        Ok(o) if o.status.success() => ToolInfo { path, version: parse(&o.stdout), error: None },
        Ok(o) => ToolInfo { path, version: None, error: Some(String::from_utf8_lossy(&o.stderr).trim().to_string()) },
        Err(e) => ToolInfo { path, version: None, error: Some(e.to_string()) },
    }
}

fn kubectl_version(out: &[u8]) -> Option<String> {
    let v: Value = serde_json::from_slice(out).ok()?;
    v.pointer("/clientVersion/gitVersion").and_then(|x| x.as_str()).map(|x| x.to_string())
}

fn helm_version(out: &[u8]) -> Option<String> {
    Some(String::from_utf8_lossy(out).trim().to_string()).filter(|s| !s.is_empty())
}

/// Describe how each context signs in, from `kubectl config view` (secrets are redacted by kubectl).
fn read_contexts() -> Result<Vec<ContextAuth>, String> {
    let out = settings::command(&settings::kubectl_for(None))
        .args(["config", "view", "-o", "json"])
        .output()
        .map_err(|e| format!("Could not run kubectl ({e})"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    let v: Value = serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())?;
    let list = |k: &str| v.get(k).and_then(|x| x.as_array()).cloned().unwrap_or_default();
    let clusters = list("clusters");
    let users = list("users");
    let s = |x: &Value, p: &str| x.pointer(p).and_then(|y| y.as_str()).unwrap_or("").to_string();
    let mut out = vec![];
    for c in list("contexts") {
        let name = s(&c, "/name");
        let cluster = s(&c, "/context/cluster");
        let user = s(&c, "/context/user");
        let server = clusters.iter().find(|x| s(x, "/name") == cluster).map(|x| s(x, "/cluster/server")).unwrap_or_default();
        let u = users.iter().find(|x| s(x, "/name") == user).and_then(|x| x.get("user")).cloned().unwrap_or(Value::Null);
        let mut notes = vec![];
        let mut program = None;
        let method = if let Some(exec) = u.get("exec") {
            let cmd = s(exec, "/command");
            let args: Vec<String> =
                exec.get("args").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            let joined = args.join(" ");
            let base = std::path::Path::new(&cmd).file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or(cmd.clone());
            let label = if base == "kubectl" && args.first().map(|a| a == "oidc-login").unwrap_or(false) {
                // `kubectl oidc-login` runs the kubectl-oidc_login plugin.
                program = Some("kubectl-oidc_login".to_string());
                notes.push("OIDC sign-in opens your browser the first time, then the token is cached.".into());
                "OIDC (kubelogin plugin)".to_string()
            } else {
                program = Some(cmd.clone());
                match base.as_str() {
                    "kubelogin" => {
                        notes.push("kubelogin may open your browser or show a device code to sign in.".into());
                        "kubelogin (Azure / OIDC)".into()
                    }
                    "gke-gcloud-auth-plugin" => "Google Cloud (gke-gcloud-auth-plugin)".into(),
                    "aws" | "aws-iam-authenticator" => "AWS (EKS)".into(),
                    "doctl" => "DigitalOcean (doctl)".into(),
                    "oci" => "Oracle Cloud (oci)".into(),
                    "teleport" | "tsh" => "Teleport".into(),
                    _ => format!("Exec plugin: {base}"),
                }
            };
            if !joined.is_empty() {
                notes.push(format!("Runs: {cmd} {joined}"));
            }
            if s(exec, "/interactiveMode") == "Always" {
                notes.push("This plugin asks questions when signing in: use \"Sign in in a terminal\".".into());
            }
            label
        } else if let Some(ap) = u.get("auth-provider") {
            let n = s(ap, "/name");
            if n == "oidc" {
                notes.push("The built-in oidc auth-provider was removed in kubectl 1.26; switch this user to the kubelogin exec plugin (kubectl oidc-login).".into());
            }
            format!("Auth provider: {n}")
        } else if u.get("token").is_some() || u.get("tokenFile").is_some() {
            "Bearer token".into()
        } else if u.get("client-certificate-data").is_some() || u.get("client-certificate").is_some() {
            "Client certificate".into()
        } else if u.get("username").is_some() {
            "Username and password".into()
        } else {
            "None / anonymous".into()
        };
        let program_found = program.as_ref().map(|p| settings::which(p).is_some());
        out.push(ContextAuth { name, cluster, server, user, method, program, program_found, notes });
    }
    Ok(out)
}

impl App {
    pub fn prefs_refresh(&mut self) {
        let ctx = self.egui_ctx.clone();
        let (k, h, c) = (self.prefs.kubectl.clone(), self.prefs.helm.clone(), self.prefs.contexts.clone());
        std::thread::spawn(move || {
            *k.lock().unwrap() = Some(detect(settings::kubectl_for(None), &["version", "--client", "-o", "json"], kubectl_version));
            ctx.request_repaint();
            *h.lock().unwrap() = Some(detect(settings::helm_cmd(), &["version", "--short"], helm_version));
            ctx.request_repaint();
            *c.lock().unwrap() = Some(read_contexts());
            ctx.request_repaint();
        });
    }

    fn run_busy(&self, label: &str, job: impl FnOnce() -> Result<String, String> + Send + 'static) {
        let busy = self.prefs.busy.clone();
        if busy.lock().unwrap().is_some() {
            return;
        }
        *busy.lock().unwrap() = Some(format!("{label}…"));
        let ctx = self.egui_ctx.clone();
        std::thread::spawn(move || {
            let r = job();
            *busy.lock().unwrap() = Some(match r {
                Ok(m) => format!("✔ {m}"),
                Err(e) => format!("⚠ {e}"),
            });
            ctx.request_repaint();
        });
    }

    pub fn preferences(&mut self, ui: &mut egui::Ui) {
        if !self.prefs.loaded {
            self.prefs.loaded = true;
            self.prefs.draft = Some(settings::get());
            self.prefs_refresh();
        }
        let mut draft = self.prefs.draft.clone().unwrap_or_else(settings::get);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Preferences").size(theme::FONT_SIZE + 4.0).color(theme::TEXT_TERTIARY));
            ui.add_space(8.0);
            ui.label(RichText::new(format!("saved in {}", settings::settings_file_path())).size(theme::FONT_SIZE_SMALL).color(theme::TEXT));
        });
        ui.add_space(12.0);
        ui.horizontal_top(|ui| {
            // Left navigation (like the original's preferences tabs).
            ui.vertical(|ui| {
                ui.set_width(180.0);
                for (s, label) in [
                    (Section::Proxy, "Proxy"),
                    (Section::Kubernetes, "Kubernetes"),
                    (Section::Authentication, "Authentication"),
                    (Section::Terminal, "Terminal"),
                    (Section::Updates, "Updates"),
                ] {
                    let active = self.prefs.section == s;
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(170.0, 32.0), egui::Sense::click());
                    let fill = if active { theme::PANEL } else if resp.hovered() { theme::HOVER } else { Color32::TRANSPARENT };
                    ui.painter().rect_filled(rect, CornerRadius::same(3), fill);
                    ui.painter().text(
                        rect.left_center() + egui::vec2(12.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        label,
                        egui::FontId::proportional(theme::FONT_SIZE),
                        if active { theme::TEXT_HI } else { theme::TEXT_MID },
                    );
                    if resp.clicked() {
                        self.prefs.section = s;
                    }
                }
            });
            ui.add_space(16.0);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
              ui.vertical(|ui| {
                ui.set_max_width(820.0);
                match self.prefs.section {
                    Section::Kubernetes => self.prefs_kubernetes(ui, &mut draft),
                    Section::Authentication => self.prefs_auth(ui, &mut draft),
                    Section::Proxy => {
                        group(ui, "HTTP Proxy", |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.https_proxy)
                                    .hint_text("Type proxy url (example: http://proxy.acme.org:8080)")
                                    .desired_width(f32::INFINITY),
                            );
                            hint(ui, "Proxy for the app's own connections: downloads and Helm. Takes an http://, https:// or socks5:// URL. It is also given to kubectl and terminals as HTTPS_PROXY.");
                        });
                        group(ui, "Certificate Trust", |ui| {
                            ui.checkbox(&mut draft.allow_untrusted_cas, "Allow untrusted Certificate Authorities");
                            hint(ui, "This will make the app trust ANY certificate authority without any validations. Needed with some corporate proxies that do certificate re-writing. Does not affect cluster communications!");
                        });
                    }
                    Section::Updates => self.prefs_updates(ui, &mut draft),
                    Section::Terminal => {
                        group(ui, "Terminal copy & paste", |ui| {
                            ui.checkbox(&mut draft.terminal_copy_on_select, "Copy on select and paste on right-click");
                        });
                        group(ui, "Font size", |ui| {
                            ui.add(egui::DragValue::new(&mut draft.terminal_font_size).range(10.0..=50.0).speed(0.2).suffix(" px"));
                        });
                    }
                }
              });
            });
        });
        // Save edits right away (like the original's preferences).
        let before = settings::get();
        if serde_json::to_string(&draft).ok() != serde_json::to_string(&before).ok() {
            settings::set(draft.clone());
        }
        self.prefs.draft = Some(draft);
        if self.prefs.busy.lock().unwrap().as_deref().map(|b| b.ends_with('…')).unwrap_or(false) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        }
    }

    fn prefs_updates(&mut self, ui: &mut egui::Ui, d: &mut Settings) {
        use crate::update::State;
        let state = self.updater.state();
        group(ui, "Version", |ui| {
            ui.label(RichText::new(format!("Cluster Desk {}", crate::update::VERSION)).color(theme::TEXT_HI));
            let text = match &state {
                State::Off(why) => why.clone(),
                State::Idle => "Not checked yet.".into(),
                State::Checking => "Checking for updates…".into(),
                State::UpToDate => "Up to date.".into(),
                State::Downloading(v) => format!("Downloading {v}…"),
                State::Ready(v) => format!("{v} is downloaded and installs when you quit."),
                State::Installed(v) => format!("{v} is installed and runs the next time you start Cluster Desk."),
                State::Failed(e) => format!("Could not update: {e}"),
            };
            hint(ui, &text);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let busy = matches!(state, State::Checking | State::Downloading(_));
                let can_check = !matches!(state, State::Off(_) | State::Ready(_) | State::Installed(_)) && !busy;
                if ui.add_enabled(can_check, egui::Button::new("Check now")).clicked() {
                    self.updater.check_now(&self.egui_ctx);
                }
                if matches!(state, State::Ready(_) | State::Installed(_)) && ui.button("Restart to update").clicked() {
                    if let Err(e) = self.updater.restart(&self.egui_ctx) {
                        self.toast(e, true);
                    }
                }
                if busy {
                    ui.spinner();
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
                }
            });
        });
        group(ui, "Automatic updates", |ui| {
            ui.checkbox(&mut d.auto_update, "Check for updates automatically");
            hint(ui, "Every 6 hours the app looks for a newer release on GitHub (github.com/DiverOfDark/cluster-desk), downloads it in the background and checks its signature. The update installs when you quit, or right away with Restart to update.");
        });
    }

    fn prefs_kubernetes(&mut self, ui: &mut egui::Ui, d: &mut Settings) {
        let busy = self.prefs.busy.lock().unwrap().clone();
        group(ui, "Kubectl binary download", |ui| {
            let info = self.prefs.kubectl.lock().unwrap().clone();
            tool_status(ui, info.as_ref());
            ui.add_space(6.0);
            ui.checkbox(&mut d.download_kubectl, "Download kubectl binaries matching the Kubernetes cluster version");
            hint(ui, "Each cluster gets the kubectl release made for its version, downloaded once and checked against the checksum built into the app. Versions without a pinned checksum are not downloaded.");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Download mirror");
                egui::ComboBox::from_id_salt("mirror").width(220.0)
                    .selected_text(if d.download_mirror == "custom" { "Custom" } else { "Default (Google)" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.download_mirror, "default".to_string(), "Default (Google)");
                        ui.selectable_value(&mut d.download_mirror, "custom".to_string(), "Custom");
                    });
            });
            if d.download_mirror == "custom" {
                ui.label("Custom mirror URL");
                ui.add(egui::TextEdit::singleline(&mut d.custom_mirror).hint_text("https://artifacts.example.com/kubernetes/kubectl").desired_width(f32::INFINITY));
                hint(ui, "The base URL of your mirror. The version and platform path are filled in.");
            }
            ui.add_space(4.0);
            ui.label("Directory for binaries");
            ui.add(
                egui::TextEdit::singleline(&mut d.binaries_dir)
                    .hint_text(settings::default_binaries_dir().display().to_string())
                    .desired_width(f32::INFINITY),
            );
            hint(ui, "The directory to download binaries into.");
            ui.add_space(4.0);
            ui.label("Path to kubectl binary");
            ui.add(egui::TextEdit::singleline(&mut d.kubectl_path).hint_text("kubectl (from PATH)").desired_width(f32::INFINITY));
            hint(ui, "The path to the kubectl binary. Defaults to kubectl on your PATH (there is no bundled kubectl).");
            if let Some(ctx) = self.backend.context_name() {
                ui.add_space(6.0);
                let st = settings::kubectl_status(&ctx).unwrap_or_else(|| "Checking the cluster version…".into());
                ui.label(RichText::new(format!("{ctx}: {st}")).color(theme::TEXT_MID));
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(busy.as_deref().map(|b| !b.ends_with('…')).unwrap_or(true), egui::Button::new("Prepare kubectl for the open cluster")).clicked() {
                    if let Some(ctx) = self.backend.context_name() {
                        settings::prepare_for_context(ctx, self.egui_ctx.clone());
                    }
                }
                if ui.button("⟳ Check again").clicked() {
                    self.prefs_refresh();
                }
            });
        });
        group(ui, "Helm", |ui| {
            let info = self.prefs.helm.lock().unwrap().clone();
            tool_status(ui, info.as_ref());
            ui.add_space(4.0);
            ui.label("Path to helm binary");
            ui.add(egui::TextEdit::singleline(&mut d.helm_path).hint_text("helm (from PATH)").desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                let can = busy.as_deref().map(|b| !b.ends_with('…')).unwrap_or(true);
                if ui.add_enabled(can, egui::Button::new(format!("Download Helm v{}", settings::HELM_VERSION))).clicked() {
                    let k = self.prefs.helm.clone();
                    self.run_busy("Downloading Helm", move || {
                        let p = settings::ensure_helm()?;
                        let mut s = settings::get();
                        s.helm_path = p.display().to_string();
                        settings::set(s);
                        *k.lock().unwrap() = Some(detect(settings::helm_cmd(), &["version", "--short"], helm_version));
                        Ok(format!("Helm saved to {}", p.display()))
                    });
                    self.prefs.draft = None;
                }
            });
            hint(ui, "Helm isn't used by any screen yet; it is prepared for Helm releases and charts.");
        });
        if let Some(b) = busy {
            let col = if b.starts_with('⚠') { theme::RED } else { theme::TEXT_MID };
            ui.label(RichText::new(b).color(col));
        }
        group(ui, "Kubeconfig files", |ui| {
            let base = std::env::var("KUBECONFIG").unwrap_or_else(|_| "~/.kube/config".into());
            ui.label(RichText::new(format!("Always read: {base}")).color(theme::TEXT_MID));
            let mut remove = None;
            for (i, f) in d.extra_kubeconfigs.iter().enumerate() {
                ui.horizontal(|ui| {
                    let exists = settings::expand(f).is_file();
                    ui.label(RichText::new(f).color(if exists { theme::ITEM_VALUE } else { theme::RED }));
                    if !exists {
                        ui.label(RichText::new("(not found)").color(theme::RED));
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                d.extra_kubeconfigs.remove(i);
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.prefs.new_kubeconfig).hint_text("/path/to/another/kubeconfig").desired_width(420.0));
                if ui.button("Add file").clicked() && !self.prefs.new_kubeconfig.trim().is_empty() {
                    d.extra_kubeconfigs.push(self.prefs.new_kubeconfig.trim().to_string());
                    self.prefs.new_kubeconfig.clear();
                }
            });
            if ui.button("Reload clusters").clicked() {
                self.reload_contexts();
            }
        });
    }

    fn prefs_auth(&mut self, ui: &mut egui::Ui, d: &mut Settings) {
        group(ui, "Where sign-in programs are found", |ui| {
            hint(ui, "Clusters that sign in with a plugin (kubelogin / kubectl oidc-login, gke-gcloud-auth-plugin, aws, …) need that program on the PATH. Apps started from the desktop often see a shorter PATH than your terminal, so add the folders here.");
            let mut remove = None;
            for (i, p) in d.extra_path.iter().enumerate() {
                ui.horizontal(|ui| {
                    let ok = settings::expand(p).is_dir();
                    ui.label(RichText::new(p).color(if ok { theme::ITEM_VALUE } else { theme::RED }));
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                d.extra_path.remove(i);
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.prefs.new_path).hint_text("~/.krew/bin").desired_width(420.0));
                if ui.button("Add folder").clicked() && !self.prefs.new_path.trim().is_empty() {
                    d.extra_path.push(self.prefs.new_path.trim().to_string());
                    self.prefs.new_path.clear();
                }
            });
            hint(ui, "Also searched automatically when present: ~/.krew/bin, ~/.local/bin, ~/go/bin, ~/bin, /usr/local/bin.");
        });

        let contexts = self.prefs.contexts.lock().unwrap().clone();
        group(ui, "Clusters and how they sign in", |ui| {
            match contexts {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Reading your kubeconfig…");
                    });
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(theme::RED));
                }
                Some(Ok(list)) => {
                    if list.is_empty() {
                        ui.label(RichText::new("No contexts in your kubeconfig.").color(theme::TEXT));
                    }
                    for c in list {
                        egui::Frame::new()
                            .fill(theme::PANEL)
                            .corner_radius(CornerRadius::same(4))
                            .stroke(Stroke::new(1.0, theme::BORDER))
                            .inner_margin(egui::Margin::symmetric(14, 10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(&c.name).color(theme::TEXT_HI).strong());
                                    ui.label(RichText::new(&c.method).color(theme::BLUE));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if ui.button("Sign in in a terminal").on_hover_text("Runs kubectl for this cluster in a terminal so sign-in prompts and device codes can be answered").clicked() {
                                            self.open_signin_terminal(&c.name);
                                        }
                                        if ui.button("Test").clicked() {
                                            let tests = self.prefs.tests.clone();
                                            let name = c.name.clone();
                                            let ctx = self.egui_ctx.clone();
                                            tests.lock().unwrap().insert(name.clone(), (true, "Testing…".into()));
                                            std::thread::spawn(move || {
                                                let r = settings::command(&settings::kubectl_for(Some(&name)))
                                                    .args(["--context", &name, "version", "-o", "json", "--request-timeout=20s"])
                                                    .stdin(std::process::Stdio::null())
                                                    .output();
                                                let res = match r {
                                                    Ok(o) => {
                                                        let v: Option<Value> = serde_json::from_slice(&o.stdout).ok();
                                                        match v.as_ref().and_then(|v| v.pointer("/serverVersion/gitVersion")).and_then(|x| x.as_str()) {
                                                            Some(ver) => (true, format!("✔ Connected — Kubernetes {ver}")),
                                                            None => (false, format!("⚠ {}", String::from_utf8_lossy(&o.stderr).trim())),
                                                        }
                                                    }
                                                    Err(e) => (false, format!("⚠ {e}")),
                                                };
                                                tests.lock().unwrap().insert(name, res);
                                                ctx.request_repaint();
                                            });
                                        }
                                    });
                                });
                                ui.label(RichText::new(format!("cluster {} · user {} · {}", c.cluster, c.user, c.server)).size(theme::FONT_SIZE_SMALL).color(theme::TEXT));
                                if let (Some(p), Some(found)) = (&c.program, c.program_found) {
                                    if found {
                                        ui.label(RichText::new(format!("✔ {p} found")).color(theme::SUCCESS));
                                    } else {
                                        ui.label(RichText::new(format!("⚠ {p} was not found — install it or add its folder above")).color(theme::RED));
                                    }
                                }
                                for n in &c.notes {
                                    ui.label(RichText::new(n).size(theme::FONT_SIZE_SMALL).color(theme::TEXT_MID));
                                }
                                if let Some((ok, msg)) = self.prefs.tests.lock().unwrap().get(&c.name).cloned() {
                                    ui.add(egui::Label::new(RichText::new(msg).color(if ok { theme::SUCCESS } else { theme::RED })).wrap());
                                }
                            });
                        ui.add_space(8.0);
                    }
                }
            }
            if ui.button("⟳ Read kubeconfig again").clicked() {
                *self.prefs.contexts.lock().unwrap() = None;
                self.prefs_refresh();
            }
        });
    }
}

fn group(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(title).size(theme::FONT_SIZE + 1.0).color(theme::TEXT_HI));
    ui.add_space(6.0);
    egui::Frame::new().inner_margin(egui::Margin { left: 2, right: 2, top: 0, bottom: 0 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
    ui.add_space(10.0);
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().line_segment([egui::pos2(r.left(), y), egui::pos2(r.right(), y)], Stroke::new(1.0, theme::BORDER));
    ui.add_space(14.0);
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(theme::FONT_SIZE_SMALL).color(theme::TEXT)).wrap());
}

fn tool_status(ui: &mut egui::Ui, info: Option<&ToolInfo>) {
    match info {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking…");
            });
        }
        Some(t) => {
            if let Some(v) = &t.version {
                ui.label(RichText::new(format!("✔ {v}")).color(theme::SUCCESS));
            }
            if let Some(p) = &t.path {
                ui.label(RichText::new(p).size(theme::FONT_SIZE_SMALL).color(theme::TEXT_MID));
            }
            if let Some(e) = &t.error {
                ui.add(egui::Label::new(RichText::new(format!("⚠ {e}")).color(theme::RED)).wrap());
            }
        }
    }
}
