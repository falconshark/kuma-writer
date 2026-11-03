//! 小黑屋：達成目標之前不能離開。
//!
//! 狀態會寫進磁碟，強制結束程式後重新開啟也會接著鎖。
//! 計時只在程式執行時累積，所以關掉程式並不能讓時間跑完。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::config;

/// 開啟緊急出口時需要一字不差輸入的句子
pub const EMERGENCY_PHRASE: &str = "我決定放棄這次的小黑屋，下次會堅持到最後";

#[derive(Serialize, Deserialize, Clone)]
pub struct LockSession {
    pub target_minutes: Option<u32>,
    /// 本次需要新增的字數
    pub target_words: Option<usize>,
    pub elapsed_secs: f64,
    pub start_words: usize,
    pub doc: PathBuf,
    /// 試圖切走或關閉視窗的次數
    pub escapes: u32,
    pub allow_emergency: bool,
}

impl LockSession {
    fn path() -> PathBuf {
        config::data_dir().join("lock.json")
    }

    pub fn load() -> Option<Self> {
        let s = fs::read_to_string(Self::path()).ok()?;
        serde_json::from_str(&s).ok()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = config::write_atomic(&Self::path(), &s);
        }
    }

    pub fn clear() {
        let _ = fs::remove_file(Self::path());
    }

    pub fn time_left(&self) -> Option<f64> {
        self.target_minutes
            .map(|m| (m as f64 * 60.0 - self.elapsed_secs).max(0.0))
    }

    pub fn words_written(&self, current: usize) -> usize {
        current.saturating_sub(self.start_words)
    }

    pub fn words_left(&self, current: usize) -> Option<usize> {
        self.target_words
            .map(|w| w.saturating_sub(self.words_written(current)))
    }

    pub fn is_complete(&self, current: usize) -> bool {
        self.time_left().is_none_or(|t| t <= 0.0) && self.words_left(current).is_none_or(|w| w == 0)
    }

    /// 0.0–1.0，同時設定兩種目標時取較慢的那一個
    pub fn progress(&self, current: usize) -> f32 {
        let t = self
            .target_minutes
            .map(|m| (self.elapsed_secs / (m as f64 * 60.0)) as f32);
        let w = self
            .target_words
            .map(|w| self.words_written(current) as f32 / w.max(1) as f32);
        match (t, w) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => 1.0,
        }
        .clamp(0.0, 1.0)
    }
}
