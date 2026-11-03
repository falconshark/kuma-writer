//! 背景的景：漸層天空、光暈、星星、層層遠山與霧，再蓋上一層顆粒與暗角。
//! 不用任何圖檔，視窗大小改變時依比例重畫。

use std::f32::consts::TAU;

use eframe::egui::{
    self, pos2, vec2, Color32, ColorImage, Mesh, Painter, Pos2, Rect, Shape, TextureHandle,
    TextureOptions, TextureWrapMode,
};

use crate::theme::{mix, Scene};

const GRAIN_SIZE: usize = 192;

/// 灰階雜訊貼圖，平鋪在整個畫面上，讓漸層不會出現色帶，也多一點紙張般的質感
pub fn grain_texture(ctx: &egui::Context) -> TextureHandle {
    let mut seed = 0x9E37_79B9_u32;
    let pixels = (0..GRAIN_SIZE * GRAIN_SIZE)
        .map(|_| {
            let v = (xorshift(&mut seed) >> 24) as u8;
            Color32::from_gray(v)
        })
        .collect();
    let image = ColorImage::new([GRAIN_SIZE, GRAIN_SIZE], pixels);
    let options = TextureOptions {
        wrap_mode: TextureWrapMode::Repeat,
        ..TextureOptions::NEAREST
    };
    ctx.load_texture("kuma-grain", image, options)
}

pub fn paint(painter: &Painter, rect: Rect, s: &Scene, grain: &TextureHandle) {
    let h = rect.height();
    let horizon = rect.top() + h * s.horizon;

    let mut mesh = Mesh::default();
    vgradient(&mut mesh, rect.left(), rect.right(), rect.top(), horizon, s.sky[0], s.sky[1]);
    if horizon < rect.bottom() {
        vgradient(&mut mesh, rect.left(), rect.right(), horizon, rect.bottom(), s.sky[1], s.fog);
    }
    if let Some((color, [gx, gy], r)) = s.glow {
        let center = pos2(rect.left() + rect.width() * gx, rect.top() + h * gy);
        glow(&mut mesh, center, h * r, color);
    }
    painter.add(Shape::mesh(mesh));

    if s.stars {
        stars(painter, rect, s);
    }

    let mut mesh = Mesh::default();
    let n = s.hills.len();
    for (i, &color) in s.hills.iter().enumerate() {
        // 越近的山越低、起伏越大，山腳淡入霧中
        let depth = i as f32 / n.max(1) as f32;
        let base = horizon + h * (0.02 + 0.09 * depth);
        let amp = h * (0.05 + 0.035 * depth);
        ridge(&mut mesh, rect, base, amp, s.seed.wrapping_add(i as u32 * 17), color, s.fog);
    }
    painter.add(Shape::mesh(mesh));

    vignette(painter, rect, s.vignette);

    let mut mesh = Mesh::with_texture(grain.id());
    let uv = Rect::from_min_size(Pos2::ZERO, rect.size() / GRAIN_SIZE as f32);
    mesh.add_rect_with_uv(rect, uv, Color32::from_white_alpha((s.grain * 255.0) as u8));
    painter.add(Shape::mesh(mesh));
}

fn vgradient(mesh: &mut Mesh, x0: f32, x1: f32, y0: f32, y1: f32, top: Color32, bottom: Color32) {
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(pos2(x0, y0), top);
    mesh.colored_vertex(pos2(x1, y0), top);
    mesh.colored_vertex(pos2(x1, y1), bottom);
    mesh.colored_vertex(pos2(x0, y1), bottom);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
}

/// 中心最亮、向外平滑淡出的圓形光暈
fn glow(mesh: &mut Mesh, center: Pos2, radius: f32, color: Color32) {
    const RINGS: usize = 24;
    const SEGMENTS: usize = 64;
    let alpha_at = |t: f32| {
        let f = 1.0 - t;
        color.gamma_multiply(f * f * f)
    };
    let start = mesh.vertices.len() as u32;
    mesh.colored_vertex(center, color);
    for ring in 1..=RINGS {
        let t = ring as f32 / RINGS as f32;
        // 內圈取樣較密，光暈中心才不會有一圈一圈的痕跡
        let r = radius * t * t;
        for k in 0..SEGMENTS {
            let a = k as f32 / SEGMENTS as f32 * TAU;
            mesh.colored_vertex(center + vec2(a.cos(), a.sin()) * r, alpha_at(t * t));
        }
    }
    let ring_start = |ring: usize| start + 1 + ((ring - 1) * SEGMENTS) as u32;
    for k in 0..SEGMENTS as u32 {
        let k2 = (k + 1) % SEGMENTS as u32;
        mesh.add_triangle(start, ring_start(1) + k, ring_start(1) + k2);
        for ring in 1..RINGS {
            let (a, b) = (ring_start(ring), ring_start(ring + 1));
            mesh.add_triangle(a + k, b + k, b + k2);
            mesh.add_triangle(a + k, b + k2, a + k2);
        }
    }
}

fn stars(painter: &Painter, rect: Rect, s: &Scene) {
    let mut seed = s.seed.wrapping_mul(2654435761).max(1);
    let count = (rect.width() * rect.height() / 9000.0) as usize;
    let mut rand = || (xorshift(&mut seed) >> 8) as f32 / (1u32 << 24) as f32;
    for _ in 0..count {
        let x = rect.left() + rand() * rect.width();
        // 越接近地平線越稀疏
        let y = rect.top() + rand() * rand() * rect.height() * s.horizon * 0.9;
        let size = 0.4 + rand() * rand() * 1.1;
        let alpha = 0.08 + rand() * rand() * 0.4;
        painter.circle_filled(pos2(x, y), size, Color32::from_white_alpha((alpha * 255.0) as u8));
    }
}

/// 一道山稜：幾個不同頻率的正弦波疊在一起，稜線以下往霧色過渡
fn ridge(mesh: &mut Mesh, rect: Rect, base: f32, amp: f32, seed: u32, color: Color32, fog: Color32) {
    let mut seed = seed.wrapping_mul(2246822519).max(1);
    let mut rand = || (xorshift(&mut seed) >> 8) as f32 / (1u32 << 24) as f32;
    let waves: [(f32, f32, f32); 4] = [
        (0.55, 0.6 + rand() * 0.8, rand() * TAU),
        (0.25, 1.6 + rand() * 1.4, rand() * TAU),
        (0.14, 3.5 + rand() * 2.5, rand() * TAU),
        (0.06, 9.0 + rand() * 6.0, rand() * TAU),
    ];
    let height = |x: f32| {
        waves
            .iter()
            .map(|&(a, f, p)| a * (x * f * TAU + p).sin())
            .sum::<f32>()
    };

    let bottom = rect.bottom();
    let foot = mix(color, fog, 0.45);
    let step = (rect.width() / 160.0).max(2.0);
    let start = mesh.vertices.len() as u32;
    let mut x = rect.left();
    let mut cols = 0;
    loop {
        let t = (x - rect.left()) / rect.width();
        let y = (base - amp * height(t)).min(bottom);
        mesh.colored_vertex(pos2(x, y), color);
        mesh.colored_vertex(pos2(x, bottom), foot);
        cols += 1;
        if x >= rect.right() {
            break;
        }
        x = (x + step).min(rect.right());
    }
    for c in 0..cols - 1 {
        let i = start + c * 2;
        mesh.add_triangle(i, i + 2, i + 1);
        mesh.add_triangle(i + 1, i + 2, i + 3);
    }
}

/// 四周微微變暗，視線自然落在中間
fn vignette(painter: &Painter, rect: Rect, strength: f32) {
    if strength <= 0.0 {
        return;
    }
    const SEGMENTS: usize = 72;
    let c = rect.center();
    let (w, h) = (rect.width(), rect.height());
    let edge = Color32::from_black_alpha((strength * 255.0) as u8);
    let mut mesh = Mesh::default();
    // 內圈透明，外圈涵蓋到四個角落
    for k in 0..SEGMENTS {
        let a = k as f32 / SEGMENTS as f32 * TAU;
        let d = vec2(a.cos(), a.sin());
        mesh.colored_vertex(c + vec2(d.x * w * 0.36, d.y * h * 0.36), Color32::TRANSPARENT);
        mesh.colored_vertex(c + vec2(d.x * w * 0.75, d.y * h * 0.75), edge);
    }
    for k in 0..SEGMENTS as u32 {
        let k2 = (k + 1) % SEGMENTS as u32;
        let (a, b, c, d) = (k * 2, k * 2 + 1, k2 * 2, k2 * 2 + 1);
        mesh.add_triangle(a, b, d);
        mesh.add_triangle(a, d, c);
    }
    painter.add(Shape::mesh(mesh));
}

fn xorshift(s: &mut u32) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}
