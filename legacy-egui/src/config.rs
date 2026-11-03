use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::audio::{Ambient, KeySound};
use crate::fonts::FontStyle;
use crate::theme::ThemeKind;

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kuma-write")
}

pub fn docs_dir() -> PathBuf {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("KumaWrite")
}

/// 先寫入暫存檔再改名，避免寫到一半當機時毀掉原檔。
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, contents)?;
    fs::rename(&tmp, path)
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub theme: ThemeKind,
    pub font_style: FontStyle,
    /// 自訂字型檔路徑，留空則自動尋找系統的中文字型
    pub font_path: Option<String>,
    pub font_size: f32,
    pub line_spacing: f32,
    pub column_width: f32,
    pub focus_paragraph: bool,
    pub typewriter: bool,
    pub start_fullscreen: bool,
    pub last_doc: Option<PathBuf>,
    pub lock_minutes: u32,
    pub lock_words: usize,
    pub ambient: Ambient,
    pub ambient_volume: f32,
    pub key_sound: KeySound,
    pub key_volume: f32,
    /// 每日字數目標
    pub daily_goal: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            theme: ThemeKind::default(),
            font_style: FontStyle::default(),
            font_path: None,
            font_size: 22.0,
            line_spacing: 1.8,
            column_width: 720.0,
            focus_paragraph: false,
            typewriter: true,
            start_fullscreen: true,
            last_doc: None,
            lock_minutes: 30,
            lock_words: 500,
            ambient: Ambient::default(),
            ambient_volume: 0.5,
            key_sound: KeySound::default(),
            key_volume: 0.5,
            daily_goal: 1000,
        }
    }
}

impl Config {
    fn path() -> PathBuf {
        data_dir().join("config.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = write_atomic(&Self::path(), &s);
        }
    }
}
