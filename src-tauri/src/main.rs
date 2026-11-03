#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod docs;
mod journal;
mod lock;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, RunEvent, State, WindowEvent};
use tauri_plugin_dialog::DialogExt;

use audio::{Ambient, Audio, KeyKind, KeySound};
use journal::Journal;

type CmdResult<T> = Result<T, String>;

fn to_msg(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct Init {
    config: Value,
    /// 只能開一篇草稿的舊版本留下的草稿內容，前端會轉成新的草稿檔
    legacy_draft: String,
    /// 還沒結束的小黑屋
    lock: Option<Value>,
    audio_error: Option<String>,
}

/// 啟動時前端要的一切：設定與小黑屋狀態。要開哪些分頁由前端依設定決定
#[tauri::command]
fn init(audio: State<Audio>) -> Init {
    Init {
        config: config::load(),
        legacy_draft: docs::take_legacy_draft(),
        lock: lock::load(),
        audio_error: audio.error.clone(),
    }
}

/// 最近開過的文件，已經不存在的會略過
#[tauri::command]
fn recent_docs(paths: Vec<PathBuf>) -> Vec<docs::DocEntry> {
    docs::entries(&paths)
}

#[tauri::command]
fn open_doc(path: PathBuf) -> CmdResult<String> {
    docs::load(&path).map_err(to_msg)
}

#[tauri::command]
fn save_doc(path: PathBuf, text: String) -> CmdResult<()> {
    docs::save(&path, &text).map_err(to_msg)
}

#[tauri::command]
fn load_draft(id: String) -> CmdResult<String> {
    docs::load_draft(&id).map_err(to_msg)
}

#[tauri::command]
fn save_draft(id: String, text: String) -> CmdResult<()> {
    docs::save_draft(&id, &text).map_err(to_msg)
}

/// 舊版的單一草稿已經轉成新的草稿檔之後呼叫
#[tauri::command]
fn clear_legacy_draft() {
    docs::clear_legacy_drafts();
}

/// 檔案還在不在（分頁還原時，被移走或刪掉的就不開）
#[tauri::command]
fn doc_exists(path: PathBuf) -> bool {
    path.is_file()
}

#[tauri::command]
fn save_config(config: Value) -> CmdResult<()> {
    config::save(&config).map_err(to_msg)
}

#[tauri::command]
fn audio_settings(audio: State<Audio>, ambient: Ambient, ambient_volume: f32, key_sound: KeySound, key_volume: f32) {
    audio.set_ambient(ambient);
    audio.set_ambient_volume(ambient_volume);
    audio.set_key_sound(key_sound);
    audio.set_key_volume(key_volume);
}

#[tauri::command]
fn audio_key(audio: State<Audio>, kind: KeyKind) {
    audio.key(kind);
}

/// 累加今天的淨增字數與寫作秒數，回傳今天累計的字數
#[tauri::command]
fn journal_add(journal: State<Mutex<Journal>>, words: i64, secs: f64) -> usize {
    let mut j = journal.lock().unwrap();
    j.add_words(words);
    if secs > 0.0 {
        j.add_secs(secs);
    }
    j.save();
    j.day(journal::today()).shown_words()
}

#[derive(Serialize)]
struct Summary {
    today: journal::Day,
    streak: usize,
    /// 由舊到新，最後一天是今天
    recent: Vec<(chrono::NaiveDate, journal::Day)>,
    best: Option<(chrono::NaiveDate, usize)>,
}

#[tauri::command]
fn journal_summary(journal: State<Mutex<Journal>>) -> Summary {
    let j = journal.lock().unwrap();
    Summary {
        today: j.day(journal::today()),
        streak: j.streak(),
        recent: j.recent(30),
        best: j.best(),
    }
}

#[tauri::command]
fn lock_save(lock: Value) -> CmdResult<()> {
    lock::save(&lock).map_err(to_msg)
}

#[tauri::command]
fn lock_clear() -> CmdResult<()> {
    lock::clear().map_err(to_msg)
}

/// 對話框的起始資料夾：上次用過的，沒有的話用系統的「文件」資料夾
fn start_dir(dir: Option<PathBuf>) -> Option<PathBuf> {
    dir.filter(|d| d.is_dir())
        .or_else(dirs::document_dir)
        .or_else(dirs::home_dir)
}

/// 問使用者要存在哪裡，dir 是上次用過的資料夾。文件一律存成純文字 .txt
#[tauri::command]
async fn pick_save_path(app: AppHandle, name: String, dir: Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("儲存文件")
        .add_filter("純文字", &["txt"])
        .set_file_name(format!("{}.txt", docs::safe_file_name(&name)));
    if let Some(dir) = start_dir(dir) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    let mut path = dialog.blocking_save_file()?.into_path().ok()?;
    // 有些平台的對話框不會自動補上副檔名
    if path.extension().is_none() {
        path.set_extension("txt");
    }
    Some(path)
}

#[tauri::command]
async fn pick_open_path(app: AppHandle, dir: Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = app
        .dialog()
        .file()
        .set_title("開啟文件")
        .add_filter("文字檔", &["txt", "md", "markdown"]);
    if let Some(dir) = start_dir(dir) {
        dialog = dialog.set_directory(dir);
    }
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    dialog.blocking_pick_file()?.into_path().ok()
}

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage({
            let audio = Audio::new();
            if let Some(e) = &audio.error {
                eprintln!("音訊無法啟動：{e}");
            }
            audio
        })
        .manage(Mutex::new(Journal::load()))
        .setup(|app| {
            // 小黑屋未結束時一律全螢幕
            let fullscreen = lock::load().is_some()
                || config::load()
                    .get("start_fullscreen")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
            if fullscreen {
                if let Some(w) = app.get_webview_window("main") {
                    w.set_fullscreen(true)?;
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            init,
            recent_docs,
            open_doc,
            save_doc,
            save_draft,
            load_draft,
            clear_legacy_draft,
            doc_exists,
            save_config,
            pick_save_path,
            pick_open_path,
            audio_settings,
            audio_key,
            journal_add,
            journal_summary,
            lock_save,
            lock_clear
        ])
        // 小黑屋期間擋下關閉視窗（前端也會擋，這裡是保險）
        .on_window_event(|_, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if lock::is_locked() {
                    api.prevent_close();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("Kuma Write 無法啟動");

    // 已經請前端存檔、準備結束了
    static QUITTING: AtomicBool = AtomicBool::new(false);

    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            // macOS 的 Cmd+Q 等直接結束程式的方式，小黑屋期間也要擋
            if lock::is_locked() {
                api.prevent_exit();
            } else if app.get_webview_window("main").is_some() && !QUITTING.swap(true, Ordering::Relaxed) {
                // 視窗還開著就要結束：先讓前端把最後的內容存好，再由前端關閉視窗
                api.prevent_exit();
                let _ = app.emit("quit-requested", ());
            }
        }
    });
}
