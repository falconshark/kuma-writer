use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use serde::Serialize;

use crate::config;

#[derive(Serialize)]
pub struct DocEntry {
    pub path: PathBuf,
    pub title: String,
    pub modified: String,
}

/// 依給定的順序列出文件資訊，讀不到的（被移走、刪掉）略過
pub fn entries(paths: &[PathBuf]) -> Vec<DocEntry> {
    paths
        .iter()
        .filter_map(|path| {
            let mtime = fs::metadata(path).and_then(|m| m.modified()).ok()?;
            let text = fs::read_to_string(path).ok()?;
            let modified = DateTime::<Local>::from(mtime).format("%m/%d %H:%M").to_string();
            Some(DocEntry { title: title_of(&text), path: path.clone(), modified })
        })
        .collect()
}

pub fn load(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
}

pub fn save(path: &Path, text: &str) -> io::Result<()> {
    config::write_atomic(path, text)
}

/// 還沒選好存檔位置的新文件，每篇一個草稿檔，存在程式自己的資料夾，當機或關掉也不會遺失。
/// id 由前端產生，只允許英數字，避免組出奇怪的路徑
fn draft_path(id: &str) -> io::Result<PathBuf> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "草稿編號不正確"));
    }
    Ok(config::data_dir().join("drafts").join(format!("{id}.txt")))
}

/// 只能開一篇草稿的舊版本留下來的檔案
fn legacy_drafts() -> [PathBuf; 2] {
    let dir = config::data_dir();
    [dir.join("draft.txt"), dir.join("draft.md")]
}

pub fn load_draft(id: &str) -> io::Result<String> {
    match fs::read_to_string(draft_path(id)?) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        r => r,
    }
}

/// 舊版的單一草稿：讀出來交給前端轉成新的草稿檔，之後就刪掉
pub fn take_legacy_draft() -> String {
    legacy_drafts()
        .iter()
        .find_map(|p| fs::read_to_string(p).ok())
        .unwrap_or_default()
}

pub fn clear_legacy_drafts() {
    for p in legacy_drafts() {
        let _ = fs::remove_file(p);
    }
}

pub fn save_draft(id: &str, text: &str) -> io::Result<()> {
    let path = draft_path(id)?;
    if text.is_empty() {
        match fs::remove_file(path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    } else {
        config::write_atomic(&path, text)
    }
}

/// 拿標題當預設檔名時，換掉檔名不能用的字元
pub fn safe_file_name(name: &str) -> String {
    let name: String = name
        .trim()
        .trim_end_matches('…')
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    if name.is_empty() || name == "（空白）" {
        "未命名".to_owned()
    } else {
        name
    }
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

    #[test]
    fn draft_ids_cannot_escape_the_folder() {
        assert!(draft_path("abc123").is_ok());
        for bad in ["", "../x", "a/b", "a.b"] {
            assert!(draft_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn file_name_is_safe() {
        assert_eq!(safe_file_name("a/b: c?"), "a_b_ c_");
        assert_eq!(safe_file_name("很長的標題…"), "很長的標題");
        assert_eq!(safe_file_name("（空白）"), "未命名");
    }
}
