use std::process::Command;
use std::sync::Arc;

use eframe::egui::{self, FontData, FontDefinitions, FontFamily};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FontStyle {
    #[default]
    Serif,
    Sans,
}

impl FontStyle {
    pub fn name(self) -> &'static str {
        match self {
            FontStyle::Serif => "明體",
            FontStyle::Sans => "黑體",
        }
    }

    pub fn toggled(self) -> Self {
        match self {
            FontStyle::Serif => FontStyle::Sans,
            FontStyle::Sans => FontStyle::Serif,
        }
    }
}

/// 載入中文字型。egui 內建字型沒有漢字，不載入的話中文會變成方塊。
pub fn install(ctx: &egui::Context, style: FontStyle, custom: Option<&str>) {
    let mut defs = FontDefinitions::default();
    if let Some((bytes, index)) = find_font(style, custom) {
        let mut data = FontData::from_owned(bytes);
        data.index = index;
        defs.font_data.insert("cjk".to_owned(), Arc::new(data));
        defs.families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "cjk".to_owned());
        defs.families
            .entry(FontFamily::Monospace)
            .or_default()
            .push("cjk".to_owned());
    }
    ctx.set_fonts(defs);
}

fn find_font(style: FontStyle, custom: Option<&str>) -> Option<(Vec<u8>, u32)> {
    if let Some(path) = custom.filter(|p| !p.is_empty()) {
        if let Ok(bytes) = std::fs::read(path) {
            return Some((bytes, 0));
        }
    }

    // Linux / BSD：交給 fontconfig 挑選。偏好細一點的字重，版面比較輕盈
    let pattern = match style {
        FontStyle::Serif => "serif:lang=zh-tw:weight=50",
        FontStyle::Sans => "sans-serif:lang=zh-tw:weight=50",
    };
    if let Ok(out) = Command::new("fc-match")
        .args(["-f", "%{file}\n%{index}", pattern])
        .output()
    {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut lines = s.lines();
        if let (Some(file), Some(index)) = (lines.next(), lines.next()) {
            if let (Ok(bytes), Ok(index)) = (std::fs::read(file), index.trim().parse()) {
                return Some((bytes, index));
            }
        }
    }

    // 常見路徑（.ttc 中索引 3 為繁體中文）
    let candidates: &[(&str, u32)] = match style {
        FontStyle::Serif => &[
            ("/usr/share/fonts/opentype/noto/NotoSerifCJK-Light.ttc", 3),
            ("/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc", 3),
            ("/usr/share/fonts/noto-cjk/NotoSerifCJK-Regular.ttc", 3),
            ("/System/Library/Fonts/Supplemental/Songti.ttc", 0),
            ("C:\\Windows\\Fonts\\mingliu.ttc", 0),
        ],
        FontStyle::Sans => &[
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Light.ttc", 3),
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 3),
            ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 3),
            ("/System/Library/Fonts/PingFang.ttc", 0),
            ("C:\\Windows\\Fonts\\msjh.ttc", 0),
        ],
    };
    let fallback: &[(&str, u32)] = &[
        ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 3),
        ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
        ("C:\\Windows\\Fonts\\msyh.ttc", 0),
    ];
    candidates
        .iter()
        .chain(fallback)
        .find_map(|(p, i)| std::fs::read(p).ok().map(|b| (b, *i)))
}
