//! Right-hand details drawer for the selected resource.

use eframe::egui;
use crate::app::{App, Dialog, View};
use crate::model::{Kind, Row, container_tone, node_roles, pod_belongs_to, pod_status};
use crate::theme;
use crate::util::{self, arr, get, i, map_pairs, s};
use egui::{Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, pos2, vec2};
use serde_json::Value;

enum Act {
    Close,
    Logs(Option<String>),
    Shell(Option<String>),
    Edit,
    Delete,
    Scale,
    Restart,
    Open(Kind, String),
    Reveal(String),
    Browse(String),
}

impl App {
    pub fn details(&mut self, ui: &mut egui::Ui) {
        let Some((kind, uid)) = self.selected.clone() else { return };
        let rows = self.rows(kind);
        let Some(row) = rows.iter().find(|r| r.uid == uid).cloned() else {
            // The object went away (deleted) – close the drawer.
            if self.store.get(&kind).map(|d| d.loaded.is_some()).unwrap_or(false) {
                self.selected = None;
            }
            return;
        };
        let mut acts: Vec<Act> = vec![];

        // Title bar
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), theme::DRAWER_TITLE_HEIGHT), Sense::hover());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, theme::BLUE);
        let mut icons: Vec<(&str, &str, Act)> = vec![];
        if kind == Kind::Pod {
            icons.push(("🗎", "Pod Logs", Act::Logs(None)));
            icons.push((">_", "Pod Shell", Act::Shell(None)));
        }
        if kind == Kind::Ingress {
            if let Some(u) = util::ingress_urls(&row.obj).into_iter().next() {
                icons.push(("🌐", "Open in browser", Act::Browse(u)));
            }
        }
        if kind.scalable() {
            icons.push(("⇕", "Scale", Act::Scale));
        }
        if kind.restartable() {
            icons.push(("⟲", "Restart", Act::Restart));
        }
        icons.push(("✎", "Edit", Act::Edit));
        icons.push(("🗑", "Delete", Act::Delete));
        icons.push(("✕", "Close", Act::Close));
        let mut x = bar.right() - 8.0;
        for (glyph, tip, act) in icons.into_iter().rev() {
            let r = egui::Rect::from_center_size(pos2(x - 14.0, bar.center().y), vec2(28.0, 28.0));
            x -= 32.0;
            let resp = ui.interact(r, ui.scope_id().with(("dicon", tip)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(4), Color32::from_white_alpha(40));
            }
            ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(15.0), theme::TEXT_HI);
            if resp.on_hover_text(tip).clicked() {
                acts.push(act);
            }
        }
        let title = format!("{}: {}", kind.singular(), row.name);
        let mut job = egui::text::LayoutJob::single_section(
            title.clone(),
            egui::TextFormat::simple(FontId::proportional(theme::FONT_SIZE + 1.0), theme::TEXT_HI),
        );
        job.wrap = egui::text::TextWrapping { max_width: (x - bar.left() - 20.0).max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
        let g = ui.painter().layout_job(job);
        // Bold title (font-weight: bold): drawn twice, offset by half a pixel.
        ui.painter().galley(pos2(bar.left() + 20.5, bar.center().y - g.size().y / 2.0), g.clone(), theme::TEXT_HI);
        ui.painter().galley(pos2(bar.left() + 20.0, bar.center().y - g.size().y / 2.0), g, theme::TEXT_HI);

        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.details_body(ui, kind, &row, &mut acts);
            });
        });

        for a in acts {
            match a {
                Act::Close => self.selected = None,
                Act::Logs(c) => self.open_logs(&row.obj, c),
                Act::Shell(c) => self.open_shell(&row.obj, c),
                Act::Edit => self.open_edit(kind, &row.ns, &row.name),
                Act::Delete => self.dialog = Some(Dialog::Delete { kind, ns: row.ns.clone(), name: row.name.clone() }),
                Act::Scale => {
                    let cur = i(&row.obj, "spec/replicas");
                    self.dialog = Some(Dialog::Scale { kind, ns: row.ns.clone(), name: row.name.clone(), current: cur, desired: cur })
                }
                Act::Restart => self.dialog = Some(Dialog::Restart { kind, ns: row.ns.clone(), name: row.name.clone() }),
                Act::Open(k, uid) => {
                    if let View::List(_) = self.view {
                        if k != kind && self.view != View::List(k) {
                            // keep current list; just switch drawer
                        }
                    }
                    self.selected = Some((k, uid));
                }
                Act::Browse(u) => {
                    if let Err(e) = util::open_url(&u) {
                        self.toast(e, true);
                    }
                }
                Act::Reveal(key) => {
                    if !self.revealed.remove(&key) {
                        self.revealed.insert(key);
                    }
                }
            }
        }
    }

    fn details_body(&self, ui: &mut egui::Ui, kind: Kind, row: &Row, acts: &mut Vec<Act>) {
        let v = &row.obj;
        let now = util::now_secs();
        kv(ui, "Created", &format!("{} ago  ({})", util::age(row.created, now), s(v, "metadata/creationTimestamp")));
        kv(ui, "Name", &row.name);
        if !row.ns.is_empty() {
            kv(ui, "Namespace", &row.ns);
        }
        let labels = map_pairs(v, "metadata/labels");
        if !labels.is_empty() {
            kv_chips(ui, "Labels", labels.iter().map(|(k, v)| if v.is_empty() { k.clone() } else { format!("{k}={v}") }).collect());
        }
        let ann = map_pairs(v, "metadata/annotations");
        if !ann.is_empty() {
            kv_chips(
                ui,
                "Annotations",
                ann.iter()
                    .map(|(k, v)| {
                        let mut t = format!("{k}={v}");
                        if t.chars().count() > 80 {
                            t = t.chars().take(80).collect::<String>() + "…";
                        }
                        t
                    })
                    .collect(),
            );
        }
        if let Some(o) = arr(v, "metadata/ownerReferences").first() {
            kv(ui, "Controlled By", &format!("{} {}", s(o, "kind"), s(o, "name")));
        }

        match kind {
            Kind::Pod => self.pod_details(ui, v, acts),
            Kind::Deployment | Kind::StatefulSet | Kind::DaemonSet | Kind::ReplicaSet | Kind::Job => {
                self.workload_details(ui, kind, row, acts)
            }
            Kind::CronJob => {
                kv(ui, "Schedule", &s(v, "spec/schedule"));
                kv(ui, "Suspend", &get(v, "spec/suspend").as_bool().unwrap_or(false).to_string());
                kv(ui, "Concurrency", &s(v, "spec/concurrencyPolicy"));
                let last = util::parse_time(&s(v, "status/lastScheduleTime")).unwrap_or(0);
                kv(ui, "Last schedule", &format!("{} ago", util::age(last, now)));
            }
            Kind::Node => self.node_details(ui, v, acts),
            Kind::Service => {
                let sel = map_pairs(v, "spec/selector");
                kv_chips(ui, "Selector", sel.iter().map(|(k, v)| format!("{k}={v}")).collect());
                kv(ui, "Type", &s(v, "spec/type"));
                kv(ui, "Session Affinity", &s(v, "spec/sessionAffinity"));
                section(ui, "Connection");
                kv(ui, "Cluster IP", &s(v, "spec/clusterIP"));
                let lb: Vec<String> = arr(v, "status/loadBalancer/ingress").iter().map(|x| s(x, "ip")).collect();
                if !lb.is_empty() {
                    kv(ui, "Load Balancer", &lb.join(", "));
                }
                let ports: Vec<String> = arr(v, "spec/ports")
                    .iter()
                    .map(|p| {
                        let name = s(p, "name");
                        let base = format!("{}/{} → {}", i(p, "port"), s(p, "protocol"), s(p, "targetPort"));
                        if name.is_empty() { base } else { format!("{name}: {base}") }
                    })
                    .collect();
                kv_lines(ui, "Ports", &ports);
                // Endpoints
                if let Some(ep) = self.rows(Kind::Endpoints).iter().find(|e| e.name == row.name && e.ns == row.ns) {
                    if let Some(crate::model::Cell::Text(t)) = ep.cells.get(2) {
                        kv(ui, "Endpoints", t);
                    }
                }
                let pods = self.rows(Kind::Pod);
                let matched: Vec<&Row> = pods
                    .iter()
                    .filter(|p| {
                        p.ns == row.ns
                            && !sel.is_empty()
                            && sel.iter().all(|(k, val)| s(&p.obj, &format!("metadata/labels/{k}")) == *val)
                    })
                    .collect();
                if !matched.is_empty() {
                    pod_list(ui, &matched, acts);
                }
            }
            Kind::Ingress => {
                // ingress-details.tsx: Ports, TLS, then per rule "Host: …" with a
                // Path | Link | Backends table, then the load balancer points.
                kv(ui, "Ports", &util::ingress_ports(v));
                let tls: Vec<String> = arr(v, "spec/tls").iter().map(|t| s(t, "secretName")).collect();
                if !tls.is_empty() {
                    kv_lines(ui, "TLS", &tls);
                }
                let class = s(v, "spec/ingressClassName");
                if !class.is_empty() {
                    kv(ui, "Class", &class);
                }
                section(ui, "Rules");
                let routes = util::ingress_routes(v);
                let mut last_host = String::new();
                for r in &routes {
                    if r.host != last_host {
                        if r.host != "*" {
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("Host: {}", r.host)).color(theme::TEXT_HI));
                        }
                        ui.add_space(4.0);
                        let widths = crate::views::col_widths(&[("Path", 1.0), ("Link", 2.4), ("Backends", 1.4)], ui.available_width());
                        crate::views::table_header(ui, &["Path", "Link", "Backends"], &widths, None);
                        last_host = r.host.clone();
                    }
                    let widths = crate::views::col_widths(&[("Path", 1.0), ("Link", 2.4), ("Backends", 1.4)], ui.available_width());
                    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                    let f = FontId::proportional(theme::FONT_SIZE - 1.0);
                    let g = crate::app::ellipsize(ui, &r.pathname, f.clone(), theme::ITEM_VALUE, widths[0] - 16.0);
                    ui.painter().galley(pos2(rect.left() + 12.0, rect.center().y - g.size().y / 2.0), g, theme::ITEM_VALUE);
                    let link_rect = egui::Rect::from_min_size(pos2(rect.left() + widths[0], rect.top()), vec2(widths[1], rect.height()));
                    let col = if r.link { theme::BLUE } else { theme::ITEM_VALUE };
                    let g = crate::app::ellipsize(ui, &r.url, f.clone(), col, widths[1] - 16.0);
                    let gw = g.size().x;
                    ui.painter().galley(pos2(link_rect.left() + 12.0, rect.center().y - g.size().y / 2.0), g, col);
                    if r.link {
                        let hit = egui::Rect::from_min_size(pos2(link_rect.left() + 12.0, rect.top() + 6.0), vec2(gw, rect.height() - 12.0));
                        let resp = ui.interact(hit, ui.scope_id().with(("ing", &r.url)), Sense::click());
                        if resp.hovered() {
                            ui.painter().line_segment([hit.left_bottom(), hit.right_bottom()], Stroke::new(1.0, theme::TEXT_HI));
                        }
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("Open in browser").clicked() {
                            acts.push(Act::Browse(r.url.clone()));
                        }
                    }
                    let g = crate::app::ellipsize(ui, &r.service, f, theme::ITEM_VALUE, widths[2] - 16.0);
                    ui.painter().galley(pos2(rect.left() + widths[0] + widths[1] + 12.0, rect.center().y - g.size().y / 2.0), g, theme::ITEM_VALUE);
                    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::BORDER));
                }
                let lbs = util::ingress_load_balancers(v);
                if !lbs.is_empty() {
                    section(ui, "Load-Balancer Ingress Points");
                    let widths = crate::views::col_widths(&[("Hostname", 1.0), ("IP", 1.0)], ui.available_width());
                    crate::views::table_header(ui, &["Hostname", "IP"], &widths, None);
                    for (h, ip) in lbs {
                        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                        let f = FontId::proportional(theme::FONT_SIZE - 1.0);
                        let h = if h.is_empty() { "-".to_string() } else { h };
                        let ip = if ip.is_empty() { "-".to_string() } else { ip };
                        ui.painter().text(pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, h, f.clone(), theme::ITEM_VALUE);
                        ui.painter().text(pos2(rect.left() + widths[0] + 12.0, rect.center().y), Align2::LEFT_CENTER, ip, f, theme::ITEM_VALUE);
                        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, theme::BORDER));
                    }
                }
            }
            Kind::ConfigMap => {
                section(ui, "Data");
                for (k, val) in map_pairs(v, "data") {
                    ui.label(RichText::new(&k).color(theme::TEXT_HI).strong());
                    code_block(ui, &val);
                    ui.add_space(6.0);
                }
            }
            Kind::Secret => {
                kv(ui, "Type", &s(v, "type"));
                section(ui, "Data");
                for (k, val) in map_pairs(v, "data") {
                    let key = format!("{}/{}/{}", row.ns, row.name, k);
                    let shown = self.revealed.contains(&key);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&k).color(theme::TEXT_HI).strong());
                        if ui.small_button(if shown { "Hide" } else { "Show" }).clicked() {
                            acts.push(Act::Reveal(key.clone()));
                        }
                    });
                    if shown {
                        code_block(ui, &base64_decode(&val));
                    } else {
                        code_block(ui, "••••••••••••");
                    }
                    ui.add_space(6.0);
                }
            }
            Kind::Pvc => {
                self.volume_usage_row(ui, &row.ns, &row.name);
                // getPods: pods in the namespace with a volume using this claim.
                let pods = self.rows(Kind::Pod);
                let users: Vec<&Row> = pods
                    .iter()
                    .filter(|p| {
                        p.ns == row.ns
                            && arr(&p.obj, "spec/volumes")
                                .iter()
                                .any(|vol| s(vol, "persistentVolumeClaim/claimName") == row.name)
                    })
                    .collect();
                if !users.is_empty() {
                    kv_row(ui, "Pods", |ui| {
                        for p in &users {
                            if ui.link(RichText::new(&p.name).color(theme::BLUE)).clicked() {
                                acts.push(Act::Open(Kind::Pod, p.uid.clone()));
                            }
                        }
                    });
                }
                kv(ui, "Access Modes", &arr(v, "spec/accessModes").iter().map(|x| s(x, "")).collect::<Vec<_>>().join(", "));
                kv(ui, "Storage Class", &s(v, "spec/storageClassName"));
                kv(ui, "Storage", &s(v, "spec/resources/requests/storage"));
                kv(ui, "Volume", &s(v, "spec/volumeName"));
                kv(ui, "Status", &s(v, "status/phase"));
            }
            Kind::Pv => {
                self.volume_usage_row(ui, &s(v, "spec/claimRef/namespace"), &s(v, "spec/claimRef/name"));
                kv(ui, "Capacity", &s(v, "spec/capacity/storage"));
                kv(ui, "Access Modes", &arr(v, "spec/accessModes").iter().map(|x| s(x, "")).collect::<Vec<_>>().join(", "));
                kv(ui, "Reclaim Policy", &s(v, "spec/persistentVolumeReclaimPolicy"));
                kv(ui, "Storage Class", &s(v, "spec/storageClassName"));
                kv(ui, "Claim", &format!("{}/{}", s(v, "spec/claimRef/namespace"), s(v, "spec/claimRef/name")));
                kv(ui, "Status", &s(v, "status/phase"));
            }
            Kind::StorageClass => {
                kv(ui, "Provisioner", &s(v, "provisioner"));
                kv(ui, "Reclaim Policy", &s(v, "reclaimPolicy"));
                kv(ui, "Binding Mode", &s(v, "volumeBindingMode"));
            }
            Kind::Namespace => {
                kv(ui, "Status", &s(v, "status/phase"));
                let pods = self.rows(Kind::Pod);
                let n = pods.iter().filter(|p| p.ns == row.name).count();
                kv(ui, "Pods", &n.to_string());
            }
            Kind::Event => {
                kv(ui, "Message", &s(v, "message"));
                kv(ui, "Reason", &s(v, "reason"));
                kv(ui, "Source", &s(v, "source/component"));
                kv(ui, "First seen", &s(v, "firstTimestamp"));
                kv(ui, "Last seen", &s(v, "lastTimestamp"));
                kv(ui, "Count", &get(v, "count").as_i64().unwrap_or(1).to_string());
                kv(ui, "Type", &s(v, "type"));
                section(ui, "Involved object");
                kv(ui, "Name", &s(v, "involvedObject/name"));
                kv(ui, "Namespace", &s(v, "involvedObject/namespace"));
                kv(ui, "Kind", &s(v, "involvedObject/kind"));
            }
            Kind::Hpa => {
                kv(ui, "Reference", &format!("{}/{}", s(v, "spec/scaleTargetRef/kind"), s(v, "spec/scaleTargetRef/name")));
                kv(ui, "Min Pods", &get(v, "spec/minReplicas").as_i64().unwrap_or(1).to_string());
                kv(ui, "Max Pods", &i(v, "spec/maxReplicas").to_string());
                kv(ui, "Replicas", &i(v, "status/currentReplicas").to_string());
            }
            Kind::ResourceQuota => {
                section(ui, "Quotas");
                for (k, hard) in map_pairs(v, "spec/hard") {
                    let used = s(v, &format!("status/used/{k}"));
                    kv(ui, &k, &format!("{} / {}", if used.is_empty() { "0".into() } else { used }, hard));
                }
            }
            Kind::ClusterRoleBinding | Kind::RoleBinding => {
                kv(ui, "Role", &format!("{} {}", s(v, "roleRef/kind"), s(v, "roleRef/name")));
                let subj: Vec<String> = arr(v, "subjects").iter().map(|x| format!("{} {}", s(x, "kind"), s(x, "name"))).collect();
                kv_lines(ui, "Subjects", &subj);
            }
            Kind::ClusterRole | Kind::Role => {
                section(ui, "Rules");
                for r in arr(v, "rules") {
                    let j = |p: &str| arr(r, p).iter().map(|x| s(x, "")).collect::<Vec<_>>().join(", ");
                    kv(ui, "Resources", &j("resources"));
                    kv(ui, "Verbs", &j("verbs"));
                    kv(ui, "API Groups", &j("apiGroups"));
                    ui.add_space(6.0);
                }
            }
            Kind::Custom(i) => {
                // custom-resources/details.tsx: every printer column (including priority ones),
                // then the status conditions.
                let info = crate::crd::info(i);
                let crd_rows = self.rows(Kind::Crd);
                if let Some(def) = crd_rows.iter().find(|r| r.name == info.key) {
                    let versions = arr(&def.obj, "spec/versions");
                    let pv = versions
                        .iter()
                        .find(|x| get(x, "storage").as_bool().unwrap_or(false) && get(x, "served").as_bool().unwrap_or(false))
                        .or_else(|| versions.iter().find(|x| get(x, "served").as_bool().unwrap_or(false)));
                    if let Some(pv) = pv {
                        for c in arr(pv, "additionalPrinterColumns") {
                            let name = s(c, "name");
                            if name.to_lowercase() == "age" {
                                continue;
                            }
                            // convertSpecValue: list → one line each, object → JSON block,
                            // boolean → True/False badge, string/number → text, nothing → "—".
                            let vals = crate::crd::json_path_values(v, &s(c, "jsonPath"));
                            let value = if vals.len() == 1 { vals[0].clone() } else { serde_json::Value::Array(vals) };
                            kv_row(ui, &crate::crd::start_case(&name), |ui| spec_value(ui, &value));
                        }
                    }
                }
                conditions_drawer(ui, v);
            }
            Kind::Crd => {
                kv(ui, "Group", &s(v, "spec/group"));
                kv(ui, "Kind", &s(v, "spec/names/kind"));
                kv(ui, "Plural", &s(v, "spec/names/plural"));
                kv(ui, "Scope", &s(v, "spec/scope"));
            }
            _ => {}
        }

        // Events for this object
        if kind != Kind::Event {
            let events = self.rows(Kind::Event);
            let mine: Vec<&Row> = events
                .iter()
                .filter(|e| {
                    s(&e.obj, "involvedObject/name") == row.name
                        && s(&e.obj, "involvedObject/kind") == kind.singular()
                        && (row.ns.is_empty() || e.ns == row.ns)
                })
                .collect();
            section(ui, "Events");
            if mine.is_empty() {
                ui.label(RichText::new("No events").color(theme::TEXT));
            }
            for e in mine.iter().take(30) {
                let warn = s(&e.obj, "type") == "Warning";
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .corner_radius(CornerRadius::same(4))
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(
                            RichText::new(s(&e.obj, "message"))
                                .color(if warn { Color32::from_rgb(0xf0, 0x80, 0x78) } else { theme::TEXT_HI }),
                        );
                        ui.label(
                            RichText::new(format!(
                                "{} · {} · {}× · last seen {} ago",
                                s(&e.obj, "reason"),
                                s(&e.obj, "source/component"),
                                get(&e.obj, "count").as_i64().unwrap_or(1),
                                util::age(util::parse_time(&s(&e.obj, "lastTimestamp")).unwrap_or(e.created), now)
                            ))
                            .size(12.0)
                            .color(theme::TEXT),
                        );
                    });
                ui.add_space(6.0);
            }
        }
    }

    fn volume_usage_row(&self, ui: &mut egui::Ui, ns: &str, claim: &str) {
        let got = self.volume_usage.as_ref().and_then(|u| u.get(&(ns.to_string(), claim.to_string())));
        match got {
            Some((used, cap)) if used.is_finite() && *cap > 0.0 => {
                usage_row(
                    ui,
                    "Usage",
                    (*used / *cap) as f32,
                    &format!(
                        "{} used of {}, {} free",
                        crate::model::bytes_to_units(*used, 2),
                        crate::model::bytes_to_units(*cap, 2),
                        crate::model::bytes_to_units((*cap - *used).max(0.0), 2)
                    ),
                    theme::YELLOW,
                );
            }
            _ => {
                let why = if self.volume_error.is_some() {
                    "not available (node stats can't be read)"
                } else if self.volume_usage.is_none() {
                    "loading…"
                } else {
                    "not reported (volume not mounted by a running pod)"
                };
                kv_colored(ui, "Usage", why, theme::TEXT);
            }
        }
    }

    fn pod_details(&self, ui: &mut egui::Ui, v: &Value, acts: &mut Vec<Act>) {
        let (st, tone) = pod_status(v);
        kv_colored(ui, "Status", &st, theme::tone(tone));
        kv(ui, "Node", &s(v, "spec/nodeName"));
        kv(ui, "Pod IP", &s(v, "status/podIP"));
        kv(ui, "Service Account", &s(v, "spec/serviceAccountName"));
        kv(ui, "QoS Class", &s(v, "status/qosClass"));
        conditions_drawer(ui, v);
        let tol = arr(v, "spec/tolerations").len();
        if tol > 0 {
            kv(ui, "Tolerations", &tol.to_string());
        }

        section(ui, "Containers");
        let statuses = arr(v, "status/containerStatuses");
        for c in arr(v, "spec/containers") {
            let name = s(c, "name");
            let cs = statuses.iter().find(|x| s(x, "name") == name);
            let tone = cs.map(container_tone).unwrap_or(crate::model::Tone::Muted);
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                ui.painter().rect_filled(r, CornerRadius::same(1), theme::tone(tone));
                ui.label(RichText::new(&name).color(theme::TEXT_HI).size(14.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(">_ Shell").clicked() {
                        acts.push(Act::Shell(Some(name.clone())));
                    }
                    if ui.small_button("🗎 Logs").clicked() {
                        acts.push(Act::Logs(Some(name.clone())));
                    }
                });
            });
            if let Some(cs) = cs {
                let state = get(cs, "state");
                let (label, extra) = if !get(state, "running").is_null() {
                    ("running", if get(cs, "ready").as_bool().unwrap_or(false) { ", ready" } else { ", not ready" }.to_string())
                } else if !get(state, "waiting").is_null() {
                    ("waiting", format!(" ({})", s(state, "waiting/reason")))
                } else {
                    ("terminated", format!(" ({})", s(state, "terminated/reason")))
                };
                kv_colored(ui, "Status", &format!("{label}{extra}"), theme::tone(tone));
                let rc = i(cs, "restartCount");
                if rc > 0 {
                    kv(ui, "Restarts", &rc.to_string());
                }
            }
            kv(ui, "Image", &s(c, "image"));
            let ports: Vec<String> = arr(c, "ports")
                .iter()
                .map(|p| format!("{}: {}/{}", s(p, "name"), i(p, "containerPort"), s(p, "protocol")))
                .collect();
            if !ports.is_empty() {
                kv_lines(ui, "Ports", &ports);
            }
            let env: Vec<String> = arr(c, "env")
                .iter()
                .map(|e| {
                    let val = s(e, "value");
                    if val.is_empty() && !get(e, "valueFrom").is_null() {
                        format!("{}: (from reference)", s(e, "name"))
                    } else {
                        format!("{}: {}", s(e, "name"), val)
                    }
                })
                .collect();
            if !env.is_empty() {
                kv_lines(ui, "Environment", &env);
            }
            let mounts: Vec<String> = arr(c, "volumeMounts")
                .iter()
                .map(|m| format!("{}  from {}{}", s(m, "mountPath"), s(m, "name"), if get(m, "readOnly").as_bool().unwrap_or(false) { " (ro)" } else { "" }))
                .collect();
            if !mounts.is_empty() {
                kv_lines(ui, "Mounts", &mounts);
            }
            let req = map_pairs(c, "resources/requests").into_iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ");
            let lim = map_pairs(c, "resources/limits").into_iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ");
            if !req.is_empty() {
                kv(ui, "Requests", &req);
            }
            if !lim.is_empty() {
                kv(ui, "Limits", &lim);
            }
            ui.add_space(10.0);
        }
    }

    fn workload_details(&self, ui: &mut egui::Ui, kind: Kind, row: &Row, acts: &mut Vec<Act>) {
        let v = &row.obj;
        match kind {
            Kind::DaemonSet => {
                kv(ui, "Pods", &format!("{} ready of {} desired", i(v, "status/numberReady"), i(v, "status/desiredNumberScheduled")));
            }
            Kind::Job => {
                kv(ui, "Completions", &get(v, "spec/completions").as_i64().unwrap_or(1).to_string());
                kv(ui, "Parallelism", &get(v, "spec/parallelism").as_i64().unwrap_or(1).to_string());
                kv(ui, "Succeeded", &i(v, "status/succeeded").to_string());
            }
            _ => {
                kv(
                    ui,
                    "Replicas",
                    &format!(
                        "{} desired, {} updated, {} total, {} available",
                        i(v, "spec/replicas"),
                        i(v, "status/updatedReplicas"),
                        i(v, "status/replicas"),
                        i(v, "status/availableReplicas").max(i(v, "status/readyReplicas"))
                    ),
                );
            }
        }
        let sel = map_pairs(v, "spec/selector/matchLabels");
        if !sel.is_empty() {
            kv_chips(ui, "Selector", sel.iter().map(|(k, v)| format!("{k}={v}")).collect());
        }
        let strat = s(v, "spec/strategy/type");
        if !strat.is_empty() {
            kv(ui, "Strategy Type", &strat);
        }
        conditions_drawer(ui, v);
        let images: Vec<String> = arr(v, "spec/template/spec/containers").iter().map(|c| s(c, "image")).collect();
        if !images.is_empty() {
            kv_lines(ui, "Images", &images);
        }
        let pods = self.rows(Kind::Pod);
        let mine: Vec<&Row> = pods.iter().filter(|p| p.ns == row.ns && pod_belongs_to(&p.obj, kind, &row.name)).collect();
        pod_list(ui, &mine, acts);
    }

    fn node_details(&self, ui: &mut egui::Ui, v: &Value, acts: &mut Vec<Act>) {
        let addrs: Vec<String> = arr(v, "status/addresses").iter().map(|a| format!("{}: {}", s(a, "type"), s(a, "address"))).collect();
        kv_lines(ui, "Addresses", &addrs);
        kv(ui, "OS", &format!("{} ({})", s(v, "status/nodeInfo/operatingSystem"), s(v, "status/nodeInfo/architecture")));
        kv(ui, "OS Image", &s(v, "status/nodeInfo/osImage"));
        kv(ui, "Kernel version", &s(v, "status/nodeInfo/kernelVersion"));
        kv(ui, "Container runtime", &s(v, "status/nodeInfo/containerRuntimeVersion"));
        kv(ui, "Kubelet version", &s(v, "status/nodeInfo/kubeletVersion"));
        kv(ui, "Roles", &node_roles(v));
        let name = s(v, "metadata/name");
        match self.node_usage.as_ref().and_then(|u| u.get(&name)) {
            Some((cpu, mem)) => {
                // Measured against allocatable, like the original's nodes list.
                let cpu_cap = util::quantity(&s(v, "status/allocatable/cpu"));
                let mem_cap = util::quantity(&s(v, "status/allocatable/memory"));
                usage_row(ui, "CPU usage", (*cpu / cpu_cap.max(0.001)) as f32,
                    &format!("{} of {} cores", crate::model::format_cores(*cpu), crate::model::format_cores(cpu_cap)), theme::BLUE);
                usage_row(ui, "Memory usage", (*mem / mem_cap.max(1.0)) as f32,
                    &format!("{} of {}", crate::model::bytes_to_units(*mem, 3), crate::model::bytes_to_units(mem_cap, 3)), theme::PURPLE);
            }
            None => {
                let why = if self.metrics_error.is_some() { "metrics server not available" } else { "loading…" };
                kv_colored(ui, "Usage", why, theme::TEXT);
            }
        }
        kv(
            ui,
            "Capacity",
            &format!(
                "CPU: {}, Memory: {}, Pods: {}",
                util::fmt_cores(util::quantity(&s(v, "status/capacity/cpu"))),
                util::fmt_bytes(util::quantity(&s(v, "status/capacity/memory"))),
                s(v, "status/capacity/pods")
            ),
        );
        kv(
            ui,
            "Allocatable",
            &format!(
                "CPU: {}, Memory: {}, Pods: {}",
                util::fmt_cores(util::quantity(&s(v, "status/allocatable/cpu"))),
                util::fmt_bytes(util::quantity(&s(v, "status/allocatable/memory"))),
                s(v, "status/allocatable/pods")
            ),
        );
        let taints: Vec<String> = arr(v, "spec/taints").iter().map(|t| format!("{}: {}", s(t, "key"), s(t, "effect"))).collect();
        if !taints.is_empty() {
            kv_lines(ui, "Taints", &taints);
        }
        conditions_drawer(ui, v);

        let pods = self.rows(Kind::Pod);
        let mine: Vec<&Row> = pods.iter().filter(|p| s(&p.obj, "spec/nodeName") == name).collect();
        pod_list(ui, &mine, acts);
    }
}

// ------------------------------------------------------------ small widgets

/// DrawerItem (drawer-item.tsx + .DrawerItem CSS): name column at least 30 % of the row
/// (minmax(30%, min-content)), 8 px top/bottom padding, 1 px borderFaintColor line underneath.
fn kv_row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let full = ui.available_width();
    let name_font = FontId::proportional(theme::FONT_SIZE);
    let name_w = ui.painter().layout_no_wrap(label.to_string(), name_font.clone(), theme::ITEM_NAME).size().x + 8.0;
    let col_w = (full * 0.30).max(name_w.min(full * 0.5));
    ui.add_space(8.0);
    ui.horizontal_top(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(col_w, 18.0), Sense::hover());
        let g = crate::app::ellipsize(ui, label, name_font, theme::ITEM_NAME, col_w - 8.0);
        ui.painter().galley(pos2(r.left(), r.center().y - g.size().y / 2.0), g, theme::ITEM_NAME);
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            add(ui)
        });
    });
    ui.add_space(8.0);
    let r = ui.max_rect();
    let y = ui.cursor().top() - 0.5;
    ui.painter().line_segment([pos2(r.left(), y), pos2(r.right(), y)], Stroke::new(1.0, theme::BORDER));
}

fn kv(ui: &mut egui::Ui, label: &str, value: &str) {
    kv_colored(ui, label, value, theme::ITEM_VALUE);
}

fn kv_colored(ui: &mut egui::Ui, label: &str, value: &str, col: Color32) {
    kv_row(ui, label, |ui| {
        ui.add(egui::Label::new(RichText::new(if value.is_empty() { "-" } else { value }).color(col)).wrap());
    });
}

fn usage_row(ui: &mut egui::Ui, label: &str, frac: f32, text: &str, color: Color32) {
    kv_row(ui, label, |ui| {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(160.0, 8.0), Sense::hover());
            let r = egui::Rect::from_center_size(r.center(), vec2(r.width(), 6.0));
            ui.painter().rect_filled(r, CornerRadius::same(3), theme::LINE_BG);
            let mut f = r;
            f.set_width(r.width() * frac.clamp(0.0, 1.0));
            ui.painter().rect_filled(f, CornerRadius::same(3), color);
            ui.label(
                RichText::new(format!("{:.0}%  ({text})", frac * 100.0)).color(theme::ITEM_VALUE),
            );
        });
    });
}

fn kv_lines(ui: &mut egui::Ui, label: &str, lines: &[String]) {
    kv_row(ui, label, |ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        for l in lines {
            ui.add(egui::Label::new(RichText::new(l).color(theme::ITEM_VALUE)).wrap());
        }
    });
}

fn kv_chips(ui: &mut egui::Ui, label: &str, chips: Vec<String>) {
    let items: Vec<(String, Color32)> = chips.into_iter().map(|c| (c, theme::ITEM_VALUE)).collect();
    kv_row(ui, label, |ui| chip_flow(ui, &items));
}


/// Labels/annotations/conditions as small badges that flow left to right and wrap as whole
/// badges. Text never wraps inside a badge; a badge wider than the row is shortened with "…"
/// and shows its full text on hover.
fn chip_flow(ui: &mut egui::Ui, items: &[(String, Color32)]) {
    let specs: Vec<BadgeSpec> =
        items.iter().map(|(t, c)| BadgeSpec { text: t.clone(), color: *c, bg: None, disabled: false, tooltip: None }).collect();
    badge_flow(ui, &specs);
}

pub struct BadgeSpec {
    pub text: String,
    pub color: Color32,
    pub bg: Option<Color32>,
    /// Badge `disabled`: opacity 0.5.
    pub disabled: bool,
    pub tooltip: Option<String>,
}

/// KubeObjectConditionsDrawer (kube-object-conditions-drawer.tsx): one badge per condition,
/// newest lastTransitionTime first, ties by type priority (Synced 6, Ready 5, Initialized 4,
/// ContainersReady/ArtifactInStorage/Released 3, PodReadyToStartContainers 2, PodScheduled 1);
/// status False/Unknown → disabled; True "Ready" → colorSuccess background with white text;
/// tooltip lists every field of the condition.
fn conditions_drawer(ui: &mut egui::Ui, v: &serde_json::Value) {
    let mut conds: Vec<&serde_json::Value> = arr(v, "status/conditions").iter().collect();
    if conds.is_empty() {
        return;
    }
    let prio = |t: &str| match t {
        "PodScheduled" => 1,
        "PodReadyToStartContainers" => 2,
        "ContainersReady" | "ArtifactInStorage" | "Released" => 3,
        "Initialized" => 4,
        "Ready" => 5,
        "Synced" => 6,
        _ => 0,
    };
    let when = |c: &serde_json::Value| util::parse_time(&s(c, "lastTransitionTime")).unwrap_or(0);
    conds.sort_by(|a, b| when(b).cmp(&when(a)).then(prio(&s(b, "type")).cmp(&prio(&s(a, "type")))));
    let specs: Vec<BadgeSpec> = conds
        .iter()
        .map(|c| {
            let ty = s(c, "type");
            let st = s(c, "status");
            let off = st == "False" || st == "Unknown";
            let ready = !off && ty == "Ready";
            let mut fields: Vec<(String, String)> = c
                .as_object()
                .map(|o| {
                    o.iter()
                        .map(|(k, val)| (k.clone(), val.as_str().map(|x| x.to_string()).unwrap_or_else(|| val.to_string())))
                        .collect()
                })
                .unwrap_or_default();
            fields.sort_by(|a, b| a.0.cmp(&b.0));
            let tip = fields.iter().map(|(k, val)| format!("{k}: {val}")).collect::<Vec<_>>().join("\n");
            BadgeSpec {
                text: ty,
                color: if ready { Color32::WHITE } else { theme::ITEM_VALUE },
                bg: if ready { Some(theme::SUCCESS) } else { None },
                disabled: off,
                tooltip: Some(tip),
            }
        })
        .collect();
    kv_row(ui, "Conditions", |ui| badge_flow(ui, &specs));
}

fn badge_flow(ui: &mut egui::Ui, items: &[BadgeSpec]) {
    // DrawerItemLabels → Badge (badge.tsx + badge.module CSS): background colorVague, 3 px radius,
    // padding 0.2rem × 0.4rem, 4 px margin around each badge (so 8 px between them), one line with
    // an ellipsis; the full label is the tooltip; clicking a cut-off badge expands it to wrap.
    let max_w = ui.available_width().max(60.0);
    let font = FontId::proportional(theme::FONT_SIZE);
    let pad = vec2(6.4, 3.2);
    let gap = 8.0;
    struct Placed {
        galley: std::sync::Arc<egui::Galley>,
        id: egui::Id,
        expandable: bool,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    }
    let mut placed: Vec<Placed> = Vec::with_capacity(items.len());
    let (mut x, mut y, mut line_h) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (i, spec) in items.iter().enumerate() {
        let (t, c) = (&spec.text, &spec.color);
        let id = ui.scope_id().with(("badge", i, t));
        let expanded = ui.data(|d| d.get_temp::<bool>(id).unwrap_or(false));
        let one_line = crate::app::ellipsize(ui, t, font.clone(), *c, max_w - 2.0 * pad.x);
        let expandable = one_line.elided;
        let galley = if expanded && expandable {
            let mut job = egui::text::LayoutJob::single_section(t.clone(), egui::TextFormat::simple(font.clone(), *c));
            job.wrap = egui::text::TextWrapping { max_width: max_w - 2.0 * pad.x, break_anywhere: true, ..Default::default() };
            ui.painter().layout_job(job)
        } else {
            one_line
        };
        let w = galley.size().x + 2.0 * pad.x;
        let h = galley.size().y + 2.0 * pad.y;
        if x > 0.0 && x + w > max_w {
            x = 0.0;
            y += line_h + gap;
            line_h = 0.0;
        }
        placed.push(Placed { galley, id, expandable, x, y, w, h });
        x += w + gap;
        line_h = line_h.max(h);
    }
    let total_h = if placed.is_empty() { 0.0 } else { y + line_h };
    let (rect, _) = ui.allocate_exact_size(vec2(max_w, total_h), Sense::hover());
    for (i, p) in placed.into_iter().enumerate() {
        let r = egui::Rect::from_min_size(rect.min + vec2(p.x, p.y), vec2(p.w, p.h));
        let resp = ui.interact(r, p.id, if p.expandable { Sense::click() } else { Sense::hover() });
        // .interactive:hover → mainBackground
        let spec = &items[i];
        let mut fill = if p.expandable && resp.hovered() { theme::CONTENT } else { spec.bg.unwrap_or(theme::SIDEBAR) };
        let mut text_col = spec.color;
        if spec.disabled {
            fill = fill.gamma_multiply(0.5);
            text_col = text_col.gamma_multiply(0.5);
        }
        ui.painter().rect_filled(r, CornerRadius::same(3), fill);
        ui.painter().galley_with_override_text_color(r.min + pad, p.galley.clone(), text_col);
        if p.expandable && resp.clicked() {
            let cur = ui.data(|d| d.get_temp::<bool>(p.id).unwrap_or(false));
            ui.data_mut(|d| d.insert_temp(p.id, !cur));
        }
        let resp = if p.expandable { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp };
        resp.on_hover_text(spec.tooltip.clone().unwrap_or_else(|| spec.text.clone()));
    }
}

/// Drawer sub-title: a full-width band in drawerSubtitleBackground.
fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(14.0);
    egui::Frame::new()
        .fill(theme::SUBTITLE_BG)
        .inner_margin(egui::Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).color(theme::TEXT_HI));
        });
    ui.add_space(10.0);
}

fn code_block(ui: &mut egui::Ui, text: &str) {
    egui::Frame::new()
        .fill(theme::TERMINAL)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(RichText::new(text).monospace().color(theme::TEXT_MID)).wrap());
        });
}

fn pod_list(ui: &mut egui::Ui, pods: &[&Row], acts: &mut Vec<Act>) {
    section(ui, "Pods");
    if pods.is_empty() {
        ui.label(RichText::new("No pods").color(theme::TEXT));
        return;
    }
    let now = util::now_secs();
    for (n, p) in pods.iter().enumerate() {
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        crate::views::row_bg(ui.painter(), rect, n, resp.hovered(), false);
        let (st, tone) = pod_status(&p.obj);
        let w = rect.width();
        let g = crate::app::ellipsize(ui, &p.name, FontId::proportional(13.0), theme::BLUE, w * 0.5);
        ui.painter().galley(pos2(rect.left() + 10.0, rect.center().y - g.size().y / 2.0), g, theme::BLUE);
        ui.painter().text(
            pos2(rect.left() + w * 0.55, rect.center().y),
            Align2::LEFT_CENTER,
            s(&p.obj, "spec/nodeName"),
            FontId::proportional(12.5),
            theme::TEXT_MID,
        );
        ui.painter().text(
            pos2(rect.left() + w * 0.75, rect.center().y),
            Align2::LEFT_CENTER,
            util::age(p.created, now),
            FontId::proportional(12.5),
            theme::TEXT_MID,
        );
        ui.painter().text(
            pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            st,
            FontId::proportional(12.5),
            theme::tone(tone),
        );
        if resp.clicked() {
            acts.push(Act::Open(Kind::Pod, p.uid.clone()));
        }
    }
}

pub fn base64_decode(input: &str) -> String {
    let mut buf = 0u32;
    let mut bits = 0;
    let mut out = Vec::new();
    for c in input.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => continue,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn spec_value(ui: &mut egui::Ui, v: &serde_json::Value) {
    match v {
        serde_json::Value::Array(a) => {
            if a.is_empty() {
                ui.label(RichText::new("—").color(theme::ITEM_VALUE));
            }
            for x in a {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("•").color(theme::ITEM_NAME));
                    spec_value(ui, x);
                });
            }
        }
        serde_json::Value::Object(_) => code_block(ui, &serde_json::to_string_pretty(v).unwrap_or_default()),
        serde_json::Value::Bool(b) => {
            let (t, c) = if *b { ("True", theme::SUCCESS) } else { ("False", theme::RED) };
            badge_flow(ui, &[BadgeSpec { text: t.into(), color: c, bg: None, disabled: false, tooltip: None }]);
        }
        serde_json::Value::String(x) => {
            ui.add(egui::Label::new(RichText::new(x).color(theme::ITEM_VALUE)).wrap());
        }
        serde_json::Value::Number(n) => {
            ui.label(RichText::new(n.to_string()).color(theme::ITEM_VALUE));
        }
        serde_json::Value::Null => {
            ui.label(RichText::new("—").color(theme::ITEM_VALUE));
        }
    }
}
