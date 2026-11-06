//! Application state, frame loop and the window chrome (hotbar, top bar, sidebar, status bar).

use eframe::egui;
use crate::backend::{Backend, ClusterEntry, Job, Reply, Source};
use crate::model::{GROUPS, Kind, Row};
use crate::theme;
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, Vec2, pos2, vec2};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Catalog,
    Preferences,
    Overview,
    Workloads,
    List(Kind),
}

#[derive(Default)]
pub struct KindData {
    pub rows: Arc<Vec<Row>>,
    pub version: u64,
    pub loaded: Option<Instant>,
    pub loading: bool,
    pub error: Option<String>,
    /// The namespaces `rows` were fetched for (empty = all).
    pub scope: Vec<String>,
}

pub enum DockKind {
    Logs {
        ns: String,
        pod: String,
        containers: Vec<String>,
        container: String,
        previous: bool,
        follow: bool,
        lines: Vec<String>,
        /// Lines without colour codes, lower-cased, for search.
        plain: Vec<String>,
        filter: String,
        last: Option<Instant>,
        /// Which search match is "active" (orange), and whether to scroll to it.
        match_idx: usize,
        jump: bool,
        timestamps: bool,
    },
    Edit {
        kind: Kind,
        ns: String,
        name: String,
        text: String,
        original: String,
        close_after_save: bool,
        message: Option<(String, bool)>,
    },
    Shell {
        ns: String,
        pod: String,
        containers: Vec<String>,
        container: String,
        term: Option<crate::terminal::Terminal>,
        error: Option<String>,
    },
}

pub struct DockTab {
    pub id: u64,
    pub title: String,
    pub kind: DockKind,
    pub busy: bool,
}

pub enum Dialog {
    Delete { kind: Kind, ns: String, name: String },
    Scale { kind: Kind, ns: String, name: String, current: i64, desired: i64 },
    Restart { kind: Kind, ns: String, name: String },
}

pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}


pub struct App {
    /// Screenshot mode without a window (see main.rs); `shot_ready` tells main to save.
    pub headless: bool,
    pub shot_ready: bool,
    pub prefs: crate::prefs::PrefsState,
    pub updater: crate::update::Updater,
    /// A toast went out for this downloaded update.
    update_announced: Option<String>,
    pub backend: Backend,
    pub egui_ctx: egui::Context,
    pub clusters: Vec<ClusterEntry>,
    pub contexts_error: Option<String>,
    pub contexts_loaded: bool,
    pub active: Option<usize>,
    pub view: View,
    pub back: Vec<View>,
    pub fwd: Vec<View>,
    pub store: HashMap<Kind, KindData>,
    /// Live node usage from the metrics server (None until it answers, or when it isn't installed).
    pub node_usage: Option<crate::backend::NodeUsage>,
    pub metrics_error: Option<String>,
    metrics_loading: bool,
    metrics_at: Option<Instant>,
    /// Claimed volume usage from kubelet stats.
    pub volume_usage: Option<crate::backend::VolumeUsage>,
    pub volume_error: Option<String>,
    volume_loading: bool,
    volume_at: Option<Instant>,
    pub ns_filter: BTreeSet<String>,
    /// Namespace typed into the namespace picker (for people who can't list namespaces).
    pub ns_input: String,
    pub search: String,
    pub sort: HashMap<Kind, (usize, bool)>,
    pub selected: Option<(Kind, String)>,
    pub expanded: HashSet<&'static str>,
    pub dock_open: bool,
    pub dock_tabs: Vec<DockTab>,
    pub dock_active: u64,
    pub next_tab: u64,
    pub dialog: Option<Dialog>,
    pub toasts: Vec<Toast>,
    pub revealed: HashSet<String>,
    pub filter_cache: Option<(FilterKey, Arc<Vec<usize>>)>,
    pub drawer_w: Option<f32>,
    screenshot: Option<PathBuf>,
    frame: u64,
    shot_requested: bool,
    shot_wait_real: bool,
    /// Developer test pictures: text typed into the terminal, and opening a real pod's shell.
    shot_type: Option<String>,
    shot_shell_real: bool,
    shot_typed_at: Option<u64>,
    shot_crd_real: bool,
    shot_done_at: Option<u64>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FilterKey {
    pub kind: Kind,
    pub version: u64,
    pub search: String,
    pub ns: BTreeSet<String>,
    pub sort: Option<(usize, bool)>,
}

const REFRESH: Duration = Duration::from_secs(10);
const METRICS_REFRESH: Duration = Duration::from_secs(30);

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, screenshot: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        let backend = Backend::new(cc.egui_ctx.clone());
        let mut app = App {
            headless: false,
            shot_ready: false,
            prefs: Default::default(),
            updater: Default::default(),
            update_announced: None,
            backend,
            egui_ctx: cc.egui_ctx.clone(),
            clusters: vec![ClusterEntry { name: "sample-cluster".into(), source: Source::Sample }],
            contexts_error: None,
            contexts_loaded: false,
            active: None,
            view: View::Catalog,
            back: vec![],
            fwd: vec![],
            store: HashMap::new(),
            node_usage: None,
            metrics_error: None,
            metrics_loading: false,
            metrics_at: None,
            volume_usage: None,
            volume_error: None,
            volume_loading: false,
            volume_at: None,
            ns_filter: BTreeSet::new(),
            ns_input: String::new(),
            search: String::new(),
            sort: HashMap::new(),
            selected: None,
            expanded: ["Workloads"].into_iter().collect(),
            dock_open: false,
            dock_tabs: vec![],
            dock_active: 0,
            next_tab: 1,
            dialog: None,
            toasts: vec![],
            revealed: HashSet::new(),
            filter_cache: None,
            drawer_w: None,
            screenshot,
            frame: 0,
            shot_requested: false,
            shot_wait_real: false,
            shot_type: None,
            shot_shell_real: false,
            shot_typed_at: None,
            shot_crd_real: false,
            shot_done_at: None,
        };
        if let (Some(_), Ok(ctxname)) = (&app.screenshot, std::env::var("SHOT_CONTEXT")) {
            // Developer aid: picture of a real cluster.
            app.contexts_loaded = true;
            app.clusters.insert(0, ClusterEntry { name: ctxname.clone(), source: Source::Context(ctxname.clone()) });
            app.connect(0);
            // SHOT_NS=a,b: picture with those namespaces selected.
            if let Ok(ns) = std::env::var("SHOT_NS") {
                app.ns_filter = ns.split(',').map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).collect();
            }
            app.view = if std::env::var("SHOT_VIEW").as_deref() == Ok("kind:node") {
                View::List(Kind::Node)
            } else {
                View::List(Kind::Pod)
            };
            app.shot_wait_real = true;
            if std::env::var("SHOT_VIEW").as_deref() == Ok("shell") {
                app.shot_shell_real = true;
            }
            match std::env::var("SHOT_VIEW").as_deref() {
                Ok("prefs") => app.view = View::Preferences,
                Ok("prefs-auth") => {
                    app.view = View::Preferences;
                    app.prefs.section = crate::prefs::Section::Authentication;
                }
                _ => {}
            }
            if std::env::var("SHOT_VIEW").as_deref() == Ok("crd") {
                app.shot_crd_real = true;
                app.view = View::List(Kind::Crd);
            }
        } else if app.screenshot.is_some() {
            // Deterministic picture: the sample cluster's pods list.
            app.contexts_loaded = true;
            app.connect(0);
            for k in crate::model::ALL_KINDS {
                let reply = app.backend.run_sample_now(&Job::List { kind: *k, namespaces: vec![] });
                app.handle(reply);
            }
            let reply = app.backend.run_sample_now(&Job::NodeMetrics);
            app.handle(reply);
            app.metrics_at = Some(Instant::now());
            let reply = app.backend.run_sample_now(&Job::VolumeStats { nodes: vec![] });
            app.handle(reply);
            app.volume_at = Some(Instant::now());
            app.view = View::List(Kind::Pod);
            app.back.clear();
            // Developer aid: SHOT_VIEW picks another screen for test pictures.
            let pick = std::env::var("SHOT_VIEW").unwrap_or_default();
            let first = |app: &App, k: Kind, pre: &str| app.rows(k).iter().find(|r| r.name.starts_with(pre)).cloned();
            match pick.as_str() {
                "overview" => app.view = View::Overview,
                "workloads" => app.view = View::Workloads,
                "catalog" => app.view = View::Catalog,
                "prefs" => app.view = View::Preferences,
                "prefs-updates" => {
                    app.view = View::Preferences;
                    app.prefs.section = crate::prefs::Section::Updates;
                }
                "prefs-auth" => {
                    app.view = View::Preferences;
                    app.prefs.section = crate::prefs::Section::Authentication;
                }
                "crd" | "crddetail" => {
                    app.expanded.insert("Custom Resources");
                    app.expanded.insert("cert-manager.io");
                    if let Some((i, _)) = crate::crd::all().into_iter().find(|c| c.1.plural == "certificates") {
                        app.view = View::List(Kind::Custom(i));
                        app.handle(app.backend.run_sample_now(&Job::List { kind: Kind::Custom(i), namespaces: vec![] }));
                        if pick == "crddetail" {
                            if let Some(r) = app.rows(Kind::Custom(i)).iter().find(|r| r.name == "api-internal") {
                                app.selected = Some((Kind::Custom(i), r.uid.clone()));
                            }
                        }
                    }
                }
                "deployments" => {
                    app.view = View::List(Kind::Deployment);
                    if let Some(r) = first(&app, Kind::Deployment, "web") {
                        app.selected = Some((Kind::Deployment, r.uid));
                    }
                }
                "details" => {
                    if let Some(r) = first(&app, Kind::Pod, "worker-6f7c9d8b7-zt") {
                        app.selected = Some((Kind::Pod, r.uid));
                    }
                }
                "logs" | "edit" | "shell" => {
                    if let Some(r) = first(&app, Kind::Pod, "worker-6f7c9d8b7-zt") {
                        match pick.as_str() {
                            "logs" => {
                                let web = first(&app, Kind::Pod, "web-").unwrap_or(r.clone());
                                app.open_logs(&web.obj, None);
                                if let Some(crate::app::DockKind::Logs { filter, jump, .. }) = app.dock_tabs.last_mut().map(|t| &mut t.kind) {
                                    *filter = "api/".into();
                                    *jump = true;
                                }
                            }
                            "edit" => app.open_edit(Kind::Pod, &r.ns, &r.name),
                            _ => {
                                app.open_shell(&r.obj, None);
                                app.shot_type = Some("ls\remoji\rcolors\r".into());
                            }
                        }
                    }
                }
                "scale" => {
                    app.view = View::List(Kind::Deployment);
                    app.dialog = Some(Dialog::Scale { kind: Kind::Deployment, ns: "shop".into(), name: "web".into(), current: 3, desired: 5 });
                }
                s if s.starts_with("kind:") => {
                    if let Some(k) = crate::model::ALL_KINDS.iter().find(|k| format!("{:?}", k).eq_ignore_ascii_case(&s[5..])) {
                        app.view = View::List(*k);
                    }
                }
                _ => {}
            }
        } else {
            app.backend.discover_contexts();
        }
        app
    }

    // ---------------------------------------------------------------- data

    pub fn rows(&self, kind: Kind) -> Arc<Vec<Row>> {
        self.store.get(&kind).map(|d| d.rows.clone()).unwrap_or_default()
    }

    /// Namespaces a list of `kind` is fetched for: the selected ones (empty = all). Pods are
    /// fetched from every namespace while node or cluster figures are shown, since those add up
    /// the requests of all pods.
    fn list_scope(&self, kind: Kind) -> Vec<String> {
        let all_pods = matches!(self.view, View::Overview | View::List(Kind::Node))
            || self.selected.as_ref().is_some_and(|s| s.0 == Kind::Node);
        if !kind.namespaced() || (kind == Kind::Pod && all_pods) { vec![] } else { self.ns_filter.iter().cloned().collect() }
    }

    /// Pods of every namespace, once loaded (None while only some namespaces are loaded).
    pub fn all_pods(&self) -> Option<Arc<Vec<Row>>> {
        let d = self.store.get(&Kind::Pod)?;
        (d.loaded.is_some() && d.error.is_none() && d.scope.is_empty()).then(|| d.rows.clone())
    }

    pub fn ensure(&mut self, kind: Kind) {
        if self.backend.source.is_none() {
            return;
        }
        let scope = self.list_scope(kind);
        let d = self.store.entry(kind).or_default();
        // Definitions change rarely and can be huge, so they refresh every minute.
        let every = if kind == Kind::Crd { REFRESH * 6 } else { REFRESH };
        let stale = d.loaded.map(|t| t.elapsed() > every).unwrap_or(true);
        // Another namespace selection is fetched right away, even after an error.
        let rescoped = d.loaded.is_some() && d.scope != scope;
        if !d.loading && (rescoped || stale && (d.error.is_none() || d.loaded.map(|t| t.elapsed() > every * 3).unwrap_or(false))) {
            d.loading = true;
            self.backend.submit(Job::List { kind, namespaces: scope });
        }
    }

    pub fn reload(&mut self, kind: Kind) {
        let scope = self.list_scope(kind);
        let d = self.store.entry(kind).or_default();
        d.error = None;
        d.loaded = None;
        if !d.loading {
            d.loading = true;
            self.backend.submit(Job::List { kind, namespaces: scope });
        }
    }

    /// Ask the metrics server for node usage now and then (less often when it isn't available).
    pub fn ensure_metrics(&mut self) {
        if self.backend.source.is_none() || self.metrics_loading {
            return;
        }
        // The original's nodes list re-reads metrics every 30 s (interval(30, …) in nodes/route.tsx).
        let every = if self.metrics_error.is_some() { METRICS_REFRESH * 2 } else { METRICS_REFRESH };
        if self.metrics_at.map(|t| t.elapsed() > every).unwrap_or(true) {
            self.metrics_loading = true;
            self.metrics_at = Some(Instant::now());
            self.backend.submit(Job::NodeMetrics);
        }
    }

    /// Ask every node's kubelet for volume usage (needs the node list first), every 30 s.
    pub fn ensure_volume_stats(&mut self) {
        if self.backend.source.is_none() || self.volume_loading {
            return;
        }
        self.ensure(Kind::Node);
        let nodes: Vec<String> = self.rows(Kind::Node).iter().map(|r| r.name.clone()).collect();
        if nodes.is_empty() {
            return;
        }
        let every = if self.volume_error.is_some() { METRICS_REFRESH * 2 } else { METRICS_REFRESH };
        if self.volume_at.map(|t| t.elapsed() > every).unwrap_or(true) {
            self.volume_loading = true;
            self.volume_at = Some(Instant::now());
            self.backend.submit(Job::VolumeStats { nodes });
        }
    }

    /// Put volume usage into the Usage column of the claims and volumes lists.
    fn apply_volume_usage(&mut self) {
        let usage = self.volume_usage.clone().unwrap_or_default();
        for kind in [Kind::Pvc, Kind::Pv] {
            let Some(col) = kind.columns().iter().position(|c| c.0 == "Usage") else { continue };
            let Some(d) = self.store.get_mut(&kind) else { continue };
            let rows = Arc::make_mut(&mut d.rows);
            for r in rows.iter_mut() {
                let key = match kind {
                    Kind::Pvc => (r.ns.clone(), r.name.clone()),
                    _ => (
                        crate::util::s(&r.obj, "spec/claimRef/namespace"),
                        crate::util::s(&r.obj, "spec/claimRef/name"),
                    ),
                };
                let (used, cap) = usage.get(&key).copied().unwrap_or((f64::NAN, f64::NAN));
                if let Some(c) = r.cells.get_mut(col) {
                    *c = crate::model::Cell::Meter(crate::model::volume_meter(used, cap));
                }
            }
            d.version += 1;
        }
    }

    /// Fill the CPU and Memory columns of the nodes table the way the original does:
    /// usage from the metrics server over requests of the node's running pods, against allocatable.
    fn apply_node_metrics(&mut self) {
        // Requests are only shown when they count every namespace's pods.
        let pods = self.all_pods();
        let usage = self.node_usage.clone().unwrap_or_default();
        let Some(d) = self.store.get_mut(&Kind::Node) else { return };
        let rows = Arc::make_mut(&mut d.rows);
        for r in rows.iter_mut() {
            let (cpu, mem) = usage.get(&r.name).copied().unwrap_or((f64::NAN, f64::NAN));
            let req = pods.as_ref().map(|pods| crate::model::node_requests(&r.name, pods));
            if r.cells.len() > 2 {
                r.cells[1] = crate::model::Cell::Meter(crate::model::node_cpu_meter(&r.obj, cpu, req.map(|x| x.0)));
                r.cells[2] = crate::model::Cell::Meter(crate::model::node_memory_meter(&r.obj, mem, req.map(|x| x.1)));
            }
        }
        d.version += 1;
    }

    pub fn connect(&mut self, idx: usize) {
        let Some(c) = self.clusters.get(idx).cloned() else { return };
        self.backend.connect(c.source);
        self.active = Some(idx);
        self.store.clear();
        self.node_usage = None;
        self.metrics_error = None;
        self.metrics_loading = false;
        self.metrics_at = None;
        self.volume_usage = None;
        self.volume_error = None;
        self.volume_loading = false;
        self.volume_at = None;
        self.selected = None;
        self.ns_filter.clear();
        self.filter_cache = None;
        self.dock_tabs.clear();
        self.dock_open = false;
        self.navigate(View::Overview);
        self.ensure(Kind::Namespace);
        // Custom resource definitions are loaded right away so the menu lists them.
        self.ensure(Kind::Crd);
        // Find the cluster version and prepare the matching kubectl (Preferences → Kubernetes).
        if let Some(ctx_name) = self.backend.context_name() {
            crate::settings::prepare_for_context(ctx_name, self.egui_ctx.clone());
        }
    }

    pub fn navigate(&mut self, v: View) {
        if v != self.view {
            self.back.push(self.view);
            self.fwd.clear();
            self.view = v;
            self.search.clear();
        }
    }

    pub fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toasts.push(Toast { text: text.into(), error, at: Instant::now() });
    }

    fn handle(&mut self, reply: Reply) {
        match reply {
            Reply::Contexts { names, current, error } => {
                self.contexts_loaded = true;
                self.contexts_error = error;
                let sample = self.clusters.iter().find(|c| c.source == Source::Sample).cloned();
                let mut list: Vec<ClusterEntry> = names
                    .iter()
                    .map(|n| ClusterEntry { name: n.clone(), source: Source::Context(n.clone()) })
                    .collect();
                list.extend(sample);
                let active_src = self.active.and_then(|i| self.clusters.get(i)).map(|c| c.source.clone());
                self.clusters = list;
                self.active = active_src.and_then(|s| self.clusters.iter().position(|c| c.source == s));
                if self.active.is_none() {
                    let target = current
                        .and_then(|cur| self.clusters.iter().position(|c| c.source == Source::Context(cur.clone())))
                        .unwrap_or(self.clusters.len() - 1);
                    self.connect(target);
                }
            }
            Reply::List { generation, kind, namespaces, result } => {
                if generation != self.backend.generation {
                    return;
                }
                let d = self.store.entry(kind).or_default();
                d.loading = false;
                d.loaded = Some(Instant::now());
                d.scope = namespaces;
                match result {
                    Ok(rows) => {
                        d.rows = Arc::new(rows);
                        d.version += 1;
                        d.error = None;
                    }
                    Err(e) => d.error = Some(e),
                }
                if kind == Kind::Node || kind == Kind::Pod {
                    self.apply_node_metrics();
                }
                if kind == Kind::Crd {
                    // Every definition becomes its own kind in the Custom Resources menu.
                    for r in self.rows(Kind::Crd).iter() {
                        crate::crd::register(&r.obj);
                    }
                }
                if kind == Kind::Pvc || kind == Kind::Pv {
                    self.apply_volume_usage();
                }
            }
            Reply::VolumeStats { generation, result } => {
                if generation != self.backend.generation {
                    return;
                }
                self.volume_loading = false;
                match result {
                    Ok(u) => {
                        self.volume_usage = Some(u);
                        self.volume_error = None;
                    }
                    Err(e) => self.volume_error = Some(e),
                }
                self.apply_volume_usage();
            }
            Reply::NodeMetrics { generation, result } => {
                if generation != self.backend.generation {
                    return;
                }
                self.metrics_loading = false;
                match result {
                    Ok(u) => {
                        self.node_usage = Some(u);
                        self.metrics_error = None;
                        self.apply_node_metrics();
                    }
                    Err(e) => {
                        self.metrics_error = Some(e);
                    }
                }
            }
            Reply::Text { generation, tab, result } => {
                if generation != self.backend.generation {
                    return;
                }
                if let Some(t) = self.dock_tabs.iter_mut().find(|t| t.id == tab) {
                    t.busy = false;
                    match (&mut t.kind, result) {
                        (DockKind::Logs { lines, plain, last, .. }, r) => {
                            *last = Some(Instant::now());
                            *lines = match r {
                                Ok(text) => text.lines().map(|l| l.to_string()).collect(),
                                Err(e) => vec![format!("Error: {e}")],
                            };
                            *plain = lines.iter().map(|l| crate::ansi::strip(l).to_lowercase()).collect();
                        }
                        (DockKind::Edit { text, original, message, .. }, r) => match r {
                            Ok(y) => {
                                *text = y.clone();
                                *original = y;
                            }
                            Err(e) => *message = Some((e, true)),
                        },
                        (DockKind::Shell { .. }, _) => {}
                    }
                }
            }
            Reply::Action { generation, result, refresh, tab } => {
                if generation != self.backend.generation {
                    return;
                }
                let ok = result.is_ok();
                if let Some(tab) = tab {
                    let mut close = false;
                    if let Some(t) = self.dock_tabs.iter_mut().find(|t| t.id == tab) {
                        t.busy = false;
                        if let DockKind::Edit { message, close_after_save, text, original, .. } = &mut t.kind {
                            *message = Some((
                                match &result {
                                    Ok(m) => m.clone(),
                                    Err(e) => e.clone(),
                                },
                                !ok,
                            ));
                            if ok {
                                *original = text.clone();
                                close = *close_after_save;
                            }
                        }
                    }
                    if close {
                        self.close_tab(tab);
                    }
                }
                match result {
                    Ok(m) => self.toast(if m.is_empty() { "Done".to_string() } else { m }, false),
                    Err(e) => self.toast(e, true),
                }
                self.reload(refresh);
                self.reload(Kind::Event);
                if matches!(refresh, Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet | Kind::ReplicaSet) {
                    self.reload(Kind::Pod);
                }
            }
        }
    }

    pub fn close_tab(&mut self, id: u64) {
        self.dock_tabs.retain(|t| t.id != id);
        if self.dock_active == id {
            self.dock_active = self.dock_tabs.last().map(|t| t.id).unwrap_or(0);
        }
        if self.dock_tabs.is_empty() {
            self.dock_open = false;
        }
    }

    pub fn reload_contexts(&mut self) {
        self.contexts_loaded = false;
        self.backend.discover_contexts();
    }

    /// A terminal running `kubectl get namespaces` for a context, so sign-in prompts
    /// (passwords, device codes, MFA) can be answered.
    pub fn open_signin_terminal(&mut self, context: &str) {
        let args: Vec<String> = vec!["--context".into(), context.into(), "get".into(), "namespaces".into()];
        let (term, error) = match crate::terminal::Terminal::spawn_program(&self.egui_ctx, &crate::settings::kubectl_for(Some(context)), &args) {
            Ok(t) => (Some(t), None),
            Err(e) => (None, Some(e)),
        };
        let id = self.next_tab;
        self.next_tab += 1;
        self.dock_tabs.push(DockTab {
            id,
            title: format!("Sign in: {context}"),
            kind: DockKind::Shell { ns: String::new(), pod: String::new(), containers: vec![], container: String::new(), term, error },
            busy: false,
        });
        self.dock_active = id;
        self.dock_open = true;
    }

    pub fn cluster_name(&self) -> String {
        self.active
            .and_then(|i| self.clusters.get(i))
            .map(|c| c.name.clone())
            .unwrap_or_default()
    }

    // ---------------------------------------------------------------- chrome

    fn hotbar(&mut self, ui: &mut egui::Ui) {
        let painter = ui.painter().clone();
        let full = ui.max_rect();
        let mut y = full.top() + 10.0;
        let x = full.center().x;
        // Catalog button
        let cell = vec2(theme::HOTBAR_CELL_SIZE, theme::HOTBAR_CELL_SIZE);
        let pitch = theme::HOTBAR_CELL_SIZE + 2.0 * theme::HOTBAR_CELL_MARGIN;
        let r = Rect::from_center_size(pos2(x, y + 20.0), cell);
        let resp = ui.interact(r, ui.scope_id().with("catalog"), Sense::click());
        let active = self.view == View::Catalog;
        painter.rect_filled(
            r,
            CornerRadius::same(6),
            if active { theme::BLUE } else if resp.hovered() { theme::SIDEBAR_HOVER } else { theme::HOTBAR_CELL },
        );
        draw_grid_icon(&painter, r.center(), if active || resp.hovered() { theme::TEXT_HI } else { theme::TEXT_MID });
        if resp.on_hover_text("Catalog").clicked() {
            self.navigate(View::Catalog);
        }
        y += pitch - theme::HOTBAR_CELL_MARGIN;
        painter.line_segment(
            [pos2(full.left() + 14.0, y), pos2(full.right() - 14.0, y)],
            Stroke::new(1.0, theme::BORDER),
        );
        y += 10.0;
        let mut connect = None;
        for (i, c) in self.clusters.iter().enumerate() {
            let r = Rect::from_center_size(pos2(x, y + theme::HOTBAR_CELL_MARGIN + 20.0), cell);
            if r.bottom() > full.bottom() - 10.0 {
                break;
            }
            let resp = ui.interact(r, ui.scope_id().with(("cl", i)), Sense::click());
            let is_active = self.active == Some(i);
            if is_active {
                painter.rect_filled(
                    Rect::from_min_size(pos2(full.left(), r.top() + 6.0), vec2(3.0, r.height() - 12.0)),
                    CornerRadius::same(2),
                    theme::TEXT_HI,
                );
            }
            painter.rect_filled(r, CornerRadius::same(6), theme::avatar(&c.name));
            if is_active || resp.hovered() {
                painter.rect_stroke(
                    r.expand(2.0),
                    CornerRadius::same(7),
                    Stroke::new(2.0, if is_active { theme::TEXT_HI } else { theme::GREY }),
                    egui::StrokeKind::Outside,
                );
            }
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                theme::initials(&c.name),
                FontId::proportional(15.0),
                theme::TEXT_HI,
            );
            if is_active {
                let dot = pos2(r.right() - 2.0, r.bottom() - 2.0);
                painter.circle_filled(dot, 5.0, theme::HOTBAR);
                let ok = self.store.get(&Kind::Namespace).map(|d| d.error.is_none() && d.loaded.is_some());
                let col = match ok {
                    Some(true) => theme::GREEN,
                    Some(false) if self.store.get(&Kind::Namespace).and_then(|d| d.error.as_ref()).is_some() => {
                        theme::RED
                    }
                    _ => theme::ORANGE,
                };
                painter.circle_filled(dot, 3.5, col);
            }
            let source = match &c.source {
                Source::Sample => "built-in sample".to_string(),
                Source::Context(_) => "kubeconfig".to_string(),
            };
            if resp.on_hover_text(format!("{}\n{}", c.name, source)).clicked() {
                connect = Some(i);
            }
            y += pitch;
        }
        // Preferences, at the bottom of the cluster bar.
        let gr = Rect::from_center_size(pos2(x, full.bottom() - 30.0), vec2(40.0, 40.0));
        let gresp = ui.interact(gr, ui.scope_id().with("prefs"), Sense::click());
        let gactive = self.view == View::Preferences;
        painter.rect_filled(
            gr,
            CornerRadius::same(6),
            if gactive { theme::BLUE } else if gresp.hovered() { theme::SIDEBAR_HOVER } else { Color32::TRANSPARENT },
        );
        painter.text(gr.center(), Align2::CENTER_CENTER, "⚙", FontId::proportional(20.0), if gactive || gresp.hovered() { theme::TEXT_HI } else { theme::TEXT_MID });
        if gresp.on_hover_text("Preferences").clicked() {
            self.navigate(View::Preferences);
        }
        if let Some(i) = connect {
            if self.active == Some(i) {
                if self.view == View::Catalog {
                    self.navigate(View::Overview);
                }
            } else {
                self.connect(i);
            }
        }
    }

    fn topbar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter().clone();
        let mut x = rect.left() + 12.0;
        let cy = rect.center().y;
        let mut icon_btn = |ui: &mut egui::Ui, id: &str, glyph: &str, enabled: bool, tip: &str| -> bool {
            let r = Rect::from_center_size(pos2(x + 13.0, cy), vec2(26.0, 24.0));
            x += 30.0;
            let resp = ui.interact(r, ui.scope_id().with(id), if enabled { Sense::click() } else { Sense::hover() });
            if enabled && resp.hovered() {
                painter.rect_filled(r, CornerRadius::same(4), theme::HOVER);
            }
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                glyph,
                FontId::proportional(16.0),
                if !enabled {
                    theme::TEXT_DIM
                } else if resp.hovered() {
                    theme::TEXT_HI
                } else {
                    theme::TEXT_MID
                },
            );
            enabled && resp.on_hover_text(tip).clicked()
        };
        let home = icon_btn(ui, "home", "⌂", true, "Catalog");
        let back = icon_btn(ui, "back", "⏴", !self.back.is_empty(), "Back");
        let fwd = icon_btn(ui, "fwd", "⏵", !self.fwd.is_empty(), "Forward");
        if home {
            self.navigate(View::Catalog);
        }
        if back {
            if let Some(v) = self.back.pop() {
                self.fwd.push(self.view);
                self.view = v;
            }
        }
        if fwd {
            if let Some(v) = self.fwd.pop() {
                self.back.push(self.view);
                self.view = v;
            }
        }
        let title = if self.view == View::Preferences {
            "Preferences".to_string()
        } else if self.view == View::Catalog || self.active.is_none() {
            "Catalog".to_string()
        } else {
            self.cluster_name()
        };
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            title,
            FontId::proportional(13.5),
            theme::TEXT_MID,
        );
        painter.line_segment(
            [rect.left_bottom() - vec2(0.0, 0.5), rect.right_bottom() - vec2(0.0, 0.5)],
            Stroke::new(1.0, theme::HOTBAR),
        );
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        // Cluster header
        let name = self.cluster_name();
        let (hr, _) = ui.allocate_exact_size(vec2(ui.available_width(), 48.0), Sense::hover());
        let p = ui.painter();
        let av = Rect::from_min_size(hr.left_top() + vec2(12.0, 10.0), vec2(28.0, 28.0));
        p.rect_filled(av, CornerRadius::same(4), theme::avatar(&name));
        p.text(av.center(), Align2::CENTER_CENTER, theme::initials(&name), FontId::proportional(12.0), theme::TEXT_HI);
        let galley = ellipsize(ui, &name, FontId::proportional(14.0), theme::TEXT_HI, hr.width() - 64.0);
        p.galley(pos2(av.right() + 10.0, hr.center().y - galley.size().y / 2.0), galley, theme::TEXT_HI);
        p.line_segment(
            [pos2(hr.left(), hr.bottom()), pos2(hr.right(), hr.bottom())],
            Stroke::new(1.0, theme::HOTBAR),
        );
        ui.add_space(6.0);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if self.side_item(ui, "⎈", "Cluster", 0, self.view == View::Overview, None) {
                self.navigate(View::Overview);
            }
            if self.side_item(ui, "🖥", "Nodes", 0, self.view == View::List(Kind::Node), None) {
                self.navigate(View::List(Kind::Node));
            }
            for (gi, g) in GROUPS.iter().enumerate() {
                let open = self.expanded.contains(g.title);
                let child_active = match self.view {
                    View::List(Kind::Custom(_)) => g.title == "Custom Resources",
                    View::List(k) => g.kinds.contains(&k),
                    View::Workloads => gi == 0,
                    _ => false,
                };
                if self.side_item(ui, g.icon, g.title, 0, false, Some((open, child_active))) {
                    if open {
                        self.expanded.remove(g.title);
                    } else {
                        self.expanded.insert(g.title);
                    }
                }
                if open {
                    if gi == 0 && self.side_item(ui, "", "Overview", 1, self.view == View::Workloads, None) {
                        self.navigate(View::Workloads);
                    }
                    for k in g.kinds {
                        if self.side_item(ui, "", k.short_title(), 1, self.view == View::List(*k), None) {
                            self.navigate(View::List(*k));
                        }
                    }
                    if g.title == "Custom Resources" {
                        self.custom_resource_menu(ui);
                    }
                }
                // Namespaces and Events sit between Storage and Access Control, like the original's menu.
                if g.title == "Storage" {
                    if self.side_item(ui, "▤", "Namespaces", 0, self.view == View::List(Kind::Namespace), None) {
                        self.navigate(View::List(Kind::Namespace));
                    }
                    if self.side_item(ui, "🕓", "Events", 0, self.view == View::List(Kind::Event), None) {
                        self.navigate(View::List(Kind::Event));
                    }
                }
            }
        });
    }

    /// Custom Resources → one collapsible entry per API group → one entry per kind
    /// (groups-sidebar-items-computed in the original).
    fn custom_resource_menu(&mut self, ui: &mut egui::Ui) {
        // Only the definitions of the cluster that is open (the registry outlives a switch).
        let present: std::collections::HashSet<String> = self.rows(Kind::Crd).iter().map(|r| r.name.clone()).collect();
        let all: Vec<_> = crate::crd::all().into_iter().filter(|c| present.contains(c.1.key)).collect();
        if all.is_empty() && self.store.get(&Kind::Crd).map(|d| d.loading).unwrap_or(false) {
            return;
        }
        let mut i = 0;
        while i < all.len() {
            let group = all[i].1.group;
            let members: Vec<(u16, &'static crate::crd::CrdInfo)> =
                all[i..].iter().take_while(|c| c.1.group == group).copied().collect();
            i += members.len();
            let open = self.expanded.contains(group);
            let child_active = matches!(self.view, View::List(Kind::Custom(k)) if members.iter().any(|m| m.0 == k));
            if self.side_item(ui, "", group, 1, false, Some((open, child_active))) {
                if open {
                    self.expanded.remove(group);
                } else {
                    self.expanded.insert(group);
                }
            }
            if open {
                for (idx, info) in members {
                    let k = Kind::Custom(idx);
                    if self.side_item(ui, "", info.title, 2, self.view == View::List(k), None) {
                        self.navigate(View::List(k));
                    }
                }
            }
        }
    }

    /// One sidebar entry. `group` = Some((expanded, child_active)) for collapsible headers.
    fn side_item(
        &mut self,
        ui: &mut egui::Ui,
        icon: &str,
        label: &str,
        depth: u8,
        active: bool,
        group: Option<(bool, bool)>,
    ) -> bool {
        let h = if depth == 0 { 32.0 } else { 28.0 };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), h), Sense::click());
        let p = ui.painter();
        let inner = rect.shrink2(vec2(8.0, 1.0));
        if active {
            p.rect_filled(inner, CornerRadius::same(4), theme::BLUE);
        } else if resp.hovered() {
            p.rect_filled(inner, CornerRadius::same(4), theme::SIDEBAR_HOVER);
        }
        let color = if active {
            theme::TEXT_HI
        } else if resp.hovered() || group.map(|g| g.1).unwrap_or(false) {
            Color32::from_rgb(0xdd, 0xdf, 0xe2)
        } else {
            theme::TEXT_MID
        };
        let mut x = inner.left() + 10.0;
        if depth == 0 {
            p.text(pos2(x + 8.0, rect.center().y), Align2::CENTER_CENTER, icon, FontId::proportional(14.0), color);
        }
        x += 26.0 + 14.0 * depth.saturating_sub(1) as f32;
        // Long names (API groups) are shortened with "…" to fit the sidebar.
        let g = ellipsize(ui, label, FontId::proportional(13.5), color, inner.right() - x - if group.is_some() { 26.0 } else { 6.0 });
        p.galley(pos2(x, rect.center().y - g.size().y / 2.0), g, color);
        if let Some((open, _)) = group {
            let c = pos2(inner.right() - 14.0, rect.center().y);
            let pts = if open {
                vec![c + vec2(-4.0, -2.0), c + vec2(0.0, 2.0), c + vec2(4.0, -2.0)]
            } else {
                vec![c + vec2(-2.0, -4.0), c + vec2(2.0, 0.0), c + vec2(-2.0, 4.0)]
            };
            p.add(egui::Shape::line(pts, Stroke::new(1.5, theme::TEXT)));
        }
        resp.clicked()
    }

    fn statusbar(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let p = ui.painter();
        let font = FontId::proportional(theme::FONT_SIZE_SMALL);
        let left = if let Some(i) = self.active {
            let c = &self.clusters[i];
            let state = match self.store.get(&Kind::Namespace) {
                Some(d) if d.error.is_some() => "connection problem",
                Some(d) if d.loaded.is_some() => "connected",
                _ => "connecting…",
            };
            format!("⎈  {}  ·  {}", c.name, state)
        } else {
            "⎈  No cluster selected".to_string()
        };
        p.text(pos2(rect.left() + 10.0, rect.center().y), Align2::LEFT_CENTER, left, font.clone(), theme::TEXT_HI);
        if let crate::update::State::Ready(v) | crate::update::State::Installed(v) = self.updater.state() {
            let g = p.layout_no_wrap(format!("⬆  Update {v} ready · restart"), font.clone(), theme::TEXT_HI);
            let r = Rect::from_min_size(pos2(rect.right() - 10.0 - g.size().x, rect.center().y - g.size().y / 2.0), g.size());
            p.galley(r.min, g, theme::TEXT_HI);
            let resp = ui.interact(r.expand(4.0), egui::Id::unique("statusbar_update"), Sense::click()).on_hover_text("Open Preferences → Updates");
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.clicked() {
                self.prefs.section = crate::prefs::Section::Updates;
                self.navigate(View::Preferences);
            }
            return;
        }
        let loading = self.store.values().any(|d| d.loading);
        let right = if loading {
            "Refreshing…".to_string()
        } else if self.backend.is_sample() {
            "Sample data".to_string()
        } else if let Some(ctx) = self.backend.context_name() {
            // e.g. "kubectl v1.37.1" once the matching kubectl is prepared
            match crate::settings::kubectl_status(&ctx) {
                Some(s) if s.contains("using kubectl v") => {
                    s.split("using ").nth(1).map(|x| x.replace(" (downloaded)", "")).unwrap_or_else(|| "via kubectl".into())
                }
                Some(s) if s.contains("preparing") => "preparing kubectl…".to_string(),
                _ => "via kubectl".to_string(),
            }
        } else {
            String::new()
        };
        p.text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, right, font, theme::TEXT_HI);
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        self.toasts.retain(|t| t.at.elapsed() < Duration::from_secs(if t.error { 10 } else { 5 }));
        if self.toasts.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        let mut y = screen.bottom() - 34.0;
        let mut dismiss = None;
        for (i, t) in self.toasts.iter().enumerate().rev() {
            let area = egui::Area::new(egui::Id::unique(("toast", i)))
                .order(egui::Order::Foreground)
                .pivot(Align2::RIGHT_BOTTOM)
                .fixed_pos(pos2(screen.right() - 16.0, y));
            let r = area.show(ctx, |ui| {
                egui::Frame::new()
                    .fill(if t.error { theme::RED } else { Color32::from_rgb(0x3b, 0x8e, 0x3f) })
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.set_max_width(380.0);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(if t.error { "⚠" } else { "✔" }).color(theme::TEXT_HI));
                            ui.label(egui::RichText::new(&t.text).color(theme::TEXT_HI));
                        });
                    })
                    .response
            });
            if r.inner.interact(Sense::click()).clicked() {
                dismiss = Some(i);
            }
            y -= r.response.rect.height() + 8.0;
        }
        if let Some(i) = dismiss {
            self.toasts.remove(i);
        }
        ctx.request_repaint_after(Duration::from_millis(500));
    }
}

pub fn ellipsize(ui: &egui::Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_owned(), egui::TextFormat::simple(font, color));
    job.wrap = egui::text::TextWrapping {
        max_width: max_w.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    ui.painter().layout_job(job)
}

fn draw_grid_icon(p: &egui::Painter, c: Pos2, col: Color32) {
    for dx in [-5.0, 5.0] {
        for dy in [-5.0, 5.0] {
            p.rect_filled(Rect::from_center_size(c + vec2(dx, dy), Vec2::splat(7.0)), CornerRadius::same(1), col);
        }
    }
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        // A downloaded update replaces this copy on the way out.
        let _ = self.updater.install();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.headless && self.screenshot.is_none() {
            self.updater.start(&ctx);
            if let crate::update::State::Ready(v) = self.updater.state() {
                if self.update_announced.as_ref() != Some(&v) {
                    self.update_announced = Some(v.clone());
                    self.toast(format!("Cluster Desk {v} is downloaded and installs when you quit."), false);
                }
            }
        }
        while let Ok(r) = self.backend.rx.try_recv() {
            self.handle(r);
        }
        self.frame += 1;

        // Keep the visible data fresh.
        if self.active.is_some() {
            self.ensure(Kind::Namespace);
            self.ensure(Kind::Crd);
            match self.view {
                View::List(k) => {
                    self.ensure(k);
                    if k == Kind::Pvc || k == Kind::Pv {
                        self.ensure_volume_stats();
                    }
                    if k == Kind::Node {
                        // The original loads pods from all namespaces for the requests figures.
                        self.ensure(Kind::Pod);
                        self.ensure_metrics();
                    }
                }
                View::Overview => {
                    self.ensure_metrics();
                    self.ensure(Kind::Node);
                    self.ensure(Kind::Pod);
                    self.ensure(Kind::Event);
                }
                View::Workloads => {
                    for k in GROUPS[0].kinds {
                        self.ensure(*k);
                    }
                    self.ensure(Kind::Event);
                }
                View::Catalog | View::Preferences => {}
            }
            if self.selected.as_ref().map(|s| s.0 == Kind::Node).unwrap_or(false) {
                self.ensure_metrics();
            }
            if self.selected.as_ref().map(|s| matches!(s.0, Kind::Pvc | Kind::Pv)).unwrap_or(false) {
                self.ensure_volume_stats();
            }
            if self.selected.is_some() {
                self.ensure(Kind::Event);
                self.ensure(Kind::Pod);
            }
            ctx.request_repaint_after(Duration::from_secs(2));
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.dialog.is_some() {
                self.dialog = None;
            } else {
                self.selected = None;
            }
        }

        egui::Panel::bottom("status")
            .exact_size(theme::STATUSBAR_HEIGHT)
            .frame(egui::Frame::new().fill(theme::BLUE))
            .show_separator_line(false)
            .show(ui, |ui| self.statusbar(ui));
        egui::Panel::left("hotbar")
            .exact_size(theme::HOTBAR_WIDTH)
            .resizable(false)
            .frame(egui::Frame::new().fill(theme::HOTBAR))
            .show_separator_line(false)
            .show(ui, |ui| self.hotbar(ui));
        egui::Panel::top("topbar")
            .exact_size(theme::TOPBAR_HEIGHT)
            .frame(egui::Frame::new().fill(theme::TOPBAR))
            .show_separator_line(false)
            .show(ui, |ui| self.topbar(ui));

        let in_cluster = self.active.is_some() && !matches!(self.view, View::Catalog | View::Preferences);
        if in_cluster || (self.view == View::Preferences && !self.dock_tabs.is_empty()) {
            if in_cluster {
                egui::Panel::left("sidebar")
                    .default_size(theme::SIDEBAR_WIDTH)
                    .size_range(170.0..=360.0)
                    .resizable(true)
                    .frame(egui::Frame::new().fill(theme::SIDEBAR))
                    .show_separator_line(false)
                    .show(ui, |ui| self.sidebar(ui));
            }
            if self.dock_open && !self.dock_tabs.is_empty() {
                egui::Panel::bottom("dock")
                    .default_size(300.0)
                    .size_range(120.0..=800.0)
                    .resizable(true)
                    .frame(egui::Frame::new().fill(theme::CONTENT))
                    .show(ui, |ui| self.dock(ui));
            } else if !self.dock_tabs.is_empty() {
                egui::Panel::bottom("dock_min")
                    .exact_size(32.0)
                    .resizable(false)
                    .frame(egui::Frame::new().fill(theme::DOCK_HEAD))
                    .show_separator_line(false)
                    .show(ui, |ui| self.dock(ui));
            }
        }
        let central = egui::CentralPanel::no_frame()
            .frame(egui::Frame::new().fill(theme::CONTENT).inner_margin(egui::Margin::symmetric(20, 14)))
            .show(ui, |ui| match self.view {
                View::Catalog => self.catalog(ui),
                View::Preferences => self.preferences(ui),
                View::Overview => self.overview(ui),
                View::Workloads => self.workloads(ui),
                View::List(k) => self.list_view(ui, k),
            })
            .response
            .rect;

        // Details drawer slides over the content from the right, like the original.
        if in_cluster && self.selected.is_some() {
            // Default drawer width 725 px (drawer storage), never wider than the content.
            let w = self.drawer_w.unwrap_or(theme::DRAWER_WIDTH).clamp(320.0, (central.width() * 0.95).max(320.0));
            let rect = Rect::from_min_max(pos2(central.right() - w, central.top()), central.right_bottom());
            egui::Area::new(egui::Id::unique("details_drawer"))
                .order(egui::Order::Middle)
                .fixed_pos(rect.min)
                .show(&ctx, |ui| {
                    ui.painter().rect_filled(
                        rect.translate(vec2(-6.0, 0.0)).with_max_x(rect.left()),
                        CornerRadius::ZERO,
                        Color32::from_black_alpha(60),
                    );
                    egui::Frame::new()
                        .fill(theme::CONTENT)
                        .stroke(Stroke::new(1.0, theme::HOTBAR))
                        .show(ui, |ui| {
                            ui.set_min_size(rect.size());
                            ui.set_max_size(rect.size());
                            self.details(ui);
                        });
                    let handle = Rect::from_min_max(rect.left_top(), pos2(rect.left() + 5.0, rect.bottom()));
                    let r = ui.interact(handle, egui::Id::unique("drawer_resize"), Sense::drag());
                    if r.hovered() || r.dragged() {
                        ctx.set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
                    }
                    if r.dragged() {
                        self.drawer_w = Some((w - r.drag_delta().x).clamp(320.0, (central.width() * 0.95).max(320.0)));
                    }
                });
        }

        self.dialogs(&ctx);
        self.toasts(&ctx);

        if let Some(path) = self.screenshot.clone() {
            ctx.request_repaint();
            // Real-cluster custom resources test: open the first group with a namespaced kind.
            if self.shot_crd_real && self.store.get(&Kind::Crd).map(|d| d.loaded.is_some()).unwrap_or(false) {
                self.shot_crd_real = false;
                self.expanded.insert("Custom Resources");
                if let Some((i, info)) = crate::crd::all().into_iter().find(|c| c.1.group.contains("cert-manager") && c.1.plural == "certificates")
                    .or_else(|| crate::crd::all().into_iter().next())
                {
                    self.expanded.insert(info.group);
                    self.view = View::List(Kind::Custom(i));
                }
            }
            let real_ready = !self.shot_wait_real
                || match self.view {
                    View::List(Kind::Node) => self.node_usage.is_some() || self.metrics_error.is_some(),
                    View::List(k) => self.store.get(&k).map(|d| d.loaded.is_some()).unwrap_or(false),
                    View::Preferences => {
                        self.prefs.contexts.lock().map(|c| c.is_some()).unwrap_or(false)
                            && self.prefs.helm.lock().map(|c| c.is_some()).unwrap_or(false)
                            && self.backend.context_name().and_then(|c| crate::settings::kubectl_status(&c)).map(|s| !s.ends_with('…')).unwrap_or(true)
                    }
                    _ => self.store.get(&Kind::Pod).map(|d| d.loaded.is_some()).unwrap_or(false),
                };
            // Real-cluster shell test: open the first running pod once pods are in, type, wait.
            if self.shot_shell_real && self.dock_tabs.is_empty() {
                if let Some(p) = self.rows(Kind::Pod).iter().find(|p| s_phase(&p.obj) == "Running").cloned() {
                    self.open_shell(&p.obj, None);
                    self.shot_type = Some("ls --color=always /; echo '🚀 ok ✅ ünïcödé ┌─┐'; tput cols\r".into());
                    self.shot_typed_at = Some(self.frame + 180);
                }
            }
            if let Some(t) = self.shot_type.clone() {
                let due = self.shot_typed_at.map(|f| self.frame >= f).unwrap_or(self.frame >= 30);
                if due {
                    if let Some(crate::app::DockKind::Shell { term: Some(term), .. }) =
                        self.dock_tabs.last_mut().map(|t| &mut t.kind)
                    {
                        term.inject(&t);
                    }
                    self.shot_type = None;
                    self.shot_done_at = Some(self.frame);
                }
            }
            let typed_ok = self.shot_type.is_none() && self.shot_done_at.map(|f| self.frame >= f + 120).unwrap_or(!self.shot_shell_real);
            if self.frame >= 120 && real_ready && typed_ok && !self.shot_requested && self.headless {
                // Off-screen picture: main() renders and saves it.
                self.shot_requested = true;
                self.shot_ready = true;
            }
            if self.frame >= 120 && real_ready && typed_ok && !self.shot_requested {
                self.shot_requested = true;
                ctx.request_screenshot(move |img: Arc<egui::ColorImage>| {
                    let [w, h] = img.size;
                    let mut buf = Vec::with_capacity(w * h * 4);
                    for p in &img.pixels {
                        buf.extend_from_slice(&[p.r(), p.g(), p.b(), 255]);
                    }
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    match image::save_buffer(&path, &buf, w as u32, h as u32, image::ExtendedColorType::Rgba8) {
                        Ok(()) => eprintln!("saved screenshot to {}", path.display()),
                        Err(e) => eprintln!("could not save screenshot: {e}"),
                    }
                    std::process::exit(0);
                });
            }
            if self.frame > 3000 && !self.headless {
                eprintln!("screenshot was not delivered");
                std::process::exit(1);
            }
        }
    }

    fn clear_color(&self, _v: &egui::Visuals) -> [f32; 4] {
        theme::CONTENT.to_normalized_gamma_f32()
    }
}

fn s_phase(v: &serde_json::Value) -> String {
    crate::util::s(v, "status/phase")
}
