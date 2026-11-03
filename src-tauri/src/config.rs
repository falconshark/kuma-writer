use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value;

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

fn config_path() -> PathBuf {
    data_dir().join("config.json")
}

/// 設定的內容由前端決定，這裡只負責讀寫；讀不到時回傳空物件，由前端補上預設值。
pub fn load() -> Value {
    fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| Value::Object(Default::default()))
}

pub fn save(config: &Value) -> io::Result<()> {
    let s = serde_json::to_string_pretty(config)?;
    write_atomic(&config_path(), &s)
}
