//! Main content views: catalog, cluster overview, workloads overview and resource tables.

use eframe::egui;
use crate::app::{App, FilterKey, View, ellipsize};
use crate::backend::Source;
use crate::model::{Cell, GROUPS, Kind, Row, Tone, pod_status};
use crate::theme;
use crate::util::{self, arr, s};
use egui::{Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Stroke, pos2, vec2};
use std::sync::Arc;

/// Table rows: rowLineHeight 17 + 2 × rowPadding 8 (table.tsx defaultProps).
pub const ROW_H: f32 = 33.0;
const HEAD_H: f32 = 33.0;

pub enum RowAction {
    Open(usize),
    Logs(usize),
    Shell(usize),
    Edit(usize),
    Delete(usize),
    Scale(usize),
    Restart(usize),
    Browse(String),
}

impl App {
    // ------------------------------------------------------------ catalog

    pub fn catalog(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Catalog").size(theme::FONT_SIZE + 4.0).color(theme::TEXT_TERTIARY));
            ui.add_space(8.0);
            let n = self.clusters.len();
            ui.label(RichText::new(format!("{n} cluster{}", if n == 1 { "" } else { "s" })).color(theme::TEXT));
        });
        ui.add_space(4.0);
        ui.label(
            RichText::new("Clusters come from your kubeconfig. Click one to open it.").color(theme::TEXT),
        );
        if let Some(e) = &self.contexts_error {
            ui.add_space(6.0);
            notice(ui, &format!("Could not read your kubeconfig: {e}\nThe sample cluster is still available."), true);
        } else if !self.contexts_loaded {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Looking for clusters in your kubeconfig…").color(theme::TEXT));
            });
        }
        ui.add_space(12.0);

        let cols = [("Name", 3.0), ("Source", 1.6), ("Distribution", 1.4), ("Status", 1.2)];
        let width = ui.available_width();
        let widths = col_widths(&cols, width);
        table_header(ui, &cols.iter().map(|c| c.0).collect::<Vec<_>>(), &widths, None);
        let mut open = None;
        for (i, c) in self.clusters.iter().enumerate() {
            let (rect, resp) = ui.allocate_exact_size(vec2(width, 42.0), Sense::click());
            let p = ui.painter();
            if resp.hovered() {
                p.rect_filled(rect, CornerRadius::ZERO, theme::HOVER);
            } else if i % 2 == 1 {
                p.rect_filled(rect, CornerRadius::ZERO, theme::STRIPE);
            }
            let mut x = rect.left();
            let av = Rect::from_min_size(pos2(x + 12.0, rect.center().y - 12.0), vec2(24.0, 24.0));
            p.rect_filled(av, CornerRadius::same(4), theme::avatar(&c.name));
            p.text(av.center(), Align2::CENTER_CENTER, theme::initials(&c.name), FontId::proportional(11.0), theme::TEXT_HI);
            let g = ellipsize(ui, &c.name, FontId::proportional(13.5), theme::TEXT_HI, widths[0] - 60.0);
            p.galley(pos2(av.right() + 12.0, rect.center().y - g.size().y / 2.0), g, theme::TEXT_HI);
            x += widths[0];
            let (src, dist) = match &c.source {
                Source::Sample => ("built-in sample", "sample"),
                Source::Context(_) => ("kubeconfig", "kubernetes"),
            };
            p.text(pos2(x + 12.0, rect.center().y), Align2::LEFT_CENTER, src, FontId::proportional(13.0), theme::TEXT_MID);
            x += widths[1];
            p.text(pos2(x + 12.0, rect.center().y), Align2::LEFT_CENTER, dist, FontId::proportional(13.0), theme::TEXT_MID);
            x += widths[2];
            let connected = self.active == Some(i);
            p.text(
                pos2(x + 12.0, rect.center().y),
                Align2::LEFT_CENTER,
                if connected { "connected" } else { "disconnected" },
                FontId::proportional(13.0),
                if connected { theme::GREEN } else { theme::TEXT },
            );
            if resp.clicked() {
                open = Some(i);
            }
        }
        if let Some(i) = open {
            if self.active == Some(i) {
                self.navigate(View::Overview);
            } else {
                self.connect(i);
            }
        }
    }

    // ------------------------------------------------------------ cluster overview

    pub fn overview(&mut self, ui: &mut egui::Ui) {
        let nodes = self.rows(Kind::Node);
        let pods = self.rows(Kind::Pod);
        let events = self.rows(Kind::Event);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Cluster").size(theme::FONT_SIZE + 4.0).color(theme::TEXT_TERTIARY));
            ui.add_space(8.0);
            ui.label(RichText::new(self.cluster_name()).color(theme::TEXT));
        });
        ui.add_space(10.0);
        if let Some(e) = self.store.get(&Kind::Node).and_then(|d| d.error.clone()) {
            notice(ui, &format!("Could not load cluster data:\n{e}"), true);
            if ui.button("Retry").clicked() {
                self.reload(Kind::Node);
                self.reload(Kind::Pod);
                self.reload(Kind::Namespace);
            }
            return;
        }
        if nodes.is_empty() && self.store.get(&Kind::Node).map(|d| d.loaded.is_none()).unwrap_or(true) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Loading cluster metrics…").color(theme::TEXT));
            });
            return;
        }

        // Sum capacity and requests.
        let (mut cpu_cap, mut mem_cap, mut pods_cap) = (0.0, 0.0, 0.0);
        let (mut cpu_alloc, mut mem_alloc) = (0.0, 0.0);
        for n in nodes.iter() {
            cpu_cap += util::quantity(&s(&n.obj, "status/capacity/cpu"));
            mem_cap += util::quantity(&s(&n.obj, "status/capacity/memory"));
            pods_cap += util::quantity(&s(&n.obj, "status/capacity/pods"));
            cpu_alloc += util::quantity(&s(&n.obj, "status/allocatable/cpu"));
            mem_alloc += util::quantity(&s(&n.obj, "status/allocatable/memory"));
        }
        let (mut cpu_req, mut cpu_lim, mut mem_req, mut mem_lim, mut running) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for p in pods.iter() {
            let phase = s(&p.obj, "status/phase");
            if phase == "Succeeded" || phase == "Failed" {
                continue;
            }
            running += 1.0;
            for c in arr(&p.obj, "spec/containers") {
                cpu_req += util::quantity(&s(c, "resources/requests/cpu"));
                cpu_lim += util::quantity(&s(c, "resources/limits/cpu"));
                mem_req += util::quantity(&s(c, "resources/requests/memory"));
                mem_lim += util::quantity(&s(c, "resources/limits/memory"));
            }
        }

        let ready = nodes.iter().filter(|n| crate::model::node_ready(&n.obj)).count();
        // Live usage from the metrics server, when it is installed.
        let usage = self.node_usage.as_ref().map(|u| {
            u.iter()
                .filter(|(n, _)| nodes.iter().any(|r| &r.name == *n))
                .fold((0.0, 0.0), |a, (_, (c, m))| (a.0 + c, a.1 + m))
        });
        let cards: [(&str, Vec<(String, f32, Color32)>, f32, String); 3] = [
            (
                "CPU",
                vec![
                    (format!("Requests: {}", util::fmt_cores(cpu_req)), cpu_req as f32, theme::BLUE),
                    (format!("Limits: {}", util::fmt_cores(cpu_lim)), cpu_lim as f32, theme::PURPLE),
                    (format!("Allocatable: {}", util::fmt_cores(cpu_alloc)), 0.0, theme::YELLOW),
                    (format!("Capacity: {}", util::fmt_cores(cpu_cap)), 0.0, theme::GREY),
                ],
                cpu_alloc.max(0.001) as f32,
                format!("{:.0}%", 100.0 * cpu_req / cpu_alloc.max(0.001)),
            ),
            (
                "Memory",
                vec![
                    (format!("Requests: {}", util::fmt_bytes(mem_req)), mem_req as f32, theme::BLUE),
                    (format!("Limits: {}", util::fmt_bytes(mem_lim)), mem_lim as f32, theme::PURPLE),
                    (format!("Allocatable: {}", util::fmt_bytes(mem_alloc)), 0.0, theme::YELLOW),
                    (format!("Capacity: {}", util::fmt_bytes(mem_cap)), 0.0, theme::GREY),
                ],
                mem_alloc.max(1.0) as f32,
                format!("{:.0}%", 100.0 * mem_req / mem_alloc.max(1.0)),
            ),
            (
                "Pods",
                vec![
                    (format!("Usage: {running}"), running as f32, theme::BLUE),
                    (format!("Capacity: {pods_cap}"), 0.0, theme::GREY),
                    (format!("Nodes ready: {ready}/{}", nodes.len()), 0.0, theme::GREEN),
                ],
                pods_cap.max(1.0) as f32,
                format!("{running}"),
            ),
        ];
        let mut cards = cards;
        if let Some((cpu_use, mem_use)) = usage {
            // Usage becomes the innermost ring and the headline number.
            cards[0].1.insert(0, (format!("Usage: {}", util::fmt_cores(cpu_use)), cpu_use as f32, theme::GREEN));
            cards[0].3 = format!("{:.0}%", 100.0 * cpu_use / cpu_alloc.max(0.001));
            cards[1].1.insert(0, (format!("Usage: {}", util::fmt_bytes(mem_use)), mem_use as f32, theme::GREEN));
            cards[1].3 = format!("{:.0}%", 100.0 * mem_use / mem_alloc.max(1.0));
        }
        let avail = ui.available_width();
        let gap = 16.0;
        let cw = ((avail - gap * 2.0) / 3.0).max(200.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (title, segs, total, center) in cards.iter() {
                card(ui, cw, 230.0, title, |ui, rect| {
                    let c = pos2(rect.left() + 90.0, rect.center().y + 6.0);
                    // Rings from the outside in: the first value is outermost.
                    let rings: Vec<&(String, f32, Color32)> = segs.iter().filter(|x| x.1 > 0.0 || x.0.starts_with("Usage")).collect();
                    let mut r = 66.0;
                    for (k, seg) in rings.iter().take(3).enumerate() {
                        let thick = if k == 0 { 11.0 } else { 8.0 };
                        donut(ui.painter(), c, r, thick, &[(seg.1 / total, seg.2)]);
                        r -= thick + 6.0;
                    }
                    ui.painter().text(c, Align2::CENTER_CENTER, center, FontId::proportional(16.0), theme::TEXT_HI);
                    let mut y = rect.top() + 56.0;
                    for (label, _, col) in segs {
                        let x = rect.left() + 178.0;
                        ui.painter().circle_filled(pos2(x, y), 5.0, *col);
                        ui.painter().text(pos2(x + 12.0, y), Align2::LEFT_CENTER, label, FontId::proportional(12.5), theme::TEXT_MID);
                        y += 24.0;
                    }
                });
            }
        });
        ui.add_space(18.0);

        // Warnings
        let warnings: Vec<&Row> = events.iter().filter(|e| s(&e.obj, "type") == "Warning").collect();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Warnings").size(16.0).color(theme::TEXT_HI));
            ui.label(RichText::new(format!("{}", warnings.len())).color(theme::TEXT));
        });
        ui.add_space(6.0);
        if warnings.is_empty() {
            notice(ui, "No issues found", false);
            return;
        }
        let cols = [("Message", 5.0), ("Object", 2.0), ("Type", 0.9), ("Age", 0.7)];
        let width = ui.available_width();
        let widths = col_widths(&cols, width);
        table_header(ui, &cols.iter().map(|c| c.0).collect::<Vec<_>>(), &widths, None);
        let now = util::now_secs();
        let mut open = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, e) in warnings.iter().enumerate() {
                let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                let p = ui.painter();
                row_bg(p, rect, i, resp.hovered(), false);
                let cells = [
                    Cell::Text(s(&e.obj, "message")),
                    Cell::Text(format!("{}: {}", s(&e.obj, "involvedObject/kind"), s(&e.obj, "involvedObject/name"))),
                    Cell::Status("Warning".into(), Tone::Err),
                    e.cells.last().cloned().unwrap_or(Cell::Age(e.created)),
                ];
                paint_cells(ui, rect, &widths, &cells, now);
                if resp.clicked() {
                    open = Some(e.uid.clone());
                }
            }
        });
        if let Some(uid) = open {
            self.selected = Some((Kind::Event, uid));
        }
    }

    // ------------------------------------------------------------ workloads overview

    pub fn workloads(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().id_salt("workloads_scroll").auto_shrink([false, false]).show(ui, |ui| {
            self.workloads_inner(ui);
        });
    }

    fn workloads_inner(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Overview").size(theme::FONT_SIZE + 4.0).color(theme::TEXT_TERTIARY));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.namespace_picker(ui);
            });
        });
        ui.add_space(12.0);
        let avail = ui.available_width();
        let per_row = ((avail + 16.0) / 290.0).floor().max(1.0) as usize;
        let cw = (avail - 16.0 * (per_row as f32 - 1.0)) / per_row as f32;
        let kinds = GROUPS[0].kinds;
        let mut nav = None;
        for chunk in kinds.chunks(per_row) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                for k in chunk {
                    let rows = self.rows(*k);
                    let rows: Vec<&Row> = rows.iter().filter(|r| self.ns_ok(&r.ns)).collect();
                    let stats = workload_stats(*k, &rows);
                    let total: f32 = stats.iter().map(|x| x.1).sum();
                    let resp = card(ui, cw, 150.0, &format!("{} ({})", k.title(), rows.len()), |ui, rect| {
                        let c = pos2(rect.left() + 64.0, rect.center().y + 12.0);
                        let segs: Vec<(f32, Color32)> =
                            stats.iter().map(|(_, v, col)| (*v / total.max(1.0), *col)).collect();
                        if total == 0.0 {
                            donut(ui.painter(), c, 38.0, 9.0, &[]);
                        } else {
                            donut(ui.painter(), c, 38.0, 9.0, &segs);
                        }
                        let mut y = rect.top() + 58.0;
                        for (label, v, col) in &stats {
                            let x = rect.left() + 124.0;
                            ui.painter().circle_filled(pos2(x, y), 4.5, *col);
                            ui.painter().text(
                                pos2(x + 11.0, y),
                                Align2::LEFT_CENTER,
                                format!("{label}: {v}"),
                                FontId::proportional(12.5),
                                theme::TEXT_MID,
                            );
                            y += 20.0;
                        }
                    });
                    if resp.clicked() {
                        nav = Some(*k);
                    }
                }
            });
            ui.add_space(16.0);
        }
        if let Some(k) = nav {
            self.navigate(View::List(k));
        }
        ui.label(RichText::new("Events").size(16.0).color(theme::TEXT_HI));
        ui.add_space(6.0);
        let rows = self.rows(Kind::Event);
        let idx: Vec<usize> = (0..rows.len()).filter(|i| self.ns_ok(&rows[*i].ns)).collect();
        if let Some(i) = self.resource_table(ui, Kind::Event, &rows, &idx, Some(ROW_H * 12.0)) {
            if let RowAction::Open(i) = i {
                self.selected = Some((Kind::Event, rows[i].uid.clone()));
            }
        }
    }

    pub fn ns_ok(&self, ns: &str) -> bool {
        self.ns_filter.is_empty() || ns.is_empty() || self.ns_filter.contains(ns)
    }

    // ------------------------------------------------------------ resource list

    pub fn namespace_picker(&mut self, ui: &mut egui::Ui) {
        let label = if self.ns_filter.is_empty() {
            "All namespaces".to_string()
        } else {
            self.ns_filter.iter().cloned().collect::<Vec<_>>().join(", ")
        };
        // Listed namespaces plus any typed in, so typed ones can be unticked again.
        let mut names: std::collections::BTreeSet<String> = self.rows(Kind::Namespace).iter().map(|r| r.name.clone()).collect();
        let cant_list = names.is_empty() && self.store.get(&Kind::Namespace).is_some_and(|d| d.error.is_some());
        names.extend(self.ns_filter.iter().cloned());
        egui::ComboBox::from_id_salt("nsfilter")
            .width(190.0)
            .selected_text(RichText::new(format!("Namespace: {label}")).color(theme::TEXT_MID))
            // Stays open while namespaces are ticked or typed.
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                // A namespace can be typed in when the cluster doesn't allow listing them.
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.ns_input).hint_text("Type a namespace, press Enter").desired_width(f32::INFINITY),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    let typed = self.ns_input.trim().to_string();
                    if !typed.is_empty() {
                        self.ns_filter.insert(typed);
                    }
                    self.ns_input.clear();
                    edit.request_focus();
                }
                if cant_list {
                    ui.label(RichText::new("Namespaces can't be listed with your access; type the ones you can use.").color(theme::TEXT_MID).small());
                }
                ui.separator();
                if ui.selectable_label(self.ns_filter.is_empty(), "All namespaces").clicked() {
                    self.ns_filter.clear();
                }
                for n in names {
                    let mut on = self.ns_filter.contains(&n);
                    if ui.checkbox(&mut on, &n).changed() {
                        if on {
                            self.ns_filter.insert(n);
                        } else {
                            self.ns_filter.remove(&n);
                        }
                    }
                }
            });
    }

    fn filtered(&mut self, kind: Kind) -> (Arc<Vec<Row>>, Arc<Vec<usize>>) {
        let d = self.store.entry(kind).or_default();
        let rows = d.rows.clone();
        let key = FilterKey {
            kind,
            version: d.version,
            search: self.search.to_lowercase(),
            ns: if kind.namespaced() { self.ns_filter.clone() } else { Default::default() },
            sort: self.sort.get(&kind).copied(),
        };
        if let Some((k, v)) = &self.filter_cache {
            if *k == key {
                return (rows, v.clone());
            }
        }
        let terms: Vec<&str> = key.search.split_whitespace().collect();
        let mut idx: Vec<usize> = (0..rows.len())
            .filter(|&i| {
                let r = &rows[i];
                (key.ns.is_empty() || key.ns.contains(&r.ns)) && terms.iter().all(|t| r.search.contains(t))
            })
            .collect();
        if let Some((col, asc)) = key.sort {
            let keys: Vec<String> = rows.iter().map(|r| r.cells.get(col).map(|c| c.sort_text()).unwrap_or_default()).collect();
            idx.sort_by(|a, b| {
                let o = natural_cmp(&keys[*a], &keys[*b]);
                if asc { o } else { o.reverse() }
            });
        } else if kind == Kind::Event {
            idx.sort_by_key(|i| match rows[*i].cells.last() {
                Some(Cell::Age(t)) => -*t,
                _ => 0,
            });
        }
        let v = Arc::new(idx);
        self.filter_cache = Some((key, v.clone()));
        (rows, v)
    }

    pub fn list_view(&mut self, ui: &mut egui::Ui, kind: Kind) {
        let (rows, idx) = self.filtered(kind);
        // Header line: title, count, namespace filter, search.
        ui.horizontal(|ui| {
            ui.label(RichText::new(kind.title()).size(theme::FONT_SIZE + 4.0).color(theme::TEXT_TERTIARY));
            ui.add_space(6.0);
            ui.label(RichText::new(format!("{} items", idx.len())).color(theme::TEXT));
            let d = self.store.get(&kind);
            if d.map(|d| d.loading).unwrap_or(false) {
                ui.spinner();
            }
            if matches!(kind, Kind::Pvc | Kind::Pv) && self.volume_usage.is_none() {
                if let Some(e) = &self.volume_error {
                    ui.add_space(10.0);
                    ui.label(RichText::new("volume usage not available").size(12.0).color(theme::TEXT))
                        .on_hover_text(e.clone());
                }
            }
            if kind == Kind::Node {
                ui.add_space(10.0);
                if self.node_usage.is_some() {
                    ui.label(RichText::new("● live usage from metrics server").size(12.0).color(theme::GREEN));
                } else if self.metrics_error.is_some() {
                    ui.label(RichText::new("metrics server not available, showing capacity").size(12.0).color(theme::TEXT))
                        .on_hover_text(self.metrics_error.clone().unwrap_or_default());
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let hint = format!("Search {}…", kind.title());
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text(hint)
                        .desired_width(230.0)
                        .margin(egui::Margin::symmetric(8, 5)),
                );
                if kind.namespaced() {
                    self.namespace_picker(ui);
                }
            });
        });
        ui.add_space(10.0);

        if let Some(e) = self.store.get(&kind).and_then(|d| d.error.clone()) {
            let what = match kind {
                // failedToLoadMessage of the original's custom resource view
                Kind::Custom(i) => format!("Failed to load {}", crate::crd::info(i).plural),
                _ => format!("Could not load {}", kind.title()),
            };
            notice(ui, &format!("{what}:\n{e}"), true);
            if kind.namespaced() && self.ns_filter.is_empty() && e.contains("at the cluster scope") {
                ui.label(
                    RichText::new("Your access may be limited to some namespaces: pick or type one in the Namespace menu above to load just those.")
                        .color(theme::TEXT_MID),
                );
            }
            if ui.button("Retry").clicked() {
                self.reload(kind);
            }
            return;
        }
        if rows.is_empty() && self.store.get(&kind).map(|d| d.loaded.is_none()).unwrap_or(true) {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Loading…").color(theme::TEXT));
            });
            return;
        }
        let action = self.resource_table(ui, kind, &rows, &idx, None);
        if let Some(a) = action {
            self.row_action(kind, &rows, a);
        }
    }

    pub fn row_action(&mut self, kind: Kind, rows: &[Row], a: RowAction) {
        match a {
            RowAction::Open(i) => self.selected = Some((kind, rows[i].uid.clone())),
            RowAction::Logs(i) => self.open_logs(&rows[i].obj, None),
            RowAction::Shell(i) => self.open_shell(&rows[i].obj, None),
            RowAction::Edit(i) => self.open_edit(kind, &rows[i].ns.clone(), &rows[i].name.clone()),
            RowAction::Delete(i) => {
                self.dialog = Some(crate::app::Dialog::Delete { kind, ns: rows[i].ns.clone(), name: rows[i].name.clone() })
            }
            RowAction::Scale(i) => {
                let cur = util::i(&rows[i].obj, "spec/replicas");
                self.dialog = Some(crate::app::Dialog::Scale {
                    kind,
                    ns: rows[i].ns.clone(),
                    name: rows[i].name.clone(),
                    current: cur,
                    desired: cur,
                })
            }
            RowAction::Browse(u) => {
                if let Err(e) = util::open_url(&u) {
                    self.toast(e, true);
                }
            }
            RowAction::Restart(i) => {
                self.dialog = Some(crate::app::Dialog::Restart { kind, ns: rows[i].ns.clone(), name: rows[i].name.clone() })
            }
        }
    }

    /// Virtualised table: only visible rows are laid out, so thousands of items stay fast.
    pub fn resource_table(
        &mut self,
        ui: &mut egui::Ui,
        kind: Kind,
        rows: &[Row],
        idx: &[usize],
        max_h: Option<f32>,
    ) -> Option<RowAction> {
        let cols = kind.columns();
        let menu_w = 36.0;
        let width = ui.available_width();
        let widths = col_widths(cols, width - menu_w);
        let sort = self.sort.get(&kind).copied();
        if let Some(c) = table_header(ui, &cols.iter().map(|c| c.0).collect::<Vec<_>>(), &widths, Some(sort)) {
            let next = match sort {
                Some((sc, true)) if sc == c => Some((c, false)),
                Some((sc, false)) if sc == c => None,
                _ => Some((c, true)),
            };
            match next {
                Some(n) => self.sort.insert(kind, n),
                None => self.sort.remove(&kind),
            };
        }
        if idx.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Item list is empty").color(theme::TEXT));
            });
            return None;
        }
        let sel_uid = self.selected.as_ref().filter(|s| s.0 == kind).map(|s| s.1.clone());
        let now = util::now_secs();
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt(("table", format!("{kind:?}")))
            .auto_shrink([false, max_h.is_some()])
            .max_height(max_h.unwrap_or(f32::INFINITY))
            .show_rows(ui, ROW_H, idx.len(), |ui, range| {
                ui.spacing_mut().item_spacing.y = 0.0;
                for vis in range {
                    let i = idx[vis];
                    let row = &rows[i];
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW_H), Sense::click());
                    if !ui.is_rect_visible(rect) {
                        continue;
                    }
                    let selected = sel_uid.as_deref() == Some(row.uid.as_str());
                    row_bg(ui.painter(), rect, vis, resp.hovered(), selected);
                    let cells_rect = Rect::from_min_max(rect.min, pos2(rect.right() - menu_w, rect.bottom()));
                    paint_cells(ui, cells_rect, &widths, &row.cells, now);
                    // Hover tooltips of the usage bars (the original shows them under the cell).
                    if let Some(pos) = resp.hover_pos() {
                        let mut x = cells_rect.left();
                        for (cell, w) in row.cells.iter().zip(&widths) {
                            if pos.x >= x && pos.x < x + w {
                                if let Cell::Meter(m) = cell {
                                    if !m.tooltip.is_empty() {
                                        resp.clone().on_hover_text_at_pointer(m.tooltip.clone());
                                    }
                                }
                            }
                            x += w;
                        }
                    }
                    // Row menu (⋮)
                    let mr = Rect::from_center_size(pos2(rect.right() - menu_w / 2.0, rect.center().y), vec2(24.0, 24.0));
                    let mresp = ui.interact(mr, ui.scope_id().with(("menu", i)), Sense::click());
                    if mresp.hovered() {
                        ui.painter().rect_filled(mr, CornerRadius::same(12), theme::HOVER);
                    }
                    ui.painter().text(mr.center(), Align2::CENTER_CENTER, "⋮", FontId::proportional(16.0), theme::TEXT_MID);
                    let menu = |ui: &mut egui::Ui, action: &mut Option<RowAction>| {
                        if kind == Kind::Pod {
                            if ui.button("🗎  Logs").clicked() {
                                *action = Some(RowAction::Logs(i));
                            }
                            if ui.button(">_  Shell").clicked() {
                                *action = Some(RowAction::Shell(i));
                            }
                        }
                        if kind.scalable() && ui.button("⇕  Scale").clicked() {
                            *action = Some(RowAction::Scale(i));
                        }
                        if kind.restartable() && ui.button("⟲  Restart").clicked() {
                            *action = Some(RowAction::Restart(i));
                        }
                        if kind == Kind::Ingress {
                            for u in util::ingress_urls(&row.obj).into_iter().take(6) {
                                if ui.button(format!("🌐  Open {u}")).clicked() {
                                    *action = Some(RowAction::Browse(u));
                                }
                            }
                        }
                        if ui.button("✎  Edit").clicked() {
                            *action = Some(RowAction::Edit(i));
                        }
                        if ui.button("🗑  Delete").clicked() {
                            *action = Some(RowAction::Delete(i));
                        }
                    };
                    egui::Popup::menu(&mresp).show(|ui| menu(ui, &mut action));
                    resp.context_menu(|ui| menu(ui, &mut action));
                    if resp.clicked() && !mresp.clicked() {
                        action = Some(RowAction::Open(i));
                    }
                }
            });
        action
    }
}

// ------------------------------------------------------------ painting helpers

/// Share the width by the columns' flex weights (as the original's table does), but never
/// narrower than the header title, and usage bars never narrower than 120 px so "usage / requests"
/// stays readable in a small window (the original lets such cells clip instead).
pub fn col_widths(cols: &[(&str, f32)], width: f32) -> Vec<f32> {
    let mins: Vec<f32> = cols
        .iter()
        .map(|(t, _)| {
            let header = t.chars().count() as f32 * 7.5 + 30.0;
            match *t {
                "CPU" | "Memory" | "Disk" => header.max(120.0),
                "Usage" => header.max(150.0),
                "Name" => header.max(150.0),
                _ => header,
            }
        })
        .collect();
    let mut fixed = vec![false; cols.len()];
    let mut out = vec![0.0; cols.len()];
    for _ in 0..cols.len() {
        let free: f32 = width - (0..cols.len()).filter(|&i| fixed[i]).map(|i| out[i]).sum::<f32>();
        let weight: f32 = (0..cols.len()).filter(|&i| !fixed[i]).map(|i| cols[i].1).sum();
        let mut changed = false;
        for i in 0..cols.len() {
            if !fixed[i] {
                out[i] = free.max(0.0) * cols[i].1 / weight.max(0.0001);
                if out[i] < mins[i] {
                    out[i] = mins[i];
                    fixed[i] = true;
                    changed = true;
                }
            }
        }
        if !changed {
            break;
        }
    }
    out
}

pub fn table_header(ui: &mut egui::Ui, titles: &[&str], widths: &[f32], sort: Option<Option<(usize, bool)>>) -> Option<usize> {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, HEAD_H), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, theme::TABLE_HEAD);
    let mut x = rect.left();
    let mut clicked = None;
    for (ci, (t, w)) in titles.iter().zip(widths).enumerate() {
        let cr = Rect::from_min_size(pos2(x, rect.top()), vec2(*w, HEAD_H));
        let mut color = theme::TABLE_HEAD_TEXT;
        if sort.is_some() {
            let r = ui.interact(cr, ui.scope_id().with(("hdr", ci)), Sense::click());
            if r.hovered() {
                color = theme::TEXT_HI;
            }
            if r.clicked() {
                clicked = Some(ci);
            }
        }
        let g = ellipsize(ui, t, FontId::proportional(12.5), color, w - 26.0);
        let gw = g.size().x;
        p.galley(pos2(x + 12.0, rect.center().y - g.size().y / 2.0), g, color);
        if let Some(Some((sc, asc))) = sort {
            if sc == ci {
                p.text(
                    pos2(x + 16.0 + gw, rect.center().y),
                    Align2::LEFT_CENTER,
                    if asc { "▲" } else { "▼" },
                    FontId::proportional(9.0),
                    theme::BLUE,
                );
            }
        }
        x += w;
    }
    clicked
}

pub fn row_bg(p: &egui::Painter, rect: Rect, i: usize, hovered: bool, selected: bool) {
    let fill = if selected {
        theme::SELECTED
    } else if hovered {
        theme::HOVER
    } else if i % 2 == 0 {
        // .Table.striped .TableRow:nth-child(odd)
        theme::STRIPE
    } else {
        Color32::TRANSPARENT
    };
    if fill != Color32::TRANSPARENT {
        p.rect_filled(rect, CornerRadius::ZERO, fill);
    }
    p.line_segment(
        [pos2(rect.left(), rect.bottom() - 0.5), pos2(rect.right(), rect.bottom() - 0.5)],
        Stroke::new(1.0, Color32::from_rgb(0x28, 0x2b, 0x30)),
    );
}

pub fn paint_cells(ui: &egui::Ui, rect: Rect, widths: &[f32], cells: &[Cell], now: i64) {
    let p = ui.painter();
    let mut x = rect.left();
    let font = FontId::proportional(13.0);
    for (ci, (cell, w)) in cells.iter().zip(widths).enumerate() {
        let inner_w = w - 20.0;
        let left = x + 12.0;
        let cy = rect.center().y;
        match cell {
            Cell::Text(t) => {
                let col = if ci == 0 { Color32::from_rgb(0xe4, 0xe6, 0xe8) } else { theme::TEXT_MID };
                let g = ellipsize(ui, t, font.clone(), col, inner_w);
                p.galley(pos2(left, cy - g.size().y / 2.0), g, col);
            }
            Cell::Status(t, tone) => {
                let g = ellipsize(ui, t, font.clone(), theme::tone(*tone), inner_w);
                p.galley(pos2(left, cy - g.size().y / 2.0), g, theme::tone(*tone));
            }
            Cell::Age(ts) => {
                p.text(pos2(left, cy), Align2::LEFT_CENTER, util::age(*ts, now), font.clone(), theme::TEXT_MID);
            }
            Cell::Meter(m) => paint_meter(ui, Rect::from_min_size(pos2(left, rect.top()), vec2(inner_w, rect.height())), m),
            Cell::Boxes(b) => {
                let mut bx = left;
                for t in b.iter().take(((inner_w / 12.0) as usize).max(1)) {
                    p.rect_filled(
                        Rect::from_min_size(pos2(bx, cy - 4.5), vec2(9.0, 9.0)),
                        CornerRadius::same(1),
                        theme::tone(*t),
                    );
                    bx += 12.0;
                }
            }
            Cell::Tags(tags) => {
                let mut tx = left;
                for (t, tone) in tags {
                    let g = ellipsize(ui, t, font.clone(), theme::tone(*tone), (left + inner_w - tx).max(10.0));
                    let gw = g.size().x;
                    p.galley(pos2(tx, cy - g.size().y / 2.0), g, theme::tone(*tone));
                    tx += gw + 10.0;
                    if tx > left + inner_w {
                        break;
                    }
                }
            }
        }
        x += w;
    }
}

pub fn notice(ui: &mut egui::Ui, text: &str, error: bool) {
    egui::Frame::new()
        .fill(if error { Color32::from_rgb(0x3a, 0x22, 0x22) } else { theme::PANEL })
        .stroke(Stroke::new(1.0, if error { Color32::from_rgb(0x6a, 0x2a, 0x28) } else { theme::BORDER }))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).color(if error { Color32::from_rgb(0xf0, 0xb4, 0xb0) } else { theme::TEXT_MID }));
        });
}

pub fn card(ui: &mut egui::Ui, w: f32, h: f32, title: &str, body: impl FnOnce(&mut egui::Ui, Rect)) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(6), if resp.hovered() { Color32::from_rgb(0x29, 0x2e, 0x33) } else { theme::PANEL });
    p.text(rect.left_top() + vec2(16.0, 18.0), Align2::LEFT_CENTER, title, FontId::proportional(14.0), theme::TEXT_HI);
    body(ui, rect);
    resp
}

/// The original's LineProgress in a nodes cell: a 5 px bar (requests layer at 50 % opacity under
/// the usage layer) with the small usage text right-aligned 4 px below it.
pub fn paint_meter(ui: &egui::Ui, cell: Rect, m: &crate::model::Meter) {
    let p = ui.painter();
    let font = FontId::proportional(theme::FONT_SIZE_SMALL);
    let g = ellipsize(ui, &m.text, font, theme::TEXT, cell.width());
    let bar_h = 5.0;
    let gap = 4.0;
    let total = if m.bar { bar_h + gap + g.size().y } else { g.size().y };
    let top = cell.center().y - total / 2.0;
    if m.bar {
        let bar = Rect::from_min_size(pos2(cell.left(), top), vec2(cell.width().max(10.0), bar_h));
        let col = theme::bar(m.color);
        p.rect_filled(bar, CornerRadius::same(2), theme::LINE_BG);
        if let Some(sec) = m.secondary {
            let mut r = bar;
            r.set_width(bar.width() * sec / 100.0);
            p.rect_filled(r, CornerRadius::same(2), col.gamma_multiply(0.5));
        }
        if let Some(v) = m.value {
            let mut r = bar;
            r.set_width(bar.width() * v / 100.0);
            p.rect_filled(r, CornerRadius::same(2), col);
        }
    }
    let ty = if m.bar { top + bar_h + gap } else { top };
    p.galley(pos2(cell.right() - g.size().x, ty), g, theme::TEXT);
}

/// Ring chart. Each segment is (fraction of the full circle, colour).
pub fn donut(p: &egui::Painter, c: Pos2, r: f32, thick: f32, segs: &[(f32, Color32)]) {
    p.circle_stroke(c, r, Stroke::new(thick, Color32::from_rgb(0x36, 0x3a, 0x40)));
    let mut start = -std::f32::consts::FRAC_PI_2;
    for (frac, col) in segs {
        let frac = frac.clamp(0.0, 1.0);
        if frac <= 0.0 {
            continue;
        }
        let sweep = frac * std::f32::consts::TAU;
        let n = ((sweep / 0.05) as usize).max(2);
        let pts: Vec<Pos2> = (0..=n)
            .map(|k| {
                let a = start + sweep * k as f32 / n as f32;
                c + vec2(a.cos(), a.sin()) * r
            })
            .collect();
        p.add(egui::Shape::line(pts, Stroke::new(thick, *col)));
        start += sweep;
    }
}

fn workload_stats(kind: Kind, rows: &[&Row]) -> Vec<(&'static str, f32, Color32)> {
    let mut ok = 0.0;
    let mut warn = 0.0;
    let mut err = 0.0;
    let mut done = 0.0;
    for r in rows {
        match kind {
            Kind::Pod => match pod_status(&r.obj).1 {
                Tone::Ok => ok += 1.0,
                Tone::Warn => warn += 1.0,
                Tone::Err => err += 1.0,
                _ => done += 1.0,
            },
            Kind::Job => {
                if util::i(&r.obj, "status/failed") > 0 {
                    err += 1.0
                } else if util::i(&r.obj, "status/succeeded") > 0 {
                    done += 1.0
                } else {
                    ok += 1.0
                }
            }
            Kind::CronJob => {
                if r.obj["spec"]["suspend"].as_bool().unwrap_or(false) {
                    warn += 1.0
                } else {
                    ok += 1.0
                }
            }
            Kind::DaemonSet => {
                if util::i(&r.obj, "status/numberReady") >= util::i(&r.obj, "status/desiredNumberScheduled") {
                    ok += 1.0
                } else {
                    warn += 1.0
                }
            }
            _ => {
                if util::i(&r.obj, "status/readyReplicas") >= util::i(&r.obj, "spec/replicas") {
                    ok += 1.0
                } else {
                    warn += 1.0
                }
            }
        }
    }
    match kind {
        Kind::Pod => vec![
            ("Running", ok, theme::GREEN),
            ("Pending", warn, theme::ORANGE),
            ("Failed", err, theme::RED),
            ("Succeeded", done, theme::GREY),
        ],
        Kind::Job => vec![("Running", ok, theme::GREEN), ("Succeeded", done, theme::GREY), ("Failed", err, theme::RED)],
        Kind::CronJob => vec![("Scheduled", ok, theme::GREEN), ("Suspended", warn, theme::ORANGE)],
        _ => vec![("Running", ok, theme::GREEN), ("Pending", warn, theme::ORANGE)],
    }
}

/// Compare strings so that "pod-10" sorts after "pod-9".
pub fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut ai, mut bi) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (ai.peek().copied(), bi.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let mut na = String::new();
                while let Some(c) = ai.peek().copied().filter(|c| c.is_ascii_digit()) {
                    na.push(c);
                    ai.next();
                }
                let mut nb = String::new();
                while let Some(c) = bi.peek().copied().filter(|c| c.is_ascii_digit()) {
                    nb.push(c);
                    bi.next();
                }
                let o = na.len().cmp(&nb.len()).then(na.cmp(&nb));
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            (Some(x), Some(y)) => {
                let o = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
                ai.next();
                bi.next();
            }
        }
    }
}
