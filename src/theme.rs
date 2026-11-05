//! The original's "Dark" theme. Every colour below is read from the original's theme
//! declaration (see MECHANICS.md, section "Theme"); names in comments are the original's keys.

use eframe::egui;
use crate::model::Tone;
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals};

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub const HOTBAR: Color32 = hex(0x252729); // clusterMenuBackground
pub const HOTBAR_CELL: Color32 = hex(0x2e3136); // clusterMenuCellBackground
pub const TOPBAR: Color32 = hex(0x2e3136); // layoutBackground (top bar)
pub const SIDEBAR: Color32 = hex(0x36393e); // sidebarBackground
pub const SIDEBAR_HOVER: Color32 = hex(0x3a3e44); // sidebarItemHoverBackground
pub const CONTENT: Color32 = hex(0x1e2124); // mainBackground
pub const PANEL: Color32 = hex(0x262b2f); // contentColor / dialogBackground
pub const TABLE_HEAD: Color32 = hex(0x262b2f); // tableHeaderBackground
pub const TABLE_HEAD_TEXT: Color32 = hex(0xffffff); // tableHeaderColor
pub const STRIPE: Color32 = hex(0x2a2d33); // tableBgcStripe
pub const HOVER: Color32 = hex(0x383c42); // tableBgcSelected (row hover and selection)
pub const SELECTED: Color32 = hex(0x383c42); // tableBgcSelected
pub const BORDER: Color32 = hex(0x373a3e); // borderFaintColor
pub const BORDER_STRONG: Color32 = hex(0x4c5053); // borderColor
pub const INPUT: Color32 = hex(0x1e2125); // inputControlBackground
pub const INPUT_BORDER: Color32 = hex(0x414448); // inputControlBorder
pub const BUTTON: Color32 = hex(0x414448); // buttonDefaultBackground
pub const LINE_BG: Color32 = hex(0x414448); // lineProgressBackground
pub const DOCK_HEAD: Color32 = hex(0x2e3136); // dockHeadBackground
pub const DOCK_TAB_ACTIVE: Color32 = hex(0x3a3e45); // dockTabActiveBackground
pub const SUBTITLE_BG: Color32 = hex(0x373a3e); // drawerSubtitleBackground
pub const ITEM_NAME: Color32 = hex(0x87909c); // drawerItemNameColor
pub const ITEM_VALUE: Color32 = hex(0xa0a0a0); // drawerItemValueColor

pub const TEXT: Color32 = hex(0x8e9297); // textColorPrimary
pub const TEXT_MID: Color32 = hex(0xa0a0a0); // textColorSecondary
pub const TEXT_TERTIARY: Color32 = hex(0x909ba6); // textColorTertiary (list titles)
pub const TEXT_HI: Color32 = hex(0xffffff); // textColorAccent
pub const TEXT_DIM: Color32 = Color32::from_rgba_premultiplied(0x4e, 0x50, 0x53, 0x8c); // textColorDimmed #8e92978c

pub const BLUE: Color32 = hex(0x00a7a0); // primary / blue (status bar, active item, drawer title)
pub const GREEN: Color32 = hex(0x4caf50); // colorOk
pub const SUCCESS: Color32 = hex(0x43a047); // colorSuccess
pub const RED: Color32 = hex(0xce3933); // colorError
pub const SOFT_RED: Color32 = hex(0xe85555); // colorSoftError / buttonAccentBackground
pub const ORANGE: Color32 = hex(0xff9800); // colorWarning
pub const YELLOW: Color32 = hex(0xffc63d); // golden
pub const PURPLE: Color32 = hex(0xc93dce); // magenta
pub const GREY: Color32 = hex(0x4c5053); // colorTerminated

pub const TERMINAL: Color32 = hex(0x000000); // logsBackground / dockEditorBackground
pub const TERMINAL_TEXT: Color32 = hex(0xffffff); // logsForeground

// Sizes read from the original's stylesheet and settings.
pub const HOTBAR_WIDTH: f32 = 75.0; // --hotbar-width
pub const HOTBAR_CELL_SIZE: f32 = 40.0; // HotbarItems --cellWidth/--cellHeight
pub const HOTBAR_CELL_MARGIN: f32 = 12.0; // HotbarCell margin
pub const TOPBAR_HEIGHT: f32 = 40.0; // --main-layout-header
pub const STATUSBAR_HEIGHT: f32 = 21.0; // --status-bar-height
pub const SIDEBAR_WIDTH: f32 = 200.0; // sidebar storage default width
pub const DRAWER_WIDTH: f32 = 725.0; // drawer storage default width
pub const DRAWER_TITLE_HEIGHT: f32 = 54.0; // .drawer-title min-height
pub const FONT_SIZE: f32 = 14.0; // --font-size = 1.75 * 8px
pub const FONT_SIZE_SMALL: f32 = 12.0; // --font-size-small = 1.5 * 8px

pub fn bar(c: crate::model::BarColor) -> Color32 {
    match c {
        crate::model::BarColor::Cpu => BLUE,
        crate::model::BarColor::Memory => PURPLE,
        crate::model::BarColor::Disk => YELLOW,
        crate::model::BarColor::Pods => BLUE,
    }
}

pub fn tone(t: Tone) -> Color32 {
    match t {
        Tone::Ok => SUCCESS,
        Tone::Memory => hex(0x901995),
        Tone::Disk => hex(0xcc8d00),
        Tone::Pid => hex(0x068dbf),
        Tone::Network => hex(0x812727),
        Tone::Warn => ORANGE,
        Tone::Err => RED,
        Tone::Info => BLUE,
        Tone::Muted => hex(0x8e9297),
        Tone::Plain => TEXT_MID,
    }
}

/// Colour for a cluster avatar, derived from its name.
pub fn avatar(name: &str) -> Color32 {
    let palette = [
        Color32::from_rgb(0x3d, 0x90, 0xce),
        Color32::from_rgb(0x8e, 0x44, 0xad),
        Color32::from_rgb(0x16, 0xa0, 0x85),
        Color32::from_rgb(0xd3, 0x54, 0x00),
        Color32::from_rgb(0xc0, 0x39, 0x2b),
        Color32::from_rgb(0x27, 0xae, 0x60),
        Color32::from_rgb(0x2c, 0x3e, 0x50),
        Color32::from_rgb(0xb7, 0x95, 0x0b),
    ];
    let h = name.bytes().fold(7u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32));
    palette[(h as usize) % palette.len()]
}

pub fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect();
    let mut s: String = match parts.len() {
        0 => "?".into(),
        1 => parts[0].chars().take(2).collect(),
        _ => parts.iter().take(2).filter_map(|p| p.chars().next()).collect(),
    };
    s = s.to_uppercase();
    s
}

pub fn apply(ctx: &egui::Context) {
    // Characters no font can draw show as a box (as in a normal window) instead of stopping.
    ctx.set_missing_glyph_policy(egui::epaint::text::MissingGlyphPolicy::Tofu);
    let mut v = Visuals::dark();
    v.panel_fill = CONTENT;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0, BORDER_STRONG);
    v.window_corner_radius = CornerRadius::same(6);
    v.extreme_bg_color = INPUT;
    v.faint_bg_color = STRIPE;
    v.code_bg_color = TERMINAL;
    v.hyperlink_color = BLUE;
    v.selection.bg_fill = Color32::from_rgb(0x26, 0x4f, 0x78); // terminal selectionBackground
    v.selection.stroke = Stroke::new(1.0, TEXT_HI);
    v.override_text_color = None;

    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_MID);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.inactive.bg_fill = INPUT;
    v.widgets.inactive.weak_bg_fill = BUTTON;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_HI);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, INPUT_BORDER);
    v.widgets.hovered.bg_fill = INPUT;
    v.widgets.hovered.weak_bg_fill = hex(0x474a4f);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_HI);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, hex(0x474a4f)); // inputControlHoverBorder
    v.widgets.active.bg_fill = BLUE;
    v.widgets.active.weak_bg_fill = BLUE;
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_HI);
    v.widgets.open.weak_bg_fill = INPUT;
    v.widgets.open.bg_fill = INPUT;
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = CornerRadius::same(3); // --border-radius
    }
    ctx.set_visuals_of(egui::Theme::Dark, v);
    ctx.set_theme(egui::ThemePreference::Dark);

    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::new(FONT_SIZE_SMALL, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(FONT_SIZE, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(FONT_SIZE, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(19.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size.y = 24.0;
    });
}
