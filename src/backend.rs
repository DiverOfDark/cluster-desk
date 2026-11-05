//! Talks to clusters off the UI thread: real clusters through `kubectl`, or the built-in sample cluster.

use eframe::egui;
use crate::demo::DemoCluster;
use crate::model::{Kind, Row, build_row};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Source {
    Sample,
    Context(String),
}

#[derive(Clone, Debug)]
pub struct ClusterEntry {
    pub name: String,
    pub source: Source,
}

#[derive(Clone, Debug)]
pub enum Job {
    List(Kind),
    Logs { tab: u64, ns: String, pod: String, container: String, previous: bool },
    Yaml { tab: u64, kind: Kind, ns: String, name: String },
    Replace { tab: u64, kind: Kind, ns: String, name: String, text: String },
    Delete { kind: Kind, ns: String, name: String },
    Scale { kind: Kind, ns: String, name: String, replicas: i64 },
    Restart { kind: Kind, ns: String, name: String },
    NodeMetrics,
    /// Volume usage from every node's kubelet stats (nodes/{name}/proxy/stats/summary).
    VolumeStats { nodes: Vec<String> },
}

/// Claimed volume usage: (namespace, claim name) -> (used bytes, capacity bytes).
pub type VolumeUsage = std::collections::HashMap<(String, String), (f64, f64)>;

/// Live usage per node from the metrics server: name -> (cpu cores, memory bytes).
pub type NodeUsage = std::collections::HashMap<String, (f64, f64)>;

pub enum Reply {
    List { generation: u64, kind: Kind, result: Result<Vec<Row>, String> },
    Text { generation: u64, tab: u64, result: Result<String, String> },
    Action { generation: u64, result: Result<String, String>, refresh: Kind, tab: Option<u64> },
    Contexts { names: Vec<String>, current: Option<String>, error: Option<String> },
    NodeMetrics { generation: u64, result: Result<NodeUsage, String> },
    VolumeStats { generation: u64, result: Result<VolumeUsage, String> },
}

fn parse_volume_stats(text: &str, out: &mut VolumeUsage) -> Result<(), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("Unexpected stats answer: {e}"))?;
    for pod in crate::util::arr(&v, "pods") {
        for vol in crate::util::arr(pod, "volume") {
            let name = crate::util::s(vol, "pvcRef/name");
            if name.is_empty() {
                continue;
            }
            let ns = crate::util::s(vol, "pvcRef/namespace");
            let used = vol.get("usedBytes").and_then(|x| x.as_f64()).unwrap_or(f64::NAN);
            let cap = vol.get("capacityBytes").and_then(|x| x.as_f64()).unwrap_or(f64::NAN);
            // The same claim can be mounted by several pods; keep the first answer.
            out.entry((ns, name)).or_insert((used, cap));
        }
    }
    Ok(())
}

/// Ask every node in parallel; nodes that refuse are skipped, all refusing is an error.
fn fetch_volume_stats(ctx: &str, nodes: &[String]) -> Result<VolumeUsage, String> {
    let results: Vec<Result<String, String>> = std::thread::scope(|sc| {
        let handles: Vec<_> = nodes
            .iter()
            .map(|n| {
                let mut a = base(ctx);
                a.extend(["get".into(), "--raw".into(), format!("/api/v1/nodes/{n}/proxy/stats/summary")]);
                sc.spawn(move || kubectl(&a, None))
            })
            .collect();
        handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err("stats thread failed".into()))).collect()
    });
    let mut out = VolumeUsage::new();
    let mut last_err = None;
    let mut ok = 0;
    for r in results {
        match r.and_then(|t| parse_volume_stats(&t, &mut out)) {
            Ok(()) => ok += 1,
            Err(e) => last_err = Some(e),
        }
    }
    if ok == 0 && !nodes.is_empty() {
        return Err(last_err.unwrap_or_else(|| "no node answered".into()));
    }
    Ok(out)
}

fn parse_node_metrics(text: &str) -> Result<NodeUsage, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("Unexpected metrics answer: {e}"))?;
    Ok(crate::util::arr(&v, "items")
        .iter()
        .map(|it| {
            (
                crate::util::s(it, "metadata/name"),
                (
                    crate::util::quantity(&crate::util::s(it, "usage/cpu")),
                    crate::util::quantity(&crate::util::s(it, "usage/memory")),
                ),
            )
        })
        .collect())
}

pub struct Backend {
    pub generation: u64,
    pub source: Option<Source>,
    demo: Arc<Mutex<DemoCluster>>,
    tx: Sender<Reply>,
    pub rx: Receiver<Reply>,
    ctx: egui::Context,
}

fn kubectl(args: &[String], stdin: Option<&str>) -> Result<String, String> {
    // Without an installed kubectl, the first call downloads one (the original bundles it).
    if let Err(why) = crate::settings::ensure_fallback_kubectl() {
        return Err(why);
    }
    // The kubectl prepared for this context (Preferences), with PATH/KUBECONFIG/proxy applied.
    let context = args.iter().position(|a| a == "--context").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
    let mut cmd: Command = crate::settings::command(&crate::settings::kubectl_for(context));
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not run kubectl ({e}). Is kubectl installed and on your PATH?"))?;
    if let Some(input) = stdin {
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(input.as_bytes());
        }
    }
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { format!("kubectl exited with {}", out.status) } else { err })
    }
}

fn base(ctx: &str) -> Vec<String> {
    vec!["--context".into(), ctx.into(), "--request-timeout=30s".into()]
}

fn ns_args(kind: Kind, ns: &str) -> Vec<String> {
    if kind.namespaced() && !ns.is_empty() { vec!["-n".into(), ns.into()] } else { vec![] }
}

fn run_kube(ctx: &str, job: &Job) -> Result<String, String> {
    let mut a = base(ctx);
    match job {
        Job::List(kind) => {
            a.extend(["get".into(), kind.resource().into(), "-o".into(), "json".into()]);
            if kind.namespaced() {
                a.push("--all-namespaces".into());
            }
        }
        Job::Logs { ns, pod, container, previous, .. } => {
            a.extend(["logs".into(), pod.clone(), "-n".into(), ns.clone(), "-c".into(), container.clone()]);
            a.extend(["--tail=500".into() /* logLinesToLoad = 500 */, "--timestamps".into()]);
            if *previous {
                a.push("--previous".into());
            }
        }
        Job::Yaml { kind, ns, name, .. } => {
            a.extend(["get".into(), kind.resource().into(), name.clone(), "-o".into(), "json".into()]);
            a.extend(ns_args(*kind, ns));
        }
        Job::Replace { text, .. } => {
            a.extend(["replace".into(), "-f".into(), "-".into()]);
            return kubectl(&a, Some(text));
        }
        Job::Delete { kind, ns, name } => {
            a.extend(["delete".into(), kind.resource().into(), name.clone(), "--wait=false".into()]);
            a.extend(ns_args(*kind, ns));
        }
        Job::Scale { kind, ns, name, replicas } => {
            a.extend(["scale".into(), kind.resource().into(), name.clone(), format!("--replicas={replicas}")]);
            a.extend(ns_args(*kind, ns));
        }
        Job::Restart { kind, ns, name } => {
            a.extend(["rollout".into(), "restart".into(), format!("{}/{}", kind.resource(), name)]);
            a.extend(ns_args(*kind, ns));
        }
        Job::NodeMetrics => {
            a.extend(["get".into(), "--raw".into(), "/apis/metrics.k8s.io/v1beta1/nodes".into()]);
        }
        Job::VolumeStats { .. } => unreachable!("volume stats are fetched per node"),
    }
    kubectl(&a, None)
}

fn parse_list(kind: Kind, text: &str) -> Result<Vec<Row>, String> {
    let mut v: Value = serde_json::from_str(text).map_err(|e| format!("Unexpected answer from kubectl: {e}"))?;
    let items = match v.get_mut("items") {
        Some(Value::Array(a)) => std::mem::take(a),
        _ => vec![],
    };
    Ok(items
        .into_iter()
        .map(|mut it| {
            // managedFields are never shown; CRD validation schemas can be tens of MB per cluster.
            // (Editing always fetches the full object fresh, so nothing is lost on save.)
            if let Some(m) = it.get_mut("metadata").and_then(|m| m.as_object_mut()) {
                m.remove("managedFields");
            }
            if kind == Kind::Crd {
                if let Some(vs) = it.pointer_mut("/spec/versions").and_then(|x| x.as_array_mut()) {
                    for ver in vs {
                        if let Some(o) = ver.as_object_mut() {
                            o.remove("schema");
                        }
                    }
                }
            }
            build_row(kind, it)
        })
        .collect())
}

impl Backend {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        Backend {
            generation: 0,
            source: None,
            demo: Arc::new(Mutex::new(DemoCluster::new())),
            tx,
            rx,
            ctx,
        }
    }

    pub fn connect(&mut self, source: Source) {
        self.generation += 1;
        self.source = Some(source);
    }

    /// The kubeconfig context in use, if a real cluster is open.
    pub fn context_name(&self) -> Option<String> {
        match &self.source {
            Some(Source::Context(c)) => Some(c.clone()),
            _ => None,
        }
    }

    /// Command runner for the sample cluster's simulated shell.
    pub fn sample_runner(&self, pod: &str, container: &str) -> Box<dyn Fn(&str) -> String + Send> {
        let demo = self.demo.clone();
        let (pod, container) = (pod.to_string(), container.to_string());
        Box::new(move |cmd: &str| demo.lock().map(|d| d.exec(&pod, &container, cmd)).unwrap_or_default())
    }

    pub fn is_sample(&self) -> bool {
        self.source == Some(Source::Sample)
    }

    pub fn discover_contexts(&self) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let names = kubectl(&["config".into(), "get-contexts".into(), "-o".into(), "name".into()], None);
            let current = kubectl(&["config".into(), "current-context".into()], None)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let reply = match names {
                Ok(n) => Reply::Contexts {
                    names: n.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect(),
                    current,
                    error: None,
                },
                Err(e) => Reply::Contexts { names: vec![], current: None, error: Some(e) },
            };
            let _ = tx.send(reply);
            ctx.request_repaint();
        });
    }

    /// Run a sample-cluster job immediately (no I/O); used for deterministic screenshots too.
    pub fn run_sample_now(&self, job: &Job) -> Reply {
        run_sample(&self.demo, self.generation, job)
    }

    pub fn submit(&self, job: Job) {
        let Some(source) = self.source.clone() else { return };
        let generation = self.generation;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let demo = self.demo.clone();
        std::thread::spawn(move || {
            let reply = match source {
                Source::Sample => run_sample(&demo, generation, &job),
                Source::Context(c) if matches!(job, Job::VolumeStats { .. }) => {
                    let Job::VolumeStats { nodes } = &job else { unreachable!() };
                    Reply::VolumeStats { generation, result: fetch_volume_stats(&c, nodes) }
                }
                Source::Context(c) => {
                    let res = run_kube(&c, &job);
                    match &job {
                        Job::List(kind) => Reply::List {
                            generation,
                            kind: *kind,
                            result: res.and_then(|t| parse_list(*kind, &t)),
                        },
                        Job::Yaml { tab, .. } => Reply::Text {
                            generation,
                            tab: *tab,
                            result: res.and_then(|t| {
                                serde_json::from_str::<Value>(&t)
                                    .map(|v| crate::util::to_yaml(&v))
                                    .map_err(|e| e.to_string())
                            }),
                        },
                        Job::Logs { tab, .. } => {
                            Reply::Text { generation, tab: *tab, result: res }
                        }
                        Job::Replace { tab, kind, .. } => Reply::Action {
                            generation,
                            result: res.map(|o| o.trim().to_string()),
                            refresh: *kind,
                            tab: Some(*tab),
                        },
                        Job::Delete { kind, .. } | Job::Scale { kind, .. } | Job::Restart { kind, .. } => {
                            Reply::Action { generation, result: res.map(|o| o.trim().to_string()), refresh: *kind, tab: None }
                        }
                        Job::NodeMetrics => {
                            Reply::NodeMetrics { generation, result: res.and_then(|t| parse_node_metrics(&t)) }
                        }
                        Job::VolumeStats { .. } => unreachable!(),
                    }
                }
            };
            let _ = tx.send(reply);
            ctx.request_repaint();
        });
    }
}

fn run_sample(demo: &Arc<Mutex<DemoCluster>>, generation: u64, job: &Job) -> Reply {
    let mut d = demo.lock().unwrap();
    match job {
        Job::List(kind) => Reply::List {
            generation,
            kind: *kind,
            result: Ok(d.list(*kind).into_iter().map(|v| build_row(*kind, v)).collect()),
        },
        Job::Logs { tab, ns, pod, container, .. } => {
            Reply::Text { generation, tab: *tab, result: Ok(d.logs(ns, pod, container)) }
        }
        Job::Yaml { tab, kind, ns, name } => {
            let v = d
                .list(*kind)
                .into_iter()
                .find(|v| crate::util::s(v, "metadata/name") == *name && crate::util::s(v, "metadata/namespace") == *ns);
            Reply::Text {
                generation,
                tab: *tab,
                result: v.map(|v| crate::util::to_yaml(&v)).ok_or_else(|| "not found".to_string()),
            }
        }
        Job::Replace { tab, kind, ns, name, text } => Reply::Action {
            generation,
            result: d.replace(*kind, ns, name, text),
            refresh: *kind,
            tab: Some(*tab),
        },
        Job::Delete { kind, ns, name } => {
            Reply::Action { generation, result: d.delete(*kind, ns, name), refresh: *kind, tab: None }
        }
        Job::Scale { kind, ns, name, replicas } => Reply::Action {
            generation,
            result: d.scale(*kind, ns, name, *replicas),
            refresh: *kind,
            tab: None,
        },
        Job::Restart { kind, ns, name } => {
            Reply::Action { generation, result: d.restart(*kind, ns, name), refresh: *kind, tab: None }
        }
        Job::NodeMetrics => Reply::NodeMetrics { generation, result: Ok(d.node_metrics()) },
        Job::VolumeStats { .. } => Reply::VolumeStats { generation, result: Ok(d.volume_stats()) },
    }
}
