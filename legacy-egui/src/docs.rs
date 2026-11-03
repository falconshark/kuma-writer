use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local};

use crate::config;

pub struct DocEntry {
    pub path: PathBuf,
    pub title: String,
    pub modified: String,
}

pub struct DocStore {
    pub dir: PathBuf,
}

impl DocStore {
    pub fn new() -> Self {
        let dir = config::docs_dir();
        let _ = fs::create_dir_all(&dir);
        Self { dir }
    }

    /// 最近修改的排在前面
    pub fn list(&self) -> Vec<DocEntry> {
        let Ok(rd) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut items: Vec<(SystemTime, DocEntry)> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("md" | "txt")))
            .filter_map(|path| {
                let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
                let text = fs::read_to_string(&path).unwrap_or_default();
                let modified = DateTime::<Local>::from(mtime).format("%m/%d %H:%M").to_string();
                Some((mtime, DocEntry { title: title_of(&text), path, modified }))
            })
            .collect();
        items.sort_by(|a, b| b.0.cmp(&a.0));
        items.into_iter().map(|(_, e)| e).collect()
    }

    pub fn new_doc_path(&self) -> PathBuf {
        let stamp = Local::now().format("%Y%m%d-%H%M%S");
        self.dir.join(format!("{stamp}.md"))
    }
}

pub fn load(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

pub fn save(path: &Path, text: &str) -> io::Result<()> {
    config::write_atomic(path, text)
}

/// 以第一個非空白行當作標題
pub fn title_of(text: &str) -> String {
    let line = text
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty());
    match line {
        Some(l) if l.chars().count() > 24 => format!("{}…", l.chars().take(24).collect::<String>()),
        Some(l) => l.to_owned(),
        None => "（空白）".to_owned(),
    }
}
