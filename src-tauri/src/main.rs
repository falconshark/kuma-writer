#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod docs;

use std::path::PathBuf;

use serde::Serialize;
use serde_json::Value;
use tauri::Manager;

type CmdResult<T> = Result<T, String>;

fn to_msg(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct Init {
    config: Value,
    docs_dir: PathBuf,
    doc_path: PathBuf,
    text: String,
}

/// 啟動時前端要的一切：設定，以及上次寫到一半的文件（沒有就開新的）
#[tauri::command]
fn init() -> Init {
    let config = config::load();
    let doc_path = config
        .get("last_doc")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .filter(|p| p.exists())
        .unwrap_or_else(docs::new_doc_path);
    let text = docs::load(&doc_path).unwrap_or_default();
    Init {
        config,
        docs_dir: config::docs_dir(),
        doc_path,
        text,
    }
}

#[tauri::command]
fn list_docs() -> Vec<docs::DocEntry> {
    docs::list()
}

#[tauri::command]
fn open_doc(path: PathBuf) -> CmdResult<String> {
    docs::load(&path).map_err(to_msg)
}

#[tauri::command]
fn new_doc() -> PathBuf {
    docs::new_doc_path()
}

#[tauri::command]
fn save_doc(path: PathBuf, text: String) -> CmdResult<()> {
    // 空白的新文件不寫檔，免得留下一堆空檔案
    if text.is_empty() && !path.exists() {
        return Ok(());
    }
    docs::save(&path, &text).map_err(to_msg)
}

#[tauri::command]
fn save_config(config: Value) -> CmdResult<()> {
    config::save(&config).map_err(to_msg)
}

#[tauri::command]
fn export_txt(text: String, name: String) -> CmdResult<PathBuf> {
    docs::export_txt(&text, &name).map_err(to_msg)
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let fullscreen = config::load()
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
            list_docs,
            open_doc,
            new_doc,
            save_doc,
            save_config,
            export_txt
        ])
        .run(tauri::generate_context!())
        .expect("Kuma Write 無法啟動");
}
