//! Resource kinds, table columns and row building from Kubernetes JSON objects.

use crate::util::{self, arr, get, i, map_pairs, s};
use serde_json::Value;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    /// A custom resource type, index into the crd registry.
    Custom(u16),
    Node,
    Pod,
    Deployment,
    DaemonSet,
    StatefulSet,
    ReplicaSet,
    Job,
    CronJob,
    ConfigMap,
    Secret,
    ResourceQuota,
    LimitRange,
    Hpa,
    Pdb,
    Service,
    Endpoints,
    Ingress,
    NetworkPolicy,
    Pvc,
    Pv,
    StorageClass,
    Namespace,
    Event,
    ServiceAccount,
    ClusterRole,
    Role,
    ClusterRoleBinding,
    RoleBinding,
    Crd,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    /// Node condition colours from the original's stylesheet.
    Memory,
    Disk,
    Pid,
    Network,
    Ok,
    Warn,
    Err,
    Info,
    Muted,
    Plain,
}

#[derive(Clone, Debug)]
pub enum Cell {
    Text(String),
    Status(String, Tone),
    /// Little coloured squares, one per container.
    Boxes(Vec<Tone>),
    /// Creation timestamp (epoch seconds) rendered as an age.
    Age(i64),
    /// Several coloured words (conditions).
    Tags(Vec<(String, Tone)>),
    /// A usage bar like the original's LineProgress: `value` (usage) drawn over a half-opaque
    /// `secondary` (requests), both as percent of the bar, text under it and a hover tooltip.
    Meter(Meter),
}

impl Cell {
    pub fn sort_text(&self) -> String {
        match self {
            Cell::Text(t) | Cell::Status(t, _) => t.clone(),
            Cell::Boxes(b) => format!("{:05}", b.len()),
            Cell::Age(t) => format!("{:020}", i64::MAX - t),
            Cell::Tags(t) => t.iter().map(|x| x.0.as_str()).collect::<Vec<_>>().join(","),
            Cell::Meter(m) => format!("{:020.6}", if m.sort.is_finite() { m.sort } else { 0.0 }),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)] // Pods (hidden column) kept for parity with the original.
pub enum BarColor {
    Cpu,    // --blue
    Memory, // --magenta
    Disk,   // --golden
    Pods,   // --primary
}

#[derive(Clone, Debug)]
pub struct Meter {
    pub color: BarColor,
    /// Percent of the bar, already capped at 100 (None = not drawn).
    pub value: Option<f32>,
    pub secondary: Option<f32>,
    /// None = text only, no bar.
    pub bar: bool,
    pub text: String,
    pub tooltip: String,
    /// What the column sorts by.
    pub sort: f64,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub obj: Value,
    pub name: String,
    pub ns: String,
    pub uid: String,
    pub created: i64,
    pub cells: Vec<Cell>,
    pub search: String,
}

pub struct Group {
    pub title: &'static str,
    pub icon: &'static str,
    pub kinds: &'static [Kind],
}

pub const GROUPS: &[Group] = &[
    Group {
        title: "Workloads",
        icon: "▣",
        kinds: &[
            Kind::Pod,
            Kind::Deployment,
            Kind::DaemonSet,
            Kind::StatefulSet,
            Kind::ReplicaSet,
            Kind::Job,
            Kind::CronJob,
        ],
    },
    Group {
        title: "Config",
        icon: "☰",
        kinds: &[
            Kind::ConfigMap,
            Kind::Secret,
            Kind::ResourceQuota,
            Kind::LimitRange,
            Kind::Hpa,
            Kind::Pdb,
        ],
    },
    Group {
        title: "Network",
        icon: "⇄",
        kinds: &[Kind::Service, Kind::Endpoints, Kind::Ingress, Kind::NetworkPolicy],
    },
    Group {
        title: "Storage",
        icon: "⛁",
        kinds: &[Kind::Pvc, Kind::Pv, Kind::StorageClass],
    },
    Group {
        title: "Access Control",
        icon: "🔒",
        kinds: &[
            Kind::ServiceAccount,
            Kind::ClusterRole,
            Kind::Role,
            Kind::ClusterRoleBinding,
            Kind::RoleBinding,
        ],
    },
    Group {
        title: "Custom Resources",
        icon: "⬡",
        kinds: &[Kind::Crd],
    },
];

pub const ALL_KINDS: &[Kind] = &[
    Kind::Node,
    Kind::Pod,
    Kind::Deployment,
    Kind::DaemonSet,
    Kind::StatefulSet,
    Kind::ReplicaSet,
    Kind::Job,
    Kind::CronJob,
    Kind::ConfigMap,
    Kind::Secret,
    Kind::ResourceQuota,
    Kind::LimitRange,
    Kind::Hpa,
    Kind::Pdb,
    Kind::Service,
    Kind::Endpoints,
    Kind::Ingress,
    Kind::NetworkPolicy,
    Kind::Pvc,
    Kind::Pv,
    Kind::StorageClass,
    Kind::Namespace,
    Kind::Event,
    Kind::ServiceAccount,
    Kind::ClusterRole,
    Kind::Role,
    Kind::ClusterRoleBinding,
    Kind::RoleBinding,
    Kind::Crd,
];

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Kind::Custom(i) => crate::crd::info(i).kind,
            Kind::Node => "Nodes",
            Kind::Pod => "Pods",
            Kind::Deployment => "Deployments",
            Kind::DaemonSet => "Daemon Sets",
            Kind::StatefulSet => "Stateful Sets",
            Kind::ReplicaSet => "Replica Sets",
            Kind::Job => "Jobs",
            Kind::CronJob => "Cron Jobs",
            Kind::ConfigMap => "Config Maps",
            Kind::Secret => "Secrets",
            Kind::ResourceQuota => "Resource Quotas",
            Kind::LimitRange => "Limit Ranges",
            Kind::Hpa => "Horizontal Pod Autoscalers",
            Kind::Pdb => "Pod Disruption Budgets",
            Kind::Service => "Services",
            Kind::Endpoints => "Endpoints",
            Kind::Ingress => "Ingresses",
            Kind::NetworkPolicy => "Network Policies",
            Kind::Pvc => "Persistent Volume Claims",
            Kind::Pv => "Persistent Volumes",
            Kind::StorageClass => "Storage Classes",
            Kind::Namespace => "Namespaces",
            Kind::Event => "Events",
            Kind::ServiceAccount => "Service Accounts",
            Kind::ClusterRole => "Cluster Roles",
            Kind::Role => "Roles",
            Kind::ClusterRoleBinding => "Cluster Role Bindings",
            Kind::RoleBinding => "Role Bindings",
            Kind::Crd => "Definitions",
        }
    }

    pub fn short_title(self) -> &'static str {
        match self {
            Kind::Hpa => "HPA",
            Kind::Pdb => "Pod Disruption Budgets",
            k => k.title(),
        }
    }

    pub fn singular(self) -> &'static str {
        match self {
            Kind::Custom(i) => crate::crd::info(i).kind,
            Kind::Node => "Node",
            Kind::Pod => "Pod",
            Kind::Deployment => "Deployment",
            Kind::DaemonSet => "DaemonSet",
            Kind::StatefulSet => "StatefulSet",
            Kind::ReplicaSet => "ReplicaSet",
            Kind::Job => "Job",
            Kind::CronJob => "CronJob",
            Kind::ConfigMap => "ConfigMap",
            Kind::Secret => "Secret",
            Kind::ResourceQuota => "ResourceQuota",
            Kind::LimitRange => "LimitRange",
            Kind::Hpa => "HorizontalPodAutoscaler",
            Kind::Pdb => "PodDisruptionBudget",
            Kind::Service => "Service",
            Kind::Endpoints => "Endpoints",
            Kind::Ingress => "Ingress",
            Kind::NetworkPolicy => "NetworkPolicy",
            Kind::Pvc => "PersistentVolumeClaim",
            Kind::Pv => "PersistentVolume",
            Kind::StorageClass => "StorageClass",
            Kind::Namespace => "Namespace",
            Kind::Event => "Event",
            Kind::ServiceAccount => "ServiceAccount",
            Kind::ClusterRole => "ClusterRole",
            Kind::Role => "Role",
            Kind::ClusterRoleBinding => "ClusterRoleBinding",
            Kind::RoleBinding => "RoleBinding",
            Kind::Crd => "CustomResourceDefinition",
        }
    }

    /// Name kubectl understands.
    pub fn resource(self) -> &'static str {
        match self {
            Kind::Custom(i) => crate::crd::info(i).key,
            Kind::Node => "nodes",
            Kind::Pod => "pods",
            Kind::Deployment => "deployments.apps",
            Kind::DaemonSet => "daemonsets.apps",
            Kind::StatefulSet => "statefulsets.apps",
            Kind::ReplicaSet => "replicasets.apps",
            Kind::Job => "jobs.batch",
            Kind::CronJob => "cronjobs.batch",
            Kind::ConfigMap => "configmaps",
            Kind::Secret => "secrets",
            Kind::ResourceQuota => "resourcequotas",
            Kind::LimitRange => "limitranges",
            Kind::Hpa => "horizontalpodautoscalers.autoscaling",
            Kind::Pdb => "poddisruptionbudgets.policy",
            Kind::Service => "services",
            Kind::Endpoints => "endpoints",
            Kind::Ingress => "ingresses.networking.k8s.io",
            Kind::NetworkPolicy => "networkpolicies.networking.k8s.io",
            Kind::Pvc => "persistentvolumeclaims",
            Kind::Pv => "persistentvolumes",
            Kind::StorageClass => "storageclasses.storage.k8s.io",
            Kind::Namespace => "namespaces",
            Kind::Event => "events",
            Kind::ServiceAccount => "serviceaccounts",
            Kind::ClusterRole => "clusterroles.rbac.authorization.k8s.io",
            Kind::Role => "roles.rbac.authorization.k8s.io",
            Kind::ClusterRoleBinding => "clusterrolebindings.rbac.authorization.k8s.io",
            Kind::RoleBinding => "rolebindings.rbac.authorization.k8s.io",
            Kind::Crd => "customresourcedefinitions.apiextensions.k8s.io",
        }
    }

    pub fn namespaced(self) -> bool {
        if let Kind::Custom(i) = self {
            return crate::crd::info(i).namespaced;
        }
        !matches!(
            self,
            Kind::Node
                | Kind::Pv
                | Kind::StorageClass
                | Kind::Namespace
                | Kind::ClusterRole
                | Kind::ClusterRoleBinding
                | Kind::Crd
        )
    }

    pub fn scalable(self) -> bool {
        matches!(self, Kind::Deployment | Kind::StatefulSet | Kind::ReplicaSet)
    }

    pub fn restartable(self) -> bool {
        matches!(self, Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet)
    }

    /// Column titles and relative widths.
    pub fn columns(self) -> &'static [(&'static str, f32)] {
        match self {
            Kind::Custom(i) => crate::crd::info(i).columns,
            Kind::Pod => &[
                ("Name", 3.2),
                ("Namespace", 1.4),
                ("Containers", 1.1),
                ("Restarts", 0.8),
                ("Controlled By", 1.2),
                ("Node", 1.2),
                ("QoS", 1.0),
                ("Age", 0.7),
                ("Status", 1.1),
            ],
            Kind::Deployment => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Pods", 0.8),
                ("Replicas", 0.8),
                ("Age", 0.7),
                ("Conditions", 1.8),
            ],
            Kind::DaemonSet => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Pods", 0.8),
                ("Node Selector", 2.0),
                ("Age", 0.7),
            ],
            Kind::StatefulSet => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Pods", 0.8),
                ("Replicas", 0.8),
                ("Age", 0.7),
            ],
            Kind::ReplicaSet => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Desired", 0.8),
                ("Current", 0.8),
                ("Ready", 0.8),
                ("Age", 0.7),
            ],
            Kind::Job => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Completions", 1.0),
                ("Age", 0.7),
                ("Conditions", 1.2),
            ],
            Kind::CronJob => &[
                ("Name", 2.6),
                ("Namespace", 1.4),
                ("Schedule", 1.2),
                ("Suspend", 0.8),
                ("Active", 0.7),
                ("Last schedule", 1.0),
                ("Age", 0.7),
            ],
            // Same visible columns and flex weights as the original's nodes list
            // (Pods, Instance Type, Node Group and Capacity are hidden there by default).
            Kind::Node => &[
                ("Name", 1.0),
                ("CPU", 0.6),
                ("Memory", 0.6),
                ("Disk", 0.6),
                ("Roles", 1.5),
                ("Taints", 0.6),
                ("Version", 1.0),
                ("Internal IP", 1.3),
                ("Age", 0.4),
                ("Schedulable", 0.6),
                ("Conditions", 1.7),
            ],
            Kind::ConfigMap => &[("Name", 3.0), ("Namespace", 1.4), ("Keys", 3.0), ("Age", 0.7)],
            Kind::Secret => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Labels", 2.0),
                ("Keys", 1.6),
                ("Type", 1.8),
                ("Age", 0.7),
            ],
            Kind::ResourceQuota | Kind::LimitRange | Kind::ServiceAccount | Kind::Role => {
                &[("Name", 3.0), ("Namespace", 1.4), ("Age", 0.7)]
            }
            Kind::Hpa => &[
                ("Name", 2.6),
                ("Namespace", 1.4),
                ("Metrics", 1.6),
                ("Min Pods", 0.8),
                ("Max Pods", 0.8),
                ("Replicas", 0.8),
                ("Age", 0.7),
            ],
            Kind::Pdb => &[
                ("Name", 2.6),
                ("Namespace", 1.4),
                ("Min Available", 1.0),
                ("Max Unavailable", 1.0),
                ("Current Healthy", 1.0),
                ("Desired Healthy", 1.0),
                ("Age", 0.7),
            ],
            Kind::Service => &[
                ("Name", 2.4),
                ("Namespace", 1.3),
                ("Type", 1.0),
                ("Cluster IP", 1.1),
                ("Ports", 1.6),
                ("External IP", 1.0),
                ("Selector", 1.8),
                ("Age", 0.7),
                ("Status", 0.8),
            ],
            Kind::Endpoints => &[("Name", 2.6), ("Namespace", 1.4), ("Endpoints", 3.0), ("Age", 0.7)],
            Kind::Ingress => &[
                ("Name", 2.6),
                ("Namespace", 1.4),
                ("LoadBalancers", 1.4),
                ("Rules", 3.0),
                ("Age", 0.7),
            ],
            Kind::NetworkPolicy => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Policy Types", 1.6),
                ("Age", 0.7),
            ],
            Kind::Pvc => &[
                ("Name", 2.6),
                ("Namespace", 1.4),
                ("Storage class", 1.2),
                ("Size", 0.8),
                ("Usage", 1.2),
                ("Volume", 2.0),
                ("Age", 0.7),
                ("Status", 0.9),
            ],
            Kind::Pv => &[
                ("Name", 2.6),
                ("Storage Class", 1.2),
                ("Capacity", 0.8),
                ("Usage", 1.2),
                ("Claim", 2.0),
                ("Age", 0.7),
                ("Status", 0.9),
            ],
            Kind::StorageClass => &[
                ("Name", 2.0),
                ("Provisioner", 2.4),
                ("Reclaim Policy", 1.1),
                ("Default", 0.7),
                ("Age", 0.7),
            ],
            Kind::Namespace => &[("Name", 2.4), ("Labels", 4.0), ("Age", 0.7), ("Status", 0.9)],
            Kind::Event => &[
                ("Type", 0.8),
                ("Message", 4.4),
                ("Namespace", 1.2),
                ("Involved Object", 2.0),
                ("Source", 1.3),
                ("Count", 0.6),
                ("Age", 0.7),
                ("Last Seen", 0.8),
            ],
            Kind::ClusterRole => &[("Name", 4.0), ("Age", 0.7)],
            Kind::ClusterRoleBinding => &[("Name", 3.0), ("Bindings", 3.0), ("Age", 0.7)],
            Kind::RoleBinding => &[
                ("Name", 3.0),
                ("Namespace", 1.4),
                ("Bindings", 2.4),
                ("Age", 0.7),
            ],
            Kind::Crd => &[
                ("Resource", 2.6),
                ("Group", 2.2),
                ("Version", 0.8),
                ("Scope", 0.9),
                ("Age", 0.7),
            ],
        }
    }
}

fn t(x: impl Into<String>) -> Cell {
    Cell::Text(x.into())
}

pub fn created_of(v: &Value) -> i64 {
    util::parse_time(&s(v, "metadata/creationTimestamp")).unwrap_or(0)
}

/// Overall pod status as Lens-style apps show it.
pub fn pod_status(v: &Value) -> (String, Tone) {
    if !get(v, "metadata/deletionTimestamp").is_null() {
        return ("Terminating".into(), Tone::Muted);
    }
    for cs in arr(v, "status/containerStatuses") {
        let reason = s(cs, "state/waiting/reason");
        if !reason.is_empty() && reason != "ContainerCreating" {
            return (reason, Tone::Err);
        }
        let treason = s(cs, "state/terminated/reason");
        if !treason.is_empty() && treason != "Completed" && s(v, "status/phase") != "Succeeded" {
            return (treason, Tone::Err);
        }
    }
    let phase = s(v, "status/phase");
    let tone = match phase.as_str() {
        "Running" => Tone::Ok,
        "Succeeded" => Tone::Muted,
        "Pending" => Tone::Warn,
        "Failed" => Tone::Err,
        _ => Tone::Plain,
    };
    let label = if phase == "Succeeded" { "Completed".to_string() } else { phase };
    (label, tone)
}

pub fn container_tone(cs: &Value) -> Tone {
    let st = get(cs, "state");
    if !get(st, "running").is_null() {
        if get(cs, "ready").as_bool().unwrap_or(false) { Tone::Ok } else { Tone::Warn }
    } else if !get(st, "waiting").is_null() {
        let r = s(st, "waiting/reason");
        if r == "ContainerCreating" || r == "PodInitializing" { Tone::Warn } else { Tone::Err }
    } else if !get(st, "terminated").is_null() {
        if i(st, "terminated/exitCode") == 0 { Tone::Muted } else { Tone::Err }
    } else {
        Tone::Muted
    }
}

fn conditions_true(v: &Value) -> Vec<(String, Tone)> {
    arr(v, "status/conditions")
        .iter()
        .filter(|c| s(c, "status") == "True")
        .map(|c| {
            let ty = s(c, "type");
            let tone = match ty.as_str() {
                "Available" | "Complete" | "Ready" => Tone::Ok,
                "Progressing" => Tone::Info,
                "Failed" | "ReplicaFailure" => Tone::Err,
                _ => Tone::Warn,
            };
            (ty, tone)
        })
        .collect()
}

fn labels_text(v: &Value) -> String {
    map_pairs(v, "metadata/labels")
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn ports_text(v: &Value) -> String {
    arr(v, "spec/ports")
        .iter()
        .map(|p| {
            let port = i(p, "port");
            let proto = s(p, "protocol");
            let node = i(p, "nodePort");
            if node > 0 {
                format!("{port}:{node}/{proto}")
            } else {
                format!("{port}/{proto}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn node_roles(v: &Value) -> String {
    let mut roles: Vec<String> = map_pairs(v, "metadata/labels")
        .into_iter()
        .filter_map(|(k, _)| k.strip_prefix("node-role.kubernetes.io/").map(|r| r.to_string()))
        .collect();
    roles.sort();
    if roles.is_empty() { "<none>".into() } else { roles.join(", ") }
}

pub fn node_ready(v: &Value) -> bool {
    arr(v, "status/conditions")
        .iter()
        .any(|c| s(c, "type") == "Ready" && s(c, "status") == "True")
}

pub fn build_row(kind: Kind, v: Value) -> Row {
    let name = s(&v, "metadata/name");
    let ns = s(&v, "metadata/namespace");
    let created = created_of(&v);
    let cells = cells_for(kind, &v, created);
    let mut search = format!("{} {}", name, ns);
    for c in &cells {
        if let Cell::Text(x) | Cell::Status(x, _) = c {
            search.push(' ');
            search.push_str(x);
        }
    }
    search.push(' ');
    search.push_str(&labels_text(&v));
    Row {
        uid: s(&v, "metadata/uid"),
        name,
        ns,
        created,
        cells,
        search: search.to_lowercase(),
        obj: v,
    }
}

fn cells_for(kind: Kind, v: &Value, created: i64) -> Vec<Cell> {
    let name = s(v, "metadata/name");
    let ns = s(v, "metadata/namespace");
    let age = Cell::Age(created);
    match kind {
        Kind::Custom(i) => {
            // Name, Namespace (namespaced kinds), the printer columns, Age.
            let info = crate::crd::info(i);
            let mut cells = vec![t(name)];
            if info.namespaced {
                cells.push(t(ns));
            }
            for path in &info.printer {
                cells.push(t(crate::crd::json_path_text(v, path)));
            }
            cells.push(age);
            cells
        }
        Kind::Pod => {
            let statuses = arr(v, "status/containerStatuses");
            let mut boxes: Vec<Tone> = statuses.iter().map(container_tone).collect();
            if boxes.is_empty() {
                boxes = arr(v, "spec/containers").iter().map(|_| Tone::Muted).collect();
            }
            let restarts: i64 = statuses.iter().map(|c| i(c, "restartCount")).sum();
            let owner = arr(v, "metadata/ownerReferences")
                .first()
                .map(|o| s(o, "kind"))
                .unwrap_or_default();
            let (st, tone) = pod_status(v);
            vec![
                t(name),
                t(ns),
                Cell::Boxes(boxes),
                t(restarts.to_string()),
                t(owner),
                t(s(v, "spec/nodeName")),
                t(s(v, "status/qosClass")),
                age,
                Cell::Status(st, tone),
            ]
        }
        Kind::Deployment => {
            let want = i(v, "spec/replicas");
            let ready = i(v, "status/readyReplicas");
            vec![
                t(name),
                t(ns),
                t(format!("{ready}/{want}")),
                t(want.to_string()),
                age,
                Cell::Tags(conditions_true(v)),
            ]
        }
        Kind::DaemonSet => {
            let sel = map_pairs(v, "spec/template/spec/nodeSelector")
                .into_iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");
            vec![
                t(name),
                t(ns),
                t(format!(
                    "{}/{}",
                    i(v, "status/numberReady"),
                    i(v, "status/desiredNumberScheduled")
                )),
                t(sel),
                age,
            ]
        }
        Kind::StatefulSet => vec![
            t(name),
            t(ns),
            t(format!("{}/{}", i(v, "status/readyReplicas"), i(v, "spec/replicas"))),
            t(i(v, "spec/replicas").to_string()),
            age,
        ],
        Kind::ReplicaSet => vec![
            t(name),
            t(ns),
            t(i(v, "spec/replicas").to_string()),
            t(i(v, "status/replicas").to_string()),
            t(i(v, "status/readyReplicas").to_string()),
            age,
        ],
        Kind::Job => {
            let comp = get(v, "spec/completions").as_i64().unwrap_or(1);
            vec![
                t(name),
                t(ns),
                t(format!("{}/{}", i(v, "status/succeeded"), comp)),
                age,
                Cell::Tags(conditions_true(v)),
            ]
        }
        Kind::CronJob => {
            let last = util::parse_time(&s(v, "status/lastScheduleTime")).unwrap_or(0);
            let suspend = get(v, "spec/suspend").as_bool().unwrap_or(false);
            vec![
                t(name),
                t(ns),
                t(s(v, "spec/schedule")),
                if suspend {
                    Cell::Status("true".into(), Tone::Warn)
                } else {
                    t("false")
                },
                t(arr(v, "status/active").len().to_string()),
                Cell::Age(last),
                age,
            ]
        }
        Kind::Node => {
            let unschedulable = get(v, "spec/unschedulable").as_bool().unwrap_or(false);
            let ip = arr(v, "status/addresses")
                .iter()
                .find(|a| s(a, "type") == "InternalIP")
                .map(|a| s(a, "address"))
                .unwrap_or_default();
            vec![
                t(name),
                // CPU and Memory are filled in by `node_usage_cells` once metrics and pods are known.
                Cell::Meter(node_cpu_meter(v, f64::NAN, None)),
                Cell::Meter(node_memory_meter(v, f64::NAN, None)),
                // Disk usage only comes from Prometheus in the original; without it the cell says N/A.
                t("N/A"),
                t(node_roles(v)),
                t(arr(v, "spec/taints").len().to_string()),
                t(s(v, "status/nodeInfo/kubeletVersion")),
                t(ip),
                age,
                if unschedulable {
                    Cell::Status("False".into(), Tone::Err)
                } else {
                    Cell::Status("True".into(), Tone::Ok)
                },
                Cell::Tags(node_conditions(v)),
            ]
        }
        Kind::ConfigMap => {
            let keys: Vec<String> = map_pairs(v, "data").into_iter().map(|(k, _)| k).collect();
            vec![t(name), t(ns), t(keys.join(", ")), age]
        }
        Kind::Secret => {
            let keys: Vec<String> = map_pairs(v, "data").into_iter().map(|(k, _)| k).collect();
            vec![
                t(name),
                t(ns),
                t(labels_text(v)),
                t(keys.join(", ")),
                t(s(v, "type")),
                age,
            ]
        }
        Kind::ResourceQuota | Kind::LimitRange | Kind::ServiceAccount | Kind::Role => {
            vec![t(name), t(ns), age]
        }
        Kind::Hpa => {
            let metrics = arr(v, "spec/metrics")
                .iter()
                .map(|m| {
                    let res = s(m, "resource/name");
                    let target = i(m, "resource/target/averageUtilization");
                    if res.is_empty() {
                        s(m, "type")
                    } else {
                        format!("{res} {target}%")
                    }
                })
                .collect::<Vec<_>>()
                .join(", ");
            vec![
                t(name),
                t(ns),
                t(metrics),
                t(get(v, "spec/minReplicas").as_i64().unwrap_or(1).to_string()),
                t(i(v, "spec/maxReplicas").to_string()),
                t(i(v, "status/currentReplicas").to_string()),
                age,
            ]
        }
        Kind::Pdb => {
            let or_na = |p: &str| {
                let x = s(v, p);
                if x.is_empty() { "N/A".to_string() } else { x }
            };
            vec![
                t(name),
                t(ns),
                t(or_na("spec/minAvailable")),
                t(or_na("spec/maxUnavailable")),
                t(i(v, "status/currentHealthy").to_string()),
                t(i(v, "status/desiredHealthy").to_string()),
                age,
            ]
        }
        Kind::Service => {
            let ext: Vec<String> = arr(v, "status/loadBalancer/ingress")
                .iter()
                .map(|x| {
                    let ip = s(x, "ip");
                    if ip.is_empty() { s(x, "hostname") } else { ip }
                })
                .chain(arr(v, "spec/externalIPs").iter().map(|x| s(x, "")))
                .collect();
            let ty = s(v, "spec/type");
            let pending = ty == "LoadBalancer" && ext.is_empty();
            let sel = map_pairs(v, "spec/selector")
                .into_iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");
            vec![
                t(name),
                t(ns),
                t(ty),
                t(s(v, "spec/clusterIP")),
                t(ports_text(v)),
                t(if ext.is_empty() { "-".into() } else { ext.join(", ") }),
                t(sel),
                age,
                if pending {
                    Cell::Status("Pending".into(), Tone::Warn)
                } else {
                    Cell::Status("Active".into(), Tone::Ok)
                },
            ]
        }
        Kind::Endpoints => {
            let mut eps = vec![];
            for sub in arr(v, "subsets") {
                let ports: Vec<i64> = arr(sub, "ports").iter().map(|p| i(p, "port")).collect();
                for a in arr(sub, "addresses") {
                    for p in &ports {
                        eps.push(format!("{}:{}", s(a, "ip"), p));
                    }
                }
            }
            vec![t(name), t(ns), t(eps.join(", ")), age]
        }
        Kind::Ingress => {
            // getLoadBalancers (hostname || ip) and getRoutes ("url ⇢ service").
            let lbs: Vec<String> = util::ingress_load_balancers(v)
                .into_iter()
                .map(|(h, ip)| if h.is_empty() { ip } else { h })
                .collect();
            let rules: Vec<String> =
                util::ingress_routes(v).into_iter().map(|r| format!("{} ⇢ {}", r.url, r.service)).collect();
            vec![t(name), t(ns), t(lbs.join(", ")), t(rules.join(", ")), age]
        }
        Kind::NetworkPolicy => {
            let types: Vec<String> = arr(v, "spec/policyTypes").iter().map(|x| s(x, "")).collect();
            vec![t(name), t(ns), t(types.join(", ")), age]
        }
        Kind::Pvc => {
            let phase = s(v, "status/phase");
            let tone = if phase == "Bound" { Tone::Ok } else { Tone::Warn };
            vec![
                t(name),
                t(ns),
                t(s(v, "spec/storageClassName")),
                t(s(v, "spec/resources/requests/storage")),
                Cell::Meter(volume_meter(f64::NAN, f64::NAN)),
                t(s(v, "spec/volumeName")),
                age,
                Cell::Status(phase, tone),
            ]
        }
        Kind::Pv => {
            let phase = s(v, "status/phase");
            let tone = match phase.as_str() {
                "Bound" => Tone::Ok,
                "Available" => Tone::Info,
                _ => Tone::Warn,
            };
            let claim = if s(v, "spec/claimRef/name").is_empty() {
                String::new()
            } else {
                format!("{}/{}", s(v, "spec/claimRef/namespace"), s(v, "spec/claimRef/name"))
            };
            vec![
                t(name),
                t(s(v, "spec/storageClassName")),
                t(s(v, "spec/capacity/storage")),
                Cell::Meter(volume_meter(f64::NAN, f64::NAN)),
                t(claim),
                age,
                Cell::Status(phase, tone),
            ]
        }
        Kind::StorageClass => {
            let def = map_pairs(v, "metadata/annotations")
                .into_iter()
                .any(|(k, val)| k == "storageclass.kubernetes.io/is-default-class" && val == "true");
            vec![
                t(name),
                t(s(v, "provisioner")),
                t(s(v, "reclaimPolicy")),
                t(if def { "Yes" } else { "" }),
                age,
            ]
        }
        Kind::Namespace => {
            let phase = s(v, "status/phase");
            let tone = if phase == "Active" { Tone::Ok } else { Tone::Muted };
            vec![t(name), t(labels_text(v)), age, Cell::Status(phase, tone)]
        }
        Kind::Event => {
            let ty = s(v, "type");
            let tone = if ty == "Warning" { Tone::Err } else { Tone::Plain };
            let first = util::parse_time(&s(v, "firstTimestamp")).unwrap_or(created);
            let last = util::parse_time(&s(v, "lastTimestamp"))
                .or_else(|| util::parse_time(&s(v, "eventTime")))
                .unwrap_or(first);
            let mut src = s(v, "source/component");
            if src.is_empty() {
                src = s(v, "reportingComponent");
            }
            vec![
                Cell::Status(ty, tone),
                t(s(v, "message")),
                t(ns),
                t(format!(
                    "{}: {}",
                    s(v, "involvedObject/kind"),
                    s(v, "involvedObject/name")
                )),
                t(src),
                t(get(v, "count").as_i64().unwrap_or(1).to_string()),
                Cell::Age(first),
                Cell::Age(last),
            ]
        }
        Kind::ClusterRole => vec![t(name), age],
        Kind::ClusterRoleBinding | Kind::RoleBinding => {
            let subj: Vec<String> = arr(v, "subjects").iter().map(|x| s(x, "name")).collect();
            if kind == Kind::ClusterRoleBinding {
                vec![t(name), t(subj.join(", ")), age]
            } else {
                vec![t(name), t(ns), t(subj.join(", ")), age]
            }
        }
        Kind::Crd => {
            let version = arr(v, "spec/versions")
                .iter()
                .find(|x| get(x, "storage").as_bool().unwrap_or(false))
                .map(|x| s(x, "name"))
                .unwrap_or_default();
            vec![
                t(s(v, "spec/names/kind")),
                t(s(v, "spec/group")),
                t(version),
                t(s(v, "spec/scope")),
                age,
            ]
        }
    }
}

/// Which pod is "owned" (directly or through a ReplicaSet) by a workload, for the details panel.
pub fn pod_belongs_to(pod: &Value, kind: Kind, name: &str) -> bool {
    arr(pod, "metadata/ownerReferences").iter().any(|o| {
        let ok = s(o, "kind");
        let on = s(o, "name");
        match kind {
            Kind::Deployment => {
                ok == "ReplicaSet" && on.rsplit_once('-').map(|x| x.0) == Some(name)
            }
            _ => ok == kind.singular() && on == name,
        }
    })
}

// ------------------------------------------------------------------ node usage (original: nodes/route.tsx)

/// formatCores: "N/A" for NaN, two decimals below 10 cores, one decimal above.
pub fn format_cores(c: f64) -> String {
    if c.is_nan() {
        "N/A".into()
    } else if c < 10.0 {
        format!("{c:.2}")
    } else {
        format!("{c:.1}")
    }
}

/// bytesToUnits: powers of 1024, suffix B/KiB/MiB/GiB/TiB/PiB/EiB, "N/A" for <= 0 or not finite.
pub fn bytes_to_units(bytes: f64, precision: usize) -> String {
    if bytes <= 0.0 || !bytes.is_finite() {
        return "N/A".into();
    }
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let idx = ((bytes.ln() / 1024f64.ln()).floor() as usize).min(units.len() - 1);
    format!("{:.*}{}", precision, bytes / 1024f64.powi(idx as i32), units[idx])
}

/// bytesToUnitsAligned: below 1024 bytes show fractional Ki, otherwise bytesToUnits(precision 1)
/// with the trailing "B" removed ("5.9Gi").
pub fn bytes_to_units_aligned(bytes: f64) -> String {
    if bytes < 1024.0 {
        return format!("{:.1}Ki", bytes / 1024.0);
    }
    let s = bytes_to_units(bytes, 1);
    s.strip_suffix('B').map(|x| x.to_string()).unwrap_or(s)
}

/// LineProgress valuePercent: min(100, value / (max - min) * 100).
fn value_percent(value: f64, max: f64) -> f32 {
    (value / max * 100.0).min(100.0) as f32
}

/// Requests of the node's non-terminated pods (pods with a node and phase not Succeeded/Failed).
pub fn node_requests(node_name: &str, pods: &[Row]) -> (f64, f64) {
    let mut cpu = 0.0;
    let mut mem = 0.0;
    for p in pods {
        let phase = s(&p.obj, "status/phase");
        if s(&p.obj, "spec/nodeName") != node_name || phase == "Succeeded" || phase == "Failed" {
            continue;
        }
        for c in arr(&p.obj, "spec/containers") {
            cpu += util::quantity(&s(c, "resources/requests/cpu"));
            mem += util::quantity(&s(c, "resources/requests/memory"));
        }
    }
    (cpu, mem)
}

/// renderCpuUsage: bar max = allocatable CPU, value = metrics usage, secondary = requests;
/// text "usage / requests" when pods are loaded.
pub fn node_cpu_meter(v: &Value, usage: f64, requests: Option<f64>) -> Meter {
    let allocatable = util::quantity(&s(v, "status/allocatable/cpu"));
    let capacity = allocatable;
    let mut tip = vec![];
    if usage.is_finite() && capacity > 0.0 {
        tip.push(format!("CPU: {:.2}%, cores: {}", usage * 100.0 / capacity, format_cores(capacity)));
    }
    tip.push(format!("Usage: {} cores", format_cores(usage)));
    if let Some(r) = requests {
        let of = if allocatable > 0.0 {
            format!(" ({:.0}% of allocatable {})", r * 100.0 / allocatable, format_cores(allocatable))
        } else {
            String::new()
        };
        tip.push(format!("Requests: {} cores{of}", format_cores(r)));
    }
    meter(BarColor::Cpu, usage, requests, capacity, match requests {
        Some(r) => format!("{} / {}", format_cores(usage), format_cores(r)),
        None => format_cores(usage),
    }, tip)
}

/// renderMemoryUsage: same rules with bytes.
pub fn node_memory_meter(v: &Value, usage: f64, requests: Option<f64>) -> Meter {
    let allocatable = util::quantity(&s(v, "status/allocatable/memory"));
    let capacity = allocatable;
    let mut tip = vec![];
    if usage.is_finite() && capacity > 0.0 {
        tip.push(format!("Memory: {:.2}%, {}", usage * 100.0 / capacity, bytes_to_units(usage, 3)));
    }
    tip.push(format!("Usage: {}", bytes_to_units(usage, 3)));
    if let Some(r) = requests {
        let of = if allocatable > 0.0 {
            format!(" ({:.0}% of allocatable {})", r * 100.0 / allocatable, bytes_to_units(allocatable, 3))
        } else {
            String::new()
        };
        tip.push(format!("Requests: {}{of}", bytes_to_units(r, 3)));
    }
    meter(BarColor::Memory, usage, requests, capacity, match requests {
        Some(r) => format!("{} / {}", bytes_to_units_aligned(usage), bytes_to_units_aligned(r)),
        None => bytes_to_units_aligned(usage),
    }, tip)
}

/// renderUsage: a bar only when there is a capacity and either usage or requests (> 0).
fn meter(color: BarColor, usage: f64, requests: Option<f64>, capacity: f64, text: String, tip: Vec<String>) -> Meter {
    let has_usage = usage.is_finite();
    let has_requests = requests.map(|r| r > 0.0).unwrap_or(false);
    let bar = capacity > 0.0 && (has_usage || has_requests);
    Meter {
        color,
        value: if bar { Some(if has_usage { value_percent(usage, capacity) } else { 0.0 }) } else { None },
        secondary: if bar && has_requests { Some(value_percent(requests.unwrap_or(0.0), capacity)) } else { None },
        bar,
        text: if !bar && text.is_empty() { "N/A".into() } else { text },
        tooltip: tip.join("\n"),
        sort: usage,
    }
}

/// Node conditions shown in the list, with the original's per-condition colours.
pub fn node_conditions(v: &Value) -> Vec<(String, Tone)> {
    let mut out: Vec<(String, Tone)> = arr(v, "status/conditions")
        .iter()
        .filter(|c| s(c, "status") == "True")
        .map(|c| {
            let ty = s(c, "type");
            let tone = match ty.as_str() {
                "Ready" => Tone::Ok,
                "MemoryPressure" => Tone::Memory,
                "DiskPressure" => Tone::Disk,
                "PIDPressure" | "PidPressure" => Tone::Pid,
                "NetworkUnavailable" => Tone::Network,
                "OutOfDisk" => Tone::Err,
                _ => Tone::Plain,
            };
            (ty, tone)
        })
        .collect();
    if out.is_empty() {
        out.push(("NotReady".into(), Tone::Err));
    }
    if get(v, "spec/unschedulable").as_bool().unwrap_or(false) {
        out.push(("SchedulingDisabled".into(), Tone::Warn));
    }
    out
}

/// Volume fill level from kubelet stats: bar = used / capacity, text "42%  8.4Gi / 20.0Gi".
pub fn volume_meter(used: f64, capacity: f64) -> Meter {
    if !used.is_finite() || !capacity.is_finite() || capacity <= 0.0 {
        return Meter {
            color: BarColor::Disk,
            value: None,
            secondary: None,
            bar: false,
            text: "N/A".into(),
            tooltip: "No usage reported (volume not mounted, or node stats unavailable)".into(),
            sort: f64::NAN,
        };
    }
    let pct = used * 100.0 / capacity;
    Meter {
        color: BarColor::Disk,
        value: Some(value_percent(used, capacity)),
        secondary: None,
        bar: true,
        text: format!("{:.0}%  {} / {}", pct, bytes_to_units_aligned(used), bytes_to_units_aligned(capacity)),
        tooltip: format!(
            "Used: {} of {} ({:.2}%)\nFree: {}",
            bytes_to_units(used, 3),
            bytes_to_units(capacity, 3),
            pct,
            bytes_to_units((capacity - used).max(0.0), 3)
        ),
        sort: pct,
    }
}
