use eframe::egui::{self, Color32, CornerRadius, Shadow, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeKind {
    #[default]
    Snow,
    Night,
    Paper,
    Forest,
    Dusk,
}

/// 背景的「景」：漸層天空、光暈、遠山與霧。全部由程式繪製，見 scene.rs。
#[derive(Clone, Copy)]
pub struct Scene {
    /// 天頂與地平線的顏色
    pub sky: [Color32; 2],
    /// 地平線位置（畫面高度的比例）
    pub horizon: f32,
    /// 日或月的光暈：顏色、位置（畫面比例）、半徑（畫面高度的比例）
    pub glow: Option<(Color32, [f32; 2], f32)>,
    /// 由遠到近的山稜，空的就是一張紙
    pub hills: &'static [Color32],
    /// 山腳下的霧色
    pub fog: Color32,
    pub stars: bool,
    /// 顆粒感與暗角的強度（0–1）
    pub grain: f32,
    pub vignette: f32,
    pub seed: u32,
}

#[derive(Clone, Copy)]
pub struct Palette {
    /// 背景的代表色，用來調配其他顏色
    pub bg: Color32,
    pub text: Color32,
    /// 專注段落模式下，非目前段落的文字顏色（半透明，才能透出背景）
    pub dim: Color32,
    pub accent: Color32,
    /// 浮動選單與對話框的底色（不透明，用來調色）
    pub hud: Color32,
    /// 實際畫出來的面板底色
    pub panel: Color32,
    pub dark: bool,
    pub scene: Scene,
}

const SNOW_HILLS: &[Color32] = &[rgb(203, 211, 220), rgb(219, 225, 231), rgb(236, 239, 242)];
const NIGHT_HILLS: &[Color32] = &[rgb(30, 36, 54), rgb(22, 27, 42), rgb(15, 18, 29)];
const FOREST_HILLS: &[Color32] = &[rgb(44, 60, 53), rgb(33, 46, 40), rgb(23, 33, 29)];
const DUSK_HILLS: &[Color32] = &[rgb(104, 76, 98), rgb(74, 55, 78), rgb(46, 36, 54)];

impl ThemeKind {
    pub const ALL: [ThemeKind; 5] = [
        ThemeKind::Snow,
        ThemeKind::Night,
        ThemeKind::Paper,
        ThemeKind::Forest,
        ThemeKind::Dusk,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ThemeKind::Snow => "雪",
            ThemeKind::Night => "夜",
            ThemeKind::Paper => "紙",
            ThemeKind::Forest => "森",
            ThemeKind::Dusk => "暮",
        }
    }

    pub fn next(self) -> Self {
        let i = Self::ALL.iter().position(|&k| k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    pub fn palette(self) -> Palette {
        let (text, accent, dark, scene) = match self {
            ThemeKind::Snow => (
                rgb(66, 74, 86),
                rgb(104, 138, 170),
                false,
                Scene {
                    sky: [rgb(201, 212, 224), rgb(234, 237, 240)],
                    horizon: 0.66,
                    glow: Some((Color32::from_rgba_unmultiplied(255, 250, 240, 150), [0.74, 0.40], 0.5)),
                    hills: SNOW_HILLS,
                    fog: rgb(238, 240, 243),
                    stars: false,
                    grain: 0.035,
                    vignette: 0.08,
                    seed: 7,
                },
            ),
            ThemeKind::Night => (
                rgb(200, 206, 218),
                rgb(140, 170, 230),
                true,
                Scene {
                    sky: [rgb(11, 14, 24), rgb(30, 37, 56)],
                    horizon: 0.7,
                    glow: Some((Color32::from_rgba_unmultiplied(150, 172, 215, 60), [0.8, 0.2], 0.4)),
                    hills: NIGHT_HILLS,
                    fog: rgb(34, 41, 60),
                    stars: true,
                    grain: 0.03,
                    vignette: 0.35,
                    seed: 3,
                },
            ),
            ThemeKind::Paper => (
                rgb(60, 54, 48),
                rgb(176, 108, 62),
                false,
                Scene {
                    sky: [rgb(247, 243, 235), rgb(238, 231, 218)],
                    horizon: 1.0,
                    glow: Some((Color32::from_rgba_unmultiplied(255, 252, 245, 120), [0.5, 0.3], 0.7)),
                    hills: &[],
                    fog: rgb(238, 231, 218),
                    stars: false,
                    grain: 0.06,
                    vignette: 0.12,
                    seed: 1,
                },
            ),
            ThemeKind::Forest => (
                rgb(206, 216, 202),
                rgb(156, 196, 138),
                true,
                Scene {
                    sky: [rgb(20, 29, 27), rgb(52, 68, 60)],
                    horizon: 0.68,
                    glow: Some((Color32::from_rgba_unmultiplied(214, 222, 170, 45), [0.76, 0.64], 0.45)),
                    hills: FOREST_HILLS,
                    fog: rgb(56, 72, 64),
                    stars: false,
                    grain: 0.035,
                    vignette: 0.3,
                    seed: 11,
                },
            ),
            ThemeKind::Dusk => (
                rgb(236, 224, 232),
                rgb(240, 168, 146),
                true,
                Scene {
                    sky: [rgb(38, 32, 56), rgb(132, 92, 106)],
                    horizon: 0.7,
                    glow: Some((Color32::from_rgba_unmultiplied(255, 186, 150, 90), [0.66, 0.68], 0.5)),
                    hills: DUSK_HILLS,
                    fog: rgb(128, 92, 108),
                    stars: false,
                    grain: 0.035,
                    vignette: 0.3,
                    seed: 5,
                },
            ),
        };
        let bg = mix(scene.sky[0], scene.sky[1], 0.4);
        let hud = if dark { mix(bg, text, 0.06) } else { mix(bg, Color32::WHITE, 0.6) };
        Palette {
            bg,
            text,
            dim: text.gamma_multiply(0.3),
            accent,
            hud,
            panel: hud,
            dark,
            scene,
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if p.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    // 介面文字比內文淡一點，滑過時才變清楚
    let soft = mix(p.hud, p.text, 0.62);
    let faint = mix(p.hud, p.text, 0.14);

    v.panel_fill = p.bg;
    v.window_fill = p.panel;
    v.extreme_bg_color = mix(p.hud, p.text, 0.06);
    v.faint_bg_color = mix(p.hud, p.text, 0.04);
    v.override_text_color = None;
    v.weak_text_color = Some(mix(p.hud, p.text, 0.45));
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(1.5, p.accent);
    v.slider_trailing_fill = true;

    v.window_corner_radius = CornerRadius::same(16);
    v.menu_corner_radius = CornerRadius::same(12);
    v.window_stroke = Stroke::new(1.0, faint);
    let shadow_a = if p.dark { 110 } else { 40 };
    v.window_shadow = Shadow {
        offset: [0, 12],
        blur: 40,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(shadow_a / 2),
    };

    let w = &mut v.widgets;
    w.noninteractive.fg_stroke = Stroke::new(1.0, soft);
    w.noninteractive.bg_stroke = Stroke::new(1.0, faint);
    for (state, fg, bg) in [
        (&mut w.inactive, soft, 0.08),
        (&mut w.hovered, p.text, 0.14),
        (&mut w.active, p.text, 0.18),
        (&mut w.open, p.text, 0.12),
    ] {
        state.fg_stroke = Stroke::new(1.0, fg);
        state.bg_fill = mix(p.hud, p.text, bg);
        state.weak_bg_fill = mix(p.hud, p.text, bg);
        state.bg_stroke = Stroke::NONE;
        state.corner_radius = CornerRadius::same(8);
        state.expansion = 0.0;
    }
    ctx.set_visuals(v);
}
