//! A built-in sample cluster, so the app is useful (and screenshot-able) without any real cluster.

use crate::model::Kind;
use crate::util::{self, format_time, now_secs, s};
use serde_json::{Value, json};
use std::collections::HashMap;

pub struct DemoCluster {
    pub objects: HashMap<Kind, Vec<Value>>,
    /// Custom resources by "<plural>.<group>".
    pub custom: HashMap<String, Vec<Value>>,
    counter: u64,
}

fn meta(name: &str, ns: Option<&str>, age_s: i64, labels: Value) -> Value {
    let mut m = json!({
        "name": name,
        "uid": format!("{:08x}-demo-{}", fxhash(name, ns.unwrap_or("")), name.len()),
        "creationTimestamp": format_time(now_secs() - age_s),
        "resourceVersion": "1",
        "labels": labels,
    });
    if let Some(ns) = ns {
        m["namespace"] = json!(ns);
    }
    m
}

fn fxhash(a: &str, b: &str) -> u32 {
    let mut h: u32 = 2166136261;
    for c in a.bytes().chain(b.bytes()) {
        h ^= c as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

const D: i64 = 86400;
const H: i64 = 3600;

struct PodSpec<'a> {
    name: &'a str,
    ns: &'a str,
    node: &'a str,
    owner: (&'a str, &'a str),
    containers: &'a [(&'a str, &'a str)],
    state: &'a str, // running | crash | pending | completed | notready
    restarts: i64,
    age: i64,
    qos: &'a str,
    app: &'a str,
}

fn pod(p: PodSpec) -> Value {
    let started = format_time(now_secs() - p.age + 20);
    let statuses: Vec<Value> = p
        .containers
        .iter()
        .map(|(cname, image)| {
            let state = match p.state {
                "crash" => json!({"waiting": {"reason": "CrashLoopBackOff", "message": "back-off 5m0s restarting failed container"}}),
                "pending" => json!({"waiting": {"reason": "ContainerCreating"}}),
                "completed" => json!({"terminated": {"exitCode": 0, "reason": "Completed", "startedAt": started}}),
                _ => json!({"running": {"startedAt": started}}),
            };
            json!({
                "name": cname,
                "image": image,
                "ready": p.state == "running",
                "restartCount": p.restarts,
                "state": state,
                "containerID": format!("containerd://{:016x}", fxhash(cname, p.name)),
            })
        })
        .collect();
    let containers: Vec<Value> = p
        .containers
        .iter()
        .map(|(cname, image)| {
            json!({
                "name": cname,
                "image": image,
                "imagePullPolicy": "IfNotPresent",
                "ports": [{"containerPort": 8080, "protocol": "TCP", "name": "http"}],
                "resources": if p.qos == "BestEffort" { json!({}) } else { json!({
                    "requests": {"cpu": "100m", "memory": "128Mi"},
                    "limits": if p.qos == "Guaranteed" { json!({"cpu": "100m", "memory": "128Mi"}) } else { json!({"memory": "256Mi"}) }
                })},
                "env": [{"name": "LOG_LEVEL", "value": "info"}],
                "volumeMounts": [{"name": "kube-api-access", "mountPath": "/var/run/secrets/kubernetes.io/serviceaccount", "readOnly": true}],
            })
        })
        .collect();
    let phase = match p.state {
        "pending" => "Pending",
        "completed" => "Succeeded",
        _ => "Running",
    };
    let ready = if p.state == "running" { "True" } else { "False" };
    let hash = p.owner.1.rsplit('-').next().unwrap_or("").to_string();
    let mut labels = json!({
        "app": p.app,
        "app.kubernetes.io/name": p.app,
        "app.kubernetes.io/instance": format!("{}-{}", p.ns, p.app),
        "app.kubernetes.io/part-of": p.ns,
        "app.kubernetes.io/managed-by": "Helm",
        "helm.sh/chart": format!("{}-1.4.2", p.app),
    });
    if p.owner.0 == "ReplicaSet" {
        labels["pod-template-hash"] = json!(hash);
    }
    let mut m = meta(p.name, Some(p.ns), p.age, labels);
    if !p.owner.0.is_empty() {
        m["ownerReferences"] = json!([{"apiVersion": "apps/v1", "kind": p.owner.0, "name": p.owner.1, "controller": true}]);
    }
    let ip_tail = fxhash(p.name, p.ns) % 250 + 2;
    json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": m,
        "spec": {
            "nodeName": if p.state == "pending" { "" } else { p.node },
            "containers": containers,
            "restartPolicy": "Always",
            "serviceAccountName": "default",
            "dnsPolicy": "ClusterFirst",
            "priority": 0,
            "tolerations": [{"key": "node.kubernetes.io/not-ready", "operator": "Exists", "effect": "NoExecute", "tolerationSeconds": 300}],
            "volumes": if p.owner.0 == "StatefulSet" {
                json!([{"name": "data", "persistentVolumeClaim": {"claimName": format!("data-{}", p.name)}}])
            } else {
                json!([])
            },
        },
        "status": {
            "phase": phase,
            "qosClass": p.qos,
            "podIP": if p.state == "pending" { String::new() } else { format!("10.42.{}.{}", fxhash(p.node, "") % 3, ip_tail) },
            "hostIP": "192.168.1.1",
            "startTime": started,
            "conditions": [
                {"type": "Initialized", "status": "True"},
                {"type": "Ready", "status": ready},
                {"type": "ContainersReady", "status": ready},
                {"type": "PodScheduled", "status": if p.state == "pending" { "False" } else { "True" }},
            ],
            "containerStatuses": statuses,
        }
    })
}

fn deployment(name: &str, ns: &str, replicas: i64, ready: i64, image: &str, age: i64) -> Value {
    let mut conds = vec![json!({"type": "Progressing", "status": "True", "reason": "NewReplicaSetAvailable"})];
    conds.push(json!({"type": "Available", "status": if ready >= replicas { "True" } else { "False" }}));
    json!({
        "apiVersion": "apps/v1", "kind": "Deployment",
        "metadata": meta(name, Some(ns), age, json!({"app": name})),
        "spec": {
            "replicas": replicas,
            "selector": {"matchLabels": {"app": name}},
            "strategy": {"type": "RollingUpdate", "rollingUpdate": {"maxSurge": "25%", "maxUnavailable": "25%"}},
            "template": {"metadata": {"labels": {"app": name}}, "spec": {"containers": [{"name": name, "image": image}]}},
        },
        "status": {"replicas": replicas, "readyReplicas": ready, "availableReplicas": ready, "updatedReplicas": replicas, "conditions": conds}
    })
}

fn replicaset(dep: &str, ns: &str, hash: &str, replicas: i64, ready: i64, age: i64) -> Value {
    let mut m = meta(&format!("{dep}-{hash}"), Some(ns), age, json!({"app": dep, "pod-template-hash": hash}));
    m["ownerReferences"] = json!([{"apiVersion": "apps/v1", "kind": "Deployment", "name": dep, "controller": true}]);
    json!({
        "apiVersion": "apps/v1", "kind": "ReplicaSet", "metadata": m,
        "spec": {"replicas": replicas, "selector": {"matchLabels": {"app": dep}}},
        "status": {"replicas": replicas, "readyReplicas": ready}
    })
}

fn service(name: &str, ns: &str, ty: &str, ip: &str, ports: Value, selector: Value, age: i64) -> Value {
    let mut status = json!({});
    if ty == "LoadBalancer" {
        status = json!({"loadBalancer": {"ingress": [{"ip": "192.168.1.240"}]}});
    }
    json!({
        "apiVersion": "v1", "kind": "Service",
        "metadata": meta(name, Some(ns), age, json!({"app": name})),
        "spec": {"type": ty, "clusterIP": ip, "ports": ports, "selector": selector, "sessionAffinity": "None"},
        "status": status
    })
}

fn node(name: &str, roles: &[&str], cpu: &str, mem: &str, age: i64, ready: bool) -> Value {
    let mut labels = json!({"kubernetes.io/hostname": name, "kubernetes.io/os": "linux", "kubernetes.io/arch": "amd64"});
    for r in roles {
        labels[format!("node-role.kubernetes.io/{r}")] = json!("");
    }
    let taints = if roles.contains(&"control-plane") {
        json!([{"key": "node-role.kubernetes.io/control-plane", "effect": "NoSchedule"}])
    } else {
        json!([])
    };
    json!({
        "apiVersion": "v1", "kind": "Node",
        "metadata": meta(name, None, age, labels),
        "spec": {"podCIDR": "10.42.0.0/24", "taints": taints},
        "status": {
            "capacity": {"cpu": cpu, "memory": mem, "pods": "110", "ephemeral-storage": "102350Mi"},
            "allocatable": {"cpu": cpu, "memory": mem, "pods": "110"},
            "addresses": [{"type": "InternalIP", "address": format!("192.168.1.{}", 10 + name.bytes().last().unwrap_or(b'0').saturating_sub(b'0'))}, {"type": "Hostname", "address": name}],
            "nodeInfo": {"kubeletVersion": "v1.31.4", "osImage": "Ubuntu 24.04.1 LTS", "kernelVersion": "6.8.0-51-generic", "containerRuntimeVersion": "containerd://1.7.24", "architecture": "amd64", "operatingSystem": "linux"},
            "conditions": [
                {"type": "MemoryPressure", "status": "False"},
                {"type": "DiskPressure", "status": "False"},
                {"type": "PIDPressure", "status": "False"},
                {"type": "Ready", "status": if ready { "True" } else { "False" }},
            ]
        }
    })
}

fn simple(api: &str, kind: &str, name: &str, ns: Option<&str>, age: i64, extra: Value) -> Value {
    let mut v = json!({"apiVersion": api, "kind": kind, "metadata": meta(name, ns, age, json!({}))});
    if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
        for (k, val) in e {
            o.insert(k, val);
        }
    }
    v
}

fn event(ns: &str, ty: &str, reason: &str, kind: &str, obj: &str, msg: &str, src: &str, count: i64, ago: i64) -> Value {
    let t = format_time(now_secs() - ago);
    json!({
        "apiVersion": "v1", "kind": "Event",
        "metadata": meta(&format!("{obj}.{:x}", fxhash(msg, reason)), Some(ns), ago + 600, json!({})),
        "type": ty, "reason": reason, "message": msg, "count": count,
        "involvedObject": {"kind": kind, "name": obj, "namespace": ns},
        "source": {"component": src},
        "firstTimestamp": format_time(now_secs() - ago - 600), "lastTimestamp": t,
    })
}

impl DemoCluster {
    pub fn new() -> Self {
        let mut o: HashMap<Kind, Vec<Value>> = HashMap::new();
        let nodes = vec![
            node("node-1", &["control-plane"], "4", "16323520Ki", 92 * D, true),
            node("node-2", &["worker"], "8", "32651104Ki", 92 * D, true),
            node("node-3", &["worker"], "8", "32651104Ki", 41 * D, true),
        ];
        o.insert(Kind::Node, nodes);

        let ns_list = ["default", "kube-system", "kube-public", "monitoring", "ingress-nginx", "shop"];
        o.insert(
            Kind::Namespace,
            ns_list
                .iter()
                .map(|n| {
                    let mut v = simple("v1", "Namespace", n, None, 92 * D, json!({"status": {"phase": "Active"}}));
                    v["metadata"]["labels"] = json!({"kubernetes.io/metadata.name": n});
                    v
                })
                .collect(),
        );

        let mut pods = vec![];
        macro_rules! p {
            ($name:expr, $ns:expr, $node:expr, $ok:expr, $on:expr, $cs:expr, $st:expr, $r:expr, $age:expr, $qos:expr, $app:expr) => {
                pods.push(pod(PodSpec {
                    name: $name,
                    ns: $ns,
                    node: $node,
                    owner: ($ok, $on),
                    containers: $cs,
                    state: $st,
                    restarts: $r,
                    age: $age,
                    qos: $qos,
                    app: $app,
                }))
            };
        }
        let web = &[("web", "registry.example.com/shop/web:2.4.1")][..];
        let api = &[("api", "registry.example.com/shop/api:1.18.0"), ("envoy", "envoyproxy/envoy:v1.31.2")][..];
        let worker = &[("worker", "registry.example.com/shop/worker:1.18.0")][..];
        p!("web-7d9f8c6b5d-4xk2p", "shop", "node-2", "ReplicaSet", "web-7d9f8c6b5d", web, "running", 0, 3 * D, "Burstable", "web");
        p!("web-7d9f8c6b5d-9mzqt", "shop", "node-3", "ReplicaSet", "web-7d9f8c6b5d", web, "running", 0, 3 * D, "Burstable", "web");
        p!("web-7d9f8c6b5d-hw7rl", "shop", "node-2", "ReplicaSet", "web-7d9f8c6b5d", web, "running", 1, 3 * D, "Burstable", "web");
        p!("api-5b8c7d9f4-2lsvn", "shop", "node-3", "ReplicaSet", "api-5b8c7d9f4", api, "running", 0, 26 * H, "Guaranteed", "api");
        p!("api-5b8c7d9f4-x8jdk", "shop", "node-2", "ReplicaSet", "api-5b8c7d9f4", api, "running", 2, 26 * H, "Guaranteed", "api");
        p!("worker-6f7c9d8b7-kq5mn", "shop", "node-3", "ReplicaSet", "worker-6f7c9d8b7", worker, "running", 0, 5 * H, "Burstable", "worker");
        p!("worker-6f7c9d8b7-zt2wx", "shop", "node-2", "ReplicaSet", "worker-6f7c9d8b7", worker, "crash", 14, 5 * H, "Burstable", "worker");
        p!("redis-0", "shop", "node-3", "StatefulSet", "redis", &[("redis", "redis:7.4-alpine")], "running", 0, 12 * D, "Burstable", "redis");
        p!("postgres-0", "shop", "node-2", "StatefulSet", "postgres", &[("postgres", "postgres:16.6")], "running", 0, 12 * D, "Guaranteed", "postgres");
        p!("nightly-report-28914720-6vbqc", "shop", "node-3", "Job", "nightly-report-28914720", &[("report", "registry.example.com/shop/report:0.9.3")], "completed", 0, 9 * H, "BestEffort", "nightly-report");
        p!("thumbnailer-84c5d6f9b-p7wvz", "shop", "", "ReplicaSet", "thumbnailer-84c5d6f9b", &[("thumbnailer", "registry.example.com/shop/thumbnailer:0.3.0")], "pending", 0, 2 * 60, "Burstable", "thumbnailer");
        p!("coredns-7c65d6cfc9-8bqsx", "kube-system", "node-1", "ReplicaSet", "coredns-7c65d6cfc9", &[("coredns", "registry.k8s.io/coredns/coredns:v1.11.3")], "running", 0, 92 * D, "Burstable", "coredns");
        p!("coredns-7c65d6cfc9-tn6lh", "kube-system", "node-2", "ReplicaSet", "coredns-7c65d6cfc9", &[("coredns", "registry.k8s.io/coredns/coredns:v1.11.3")], "running", 0, 92 * D, "Burstable", "coredns");
        p!("etcd-node-1", "kube-system", "node-1", "Node", "node-1", &[("etcd", "registry.k8s.io/etcd:3.5.15-0")], "running", 0, 92 * D, "Burstable", "etcd");
        p!("kube-apiserver-node-1", "kube-system", "node-1", "Node", "node-1", &[("kube-apiserver", "registry.k8s.io/kube-apiserver:v1.31.4")], "running", 1, 92 * D, "Burstable", "kube-apiserver");
        p!("kube-controller-manager-node-1", "kube-system", "node-1", "Node", "node-1", &[("kube-controller-manager", "registry.k8s.io/kube-controller-manager:v1.31.4")], "running", 3, 92 * D, "Burstable", "kube-controller-manager");
        p!("kube-scheduler-node-1", "kube-system", "node-1", "Node", "node-1", &[("kube-scheduler", "registry.k8s.io/kube-scheduler:v1.31.4")], "running", 3, 92 * D, "Burstable", "kube-scheduler");
        for (i, n) in ["node-1", "node-2", "node-3"].iter().enumerate() {
            let pn = ["kube-proxy-6tq2d", "kube-proxy-jx8wv", "kube-proxy-r4mzc"][i];
            p!(pn, "kube-system", n, "DaemonSet", "kube-proxy", &[("kube-proxy", "registry.k8s.io/kube-proxy:v1.31.4")], "running", 0, 92 * D, "BestEffort", "kube-proxy");
            let cn = ["flannel-2bxkq", "flannel-8lwhn", "flannel-vt5cs"][i];
            p!(cn, "kube-system", n, "DaemonSet", "flannel", &[("kube-flannel", "docker.io/flannel/flannel:v0.26.1")], "running", 0, 92 * D, "Burstable", "flannel");
            let ne = ["node-exporter-5ksq7", "node-exporter-cw9xd", "node-exporter-mm2hz"][i];
            p!(ne, "monitoring", n, "DaemonSet", "node-exporter", &[("node-exporter", "quay.io/prometheus/node-exporter:v1.8.2")], "running", 0, 30 * D, "Burstable", "node-exporter");
        }
        p!("metrics-server-6f4c6b5d8-w2mjf", "kube-system", "node-2", "ReplicaSet", "metrics-server-6f4c6b5d8", &[("metrics-server", "registry.k8s.io/metrics-server/metrics-server:v0.7.2")], "running", 0, 60 * D, "Burstable", "metrics-server");
        p!("prometheus-0", "monitoring", "node-3", "StatefulSet", "prometheus", &[("prometheus", "quay.io/prometheus/prometheus:v2.55.1"), ("config-reloader", "quay.io/prometheus-operator/prometheus-config-reloader:v0.78.2")], "running", 0, 30 * D, "Burstable", "prometheus");
        p!("grafana-7b9d6f5c8-vn4kc", "monitoring", "node-2", "ReplicaSet", "grafana-7b9d6f5c8", &[("grafana", "grafana/grafana:11.3.1")], "running", 0, 30 * D, "Burstable", "grafana");
        p!("ingress-nginx-controller-5c8d7b6f9-qj6rt", "ingress-nginx", "node-2", "ReplicaSet", "ingress-nginx-controller-5c8d7b6f9", &[("controller", "registry.k8s.io/ingress-nginx/controller:v1.11.3")], "running", 0, 45 * D, "Burstable", "ingress-nginx");
        o.insert(Kind::Pod, pods);

        o.insert(
            Kind::Deployment,
            vec![
                deployment("web", "shop", 3, 3, "registry.example.com/shop/web:2.4.1", 40 * D),
                deployment("api", "shop", 2, 2, "registry.example.com/shop/api:1.18.0", 40 * D),
                deployment("worker", "shop", 2, 1, "registry.example.com/shop/worker:1.18.0", 40 * D),
                deployment("thumbnailer", "shop", 1, 0, "registry.example.com/shop/thumbnailer:0.3.0", 2 * 60),
                deployment("coredns", "kube-system", 2, 2, "registry.k8s.io/coredns/coredns:v1.11.3", 92 * D),
                deployment("metrics-server", "kube-system", 1, 1, "registry.k8s.io/metrics-server/metrics-server:v0.7.2", 60 * D),
                deployment("grafana", "monitoring", 1, 1, "grafana/grafana:11.3.1", 30 * D),
                deployment("ingress-nginx-controller", "ingress-nginx", 1, 1, "registry.k8s.io/ingress-nginx/controller:v1.11.3", 45 * D),
            ],
        );
        o.insert(
            Kind::ReplicaSet,
            vec![
                replicaset("web", "shop", "7d9f8c6b5d", 3, 3, 3 * D),
                replicaset("web", "shop", "6c8b9f7d4", 0, 0, 20 * D),
                replicaset("api", "shop", "5b8c7d9f4", 2, 2, 26 * H),
                replicaset("worker", "shop", "6f7c9d8b7", 2, 1, 5 * H),
                replicaset("thumbnailer", "shop", "84c5d6f9b", 1, 0, 2 * 60),
                replicaset("coredns", "kube-system", "7c65d6cfc9", 2, 2, 92 * D),
                replicaset("metrics-server", "kube-system", "6f4c6b5d8", 1, 1, 60 * D),
                replicaset("grafana", "monitoring", "7b9d6f5c8", 1, 1, 30 * D),
                replicaset("ingress-nginx-controller", "ingress-nginx", "5c8d7b6f9", 1, 1, 45 * D),
            ],
        );
        let ds = |name: &str, ns: &str, age: i64| {
            simple("apps/v1", "DaemonSet", name, Some(ns), age, json!({
                "spec": {"selector": {"matchLabels": {"app": name}}, "template": {"spec": {"nodeSelector": {"kubernetes.io/os": "linux"}}}},
                "status": {"desiredNumberScheduled": 3, "numberReady": 3, "currentNumberScheduled": 3}
            }))
        };
        o.insert(
            Kind::DaemonSet,
            vec![ds("kube-proxy", "kube-system", 92 * D), ds("flannel", "kube-system", 92 * D), ds("node-exporter", "monitoring", 30 * D)],
        );
        let sts = |name: &str, ns: &str, age: i64| {
            simple("apps/v1", "StatefulSet", name, Some(ns), age, json!({
                "spec": {"replicas": 1, "serviceName": name, "selector": {"matchLabels": {"app": name}}},
                "status": {"replicas": 1, "readyReplicas": 1}
            }))
        };
        o.insert(
            Kind::StatefulSet,
            vec![sts("redis", "shop", 12 * D), sts("postgres", "shop", 12 * D), sts("prometheus", "monitoring", 30 * D)],
        );
        o.insert(
            Kind::Job,
            vec![simple("batch/v1", "Job", "nightly-report-28914720", Some("shop"), 9 * H, json!({
                "spec": {"completions": 1, "parallelism": 1, "backoffLimit": 6},
                "status": {"succeeded": 1, "conditions": [{"type": "Complete", "status": "True"}]}
            }))],
        );
        o.insert(
            Kind::CronJob,
            vec![
                simple("batch/v1", "CronJob", "nightly-report", Some("shop"), 40 * D, json!({
                    "spec": {"schedule": "0 2 * * *", "suspend": false, "concurrencyPolicy": "Forbid"},
                    "status": {"lastScheduleTime": format_time(now_secs() - 9 * H)}
                })),
                simple("batch/v1", "CronJob", "db-backup", Some("shop"), 40 * D, json!({
                    "spec": {"schedule": "30 3 * * 0", "suspend": true},
                    "status": {"lastScheduleTime": format_time(now_secs() - 4 * D)}
                })),
            ],
        );
        o.insert(
            Kind::ConfigMap,
            vec![
                simple("v1", "ConfigMap", "web-config", Some("shop"), 40 * D, json!({"data": {"APP_ENV": "production", "FEATURE_FLAGS": "checkout-v2,dark-mode", "nginx.conf": "server {\n  listen 8080;\n  location / { proxy_pass http://api:8080; }\n}\n"}})),
                simple("v1", "ConfigMap", "api-config", Some("shop"), 40 * D, json!({"data": {"DB_HOST": "postgres", "REDIS_URL": "redis://redis:6379", "WORKERS": "4"}})),
                simple("v1", "ConfigMap", "kube-root-ca.crt", Some("shop"), 92 * D, json!({"data": {"ca.crt": "-----BEGIN CERTIFICATE-----\nMIIB...sample...\n-----END CERTIFICATE-----\n"}})),
                simple("v1", "ConfigMap", "coredns", Some("kube-system"), 92 * D, json!({"data": {"Corefile": ".:53 {\n    errors\n    health\n    kubernetes cluster.local in-addr.arpa ip6.arpa\n    forward . /etc/resolv.conf\n    cache 30\n}\n"}})),
                simple("v1", "ConfigMap", "prometheus-config", Some("monitoring"), 30 * D, json!({"data": {"prometheus.yml": "global:\n  scrape_interval: 30s\n"}})),
            ],
        );
        o.insert(
            Kind::Secret,
            vec![
                simple("v1", "Secret", "postgres-credentials", Some("shop"), 40 * D, json!({"type": "Opaque", "data": {"username": "c2hvcA==", "password": "c2FtcGxlLXBhc3N3b3Jk"}})),
                simple("v1", "Secret", "registry-pull", Some("shop"), 40 * D, json!({"type": "kubernetes.io/dockerconfigjson", "data": {".dockerconfigjson": "e30="}})),
                simple("v1", "Secret", "shop-tls", Some("shop"), 20 * D, json!({"type": "kubernetes.io/tls", "data": {"tls.crt": "LS0t", "tls.key": "LS0t"}})),
                simple("v1", "Secret", "grafana-admin", Some("monitoring"), 30 * D, json!({"type": "Opaque", "data": {"admin-user": "YWRtaW4=", "admin-password": "Y2hhbmdlbWU="}})),
            ],
        );
        o.insert(
            Kind::ResourceQuota,
            vec![simple("v1", "ResourceQuota", "shop-quota", Some("shop"), 40 * D, json!({"spec": {"hard": {"pods": "50", "requests.cpu": "8", "requests.memory": "16Gi"}}, "status": {"used": {"pods": "11", "requests.cpu": "1100m", "requests.memory": "1408Mi"}}}))],
        );
        o.insert(
            Kind::LimitRange,
            vec![simple("v1", "LimitRange", "shop-defaults", Some("shop"), 40 * D, json!({"spec": {"limits": [{"type": "Container", "default": {"memory": "256Mi"}, "defaultRequest": {"cpu": "100m", "memory": "128Mi"}}]}}))],
        );
        o.insert(
            Kind::Hpa,
            vec![simple("autoscaling/v2", "HorizontalPodAutoscaler", "web", Some("shop"), 40 * D, json!({"spec": {"minReplicas": 2, "maxReplicas": 10, "scaleTargetRef": {"kind": "Deployment", "name": "web"}, "metrics": [{"type": "Resource", "resource": {"name": "cpu", "target": {"type": "Utilization", "averageUtilization": 70}}}]}, "status": {"currentReplicas": 3, "desiredReplicas": 3}}))],
        );
        o.insert(
            Kind::Pdb,
            vec![simple("policy/v1", "PodDisruptionBudget", "web", Some("shop"), 40 * D, json!({"spec": {"minAvailable": 2}, "status": {"currentHealthy": 3, "desiredHealthy": 2}}))],
        );
        let ports = |p: i64| json!([{"name": "http", "port": p, "targetPort": 8080, "protocol": "TCP"}]);
        o.insert(
            Kind::Service,
            vec![
                service("kubernetes", "default", "ClusterIP", "10.43.0.1", json!([{"name": "https", "port": 443, "targetPort": 6443, "protocol": "TCP"}]), json!({}), 92 * D),
                service("web", "shop", "ClusterIP", "10.43.12.80", ports(80), json!({"app": "web"}), 40 * D),
                service("api", "shop", "ClusterIP", "10.43.55.21", ports(8080), json!({"app": "api"}), 40 * D),
                service("redis", "shop", "ClusterIP", "10.43.71.9", json!([{"port": 6379, "protocol": "TCP"}]), json!({"app": "redis"}), 12 * D),
                service("postgres", "shop", "ClusterIP", "10.43.88.140", json!([{"port": 5432, "protocol": "TCP"}]), json!({"app": "postgres"}), 12 * D),
                service("kube-dns", "kube-system", "ClusterIP", "10.43.0.10", json!([{"name": "dns", "port": 53, "protocol": "UDP"}, {"name": "dns-tcp", "port": 53, "protocol": "TCP"}]), json!({"k8s-app": "kube-dns"}), 92 * D),
                service("grafana", "monitoring", "NodePort", "10.43.140.7", json!([{"port": 3000, "nodePort": 30300, "protocol": "TCP"}]), json!({"app": "grafana"}), 30 * D),
                service("ingress-nginx-controller", "ingress-nginx", "LoadBalancer", "10.43.200.15", json!([{"name": "http", "port": 80, "nodePort": 31080, "protocol": "TCP"}, {"name": "https", "port": 443, "nodePort": 31443, "protocol": "TCP"}]), json!({"app": "ingress-nginx"}), 45 * D),
            ],
        );
        o.insert(
            Kind::Endpoints,
            vec![
                simple("v1", "Endpoints", "web", Some("shop"), 40 * D, json!({"subsets": [{"addresses": [{"ip": "10.42.1.14"}, {"ip": "10.42.2.33"}, {"ip": "10.42.1.52"}], "ports": [{"port": 8080}]}]})),
                simple("v1", "Endpoints", "api", Some("shop"), 40 * D, json!({"subsets": [{"addresses": [{"ip": "10.42.2.41"}, {"ip": "10.42.1.77"}], "ports": [{"port": 8080}]}]})),
                simple("v1", "Endpoints", "kubernetes", Some("default"), 92 * D, json!({"subsets": [{"addresses": [{"ip": "192.168.1.16"}], "ports": [{"port": 6443}]}]})),
            ],
        );
        o.insert(
            Kind::Ingress,
            vec![simple("networking.k8s.io/v1", "Ingress", "shop", Some("shop"), 40 * D, json!({
                "spec": {"ingressClassName": "nginx", "tls": [{"hosts": ["shop.example.com"], "secretName": "shop-tls"}], "rules": [{"host": "shop.example.com", "http": {"paths": [
                    {"path": "/", "pathType": "Prefix", "backend": {"service": {"name": "web", "port": {"number": 80}}}},
                    {"path": "/api", "pathType": "Prefix", "backend": {"service": {"name": "api", "port": {"number": 8080}}}}
                ]}}]},
                "status": {"loadBalancer": {"ingress": [{"ip": "192.168.1.240"}]}}
            }))],
        );
        o.insert(
            Kind::NetworkPolicy,
            vec![simple("networking.k8s.io/v1", "NetworkPolicy", "postgres-allow-api", Some("shop"), 12 * D, json!({"spec": {"podSelector": {"matchLabels": {"app": "postgres"}}, "policyTypes": ["Ingress"]}}))],
        );
        let pvc = |name: &str, ns: &str, size: &str, vol: &str, age: i64| {
            simple("v1", "PersistentVolumeClaim", name, Some(ns), age, json!({
                "spec": {"storageClassName": "local-path", "accessModes": ["ReadWriteOnce"], "resources": {"requests": {"storage": size}}, "volumeName": vol},
                "status": {"phase": "Bound", "capacity": {"storage": size}}
            }))
        };
        o.insert(
            Kind::Pvc,
            vec![
                pvc("data-postgres-0", "shop", "20Gi", "pvc-3f1c9a2e", 12 * D),
                pvc("data-redis-0", "shop", "2Gi", "pvc-7b2d4e11", 12 * D),
                pvc("data-prometheus-0", "monitoring", "50Gi", "pvc-a90c5d73", 30 * D),
            ],
        );
        let pv = |name: &str, size: &str, claim: (&str, &str), age: i64| {
            simple("v1", "PersistentVolume", name, None, age, json!({
                "spec": {"storageClassName": "local-path", "capacity": {"storage": size}, "accessModes": ["ReadWriteOnce"], "persistentVolumeReclaimPolicy": "Delete", "claimRef": {"namespace": claim.0, "name": claim.1}},
                "status": {"phase": "Bound"}
            }))
        };
        o.insert(
            Kind::Pv,
            vec![
                pv("pvc-3f1c9a2e", "20Gi", ("shop", "data-postgres-0"), 12 * D),
                pv("pvc-7b2d4e11", "2Gi", ("shop", "data-redis-0"), 12 * D),
                pv("pvc-a90c5d73", "50Gi", ("monitoring", "data-prometheus-0"), 30 * D),
            ],
        );
        let mut sc = simple("storage.k8s.io/v1", "StorageClass", "local-path", None, 92 * D, json!({"provisioner": "rancher.io/local-path", "reclaimPolicy": "Delete", "volumeBindingMode": "WaitForFirstConsumer"}));
        sc["metadata"]["annotations"] = json!({"storageclass.kubernetes.io/is-default-class": "true"});
        o.insert(Kind::StorageClass, vec![sc]);
        o.insert(
            Kind::ServiceAccount,
            ns_list.iter().map(|n| simple("v1", "ServiceAccount", "default", Some(n), 92 * D, json!({}))).collect(),
        );
        o.insert(
            Kind::ClusterRole,
            ["admin", "cluster-admin", "edit", "view", "system:node", "system:kube-scheduler"]
                .iter()
                .map(|n| simple("rbac.authorization.k8s.io/v1", "ClusterRole", n, None, 92 * D, json!({"rules": [{"apiGroups": ["*"], "resources": ["*"], "verbs": ["*"]}]})))
                .collect(),
        );
        o.insert(
            Kind::Role,
            vec![simple("rbac.authorization.k8s.io/v1", "Role", "worker-leader-election", Some("shop"), 40 * D, json!({"rules": [{"apiGroups": ["coordination.k8s.io"], "resources": ["leases"], "verbs": ["get", "update"]}]}))],
        );
        o.insert(
            Kind::ClusterRoleBinding,
            vec![simple("rbac.authorization.k8s.io/v1", "ClusterRoleBinding", "cluster-admin", None, 92 * D, json!({"roleRef": {"kind": "ClusterRole", "name": "cluster-admin"}, "subjects": [{"kind": "Group", "name": "system:masters"}]}))],
        );
        o.insert(
            Kind::RoleBinding,
            vec![simple("rbac.authorization.k8s.io/v1", "RoleBinding", "worker-leader-election", Some("shop"), 40 * D, json!({"roleRef": {"kind": "Role", "name": "worker-leader-election"}, "subjects": [{"kind": "ServiceAccount", "name": "default", "namespace": "shop"}]}))],
        );
        o.insert(
            Kind::Crd,
            vec![
                simple("apiextensions.k8s.io/v1", "CustomResourceDefinition", "certificates.cert-manager.io", None, 50 * D, json!({"spec": {"group": "cert-manager.io", "scope": "Namespaced", "names": {"kind": "Certificate", "plural": "certificates"}, "versions": [{"name": "v1", "served": true, "storage": true, "additionalPrinterColumns": [
                    {"name": "Ready", "type": "string", "jsonPath": ".status.conditions[?(@.type==\"Ready\")].status"},
                    {"name": "Secret", "type": "string", "jsonPath": ".spec.secretName"},
                    {"name": "Issuer", "type": "string", "jsonPath": ".spec.issuerRef.name", "priority": 1},
                    {"name": "Status", "type": "string", "jsonPath": ".status.conditions[?(@.type==\"Ready\")].message", "priority": 1},
                    {"name": "Age", "type": "date", "jsonPath": ".metadata.creationTimestamp"}]}]}})),
                simple("apiextensions.k8s.io/v1", "CustomResourceDefinition", "clusterissuers.cert-manager.io", None, 50 * D, json!({"spec": {"group": "cert-manager.io", "scope": "Cluster", "names": {"kind": "ClusterIssuer", "plural": "clusterissuers"}, "versions": [{"name": "v1", "served": true, "storage": true, "additionalPrinterColumns": [
                    {"name": "Ready", "type": "string", "jsonPath": ".status.conditions[?(@.type==\"Ready\")].status"}]}]}})),
                simple("apiextensions.k8s.io/v1", "CustomResourceDefinition", "servicemonitors.monitoring.coreos.com", None, 30 * D, json!({"spec": {"group": "monitoring.coreos.com", "scope": "Namespaced", "names": {"kind": "ServiceMonitor", "plural": "servicemonitors"}, "versions": [{"name": "v1", "served": true, "storage": true}]}})),
                simple("apiextensions.k8s.io/v1", "CustomResourceDefinition", "prometheusrules.monitoring.coreos.com", None, 30 * D, json!({"spec": {"group": "monitoring.coreos.com", "scope": "Namespaced", "names": {"kind": "PrometheusRule", "plural": "prometheusrules"}, "versions": [{"name": "v1", "served": true, "storage": true}]}})),
            ],
        );
        o.insert(
            Kind::Event,
            vec![
                event("shop", "Warning", "BackOff", "Pod", "worker-6f7c9d8b7-zt2wx", "Back-off restarting failed container worker in pod worker-6f7c9d8b7-zt2wx_shop", "kubelet", 63, 40),
                event("shop", "Warning", "FailedScheduling", "Pod", "thumbnailer-84c5d6f9b-p7wvz", "0/3 nodes are available: 3 Insufficient memory. preemption: 0/3 nodes are available.", "default-scheduler", 4, 75),
                event("shop", "Normal", "Pulled", "Pod", "worker-6f7c9d8b7-zt2wx", "Container image \"registry.example.com/shop/worker:1.18.0\" already present on machine", "kubelet", 14, 300),
                event("shop", "Normal", "ScalingReplicaSet", "Deployment", "thumbnailer", "Scaled up replica set thumbnailer-84c5d6f9b to 1", "deployment-controller", 1, 120),
                event("shop", "Normal", "SuccessfulCreate", "Job", "nightly-report-28914720", "Created pod: nightly-report-28914720-6vbqc", "job-controller", 1, 9 * H),
                event("shop", "Normal", "Completed", "Job", "nightly-report-28914720", "Job completed", "job-controller", 1, 9 * H - 200),
                event("monitoring", "Normal", "Started", "Pod", "prometheus-0", "Started container prometheus", "kubelet", 1, 2 * D),
            ],
        );
        let mut custom: HashMap<String, Vec<Value>> = HashMap::new();
        let cert = |name: &str, ns: &str, secret: &str, ready: bool, age: i64| {
            simple("cert-manager.io/v1", "Certificate", name, Some(ns), age, json!({
                "spec": {"secretName": secret, "dnsNames": [format!("{name}.example.com")], "issuerRef": {"kind": "ClusterIssuer", "name": "letsencrypt"}},
                "status": {"conditions": [{"type": "Ready", "status": if ready { "True" } else { "False" },
                    "message": if ready { "Certificate is up to date and has not expired" } else { "Issuing certificate as Secret does not exist" }}]}
            }))
        };
        custom.insert("certificates.cert-manager.io".into(), vec![
            cert("shop", "shop", "shop-tls", true, 20 * D),
            cert("grafana", "monitoring", "grafana-tls", true, 30 * D),
            cert("api-internal", "shop", "api-internal-tls", false, 2 * H),
        ]);
        custom.insert("clusterissuers.cert-manager.io".into(), vec![
            simple("cert-manager.io/v1", "ClusterIssuer", "letsencrypt", None, 50 * D, json!({"spec": {"acme": {"server": "https://acme-v02.api.letsencrypt.org/directory"}}, "status": {"conditions": [{"type": "Ready", "status": "True"}]}})),
        ]);
        custom.insert("servicemonitors.monitoring.coreos.com".into(), vec![
            simple("monitoring.coreos.com/v1", "ServiceMonitor", "api", Some("shop"), 30 * D, json!({"spec": {"selector": {"matchLabels": {"app": "api"}}, "endpoints": [{"port": "http", "interval": "30s"}]}})),
            simple("monitoring.coreos.com/v1", "ServiceMonitor", "node-exporter", Some("monitoring"), 30 * D, json!({"spec": {"selector": {"matchLabels": {"app": "node-exporter"}}, "endpoints": [{"port": "metrics"}]}})),
        ]);
        custom.insert("prometheusrules.monitoring.coreos.com".into(), vec![
            simple("monitoring.coreos.com/v1", "PrometheusRule", "shop-alerts", Some("monitoring"), 30 * D, json!({"spec": {"groups": [{"name": "shop", "rules": [{"alert": "HighErrorRate", "expr": "rate(http_errors_total[5m]) > 0.05"}]}]}})),
        ]);
        DemoCluster { objects: o, custom, counter: 0 }
    }

    pub fn list(&self, kind: Kind) -> Vec<Value> {
        if let Kind::Custom(i) = kind {
            return self.custom.get(crate::crd::info(i).key).cloned().unwrap_or_default();
        }
        self.objects.get(&kind).cloned().unwrap_or_default()
    }

    fn find_mut(&mut self, kind: Kind, ns: &str, name: &str) -> Option<&mut Value> {
        self.objects
            .get_mut(&kind)?
            .iter_mut()
            .find(|v| s(v, "metadata/name") == name && s(v, "metadata/namespace") == ns)
    }

    pub fn delete(&mut self, kind: Kind, ns: &str, name: &str) -> Result<String, String> {
        let list = self.objects.get_mut(&kind).ok_or("not found")?;
        let before = list.len();
        list.retain(|v| !(s(v, "metadata/name") == name && s(v, "metadata/namespace") == ns));
        if list.len() == before {
            return Err(format!("{} \"{}\" not found", kind.singular(), name));
        }
        self.push_event(ns, "Normal", "Killing", kind.singular(), name, &format!("Deleted {} {}", kind.singular(), name));
        Ok(format!("{} {} deleted", kind.singular(), name))
    }

    pub fn scale(&mut self, kind: Kind, ns: &str, name: &str, replicas: i64) -> Result<String, String> {
        let v = self.find_mut(kind, ns, name).ok_or("not found")?;
        v["spec"]["replicas"] = json!(replicas);
        v["status"]["replicas"] = json!(replicas);
        v["status"]["readyReplicas"] = json!(replicas);
        v["status"]["availableReplicas"] = json!(replicas);
        self.push_event(ns, "Normal", "ScalingReplicaSet", kind.singular(), name, &format!("Scaled {name} to {replicas}"));
        Ok(format!("{name} scaled to {replicas}"))
    }

    pub fn restart(&mut self, kind: Kind, ns: &str, name: &str) -> Result<String, String> {
        let v = self.find_mut(kind, ns, name).ok_or("not found")?;
        v["spec"]["template"]["metadata"]["annotations"] =
            json!({"kubectl.kubernetes.io/restartedAt": format_time(now_secs())});
        self.push_event(ns, "Normal", "RolloutRestart", kind.singular(), name, &format!("Restarted {name}"));
        Ok(format!("{name} restarted"))
    }

    pub fn replace(&mut self, kind: Kind, ns: &str, name: &str, text: &str) -> Result<String, String> {
        let new = util::from_yaml(text)?;
        let v = self.find_mut(kind, ns, name).ok_or("not found")?;
        *v = new;
        Ok(format!("{} {} updated", kind.singular(), name))
    }

    fn push_event(&mut self, ns: &str, ty: &str, reason: &str, kind: &str, obj: &str, msg: &str) {
        self.counter += 1;
        let e = event(ns, ty, reason, kind, obj, msg, "cluster-desk", 1, 0);
        self.objects.entry(Kind::Event).or_default().insert(0, e);
    }

    /// Sample volume usage for the sample claims.
    pub fn volume_stats(&self) -> crate::backend::VolumeUsage {
        let gib = 1024.0 * 1024.0 * 1024.0;
        [
            ("shop", "data-postgres-0", 13.7, 20.0),
            ("shop", "data-redis-0", 0.31, 2.0),
            ("monitoring", "data-prometheus-0", 46.2, 50.0),
        ]
        .iter()
        .map(|(ns, n, used, cap)| ((ns.to_string(), n.to_string()), (used * gib, cap * gib)))
        .collect()
    }

    /// Sample live usage, gently varying over time like a real metrics server.
    pub fn node_metrics(&self) -> crate::backend::NodeUsage {
        let wobble = ((now_secs() % 60) as f64 / 60.0 * std::f64::consts::TAU).sin();
        [("node-1", 1.35, 5.8), ("node-2", 3.1, 17.4), ("node-3", 2.2, 11.9)]
            .iter()
            .map(|(n, cpu, gib)| {
                (n.to_string(), (cpu + 0.08 * wobble, (gib + 0.2 * wobble) * 1024.0 * 1024.0 * 1024.0))
            })
            .collect()
    }

    pub fn logs(&self, ns: &str, pod: &str, container: &str) -> String {
        let crash = pod.starts_with("worker-6f7c9d8b7-zt2wx");
        let now = now_secs();
        let mut out = String::new();
        let lines: &[&str] = if crash {
            &[
                "level=info msg=\"starting worker\" version=1.18.0",
                "level=info msg=\"connecting to queue\" url=redis://redis:6379",
                "level=info msg=\"connected\"",
                "level=error msg=\"failed to load job definitions\" err=\"config key WORKER_QUEUES missing\"",
                "level=fatal msg=\"exiting\" code=1",
            ]
        } else if container == "envoy" {
            &[
                "[info][main] initializing epoch 0 (base id=0, hot restart version=11.104)",
                "[info][main] statically linked extensions: 212",
                "[info][upstream] cds: add 1 cluster(s), remove 0 cluster(s)",
                "[info][main] all clusters initialized. initializing init manager",
                "[info][config] all dependencies initialized. starting workers",
            ]
        } else {
            &[
                "\x1b[32mINFO\x1b[0m  \x1b[2mserver\x1b[0m starting on \x1b[36m:8080\x1b[0m",
                "\x1b[32mINFO\x1b[0m  \x1b[2mconfig\x1b[0m loaded \x1b[1menv\x1b[0m=production",
                "\x1b[32mINFO\x1b[0m  \x1b[2mhttp\x1b[0m GET /healthz \x1b[32m200\x1b[0m 0.4ms",
                "\x1b[32mINFO\x1b[0m  \x1b[2mhttp\x1b[0m GET /api/products?page=1 \x1b[32m200\x1b[0m 12.8ms",
                "\x1b[32mINFO\x1b[0m  \x1b[2mhttp\x1b[0m POST /api/cart/items \x1b[32m201\x1b[0m 18.3ms",
                "\x1b[33mWARN\x1b[0m  \x1b[2mdb\x1b[0m slow query \x1b[33mduration=812ms\x1b[0m table=orders",
                "\x1b[32mINFO\x1b[0m  \x1b[2mhttp\x1b[0m GET /api/cart \x1b[32m200\x1b[0m 5.1ms",
                "\x1b[1;31mERROR\x1b[0m \x1b[2mhttp\x1b[0m GET /api/products/9999 \x1b[31m404\x1b[0m not found",
                "\x1b[38;5;208mDEBUG\x1b[0m \x1b[2mcache\x1b[0m hit ratio \x1b[38;2;120;180;255m0.93\x1b[0m",
                "plain line without colours: GET /healthz 200 0.3ms",
            ]
        };
        let n = if crash { lines.len() } else { 60 };
        for k in 0..n {
            let ts = format_time(now - ((n - k) as i64) * 17);
            out.push_str(&ts);
            out.push(' ');
            out.push_str(lines[k % lines.len()]);
            out.push('\n');
        }
        let _ = ns;
        out
    }

    pub fn exec(&self, pod: &str, container: &str, cmd: &str) -> String {
        let c = cmd.trim();
        match c.split_whitespace().next().unwrap_or("") {
            "" => String::new(),
            "hostname" => format!("{pod}\n"),
            "whoami" => "app\n".into(),
            "pwd" => "/app\n".into(),
            "ls" => "\x1b[1;34mbin\x1b[0m  \x1b[1;34mconfig\x1b[0m  \x1b[1;34mlib\x1b[0m  \x1b[1;34mstatic\x1b[0m  \x1b[1;32mserver\x1b[0m  README.md  \x1b[1;36mcurrent -> releases/1.18.0\x1b[0m\n".into(),
            "colors" => {
                let mut s = String::new();
                for i in 0..16u8 {
                    s.push_str(&format!("\x1b[48;5;{i}m  {i:>3} \x1b[0m"));
                    if i % 8 == 7 {
                        s.push('\n');
                    }
                }
                for i in 16..232u8 {
                    s.push_str(&format!("\x1b[48;5;{i}m  \x1b[0m"));
                    if (i - 16) % 36 == 35 {
                        s.push('\n');
                    }
                }
                s.push_str("\x1b[1mbold\x1b[0m \x1b[4munderline\x1b[0m \x1b[7minverse\x1b[0m \x1b[38;2;255;140;0mtruecolor\x1b[0m\n");
                s
            }
            "emoji" => "🚀 deploy  ✅ ready  ⚠️ warning  🐳 container  ☸️ kubernetes  日本語  ┌──┬──┐ │é│ü│ └──┴──┘\n".into(),
            "top" => format!(
                "\x1b[1mtop - {} up 5:02,  load average: 0.42, 0.38, 0.35\x1b[0m\nTasks:   3 total,   1 running,   2 sleeping\n\x1b[7m  PID USER      %CPU %MEM COMMAND                \x1b[0m\n    1 app        3.1  2.4 /app/bin/server\n   42 app        0.0  0.1 sh\n   57 app        0.0  0.1 top\n",
                format_time(now_secs())
            ),
            "date" => format!("{}\n", format_time(now_secs())),
            "env" => format!("HOSTNAME={pod}\nLOG_LEVEL=info\nKUBERNETES_SERVICE_HOST=10.43.0.1\nCONTAINER={container}\n"),
            "uname" => "Linux\n".into(),
            "ps" => "PID   USER     COMMAND\n    1 app      /app/bin/server\n   42 app      sh\n".into(),
            "echo" => format!("{}\n", c.strip_prefix("echo").unwrap_or("").trim()),
            "cat" => "(sample cluster: file contents are not available)\n".into(),
            other => format!("sh: {other}: not found (sample cluster supports a few basic commands)\n"),
        }
    }
}
