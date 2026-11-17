//! Visual design: fonts, colour tokens and egui visuals for light and dark.

use std::sync::OnceLock;

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Margin, Shadow, Stroke, Vec2};

static ICONS: OnceLock<bool> = OnceLock::new();

/// Material Design icons from the Nerd Font (only shown when the font loaded).
pub mod icon {
    pub const CUBE: &str = "\u{F01A7}";
    pub const SUN: &str = "\u{F0599}";
    pub const MOON: &str = "\u{F0594}";
    pub const HELP: &str = "\u{F0625}";
    pub const FUNCTION: &str = "\u{F0871}";
    pub const CHEVRON_DOWN: &str = "\u{F0140}";
    pub const CHEVRON_RIGHT: &str = "\u{F0142}";
    pub const CHEVRON_UP: &str = "\u{F0143}";
    pub const EYE: &str = "\u{F0208}";
    pub const EYE_OFF: &str = "\u{F0209}";
    pub const TRANSLATE: &str = "\u{F05CA}";
    pub const CHECK: &str = "\u{F05E0}";
    pub const ALERT: &str = "\u{F0028}";
    pub const CROSSHAIRS: &str = "\u{F01A4}";
    pub const TUNE: &str = "\u{F062E}";
    pub const LAYERS: &str = "\u{F0328}";
    pub const RULER: &str = "\u{F046D}";
    pub const SIGMA: &str = "\u{F04A0}";
    pub const INTEGRAL: &str = "\u{F0FC8}";
    pub const INFO: &str = "\u{F02FD}";
    pub const COPY: &str = "\u{F018F}";
    pub const CAMERA: &str = "\u{F0D5D}";
    pub const BELL_CURVE: &str = "\u{F0C50}";
    pub const SINE: &str = "\u{F095B}";
    pub const RESTORE: &str = "\u{F099B}";
    pub const TARGET: &str = "\u{F04FE}";
    pub const BOOK: &str = "\u{F0B63}";
    pub const LIGHTBULB: &str = "\u{F0336}";
    pub const DOT: &str = "\u{F09DE}";
    pub const CURVE: &str = "\u{F0559}";
    pub const MAP: &str = "\u{F0982}";
    pub const COMPASS: &str = "\u{F018C}";
    pub const PLAY: &str = "\u{F040A}";
    pub const PAUSE: &str = "\u{F03E4}";
}

pub fn icons_available() -> bool {
    *ICONS.get().unwrap_or(&false)
}

/// `"<icon>  text"` when icons are available, else just `text`.
pub fn with_icon(icon: &str, text: &str) -> String {
    if icons_available() {
        if text.is_empty() { icon.to_string() } else { format!("{icon}  {text}") }
    } else {
        text.to_string()
    }
}

/// Fonts compiled into the binary (see scripts/build-fonts.py), so every
/// platform renders the same: (name, data, proportional, monospace).
const FONTS: [(&str, &[u8], bool, bool); 6] = [
    ("Inter", include_bytes!("../../assets/fonts/InterVariable.ttf"), true, false),
    ("JetBrainsMono", include_bytes!("../../assets/fonts/JetBrainsMonoNL-Regular.ttf"), false, true),
    ("DejaVuSans", include_bytes!("../../assets/fonts/DejaVuSans.ttf"), true, false),
    ("DejaVuSansMono", include_bytes!("../../assets/fonts/DejaVuSansMono.ttf"), false, true),
    ("NerdSymbols", include_bytes!("../../assets/fonts/SymbolsNerdFont-Subset.ttf"), true, true),
    // Chinese interface text.
    ("CJK", include_bytes!("../../assets/fonts/CrystalHanSans-Regular.otf"), true, true),
];

/// Install Inter (UI), JetBrains Mono (numbers, formulas), DejaVu Sans
/// (maths symbols), Nerd Font icons and a Source Han Sans subset (Chinese);
/// egui's defaults stay as the last fallback.
pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut prop: Vec<String> = Vec::new();
    let mut mono: Vec<String> = Vec::new();
    let mut icons = false;
    for (name, bytes, for_prop, for_mono) in FONTS {
        let data = FontData::from_static(bytes);
        let data = if name == "NerdSymbols" {
            icons = true;
            // Sit icons on the text baseline.
            data.tweak(egui::FontTweak { scale: 1.0, y_offset_factor: 0.06, ..Default::default() })
        } else {
            data
        };
        defs.font_data.insert(name.to_string(), std::sync::Arc::new(data));
        if for_prop {
            prop.push(name.to_string());
        }
        if for_mono {
            mono.push(name.to_string());
        }
    }
    let _ = ICONS.set(icons);
    if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
        for (i, n) in prop.into_iter().enumerate() {
            list.insert(i, n);
        }
    }
    if let Some(list) = defs.families.get_mut(&FontFamily::Monospace) {
        for (i, n) in mono.into_iter().enumerate() {
            list.insert(i, n);
        }
    }
    ctx.set_fonts(defs);
}

/// Design tokens for one theme.
#[derive(Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    pub window: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub card_border: Color32,
    pub input: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_text: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub overlay: Color32,
    pub overlay_border: Color32,
}

pub const LIGHT: Tokens = Tokens {
    dark: false,
    window: Color32::from_rgb(0xee, 0xf0, 0xf5),
    sidebar: Color32::from_rgb(0xf4, 0xf5, 0xf9),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    card_border: Color32::from_rgb(0xe1, 0xe4, 0xec),
    input: Color32::from_rgb(0xf3, 0xf4, 0xf8),
    text: Color32::from_rgb(0x1b, 0x1f, 0x2a),
    muted: Color32::from_rgb(0x5f, 0x67, 0x7a),
    faint: Color32::from_rgb(0x95, 0x9c, 0xad),
    accent: Color32::from_rgb(0x4a, 0x63, 0xe7),
    accent_soft: Color32::from_rgb(0xe6, 0xea, 0xfd),
    accent_text: Color32::from_rgb(0x2f, 0x45, 0xc4),
    ok: Color32::from_rgb(0x1f, 0x9d, 0x55),
    warn: Color32::from_rgb(0xc2, 0x7a, 0x0a),
    error: Color32::from_rgb(0xd6, 0x33, 0x3a),
    overlay: Color32::from_rgba_premultiplied(0xfc, 0xfc, 0xfe, 0xf0),
    overlay_border: Color32::from_rgb(0xdc, 0xe0, 0xea),
};

pub const DARK: Tokens = Tokens {
    dark: true,
    window: Color32::from_rgb(0x0f, 0x11, 0x16),
    sidebar: Color32::from_rgb(0x14, 0x16, 0x1d),
    card: Color32::from_rgb(0x1b, 0x1e, 0x27),
    card_border: Color32::from_rgb(0x2a, 0x2e, 0x3a),
    input: Color32::from_rgb(0x12, 0x14, 0x1a),
    text: Color32::from_rgb(0xe7, 0xe9, 0xf0),
    muted: Color32::from_rgb(0x9a, 0xa1, 0xb3),
    faint: Color32::from_rgb(0x6a, 0x71, 0x82),
    accent: Color32::from_rgb(0x7d, 0x93, 0xff),
    accent_soft: Color32::from_rgb(0x26, 0x2d, 0x4d),
    accent_text: Color32::from_rgb(0xa9, 0xb7, 0xff),
    ok: Color32::from_rgb(0x4c, 0xc3, 0x7c),
    warn: Color32::from_rgb(0xe8, 0xa5, 0x3a),
    error: Color32::from_rgb(0xff, 0x6b, 0x70),
    overlay: Color32::from_rgba_premultiplied(0x1b, 0x1e, 0x27, 0xf0),
    overlay_border: Color32::from_rgb(0x30, 0x35, 0x44),
};

pub fn tokens(ctx: &egui::Context) -> Tokens {
    if ctx.global_style().visuals.dark_mode { DARK } else { LIGHT }
}

fn visuals(t: &Tokens) -> egui::Visuals {
    let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    let r6 = CornerRadius::same(6);
    v.panel_fill = t.window;
    v.window_fill = t.card;
    v.window_stroke = Stroke::new(1.0, t.card_border);
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(if t.dark { 110 } else { 36 }) };
    v.popup_shadow = v.window_shadow;
    v.extreme_bg_color = t.input;
    v.text_edit_bg_color = Some(t.input);
    v.faint_bg_color = if t.dark { Color32::from_rgb(0x20, 0x24, 0x2e) } else { Color32::from_rgb(0xf6, 0xf7, 0xfa) };
    v.code_bg_color = t.input;
    v.hyperlink_color = t.accent_text;
    v.warn_fg_color = t.warn;
    v.error_fg_color = t.error;
    v.selection.bg_fill = if t.dark { Color32::from_rgb(0x3a, 0x48, 0x8f) } else { Color32::from_rgb(0xc9, 0xd2, 0xfb) };
    v.selection.stroke = Stroke::new(1.0, t.accent);
    v.slider_trailing_fill = true;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.collapsing_header_frame = false;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = t.card;
    w.noninteractive.weak_bg_fill = t.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, t.card_border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    w.noninteractive.corner_radius = r6;

    let button = if t.dark { Color32::from_rgb(0x25, 0x29, 0x35) } else { Color32::from_rgb(0xec, 0xee, 0xf4) };
    let button_hover = if t.dark { Color32::from_rgb(0x2e, 0x33, 0x42) } else { Color32::from_rgb(0xe2, 0xe6, 0xf0) };
    w.inactive.bg_fill = button;
    w.inactive.weak_bg_fill = button;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, t.text);
    w.inactive.corner_radius = r6;

    w.hovered.bg_fill = button_hover;
    w.hovered.weak_bg_fill = button_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, t.accent.gamma_multiply(0.5));
    w.hovered.fg_stroke = Stroke::new(1.5, t.text);
    w.hovered.corner_radius = r6;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = t.accent_soft;
    w.active.weak_bg_fill = t.accent_soft;
    w.active.bg_stroke = Stroke::new(1.0, t.accent);
    w.active.fg_stroke = Stroke::new(1.5, t.accent_text);
    w.active.corner_radius = r6;
    w.active.expansion = 0.0;

    w.open = w.active;
    v
}

pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals_of(egui::Theme::Light, visuals(&LIGHT));
    ctx.set_visuals_of(egui::Theme::Dark, visuals(&DARK));
    ctx.all_styles_mut(|style| {
        let s = &mut style.spacing;
        s.item_spacing = Vec2::new(8.0, 7.0);
        s.button_padding = Vec2::new(10.0, 4.0);
        s.interact_size = Vec2::new(36.0, 26.0);
        s.slider_width = 140.0;
        s.slider_rail_height = 4.0;
        s.combo_width = 120.0;
        s.icon_width = 16.0;
        s.icon_width_inner = 9.0;
        s.icon_spacing = 6.0;
        s.window_margin = Margin::same(10);
        s.menu_margin = Margin::same(6);
        s.indent = 14.0;
        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(18.0)),
            (TextStyle::Body, FontId::proportional(14.0)),
            (TextStyle::Button, FontId::proportional(14.0)),
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Monospace, FontId::monospace(13.5)),
        ]
        .into();
    });
}

#[cfg(test)]
mod tests {
    use skrifa::MetadataProvider;

    /// Every non-ASCII character in the sources (interface text, maths
    /// symbols, icons) must be in a bundled font; otherwise rerun
    /// scripts/build-fonts.py.
    #[test]
    fn bundled_fonts_cover_source_text() {
        let fonts: Vec<skrifa::FontRef> = super::FONTS.iter().map(|f| skrifa::FontRef::new(f.1).expect("font parses")).collect();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut stack = vec![root.join("src"), root.join("examples")];
        let mut missing = std::collections::BTreeMap::<char, String>::new();
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let text = std::fs::read_to_string(&path).unwrap();
                    for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
                        for c in line.chars().filter(|&c| c as u32 > 0x7f && !c.is_whitespace()) {
                            if !fonts.iter().any(|f| f.charmap().map(c).is_some()) {
                                missing.entry(c).or_insert_with(|| path.display().to_string());
                            }
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "characters without a bundled glyph: {missing:?}");
    }
}
