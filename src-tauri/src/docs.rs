use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::{DateTime, Local};
use serde::Serialize;

use crate::config;

#[derive(Serialize)]
pub struct DocEntry {
    pub path: PathBuf,
    pub title: String,
    pub modified: String,
}

/// 最近修改的排在前面
pub fn list() -> Vec<DocEntry> {
    let Ok(rd) = fs::read_dir(config::docs_dir()) else {
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

pub fn new_doc_path() -> PathBuf {
    let dir = config::docs_dir();
    let _ = fs::create_dir_all(&dir);
    let stamp = Local::now().format("%Y%m%d-%H%M%S");
    dir.join(format!("{stamp}.md"))
}

pub fn load(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

pub fn save(path: &Path, text: &str) -> io::Result<()> {
    config::write_atomic(path, text)
}

/// 匯出成 TXT，放在文件資料夾下的「匯出」，檔名取自標題
pub fn export_txt(text: &str, name: &str) -> io::Result<PathBuf> {
    let mut name: String = name
        .trim()
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    if name.is_empty() {
        name = "未命名".to_owned();
    }
    let path = config::docs_dir().join("匯出").join(format!("{name}.txt"));
    let mut text = text.to_owned();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    config::write_atomic(&path, &text)?;
    Ok(path)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_skips_blank_lines_and_headings() {
        assert_eq!(title_of("\n\n# 雪後的早晨\n內文"), "雪後的早晨");
        assert_eq!(title_of("   "), "（空白）");
    }
}
