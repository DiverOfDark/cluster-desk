//! A real terminal for pod shells: a pseudo-terminal running `kubectl exec -it`, an xterm-style
//! screen emulator (vt100) and an egui renderer with colours, cursor, scrollback, selection,
//! copy/paste and resize. The sample cluster gets a small simulated shell drawn the same way.

use crate::theme;
use eframe::egui;
use egui::{Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// xterm.js default scrollback (the original doesn't override it).
const SCROLLBACK: usize = 1000;

/// The original's terminal palette ("terminalColors" in its Dark theme).
const ANSI: [u32; 16] = [
    0x2e3436, 0xcc0000, 0x4e9a06, 0xc4a000, 0x3465a4, 0x75507b, 0x06989a, 0xd3d7cf, // normal
    0x555753, 0xef2929, 0x8ae234, 0xfce94f, 0x729fcf, 0xad7fa8, 0x34e2e2, 0xeeeeec, // bright
];
const FG: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const BG: Color32 = Color32::from_rgb(0x00, 0x00, 0x00);
const CURSOR: Color32 = Color32::from_rgb(0xff, 0xff, 0xff);
const SELECTION: Color32 = Color32::from_rgb(0x26, 0x4f, 0x78);

fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// xterm 256-colour table.
fn indexed(i: u8) -> Color32 {
    match i {
        0..=15 => rgb(ANSI[i as usize]),
        16..=231 => {
            let i = i - 16;
            let steps = [0u8, 95, 135, 175, 215, 255];
            Color32::from_rgb(steps[(i / 36) as usize], steps[((i / 6) % 6) as usize], steps[(i % 6) as usize])
        }
        _ => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

fn color(c: vt100::Color, default: Color32, bold: bool) -> Color32 {
    match c {
        vt100::Color::Default => default,
        // Bold text in one of the 8 basic colours uses the bright variant, like xterm.
        vt100::Color::Idx(i) if bold && i < 8 => indexed(i + 8),
        vt100::Color::Idx(i) => indexed(i),
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
    }
}

/// The simulated shell of the sample cluster.
struct SampleShell {
    line: String,
    prompt: String,
    run: Box<dyn Fn(&str) -> String + Send>,
}

pub struct Terminal {
    parser: Arc<Mutex<vt100::Parser>>,
    master: Option<Box<dyn MasterPty + Send>>,
    writer: Option<Box<dyn Write + Send>>,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    exited: Arc<AtomicBool>,
    sample: Option<SampleShell>,
    size: (u16, u16),
    scroll: usize,
    sel_start: Option<(i64, u16)>,
    sel_end: Option<(i64, u16)>,
    clipboard: Option<arboard::Clipboard>,
    pub status: String,
}

fn nz(v: u16) -> NonZeroU16 {
    NonZeroU16::new(v.max(1)).unwrap_or(NonZeroU16::MIN)
}

impl Terminal {
    fn new_parser(rows: u16, cols: u16) -> Arc<Mutex<vt100::Parser>> {
        Arc::new(Mutex::new(vt100::Parser::new(nz(rows), nz(cols), SCROLLBACK)))
    }

    /// `kubectl exec -i -t … -- sh -c "clear; (bash || ash || sh)"`, the command the original
    /// uses, with TERM and a UTF-8 locale set so colours, line drawing and emoji work.
    pub fn spawn_kubectl(
        egui_ctx: &egui::Context,
        context: &str,
        ns: &str,
        pod: &str,
        container: &str,
        windows: bool,
    ) -> Result<Self, String> {
        let (rows, cols) = (24u16, 100u16);
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Could not open a terminal: {e}"))?;
        let mut cmd = CommandBuilder::new(crate::settings::kubectl_for(Some(context)));
        for (k, v) in crate::settings::env_pairs_for(Some(context)) {
            cmd.env(k, v);
        }
        cmd.args([
            "--context",
            context,
            "exec",
            "-i",
            "-t",
            "-n",
            ns,
            pod,
            "-c",
            container,
            "--",
        ]);
        if windows {
            // Pods scheduled on Windows nodes get PowerShell, as in the original.
            cmd.arg("powershell");
        } else {
            cmd.args(["sh", "-c", "export TERM=xterm-256color; export LANG=${LANG:-C.UTF-8}; clear; (bash || ash || sh)"]);
        }
        cmd.env("TERM", "xterm-256color");
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }
        let child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| format!("Could not run kubectl ({e}). Is kubectl installed?"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let parser = Self::new_parser(rows, cols);
        let exited = Arc::new(AtomicBool::new(false));
        {
            let parser = parser.clone();
            let exited = exited.clone();
            let ctx = egui_ctx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 16384];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                exited.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            });
        }
        Ok(Terminal {
            parser,
            master: Some(pair.master),
            writer: Some(writer),
            child: Some(child),
            exited,
            sample: None,
            size: (rows, cols),
            scroll: 0,
            sel_start: None,
            sel_end: None,
            clipboard: arboard::Clipboard::new().ok(),
            status: String::new(),
        })
    }

    /// Run any program in a terminal (used to sign in to a cluster interactively).
    pub fn spawn_program(egui_ctx: &egui::Context, program: &str, args: &[String]) -> Result<Self, String> {
        let (rows, cols) = (24u16, 100u16);
        let pair = native_pty_system()
            .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
            .map_err(|e| format!("Could not open a terminal: {e}"))?;
        let mut cmd = CommandBuilder::new(program);
        cmd.args(args);
        cmd.env("TERM", "xterm-256color");
        let context = args.iter().position(|a| a == "--context").and_then(|i| args.get(i + 1)).map(|s| s.as_str());
        for (k, v) in crate::settings::env_pairs_for(context) {
            cmd.env(k, v);
        }
        if let Ok(cwd) = std::env::current_dir() {
            cmd.cwd(cwd);
        }
        let child = pair.slave.spawn_command(cmd).map_err(|e| format!("Could not run {program} ({e})"))?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
        let parser = Self::new_parser(rows, cols);
        let exited = Arc::new(AtomicBool::new(false));
        {
            let parser = parser.clone();
            let exited = exited.clone();
            let ctx = egui_ctx.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 16384];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if let Ok(mut p) = parser.lock() {
                                p.process(&buf[..n]);
                            }
                            ctx.request_repaint();
                        }
                    }
                }
                if let Ok(mut p) = parser.lock() {
                    p.process(b"\r\n\x1b[2m[finished]\x1b[0m\r\n");
                }
                exited.store(true, Ordering::SeqCst);
                ctx.request_repaint();
            });
        }
        Ok(Terminal {
            parser,
            master: Some(pair.master),
            writer: Some(writer),
            child: Some(child),
            exited,
            sample: None,
            size: (rows, cols),
            scroll: 0,
            sel_start: None,
            sel_end: None,
            clipboard: arboard::Clipboard::new().ok(),
            status: String::new(),
        })
    }

    /// A simulated shell for the sample cluster; `run` answers one command line.
    pub fn sample(pod: &str, run: Box<dyn Fn(&str) -> String + Send>) -> Self {
        let parser = Self::new_parser(24, 100);
        let prompt = format!("\x1b[1;32mapp@{pod}\x1b[0m:\x1b[1;34m/app\x1b[0m$ ");
        {
            let mut p = parser.lock().unwrap();
            p.process(
                "\x1b[2mSample cluster: a simulated shell. Try \x1b[0mls\x1b[2m, \x1b[0mtop\x1b[2m, \x1b[0mcolors\x1b[2m or \x1b[0memoji\x1b[2m (docs: https://kubernetes.io/docs/).\x1b[0m\r\n"
                    .as_bytes(),
            );
            p.process(prompt.as_bytes());
        }
        Terminal {
            parser,
            master: None,
            writer: None,
            child: None,
            exited: Arc::new(AtomicBool::new(false)),
            sample: Some(SampleShell { line: String::new(), prompt, run }),
            size: (24, 100),
            scroll: 0,
            sel_start: None,
            sel_end: None,
            clipboard: arboard::Clipboard::new().ok(),
            status: String::new(),
        }
    }

    /// Type text as if from the keyboard (used by the developer test pictures).
    pub fn inject(&mut self, s: &str) {
        self.send(s.as_bytes());
    }

    pub fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }

    fn send(&mut self, bytes: &[u8]) {
        self.scroll = 0;
        if let Some(sh) = &mut self.sample {
            let mut echo = Vec::new();
            for ch in String::from_utf8_lossy(bytes).chars() {
                match ch {
                    '\r' | '\n' => {
                        echo.extend_from_slice(b"\r\n");
                        let cmd = std::mem::take(&mut sh.line);
                        if cmd.trim() == "clear" {
                            echo.extend_from_slice(b"\x1b[2J\x1b[H");
                        } else if cmd.trim() == "exit" {
                            echo.extend_from_slice(b"exit\r\n");
                            self.exited.store(true, Ordering::SeqCst);
                        } else {
                            let out = (sh.run)(&cmd);
                            echo.extend_from_slice(out.replace('\n', "\r\n").as_bytes());
                        }
                        if !self.exited.load(Ordering::SeqCst) {
                            echo.extend_from_slice(sh.prompt.as_bytes());
                        }
                    }
                    '\x7f' | '\x08' => {
                        if let Some(c) = sh.line.pop() {
                            let w = if unicode_wide(c) { 2 } else { 1 };
                            for _ in 0..w {
                                echo.extend_from_slice(b"\x08 \x08");
                            }
                        }
                    }
                    '\x03' => {
                        sh.line.clear();
                        echo.extend_from_slice(b"^C\r\n");
                        echo.extend_from_slice(sh.prompt.as_bytes());
                    }
                    '\x0c' => {
                        echo.extend_from_slice(b"\x1b[2J\x1b[H");
                        echo.extend_from_slice(sh.prompt.as_bytes());
                        echo.extend_from_slice(sh.line.as_bytes());
                    }
                    c if c == '\x1b' || (c as u32) < 0x20 => {}
                    c => {
                        sh.line.push(c);
                        let mut b = [0u8; 4];
                        echo.extend_from_slice(c.encode_utf8(&mut b).as_bytes());
                    }
                }
            }
            if let Ok(mut p) = self.parser.lock() {
                p.process(&echo);
            }
            return;
        }
        if let Some(w) = &mut self.writer {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        if (rows, cols) == self.size || rows < 2 || cols < 10 {
            return;
        }
        self.size = (rows, cols);
        if let Ok(mut p) = self.parser.lock() {
            p.screen_mut().set_size(nz(rows), nz(cols));
        }
        if let Some(m) = &self.master {
            let _ = m.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        }
    }

    /// Draw the terminal into all available space and handle keyboard/mouse input.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let font = FontId::monospace(crate::settings::get().terminal_font_size.clamp(10.0, 50.0));
        let m = ui.painter().layout_no_wrap("M".into(), font.clone(), FG);
        let (cw, ch) = (m.size().x.max(4.0), m.size().y.max(8.0).ceil());
        let avail = ui.available_size();
        let (rect, resp) = ui.allocate_exact_size(avail, Sense::click_and_drag());
        let id = resp.id;
        let inner = rect.shrink2(vec2(6.0, 4.0));
        let rows = (inner.height() / ch).floor().max(2.0) as u16;
        let cols = (inner.width() / cw).floor().max(10.0) as u16;
        self.resize(rows, cols);

        if resp.clicked() || resp.drag_started() {
            resp.request_focus();
        }
        let focused = resp.has_focus();
        if focused {
            ui.memory_mut(|mem| {
                mem.set_focus_lock_filter(
                    id,
                    egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true },
                )
            });
            self.handle_keys(ui);
        }

        // Mouse wheel: scroll back through history (the program gets arrow keys on the
        // alternate screen, e.g. in less or vim).
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy.abs() > 0.5 {
                let alt = self.parser.lock().map(|p| p.screen().alternate_screen()).unwrap_or(false);
                let lines = (dy / ch).round() as i64;
                if alt {
                    let key: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    for _ in 0..lines.unsigned_abs().min(10) {
                        self.send(key);
                    }
                } else if lines != 0 {
                    self.scroll = (self.scroll as i64 + lines).clamp(0, SCROLLBACK as i64) as usize;
                }
            }
        }

        // Selection with the mouse; releasing copies, like most Linux terminals.
        let cell_at = |p: Pos2, scroll: usize| -> (i64, u16) {
            let r = ((p.y - inner.top()) / ch).floor() as i64;
            let c = ((p.x - inner.left()) / cw).floor().clamp(0.0, cols as f32 - 1.0) as u16;
            (r - scroll as i64, c)
        };
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.sel_start = Some(cell_at(p, self.scroll));
                self.sel_end = self.sel_start;
            }
        } else if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.sel_end = Some(cell_at(p, self.scroll));
            }
        } else if resp.drag_stopped() {
            // terminalCopyOnSelect (off by default): when on, selecting copies.
            if crate::settings::get().terminal_copy_on_select {
                if let Some(text) = self.selection_text() {
                    ui.ctx().copy_text(text);
                }
            }
        } else if resp.clicked() {
            self.sel_start = None;
            self.sel_end = None;
            // Web links addon: clicking a URL opens it in the browser.
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some(url) = self.url_at(cell_at(p, self.scroll)) {
                    if let Err(e) = crate::util::open_url(&url) {
                        self.status = e;
                    }
                }
            }
        }
        // Right-click pastes only when copy-on-select is enabled (as in the original).
        if resp.secondary_clicked() && crate::settings::get().terminal_copy_on_select {
            if let Some(text) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) {
                self.paste(&text);
            }
        }
        let over_link = resp.hover_pos().and_then(|p| self.url_at(cell_at(p, self.scroll))).is_some();
        resp.on_hover_cursor(if over_link { egui::CursorIcon::PointingHand } else { egui::CursorIcon::Text });

        self.paint(ui, rect, inner, cw, ch, &font, focused);
    }

    fn paste(&mut self, text: &str) {
        let bracketed = self.parser.lock().map(|p| p.screen().bracketed_paste()).unwrap_or(false);
        let text = text.replace("\r\n", "\r").replace('\n', "\r");
        if bracketed {
            self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        } else {
            self.send(text.as_bytes());
        }
    }

    /// The http(s) URL under a cell, if any (what the web-links addon underlines and opens).
    fn url_at(&self, (row, col): (i64, u16)) -> Option<String> {
        if row < 0 || row >= self.size.0 as i64 {
            return None;
        }
        let p = self.parser.lock().ok()?;
        let line = p.screen().contents_between(row as u16, 0, row as u16, self.size.1);
        let chars: Vec<char> = line.chars().collect();
        let col = col as usize;
        let mut start = 0;
        while start < chars.len() {
            let rest: String = chars[start..].iter().collect();
            let found = rest.find("http://").or_else(|| rest.find("https://"))?;
            let s_idx = start + rest[..found].chars().count();
            let mut e_idx = s_idx;
            while e_idx < chars.len() && !chars[e_idx].is_whitespace() && !"\"'<>`".contains(chars[e_idx]) {
                e_idx += 1;
            }
            while e_idx > s_idx && ".,;:!?)]}".contains(chars[e_idx - 1]) {
                e_idx -= 1;
            }
            if col >= s_idx && col < e_idx {
                return Some(chars[s_idx..e_idx].iter().collect());
            }
            start = e_idx.max(s_idx + 1);
        }
        None
    }

    fn selection_text(&self) -> Option<String> {
        let (a, b) = (self.sel_start?, self.sel_end?);
        if a == b {
            return None;
        }
        let (s, e) = if a <= b { (a, b) } else { (b, a) };
        let p = self.parser.lock().ok()?;
        let screen = p.screen();
        let rows = self.size.0 as i64;
        let (sr, er) = (s.0.clamp(0, rows - 1) as u16, e.0.clamp(0, rows - 1) as u16);
        let text = screen.contents_between(sr, s.1, er, e.1 + 1);
        if text.trim().is_empty() { None } else { Some(text) }
    }

    fn handle_keys(&mut self, ui: &egui::Ui) {
        let events = ui.input(|i| i.events.clone());
        let app_cursor = self.parser.lock().map(|p| p.screen().application_cursor()).unwrap_or(false);
        for e in events {
            match e {
                egui::Event::Text(t) => self.send(t.as_bytes()),
                egui::Event::Paste(t) => self.paste(&t),
                // Original key handler: Ctrl+C copies when text is selected, otherwise it goes to
                // the shell as an interrupt. Ctrl+X reaches us as a clipboard command too.
                egui::Event::Copy => match self.selection_text() {
                    Some(t) => {
                        ui.ctx().copy_text(t);
                        self.sel_start = None;
                        self.sel_end = None;
                    }
                    None => self.send(b"\x03"),
                },
                egui::Event::Cut => self.send(b"\x18"),
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    if let Some(bytes) = key_bytes(key, modifiers, app_cursor) {
                        if bytes == b"PASTE" {
                            if let Some(t) = self.clipboard.as_mut().and_then(|c| c.get_text().ok()) {
                                self.paste(&t);
                            }
                        } else {
                            self.send(&bytes);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn paint(&mut self, ui: &egui::Ui, rect: Rect, inner: Rect, cw: f32, ch: f32, font: &FontId, focused: bool) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(3), BG);
        let Ok(mut parser) = self.parser.lock() else { return };
        parser.screen_mut().set_scrollback(self.scroll);
        self.scroll = parser.screen().scrollback();
        let screen = parser.screen();
        let (rows, cols) = self.size;
        let sel = match (self.sel_start, self.sel_end) {
            (Some(a), Some(b)) if a != b => Some(if a <= b { (a, b) } else { (b, a) }),
            _ => None,
        };
        let in_sel = |r: i64, c: u16| -> bool {
            match sel {
                Some((s, e)) => (r, c) >= s && (r, c) <= e,
                None => false,
            }
        };
        for r in 0..rows {
            let y = inner.top() + r as f32 * ch;
            let abs_row = r as i64 - self.scroll as i64;
            let mut run = String::new();
            let mut run_x = inner.left();
            let mut run_style: Option<(Color32, bool, bool)> = None;
            let flush = |run: &mut String, run_x: f32, style: Option<(Color32, bool, bool)>| {
                if run.is_empty() {
                    return;
                }
                if let Some((fg, bold, underline)) = style {
                    let g = painter.layout_no_wrap(std::mem::take(run), font.clone(), fg);
                    let w = g.size().x;
                    painter.galley(pos2(run_x, y), g.clone(), fg);
                    if bold {
                        painter.galley(pos2(run_x + 0.6, y), g, fg);
                    }
                    if underline {
                        painter.line_segment([pos2(run_x, y + ch - 1.5), pos2(run_x + w, y + ch - 1.5)], Stroke::new(1.0, fg));
                    }
                }
                run.clear();
            };
            let mut c = 0u16;
            while c < cols {
                let Some(cell) = screen.cell(r, c) else { break };
                if cell.is_wide_continuation() {
                    c += 1;
                    continue;
                }
                let width = if cell.is_wide() { 2 } else { 1 };
                let x = inner.left() + c as f32 * cw;
                let mut fg = color(cell.fgcolor(), FG, cell.bold());
                let mut bg = color(cell.bgcolor(), BG, false);
                if cell.inverse() {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if cell.dim() {
                    fg = fg.gamma_multiply(0.6);
                }
                let selected = in_sel(abs_row, c);
                if selected {
                    bg = SELECTION;
                }
                if bg != BG {
                    painter.rect_filled(Rect::from_min_size(pos2(x, y), vec2(cw * width as f32, ch)), CornerRadius::ZERO, bg);
                }
                let text = cell.contents();
                let style = (fg, cell.bold(), cell.underline());
                let ascii = text.len() == 1 && text.as_bytes()[0].is_ascii_graphic() || text.is_empty() || text == " ";
                if ascii && width == 1 {
                    if run_style != Some(style) {
                        flush(&mut run, run_x, run_style);
                        run_x = x;
                        run_style = Some(style);
                    }
                    run.push_str(if text.is_empty() { " " } else { text });
                } else {
                    // Non-ASCII (box drawing, accents, emoji, CJK) is placed cell by cell so
                    // fallback fonts with other widths can't shift the rest of the line.
                    flush(&mut run, run_x, run_style);
                    run_style = None;
                    let cell_rect = Rect::from_min_size(pos2(x, y), vec2(cw * width as f32, ch));
                    if let Some(ch0) = text.chars().next() {
                        if draw_box_char(&painter, cell_rect, ch0, fg) {
                            run_x = x + cw * width as f32;
                            c += width;
                            continue;
                        }
                    }
                    let g = painter.layout_no_wrap(text.to_string(), font.clone(), fg);
                    let gx = x + ((cw * width as f32) - g.size().x).max(0.0) / 2.0;
                    painter.galley(pos2(gx, y), g, fg);
                    run_x = x + cw * width as f32;
                }
                c += width;
            }
            flush(&mut run, run_x, run_style);
        }
        // Cursor
        if self.scroll == 0 && !screen.hide_cursor() && !self.exited() {
            let (cr, cc) = screen.cursor_position();
            let r = Rect::from_min_size(pos2(inner.left() + cc as f32 * cw, inner.top() + cr as f32 * ch), vec2(cw, ch));
            if focused {
                // cursorStyle "bar", cursorBlink true (xterm.js blinks about every 600 ms).
                let t = ui.input(|i| i.time);
                if (t / 0.6) as i64 % 2 == 0 {
                    painter.rect_filled(Rect::from_min_size(r.min, vec2(1.5, ch)), CornerRadius::ZERO, CURSOR);
                }
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
            } else {
                painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, CURSOR), egui::StrokeKind::Inside);
            }
        }
        if self.scroll > 0 {
            let label = format!("↑ {} lines back — scroll down or type to return", self.scroll);
            let g = painter.layout_no_wrap(label, FontId::proportional(12.0), theme::TEXT_HI);
            let r = Rect::from_min_size(pos2(rect.right() - g.size().x - 20.0, rect.top() + 6.0), g.size() + vec2(12.0, 6.0));
            painter.rect_filled(r, CornerRadius::same(3), theme::BLUE);
            painter.galley(r.min + vec2(6.0, 3.0), g, theme::TEXT_HI);
        }
    }
}

fn unicode_wide(c: char) -> bool {
    let u = c as u32;
    (0x1100..=0x115f).contains(&u)
        || (0x2e80..=0xa4cf).contains(&u)
        || (0xac00..=0xd7a3).contains(&u)
        || (0xf900..=0xfaff).contains(&u)
        || (0xfe30..=0xfe4f).contains(&u)
        || (0xff00..=0xff60).contains(&u)
        || (0x1f300..=0x1faff).contains(&u)
        || (0x20000..=0x3fffd).contains(&u)
}

/// Bytes an xterm sends for a key press ("COPY"/"PASTE" for Ctrl+Shift+C/V).
fn key_bytes(key: egui::Key, m: egui::Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    use egui::Key as K;
    let modcode = 1 + (m.shift as u8) + 2 * (m.alt as u8) + 4 * (m.ctrl as u8);
    let csi = |final_: &str| -> Vec<u8> {
        if modcode > 1 {
            format!("\x1b[1;{modcode}{final_}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{final_}").into_bytes()
        } else {
            format!("\x1b[{final_}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modcode > 1 { format!("\x1b[{n};{modcode}~").into_bytes() } else { format!("\x1b[{n}~").into_bytes() }
    };
    if m.ctrl && m.shift && key == K::V {
        return Some(b"PASTE".to_vec());
    }
    let out = match key {
        K::Enter => b"\r".to_vec(),
        K::Tab if m.shift => b"\x1b[Z".to_vec(),
        K::Tab => b"\t".to_vec(),
        K::Backspace if m.alt => b"\x1b\x7f".to_vec(),
        K::Backspace if m.ctrl => b"\x08".to_vec(),
        K::Backspace => b"\x7f".to_vec(),
        K::Escape => b"\x1b".to_vec(),
        K::ArrowUp => csi("A"),
        K::ArrowDown => csi("B"),
        K::ArrowRight => csi("C"),
        K::ArrowLeft => csi("D"),
        K::Home => csi("H"),
        K::End => csi("F"),
        K::Insert => tilde(2),
        K::Delete => tilde(3),
        K::PageUp => tilde(5),
        K::PageDown => tilde(6),
        K::F1 => b"\x1bOP".to_vec(),
        K::F2 => b"\x1bOQ".to_vec(),
        K::F3 => b"\x1bOR".to_vec(),
        K::F4 => b"\x1bOS".to_vec(),
        K::F5 => tilde(15),
        K::F6 => tilde(17),
        K::F7 => tilde(18),
        K::F8 => tilde(19),
        K::F9 => tilde(20),
        K::F10 => tilde(21),
        K::F11 => tilde(23),
        K::F12 => tilde(24),
        _ => {
            // Ctrl+letter → control byte (Ctrl+C/X/V arrive as clipboard events instead).
            let name = key.name();
            if (m.ctrl || m.alt) && name.len() == 1 {
                let c = name.chars().next()?.to_ascii_lowercase();
                if m.ctrl && c.is_ascii_lowercase() {
                    let b = (c as u8) - b'a' + 1;
                    return Some(if m.alt { vec![0x1b, b] } else { vec![b] });
                }
                if m.alt && !m.ctrl {
                    // Alt+letter: ESC prefix (the letter itself also arrives as Text, so skip it there).
                    return None;
                }
            }
            if m.ctrl {
                return match key {
                    K::OpenBracket => Some(vec![0x1b]),
                    K::Backslash => Some(vec![0x1c]),
                    K::CloseBracket => Some(vec![0x1d]),
                    K::Space => Some(vec![0x00]),
                    _ => None,
                };
            }
            return None;
        }
    };
    Some(out)
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if let Some(c) = &mut self.child {
            let _ = c.kill();
        }
    }
}

/// Box-drawing and block characters drawn as shapes so neighbouring cells join seamlessly
/// (font glyphs leave gaps). Returns false for characters it doesn't handle.
fn draw_box_char(p: &egui::Painter, r: Rect, c: char, col: Color32) -> bool {
    // (left, right, up, down) line weights: 0 none, 1 light, 2 heavy, 3 double.
    let (l, rt, u, d): (u8, u8, u8, u8) = match c {
        '─' => (1, 1, 0, 0),
        '━' => (2, 2, 0, 0),
        '│' => (0, 0, 1, 1),
        '┃' => (0, 0, 2, 2),
        '┌' | '╭' => (0, 1, 0, 1),
        '┐' | '╮' => (1, 0, 0, 1),
        '└' | '╰' => (0, 1, 1, 0),
        '┘' | '╯' => (1, 0, 1, 0),
        '├' => (0, 1, 1, 1),
        '┤' => (1, 0, 1, 1),
        '┬' => (1, 1, 0, 1),
        '┴' => (1, 1, 1, 0),
        '┼' => (1, 1, 1, 1),
        '┏' => (0, 2, 0, 2),
        '┓' => (2, 0, 0, 2),
        '┗' => (0, 2, 2, 0),
        '┛' => (2, 0, 2, 0),
        '┣' => (0, 2, 2, 2),
        '┫' => (2, 0, 2, 2),
        '┳' => (2, 2, 0, 2),
        '┻' => (2, 2, 2, 0),
        '╋' => (2, 2, 2, 2),
        '═' => (3, 3, 0, 0),
        '║' => (0, 0, 3, 3),
        '╔' => (0, 3, 0, 3),
        '╗' => (3, 0, 0, 3),
        '╚' => (0, 3, 3, 0),
        '╝' => (3, 0, 3, 0),
        '╠' => (0, 3, 3, 3),
        '╣' => (3, 0, 3, 3),
        '╦' => (3, 3, 0, 3),
        '╩' => (3, 3, 3, 0),
        '╬' => (3, 3, 3, 3),
        '█' => {
            p.rect_filled(r, CornerRadius::ZERO, col);
            return true;
        }
        '▀' => {
            p.rect_filled(Rect::from_min_max(r.min, pos2(r.max.x, r.center().y)), CornerRadius::ZERO, col);
            return true;
        }
        '▄' => {
            p.rect_filled(Rect::from_min_max(pos2(r.min.x, r.center().y), r.max), CornerRadius::ZERO, col);
            return true;
        }
        '▌' => {
            p.rect_filled(Rect::from_min_max(r.min, pos2(r.center().x, r.max.y)), CornerRadius::ZERO, col);
            return true;
        }
        '▐' => {
            p.rect_filled(Rect::from_min_max(pos2(r.center().x, r.min.y), r.max), CornerRadius::ZERO, col);
            return true;
        }
        '░' | '▒' | '▓' => {
            let a = match c { '░' => 0.25, '▒' => 0.5, _ => 0.75 };
            p.rect_filled(r, CornerRadius::ZERO, col.gamma_multiply(a));
            return true;
        }
        _ => return false,
    };
    let cx = r.center().x.round() + 0.5;
    let cy = r.center().y.round() + 0.5;
    let stroke = |w: u8| Stroke::new(if w == 2 { 2.5 } else { 1.0 }, col);
    let seg = |a: Pos2, b: Pos2, w: u8| {
        if w == 3 {
            // Double line: two thin lines 1.5 px either side of the centre.
            let n = if (a.y - b.y).abs() < 0.1 { vec2(0.0, 1.5) } else { vec2(1.5, 0.0) };
            p.line_segment([a - n, b - n], Stroke::new(1.0, col));
            p.line_segment([a + n, b + n], Stroke::new(1.0, col));
        } else if w > 0 {
            p.line_segment([a, b], stroke(w));
        }
    };
    seg(pos2(r.left(), cy), pos2(cx, cy), l);
    seg(pos2(cx, cy), pos2(r.right(), cy), rt);
    seg(pos2(cx, r.top()), pos2(cx, cy), u);
    seg(pos2(cx, cy), pos2(cx, r.bottom()), d);
    true
}
