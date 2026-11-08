use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use serde_json::Value;

pub fn data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kuma-write")
}

/// 先寫入暫存檔再改名，避免寫到一半當機時毀掉原檔。
/// Windows 上檔案若剛好被 Dropbox 之類的同步程式打開，改名會暫時失敗，所以重試幾次
pub fn write_atomic(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    // 暫存檔用完整檔名加後綴，不會和使用者自己的檔案撞名
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".kuma-tmp");
    let tmp = path.with_file_name(name);
    fs::write(&tmp, contents)?;
    let mut tries = 0;
    loop {
        match fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if tries < 5 && e.kind() == io::ErrorKind::PermissionDenied => {
                tries += 1;
                thread::sleep(Duration::from_millis(80));
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                return Err(e);
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_atomic_replaces_and_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("kuma-write-test-{}", std::process::id()));
        let path = dir.join("note.txt");
        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
        let names: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("note.txt")]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
