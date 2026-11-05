//! Bottom dock (logs, YAML editor, command shell) and modal dialogs.

use eframe::egui;
use crate::app::{App, Dialog, DockKind, DockTab};
use crate::backend::Job;
use crate::model::Kind;
use crate::theme;
use crate::util::{arr, s};
use egui::{Align2, Color32, CornerRadius, FontId, RichText, Sense, Stroke, pos2, vec2};
use serde_json::Value;
use std::time::{Duration, Instant};

impl App {
    fn add_tab(&mut self, title: String, kind: DockKind) -> u64 {
        let id = self.next_tab;
        self.next_tab += 1;
        self.dock_tabs.push(DockTab { id, title, kind, busy: true });
        self.dock_active = id;
        self.dock_open = true;
        id
    }

    fn containers_of(pod: &Value) -> Vec<String> {
        arr(pod, "spec/containers").iter().map(|c| s(c, "name")).collect()
    }

    pub fn open_logs(&mut self, pod: &Value, container: Option<String>) {
        let ns = s(pod, "metadata/namespace");
        let name = s(pod, "metadata/name");
        let containers = Self::containers_of(pod);
        let container = container.or_else(|| containers.first().cloned()).unwrap_or_default();
        if let Some(t) = self.dock_tabs.iter().find(|t| matches!(&t.kind, DockKind::Logs { pod, container: c, .. } if *pod == name && *c == container)) {
            self.dock_active = t.id;
            self.dock_open = true;
            return;
        }
        let id = self.add_tab(
            format!("Logs: {name}"),
            DockKind::Logs {
                ns: ns.clone(),
                pod: name.clone(),
                containers,
                container: container.clone(),
                previous: false,
                follow: true,
                lines: vec![],
                plain: vec![],
                filter: String::new(),
                last: None,
                match_idx: 0,
                jump: false,
                timestamps: true,
            },
        );
        self.backend.submit(Job::Logs { tab: id, ns, pod: name, container, previous: false });
    }

    pub fn open_shell(&mut self, pod: &Value, container: Option<String>) {
        let ns = s(pod, "metadata/namespace");
        let name = s(pod, "metadata/name");
        let containers = Self::containers_of(pod);
        let container = container.or_else(|| containers.first().cloned()).unwrap_or_default();
        let windows = crate::util::map_pairs(pod, "spec/nodeSelector")
            .iter()
            .any(|(k, v)| k == "kubernetes.io/os" && v == "windows");
        let (term, error) = self.start_terminal(&ns, &name, &container, windows);
        let id = self.add_tab(
            // Tab title as in the original's execShell.
            format!("Pod: {name} (namespace: {ns})"),
            DockKind::Shell { ns, pod: name.clone(), containers, container: container.clone(), term, error },
        );
        if let Some(t) = self.dock_tabs.iter_mut().find(|t| t.id == id) {
            t.busy = false;
        }
    }

    /// A real terminal into the pod (or the simulated one for the sample cluster).
    fn start_terminal(&self, ns: &str, pod: &str, container: &str, windows: bool) -> (Option<crate::terminal::Terminal>, Option<String>) {
        match self.backend.context_name() {
            Some(ctx_name) => {
                match crate::terminal::Terminal::spawn_kubectl(&self.egui_ctx, &ctx_name, ns, pod, container, windows) {
                    Ok(t) => (Some(t), None),
                    Err(e) => (None, Some(e)),
                }
            }
            None => (Some(crate::terminal::Terminal::sample(pod, self.backend.sample_runner(pod, container))), None),
        }
    }

    pub fn open_edit(&mut self, kind: Kind, ns: &str, name: &str) {
        if let Some(t) = self.dock_tabs.iter().find(|t| matches!(&t.kind, DockKind::Edit { kind: k, ns: n, name: nm, .. } if *k == kind && n == ns && nm == name)) {
            self.dock_active = t.id;
            self.dock_open = true;
            return;
        }
        let id = self.add_tab(
            format!("{}: {}", kind.singular(), name),
            DockKind::Edit {
                kind,
                ns: ns.to_string(),
                name: name.to_string(),
                text: String::new(),
                original: String::new(),
                close_after_save: false,
                message: None,
            },
        );
        self.backend.submit(Job::Yaml { tab: id, kind, ns: ns.to_string(), name: name.to_string() });
    }

    pub fn dock(&mut self, ui: &mut egui::Ui) {
        // Tab strip
        let (strip, _) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::hover());
        ui.painter().rect_filled(strip, CornerRadius::ZERO, theme::DOCK_HEAD);
        let mut x = strip.left() + 6.0;
        let mut close = None;
        let mut activate = None;
        for t in &self.dock_tabs {
            let icon = match t.kind {
                DockKind::Logs { .. } => "🗎",
                DockKind::Edit { .. } => "✎",
                DockKind::Shell { .. } => ">_",
            };
            let label = format!("{icon}  {}", t.title);
            let g = crate::app::ellipsize(ui, &label, FontId::proportional(12.5), theme::TEXT_HI, 240.0);
            let w = g.size().x + 44.0;
            let r = egui::Rect::from_min_size(pos2(x, strip.top() + 3.0), vec2(w, strip.height() - 3.0));
            let resp = ui.interact(r, ui.scope_id().with(("tab", t.id)), Sense::click());
            let active = t.id == self.dock_active && self.dock_open;
            ui.painter().rect_filled(
                r,
                CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 },
                if active { theme::DOCK_TAB_ACTIVE } else if resp.hovered() { theme::HOVER } else { Color32::TRANSPARENT },
            );
            if active {
                ui.painter().line_segment([r.left_top() + vec2(4.0, 0.0), r.right_top() - vec2(4.0, 0.0)], Stroke::new(2.0, theme::BLUE));
            }
            let col = if active { theme::TEXT_HI } else { theme::TEXT_MID };
            ui.painter().galley(pos2(r.left() + 12.0, r.center().y - g.size().y / 2.0), g, col);
            let cr = egui::Rect::from_center_size(pos2(r.right() - 14.0, r.center().y), vec2(18.0, 18.0));
            let cresp = ui.interact(cr, ui.scope_id().with(("tabx", t.id)), Sense::click());
            if cresp.hovered() {
                ui.painter().rect_filled(cr, CornerRadius::same(3), theme::HOVER);
            }
            ui.painter().text(cr.center(), Align2::CENTER_CENTER, "✕", FontId::proportional(11.0), theme::TEXT_MID);
            if cresp.clicked() {
                close = Some(t.id);
            } else if resp.clicked() {
                activate = Some(t.id);
            }
            x += w + 2.0;
        }
        // Minimise button on the right
        let mr = egui::Rect::from_center_size(pos2(strip.right() - 18.0, strip.center().y), vec2(24.0, 22.0));
        let mresp = ui.interact(mr, ui.scope_id().with("dockmin"), Sense::click());
        if mresp.hovered() {
            ui.painter().rect_filled(mr, CornerRadius::same(3), theme::HOVER);
        }
        let open = self.dock_open;
        ui.painter().text(mr.center(), Align2::CENTER_CENTER, if open { "⏷" } else { "⏶" }, FontId::proportional(13.0), theme::TEXT_MID);
        if mresp.on_hover_text(if open { "Minimise" } else { "Open" }).clicked() {
            self.dock_open = !open;
        }
        if let Some(id) = activate {
            self.dock_active = id;
            self.dock_open = true;
        }
        if let Some(id) = close {
            self.close_tab(id);
            return;
        }
        if !open {
            return;
        }

        let Some(ti) = self.dock_tabs.iter().position(|t| t.id == self.dock_active) else { return };
        let mut job: Option<Job> = None;
        let mut close_me = false;
        let tab_id = self.dock_tabs[ti].id;
        let busy = self.dock_tabs[ti].busy;
        let mut set_busy = false;
        let mut shell_restart: Option<(String, String, String)> = None;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            match &mut self.dock_tabs[ti].kind {
                DockKind::Logs { ns, pod, containers, container, previous, follow, lines, plain, filter, last, match_idx, jump, timestamps } => {
                    let mut refresh = false;
                    let q = filter.to_lowercase();
                    // Like the original's search: every line stays, matches are highlighted and
                    // ↑/↓ (or Enter) move between them.
                    let matches: Vec<usize> = if q.is_empty() {
                        vec![]
                    } else {
                        plain.iter().enumerate().filter(|(_, l)| l.contains(&q)).map(|(i, _)| i).collect()
                    };
                    if *match_idx >= matches.len() {
                        *match_idx = 0;
                    }
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Pod").color(theme::TEXT));
                        ui.label(RichText::new(pod.as_str()).color(theme::TEXT_HI));
                        ui.add_space(8.0);
                        ui.label(RichText::new("Container").color(theme::TEXT));
                        let before = container.clone();
                        egui::ComboBox::from_id_salt(("logc", tab_id)).selected_text(container.as_str()).show_ui(ui, |ui| {
                            for c in containers.iter() {
                                ui.selectable_value(container, c.clone(), c);
                            }
                        });
                        if *container != before {
                            refresh = true;
                        }
                        if ui.checkbox(previous, "Previous container").changed() {
                            refresh = true;
                        }
                        ui.checkbox(timestamps, "Show timestamps");
                        ui.checkbox(follow, "Auto-refresh");
                        if ui.button("⟳ Refresh").clicked() {
                            refresh = true;
                        }
                        if busy {
                            ui.spinner();
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let n = matches.len();
                            if ui.add_enabled(n > 0, egui::Button::new("↓")).on_hover_text("Next match").clicked() {
                                *match_idx = (*match_idx + 1) % n.max(1);
                                *jump = true;
                            }
                            if ui.add_enabled(n > 0, egui::Button::new("↑")).on_hover_text("Previous match").clicked() {
                                *match_idx = (*match_idx + n.max(1) - 1) % n.max(1);
                                *jump = true;
                            }
                            if !q.is_empty() {
                                let label = if n == 0 { "0 / 0".to_string() } else { format!("{} / {}", *match_idx + 1, n) };
                                ui.label(RichText::new(label).color(theme::TEXT));
                            }
                            let r = ui.add(egui::TextEdit::singleline(filter).hint_text("Search logs…").desired_width(200.0));
                            if r.changed() {
                                *match_idx = 0;
                                *jump = true;
                            }
                            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && n > 0 {
                                *match_idx = (*match_idx + 1) % n;
                                *jump = true;
                                r.request_focus();
                            }
                        });
                    });
                    if *follow && !busy && last.map(|t| t.elapsed() > Duration::from_secs(3)).unwrap_or(false) {
                        refresh = true;
                    }
                    if refresh && !busy {
                        set_busy = true;
                        *last = Some(Instant::now());
                        job = Some(Job::Logs { tab: tab_id, ns: ns.clone(), pod: pod.clone(), container: container.clone(), previous: *previous });
                    }
                    ui.add_space(6.0);
                    let active_line = matches.get(*match_idx).copied();
                    terminal_frame(ui, |ui| {
                        // .LogRow: 18 px rows, monospace, "smaller" text.
                        let h = 18.0;
                        let mut area = egui::ScrollArea::both()
                            .id_salt(("logs", tab_id))
                            .auto_shrink([false, false])
                            .stick_to_bottom(q.is_empty());
                        if *jump {
                            if let Some(l) = active_line {
                                let view_h = ui.available_height();
                                area = area.vertical_scroll_offset((l as f32 * h - view_h / 2.0).max(0.0));
                            }
                            *jump = false;
                        }
                        area.show_rows(ui, h, lines.len(), |ui, range| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for i in range {
                                log_line(ui, &lines[i], &q, active_line == Some(i), *timestamps, h);
                            }
                        });
                    });
                }
                DockKind::Edit { kind, ns, name, text, original, close_after_save, message } => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{}  {}", kind.singular(), name)).color(theme::TEXT_HI));
                        if !ns.is_empty() {
                            ui.label(RichText::new(format!("namespace {ns}")).color(theme::TEXT));
                        }
                        if text != original {
                            ui.label(RichText::new("● modified").color(theme::ORANGE));
                        }
                        if busy {
                            ui.spinner();
                        }
                        if let Some((m, err)) = message {
                            ui.label(RichText::new(m.as_str()).color(if *err { theme::RED } else { theme::GREEN }));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Cancel").clicked() {
                                close_me = true;
                            }
                            let can = !busy && text != original;
                            if ui.add_enabled(can, egui::Button::new("Save & Close").fill(theme::BLUE)).clicked() {
                                *close_after_save = true;
                                set_busy = true;
                                job = Some(Job::Replace { tab: tab_id, kind: *kind, ns: ns.clone(), name: name.clone(), text: text.clone() });
                            }
                            if ui.add_enabled(can, egui::Button::new("Save")).clicked() {
                                *close_after_save = false;
                                set_busy = true;
                                job = Some(Job::Replace { tab: tab_id, kind: *kind, ns: ns.clone(), name: name.clone(), text: text.clone() });
                            }
                        });
                    });
                    ui.add_space(6.0);
                    terminal_frame(ui, |ui| {
                        egui::ScrollArea::both().id_salt(("edit", tab_id)).auto_shrink([false, false]).show(ui, |ui| {
                            ui.add(
                                egui::TextEdit::multiline(text)
                                    .code_editor()
                                    .frame(egui::Frame::NONE)
                                    .desired_width(f32::INFINITY)
                                    .desired_rows(20)
                                    .text_color(theme::TERMINAL_TEXT),
                            );
                        });
                    });
                }
                DockKind::Shell { ns, pod, containers, container, term, error } => {
                    let mut restart = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Pod").color(theme::TEXT));
                        ui.label(RichText::new(pod.as_str()).color(theme::TEXT_HI));
                        ui.label(RichText::new("Container").color(theme::TEXT));
                        let before = container.clone();
                        egui::ComboBox::from_id_salt(("shc", tab_id)).selected_text(container.as_str()).show_ui(ui, |ui| {
                            for c in containers.iter() {
                                ui.selectable_value(container, c.clone(), c);
                            }
                        });
                        if *container != before {
                            restart = true;
                        }
                        let ended = term.as_ref().map(|t| t.exited()).unwrap_or(true);
                        if ended && ui.button("⟳ Reconnect").clicked() {
                            restart = true;
                        }
                        if let Some(t) = term.as_ref() {
                            if t.exited() {
                                ui.label(RichText::new("session ended").color(theme::ORANGE));
                            }
                            if !t.status.is_empty() {
                                ui.label(RichText::new(t.status.as_str()).color(theme::TEXT));
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                RichText::new("Ctrl+C copies the selection · Ctrl+V pastes · click a link to open it")
                                    .size(theme::FONT_SIZE_SMALL)
                                    .color(theme::TEXT),
                            );
                        });
                    });
                    ui.add_space(6.0);
                    if let Some(e) = error.as_ref() {
                        ui.label(RichText::new(e.as_str()).color(theme::RED));
                    }
                    if let Some(t) = term.as_mut() {
                        t.ui(ui);
                    }
                    if restart {
                        shell_restart = Some((ns.clone(), pod.clone(), container.clone()));
                    }
                }
            }
        });
        if set_busy {
            self.dock_tabs[ti].busy = true;
        }
        if let Some((ns, pod, container)) = shell_restart {
            let (t, e) = self.start_terminal(&ns, &pod, &container, false);
            if let DockKind::Shell { term, error, .. } = &mut self.dock_tabs[ti].kind {
                *term = t;
                *error = e;
            }
        }
        if let Some(j) = job {
            self.backend.submit(j);
        }
        if close_me {
            self.close_tab(tab_id);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(d) = self.dialog.as_mut() else { return };
        let mut done: Option<Option<Job>> = None;
        let modal = egui::Modal::new(egui::Id::unique("dialog")).frame(
            egui::Frame::new()
                .fill(theme::PANEL)
                .stroke(Stroke::new(1.0, theme::BORDER))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(egui::Margin::same(22)),
        );
        let resp = modal.show(ctx, |ui| {
            ui.set_width(420.0);
            match d {
                Dialog::Delete { kind, ns, name } => {
                    ui.label(RichText::new(format!("Delete {}", kind.singular())).size(17.0).color(theme::TEXT_HI));
                    ui.add_space(10.0);
                    let where_ = if ns.is_empty() { String::new() } else { format!(" from namespace {ns}") };
                    ui.label(RichText::new(format!("Remove {} {}{}?", kind.singular(), name, where_)).color(theme::TEXT_MID));
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(egui::Button::new(RichText::new("Remove").color(theme::TEXT_HI)).fill(theme::SOFT_RED)).clicked() {
                                done = Some(Some(Job::Delete { kind: *kind, ns: ns.clone(), name: name.clone() }));
                            }
                            if ui.button("Cancel").clicked() {
                                done = Some(None);
                            }
                        });
                    });
                }
                Dialog::Restart { kind, ns, name } => {
                    ui.label(RichText::new(format!("Restart {}", kind.singular())).size(17.0).color(theme::TEXT_HI));
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Restart all pods of {name}? They are replaced one by one.")).color(theme::TEXT_MID));
                    ui.add_space(16.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("Restart").color(theme::TEXT_HI)).fill(theme::BLUE)).clicked() {
                            done = Some(Some(Job::Restart { kind: *kind, ns: ns.clone(), name: name.clone() }));
                        }
                        if ui.button("Cancel").clicked() {
                            done = Some(None);
                        }
                    });
                }
                Dialog::Scale { kind, ns, name, current, desired } => {
                    ui.label(RichText::new(format!("Scale {} {}", kind.singular(), name)).size(17.0).color(theme::TEXT_HI));
                    ui.add_space(10.0);
                    ui.label(RichText::new(format!("Current replica scale: {current}")).color(theme::TEXT_MID));
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Desired number of replicas: {desired}")).color(theme::TEXT_MID));
                    });
                    ui.horizontal(|ui| {
                        let max = (*current * 2).max(10);
                        ui.spacing_mut().slider_width = 300.0;
                        ui.add(egui::Slider::new(desired, 0..=max).show_value(false));
                        if ui.small_button("−").clicked() && *desired > 0 {
                            *desired -= 1;
                        }
                        if ui.small_button("+").clicked() {
                            *desired += 1;
                        }
                    });
                    if *desired == 0 {
                        ui.label(RichText::new("All pods will be removed.").color(theme::ORANGE));
                    }
                    ui.add_space(16.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new("Scale").color(theme::TEXT_HI)).fill(theme::BLUE)).clicked() {
                            done = Some(Some(Job::Scale { kind: *kind, ns: ns.clone(), name: name.clone(), replicas: *desired }));
                        }
                        if ui.button("Cancel").clicked() {
                            done = Some(None);
                        }
                    });
                }
            }
        });
        if resp.should_close() && done.is_none() {
            done = Some(None);
        }
        if let Some(j) = done {
            self.dialog = None;
            if let Some(j) = j {
                if let Job::Delete { kind, name, .. } = &j {
                    if self.selected.as_ref().map(|s| s.0 == *kind).unwrap_or(false) {
                        let rows = self.rows(*kind);
                        if rows.iter().any(|r| Some(&r.uid) == self.selected.as_ref().map(|s| &s.1) && r.name == *name) {
                            self.selected = None;
                        }
                    }
                }
                self.backend.submit(j);
            }
        }
    }
}

fn terminal_frame(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::TERMINAL)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_height(ui.available_height());
            add(ui)
        });
}

/// One log row: ANSI colour codes become coloured text (ansi_up rules, see ansi.rs); search
/// matches get the original's overlay colours (#8cc474b8, the active one orange).
fn log_line(ui: &mut egui::Ui, raw: &str, query: &str, active: bool, timestamps: bool, h: f32) {
    let font = FontId::monospace(theme::FONT_SIZE_SMALL);
    let (ts, rest) = match raw.split_once(' ') {
        Some((a, b)) if a.len() >= 20 && a.as_bytes().get(4) == Some(&b'-') && a.contains('T') => (a, b),
        _ => ("", raw),
    };
    let mut pieces: Vec<(String, crate::ansi::Style)> = vec![];
    if timestamps && !ts.is_empty() {
        pieces.push((format!("{ts} "), crate::ansi::Style { fg: Some(theme::GREY), ..Default::default() }));
    }
    let mut style = crate::ansi::Style::default();
    pieces.extend(crate::ansi::parse(rest, &mut style));

    // Plain lines (no colour codes) keep a gentle hint for errors and warnings (an addition;
    // coloured lines are shown exactly as the program wrote them).
    let coloured = rest.contains('\u{1b}');
    let lower = rest.to_lowercase();
    let hint = if coloured {
        None
    } else if lower.contains("error") || lower.contains("fatal") || lower.contains("panic") {
        Some(Color32::from_rgb(0xf0, 0x80, 0x78))
    } else if lower.contains("warn") {
        Some(theme::YELLOW)
    } else {
        None
    };

    // Search highlight ranges, in characters of the plain text.
    let plain: String = pieces.iter().map(|p| p.0.as_str()).collect();
    let plain_lower: Vec<char> = plain.to_lowercase().chars().collect();
    let q: Vec<char> = query.chars().collect();
    let mut hl = vec![false; plain_lower.len()];
    if !q.is_empty() && q.len() <= plain_lower.len() {
        let mut i = 0;
        while i + q.len() <= plain_lower.len() {
            if plain_lower[i..i + q.len()] == q[..] {
                for x in hl.iter_mut().skip(i).take(q.len()) {
                    *x = true;
                }
                i += q.len();
            } else {
                i += 1;
            }
        }
    }
    let overlay = if active { theme::ORANGE } else { Color32::from_rgba_unmultiplied(0x8c, 0xc4, 0x74, 0xb8) };

    let mut job = egui::text::LayoutJob::default();
    let mut pos = 0usize;
    for (text, st) in pieces {
        let mut fg = st.fg.unwrap_or(hint.unwrap_or(theme::TERMINAL_TEXT));
        if st.faint {
            fg = fg.gamma_multiply(0.6);
        }
        let mut fmt = egui::text::TextFormat::simple(font.clone(), fg);
        fmt.italics = st.italic;
        if st.underline {
            fmt.underline = egui::Stroke::new(1.0, fg);
        }
        if let Some(bg) = st.bg {
            fmt.background = bg;
        }
        // Split the piece where the highlight starts or stops.
        let chars: Vec<char> = text.chars().collect();
        let mut start = 0;
        while start < chars.len() {
            let on = hl.get(pos + start).copied().unwrap_or(false);
            let mut end = start;
            while end < chars.len() && hl.get(pos + end).copied().unwrap_or(false) == on {
                end += 1;
            }
            let seg: String = chars[start..end].iter().collect();
            let mut f = fmt.clone();
            if on {
                f.background = overlay;
            }
            if st.bold && st.fg.is_none() {
                // egui has no bold monospace face; bold default-coloured text is drawn bright
                // white so it still stands out (ansi_up uses font-weight: bold).
                f.color = Color32::WHITE;
            }
            job.append(&seg, 0.0, f);
            start = end;
        }
        pos += chars.len();
    }
    job.wrap.max_width = f32::INFINITY;
    let galley = ui.painter().layout_job(job);
    let w = galley.size().x + 32.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w.max(ui.available_width()), h), Sense::hover());
    if resp.hovered() {
        ui.painter().rect_filled(rect, CornerRadius::ZERO, Color32::from_rgb(0x35, 0x37, 0x3a)); // logRowHoverBackground
    }
    // .LogRow padding: 2px 16px
    ui.painter().galley(pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0), galley, theme::TERMINAL_TEXT);
}
