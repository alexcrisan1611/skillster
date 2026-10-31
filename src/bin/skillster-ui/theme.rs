//! The look, after Fracst's: bundled fonts, one card filling the window,
//! Dark or Light as the system is, and the icon's indigo as the accent.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle};

/// Corner radius of the window and of floating cards.
pub const RADIUS: u8 = 12;

pub struct Colours {
    pub card: Color32,
    pub card_stroke: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    /// Text and icons on a hovered or pressed control.
    pub text_strong: Color32,
    /// A faint wash over hovered rows and icon buttons.
    pub wash: Color32,
    pub separator: Color32,
    /// Code and diffs.
    pub code: Color32,
    pub button: (Color32, Color32),
    pub button_hovered: (Color32, Color32),
    pub accent: Color32,
    /// Text on the accent.
    pub on_accent: Color32,
    pub added: Color32,
    pub removed: Color32,
    pub warn: Color32,
    pub error: Color32,
    /// How dark the shadows are, from 0 to 1.
    pub shadow: f32,
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_premultiplied(r, g, b, a)
}

const DARK: Colours = Colours {
    card: Color32::from_rgb(19, 19, 24),
    card_stroke: rgba(56, 56, 64, 140),
    text: Color32::from_rgb(226, 226, 232),
    text_weak: Color32::from_rgb(150, 150, 160),
    text_strong: Color32::WHITE,
    wash: rgba(10, 10, 12, 10),
    separator: rgba(60, 60, 68, 120),
    code: Color32::from_rgb(13, 13, 17),
    button: (rgba(48, 48, 56, 200), rgba(38, 38, 46, 190)),
    button_hovered: (rgba(62, 62, 72, 220), rgba(56, 56, 66, 220)),
    accent: Color32::from_rgb(150, 140, 255),
    on_accent: Color32::from_rgb(12, 12, 16),
    added: Color32::from_rgb(120, 200, 140),
    removed: Color32::from_rgb(230, 120, 110),
    warn: Color32::from_rgb(230, 170, 90),
    error: Color32::from_rgb(230, 110, 90),
    shadow: 1.0,
};

const LIGHT: Colours = Colours {
    card: Color32::from_rgb(252, 252, 254),
    card_stroke: Color32::from_black_alpha(34),
    text: Color32::from_rgb(28, 28, 34),
    text_weak: Color32::from_rgb(100, 100, 112),
    text_strong: Color32::BLACK,
    wash: Color32::from_black_alpha(12),
    separator: Color32::from_black_alpha(28),
    code: Color32::from_rgb(240, 240, 245),
    button: (Color32::from_black_alpha(22), Color32::from_black_alpha(16)),
    button_hovered: (Color32::from_black_alpha(38), Color32::from_black_alpha(30)),
    accent: Color32::from_rgb(67, 56, 202),
    on_accent: Color32::WHITE,
    added: Color32::from_rgb(20, 120, 50),
    removed: Color32::from_rgb(180, 40, 30),
    warn: Color32::from_rgb(170, 96, 0),
    error: Color32::from_rgb(190, 50, 34),
    shadow: 0.35,
};

static LIGHT_SHOWING: AtomicBool = AtomicBool::new(false);

/// The colours showing.
pub fn colours() -> &'static Colours {
    if LIGHT_SHOWING.load(Ordering::Relaxed) { &LIGHT } else { &DARK }
}

fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        fonts.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("inter", include_bytes!("../../../assets/fonts/Inter-Regular.ttf"));
    add("inter-semibold", include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf"));
    add("mono", include_bytes!("../../../assets/fonts/JetBrainsMonoNL-Regular.ttf"));
    // The defaults stay behind ours as fallbacks, for symbols.
    fonts.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    fonts.families.entry(FontFamily::Monospace).or_default().insert(0, "mono".into());
    // Then the system's, for scripts ours lack: Chinese, Japanese, Korean.
    for (i, bytes) in system_fallbacks().into_iter().enumerate() {
        let name = format!("system-{i}");
        fonts.font_data.insert(name.clone(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            fonts.families.entry(family).or_default().push(name.clone());
        }
    }
    let fallbacks = fonts.families[&FontFamily::Proportional].clone();
    fonts.families.insert(
        FontFamily::Name(STRONG.into()),
        std::iter::once("inter-semibold".to_string()).chain(fallbacks).collect(),
    );
    ctx.set_fonts(fonts);
}

/// System fonts covering CJK, read from where each OS keeps them: the first
/// found for Chinese (which also has Japanese kana), then one for Korean.
fn system_fallbacks() -> Vec<Vec<u8>> {
    let groups: &[&[&str]] = if cfg!(windows) {
        &[
            &["C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/simsun.ttc", "C:/Windows/Fonts/YuGothR.ttc"],
            &["C:/Windows/Fonts/malgun.ttf"],
        ]
    } else if cfg!(target_os = "macos") {
        &[
            &[
                "/System/Library/Fonts/PingFang.ttc",
                "/System/Library/Fonts/STHeiti Light.ttc",
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
            ],
            &["/System/Library/Fonts/AppleSDGothicNeo.ttc"],
        ]
    } else {
        &[&[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        ]]
    };
    groups.iter().filter_map(|paths| paths.iter().find_map(|p| std::fs::read(p).ok())).collect()
}

const STRONG: &str = "strong";

/// The semibold face, for headings and the wordmark.
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(STRONG.into()))
}

/// Set the fonts and spacing once at startup, then the colours; [`apply`]
/// recolours it later. With `reduced_motion`, nothing animates.
pub fn install(ctx: &egui::Context, light: bool, reduced_motion: bool) {
    install_fonts(ctx);
    ctx.all_styles_mut(|style| {
        use FontFamily::{Monospace, Proportional};
        style.text_styles = [
            (TextStyle::Small, FontId::new(11.5, Proportional)),
            (TextStyle::Body, FontId::new(13.5, Proportional)),
            (TextStyle::Button, FontId::new(13.0, Proportional)),
            (TextStyle::Heading, strong(22.0)),
            (TextStyle::Monospace, FontId::new(12.0, Monospace)),
        ]
        .into();
        let s = &mut style.spacing;
        s.item_spacing = egui::vec2(8.0, 6.0);
        s.button_padding = egui::vec2(11.0, 5.0);
        s.interact_size.y = 26.0;
        s.scroll.bar_width = 6.0;
        style.animation_time = if reduced_motion { 0.0 } else { 0.15 };
    });
    apply(ctx, light);
}

pub fn apply(ctx: &egui::Context, light: bool) {
    LIGHT_SHOWING.store(light, Ordering::Relaxed);
    let theme = if light { egui::Theme::Light } else { egui::Theme::Dark };
    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, visuals(light));
}

fn visuals(light: bool) -> egui::Visuals {
    let c = colours();
    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    let r = CornerRadius::same(7);
    let shadow = |a: f32| Color32::from_black_alpha((a * c.shadow) as u8);
    v.panel_fill = c.card;
    v.window_fill = c.card;
    v.window_stroke = Stroke::new(1.0, c.card_stroke);
    v.window_corner_radius = CornerRadius::same(RADIUS);
    v.menu_corner_radius = CornerRadius::same(10);
    v.popup_shadow = Shadow { offset: [0, 6], blur: 18, spread: 0, color: shadow(100.0) };
    v.extreme_bg_color = c.code;
    v.code_bg_color = c.code;
    v.warn_fg_color = c.warn;
    v.error_fg_color = c.error;
    v.hyperlink_color = c.accent;
    v.selection.bg_fill = c.accent.gamma_multiply(if light { 0.35 } else { 0.6 });
    v.selection.stroke = Stroke::new(1.0, c.text_strong);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = Color32::TRANSPARENT;
    w.noninteractive.weak_bg_fill = Color32::TRANSPARENT;
    w.noninteractive.bg_stroke = Stroke::new(1.0, c.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
    w.noninteractive.corner_radius = r;

    (w.inactive.bg_fill, w.inactive.weak_bg_fill) = c.button;
    w.inactive.bg_stroke = Stroke::NONE;
    w.inactive.fg_stroke = Stroke::new(1.0, c.text);
    w.inactive.corner_radius = r;

    (w.hovered.bg_fill, w.hovered.weak_bg_fill) = c.button_hovered;
    w.hovered.bg_stroke = Stroke::NONE;
    w.hovered.fg_stroke = Stroke::new(1.5, c.text_strong);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = c.accent.gamma_multiply(if light { 0.3 } else { 0.45 });
    w.active.weak_bg_fill = c.accent.gamma_multiply(if light { 0.25 } else { 0.4 });
    w.active.bg_stroke = Stroke::NONE;
    w.active.fg_stroke = Stroke::new(1.5, c.text_strong);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;
    v
}

/// A rounded card for floating things, like the window buttons and notices.
pub fn card() -> egui::Frame {
    let c = colours();
    egui::Frame::new()
        .fill(c.card)
        .stroke(Stroke::new(1.0, c.card_stroke))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::same(12))
        .shadow(Shadow { offset: [0, 6], blur: 22, spread: 0, color: Color32::from_black_alpha((90.0 * c.shadow) as u8) })
}

pub fn pill() -> egui::Frame {
    card().corner_radius(CornerRadius::same(255)).inner_margin(Margin::symmetric(10, 5))
}
