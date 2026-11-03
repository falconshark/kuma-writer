//! 每日寫作統計：每天新增的字數與實際打字的時間。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use chrono::{Days, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::config;

#[derive(Serialize, Deserialize, Default, Clone, Copy)]
pub struct Day {
    /// 當天的淨增字數（刪除會扣回來）
    pub words: i64,
    /// 實際在寫的秒數（停筆超過一段時間不算）
    pub secs: f64,
}

impl Day {
    pub fn shown_words(&self) -> usize {
        self.words.max(0) as usize
    }
}

#[derive(Serialize, Deserialize, Default)]
pub struct Journal {
    pub days: BTreeMap<NaiveDate, Day>,
    #[serde(skip)]
    pub changed: bool,
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

impl Journal {
    fn path() -> PathBuf {
        config::data_dir().join("journal.json")
    }

    pub fn load() -> Self {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&mut self) {
        if !self.changed {
            return;
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            if config::write_atomic(&Self::path(), &s).is_ok() {
                self.changed = false;
            }
        }
    }

    pub fn add_words(&mut self, delta: i64) {
        if delta != 0 {
            self.days.entry(today()).or_default().words += delta;
            self.changed = true;
        }
    }

    pub fn add_secs(&mut self, secs: f64) {
        self.days.entry(today()).or_default().secs += secs;
        self.changed = true;
    }

    pub fn day(&self, date: NaiveDate) -> Day {
        self.days.get(&date).copied().unwrap_or_default()
    }

    /// 由舊到新，最後一天是今天
    pub fn recent(&self, n: u64) -> Vec<(NaiveDate, Day)> {
        let t = today();
        (0..n)
            .rev()
            .filter_map(|i| t.checked_sub_days(Days::new(i)))
            .map(|d| (d, self.day(d)))
            .collect()
    }

    /// 連續有寫字的天數。今天還沒寫不會中斷紀錄。
    pub fn streak(&self) -> usize {
        let mut d = today();
        if self.day(d).shown_words() == 0 {
            d = d.pred_opt().unwrap_or(d);
        }
        let mut n = 0;
        while self.day(d).shown_words() > 0 {
            n += 1;
            match d.pred_opt() {
                Some(p) => d = p,
                None => break,
            }
        }
        n
    }

    pub fn best(&self) -> Option<(NaiveDate, usize)> {
        self.days
            .iter()
            .map(|(d, day)| (*d, day.shown_words()))
            .filter(|(_, w)| *w > 0)
            .max_by_key(|(_, w)| *w)
    }
}

pub fn fmt_minutes(secs: f64) -> String {
    let m = (secs / 60.0).round() as u64;
    if m >= 60 {
        format!("{} 小時 {} 分", m / 60, m % 60)
    } else {
        format!("{m} 分鐘")
    }
}
