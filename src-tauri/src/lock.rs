//! 小黑屋：達成目標之前不能離開。
//!
//! 計時與目標判斷在前端，這裡負責把狀態寫進磁碟，並在鎖定期間擋下關閉視窗與結束程式。
//! 強制結束程式後重新開啟也會接著鎖；計時只在程式執行時累積，所以關掉程式並不能讓時間跑完。

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use crate::config;

static LOCKED: AtomicBool = AtomicBool::new(false);

fn path() -> PathBuf {
    config::data_dir().join("lock.json")
}

pub fn is_locked() -> bool {
    LOCKED.load(Ordering::Relaxed)
}

pub fn load() -> Option<Value> {
    let lock = fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .filter(Value::is_object);
    LOCKED.store(lock.is_some(), Ordering::Relaxed);
    lock
}

pub fn save(lock: &Value) -> io::Result<()> {
    LOCKED.store(true, Ordering::Relaxed);
    config::write_atomic(&path(), &serde_json::to_string_pretty(lock)?)
}

pub fn clear() -> io::Result<()> {
    LOCKED.store(false, Ordering::Relaxed);
    match fs::remove_file(path()) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
