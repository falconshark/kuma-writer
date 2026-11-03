# Kuma Write

沉浸式寫作軟體，用 Tauri 2 製作（Rust 後端 + HTML/CSS 介面），靈感來自 OmmWriter、FocusWriter 與小黑屋。

舊的 egui 版本放在 `legacy-egui/`。eframe 0.33 以後在部分 Linux 桌面（例如 MATE + X11）收不到鍵盤輸入，所以改用 Tauri。之後移植聲音、統計、小黑屋時會參考舊版。

## 開發

Linux（Debian/Ubuntu）需要先安裝：

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
npm install
```

```sh
npm run dev     # 開發模式
npm run build   # 打包成安裝檔（.deb、.AppImage 等）
```

只想編譯執行檔的話，也可以直接在 `src-tauri/` 裡執行 `cargo build`。

## 結構

- `ui/`：介面，純 HTML/CSS/JS，不需要打包工具
  - `scene.js`：五個場景（雪、夜、紙、森、暮），用 canvas 即時繪製，不用圖檔
  - `main.js`：編輯、自動儲存、浮動選單、面板、快捷鍵
- `src-tauri/`：Rust 後端，負責讀寫文件、設定與匯出

文件存在 `~/文件/KumaWrite/`（依系統的「文件」資料夾而定），格式為純文字 `.md`，停止打字約 1 秒後自動儲存。
設定存在 `~/.local/share/kuma-write/config.json`。

## 快捷鍵

| 快捷鍵 | 功能 |
| --- | --- |
| `Ctrl+O` / `Ctrl+N` | 文件列表 / 新文件 |
| `Ctrl+S` | 立即儲存 |
| `Ctrl+E` | 匯出 TXT（存到文件資料夾下的 `匯出/`） |
| `Ctrl+T` | 切換場景 |
| `Ctrl+=` / `Ctrl+-` | 放大 / 縮小字級 |
| `F9` | 打字機模式 |
| `F11` | 全螢幕 |
| `Esc` | 關閉面板；沒有面板時讓選單常駐 |
| `Ctrl+Q` | 離開 |

## 待移植

背景音與打字音效、每日統計、小黑屋、專注段落（F8）。
