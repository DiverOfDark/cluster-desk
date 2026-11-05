//! ANSI colour codes in log lines, rendered the way the original does it (the ansi_up library):
//! SGR colours and styles become coloured text, every other escape sequence is removed.

use eframe::egui::Color32;

/// ansi_up's palette: 8 normal and 8 bright colours.
const PALETTE: [[u8; 3]; 16] = [
    [0, 0, 0],
    [187, 0, 0],
    [0, 187, 0],
    [187, 187, 0],
    [0, 0, 187],
    [187, 0, 187],
    [0, 187, 187],
    [255, 255, 255],
    [85, 85, 85],
    [255, 85, 85],
    [0, 255, 0],
    [255, 255, 85],
    [85, 85, 255],
    [255, 85, 255],
    [85, 255, 255],
    [255, 255, 255],
];

fn indexed(i: u8) -> Color32 {
    match i {
        0..=15 => {
            let [r, g, b] = PALETTE[i as usize];
            Color32::from_rgb(r, g, b)
        }
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

#[derive(Clone, Copy, Default, PartialEq, Debug)]
pub struct Style {
    pub fg: Option<Color32>,
    pub bg: Option<Color32>,
    pub bold: bool,
    pub faint: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    fn apply(&mut self, params: &[u16]) {
        if params.is_empty() {
            *self = Style::default();
            return;
        }
        let mut i = 0;
        while i < params.len() {
            let p = params[i];
            match p {
                0 => *self = Style::default(),
                1 => self.bold = true,
                2 => self.faint = true,
                3 => self.italic = true,
                4 => self.underline = true,
                21 | 22 => {
                    self.bold = false;
                    self.faint = false;
                }
                23 => self.italic = false,
                24 => self.underline = false,
                30..=37 => self.fg = Some(indexed((p - 30) as u8)),
                39 => self.fg = None,
                40..=47 => self.bg = Some(indexed((p - 40) as u8)),
                49 => self.bg = None,
                90..=97 => self.fg = Some(indexed((p - 90 + 8) as u8)),
                100..=107 => self.bg = Some(indexed((p - 100 + 8) as u8)),
                38 | 48 => {
                    // 38;5;n / 38;2;r;g;b (and the same for backgrounds)
                    let color = match params.get(i + 1) {
                        Some(5) => {
                            let c = params.get(i + 2).map(|n| indexed((*n).min(255) as u8));
                            i += 2;
                            c
                        }
                        Some(2) => {
                            let c = match (params.get(i + 2), params.get(i + 3), params.get(i + 4)) {
                                (Some(r), Some(g), Some(b)) => {
                                    Some(Color32::from_rgb((*r).min(255) as u8, (*g).min(255) as u8, (*b).min(255) as u8))
                                }
                                _ => None,
                            };
                            i += 4;
                            c
                        }
                        _ => None,
                    };
                    if p == 38 {
                        self.fg = color.or(self.fg);
                    } else {
                        self.bg = color.or(self.bg);
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }
}

/// Split a line into styled pieces; escape sequences are consumed. `style` carries over from
/// the previous line, like ansi_up keeps its state between calls.
pub fn parse(line: &str, style: &mut Style) -> Vec<(String, Style)> {
    let mut out: Vec<(String, Style)> = vec![];
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let push = |out: &mut Vec<(String, Style)>, cur: &mut String, st: Style| {
        if !cur.is_empty() {
            out.push((std::mem::take(cur), st));
        }
    };
    while let Some(c) = chars.next() {
        let csi = match c {
            '\u{1b}' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    true
                }
                Some(']') => {
                    // OSC … BEL or ESC \ : drop it (hyperlinks, titles).
                    chars.next();
                    while let Some(x) = chars.next() {
                        if x == '\u{7}' {
                            break;
                        }
                        if x == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                    continue;
                }
                Some(_) => {
                    // Two-character escapes such as ESC ( B.
                    let n = chars.next();
                    if matches!(n, Some('(' | ')' | '#')) {
                        chars.next();
                    }
                    continue;
                }
                None => continue,
            },
            '\u{9b}' => true,
            '\r' => continue,
            c => {
                cur.push(c);
                false
            }
        };
        if csi {
            let mut body = String::new();
            let mut fin = None;
            for x in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&x) {
                    fin = Some(x);
                    break;
                }
                body.push(x);
            }
            if fin == Some('m') {
                push(&mut out, &mut cur, *style);
                let params: Vec<u16> = if body.is_empty() {
                    vec![]
                } else {
                    body.split([';', ':']).map(|p| p.parse::<u16>().unwrap_or(0)).collect()
                };
                style.apply(&params);
            }
        }
    }
    push(&mut out, &mut cur, *style);
    out
}

/// The line without escape sequences (what search looks at).
pub fn strip(line: &str) -> String {
    if !line.contains(['\u{1b}', '\u{9b}']) {
        return line.to_string();
    }
    let mut st = Style::default();
    parse(line, &mut st).into_iter().map(|(t, _)| t).collect()
}
