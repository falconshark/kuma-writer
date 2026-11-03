# Kuma Write

沉浸式寫作軟體，用 Tauri 2 製作（Rust 後端 + HTML/CSS 介面），靈感來自 OmmWriter、FocusWriter 與小黑屋。

早期曾用 egui 製作，但 eframe 0.33 以後在部分 Linux 桌面（例如 MATE + X11）收不到鍵盤輸入，所以改用 Tauri。舊版原始碼可以在 git 歷史的第一個提交裡找到。

## 功能

- **場景**：雪、夜、紙、森、暮五種風景背景，全部由程式即時繪製，不用圖檔
- **分頁**：可以同時開好幾篇，用分頁切換；關掉程式再開，原本開著的分頁會全部還原
- **專注**：選單只在移動滑鼠時浮現，打字時自動隱藏；可開啟專注段落（淡化其他段落）與打字機模式（目前那行保持在畫面中央）
- **聲音**：背景音（雨、海浪、風、爐火、冥想）與打字音效（輕觸、打字機、水滴）
- **統計**：今日字數、寫作時間、連續寫作天數、每日目標，以及近 30 天的長條圖
- **小黑屋**：設定「寫滿 N 分鐘」或「新增 N 字」，達成前不能關閉視窗、離開全螢幕或切換文件

## 存檔

- 文件一律存成純文字 `.txt`。開檔時也可以開 `.md`。
- 新文件一開始是草稿，每篇各自自動存在程式的資料夾裡，關掉程式也不會遺失，下次開啟會接著寫。
- 第一次按 `Ctrl+S` 時，會跳出對話框選擇存檔位置（例如 Dropbox 裡的資料夾）。之後停止打字約 1 秒就自動存回那個檔案。
- 關閉一個還沒存檔、而且有內容的草稿分頁時，會先請你選個位置存起來。
- 存檔與開檔對話框會從上次用過的資料夾開始；第一次則從系統的「文件」資料夾開始。

設定、草稿、每日統計與小黑屋狀態存在程式的資料夾：

- Linux：`~/.local/share/kuma-write/`
- macOS：`~/Library/Application Support/kuma-write/`

## 快捷鍵

macOS 上 `Ctrl` 換成 `Cmd`。

| 快捷鍵 | 功能 |
| --- | --- |
| `Ctrl+O` / `Ctrl+N` | 最近的文件 / 在新分頁開新文件 |
| `Ctrl+S` | 立即儲存；草稿的話會先問存檔位置 |
| `Ctrl+Z` | 復原 |
| `Ctrl+Shift+Z` / `Ctrl+Y` | 重做 |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | 下一個 / 上一個分頁（`Ctrl+PageDown` / `Ctrl+PageUp` 也可以） |
| `Ctrl+W` | 關閉分頁 |
| `Ctrl+L` | 小黑屋 |
| `Ctrl+T` | 切換場景 |
| `Ctrl+=` / `Ctrl+-` | 放大 / 縮小字級 |
| `F8` | 專注段落 |
| `F9` | 打字機模式 |
| `F11` | 全螢幕 |
| `Esc` | 關閉面板；沒有面板時讓選單常駐 |
| `Ctrl+Q` | 離開 |

字數統計：中日韓文字每字算 1，英文等以單字計算，標點不計。

## 小黑屋

- 視窗會置頂；切到別的視窗或被最小化時會自動搶回焦點，並記錄一次「逃跑」。
- 狀態寫入磁碟。強制結束程式後再開，會接著鎖在同一份文件上；計時只在程式開著時累積。
- 小黑屋期間不能切換、開啟或關閉分頁。
- 若進入前勾選「緊急出口」，可以一字不差地抄寫一段句子後離開。
- 小黑屋期間若文件還是草稿，`Ctrl+S` 只會存成草稿（存檔對話框會讓視窗失去焦點），出關後再選位置。

一般應用程式無法完全攔截作業系統層級的操作（切換虛擬桌面、從工作管理員結束程序等），這些不會被阻止，但小黑屋狀態會保留，下次開啟時依然要寫完。

## 安裝

### Debian / Ubuntu

```sh
sudo apt install ./Kuma\ Write_0.3.0_amd64.deb
```

### macOS

打開 `.dmg`，把 Kuma Write 拖到「應用程式」。

因為沒有 Apple 開發者簽章，第一次開啟時系統會擋下來。請在「應用程式」裡對 Kuma Write 按右鍵 →「打開」，或在終端機執行：

```sh
xattr -cr "/Applications/Kuma Write.app"
```

## 開發

Linux（Debian/Ubuntu）需要先安裝：

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libasound2-dev
npm install
```

```sh
npm run dev         # 開發模式
npm run build:deb   # 打包 .deb（在 Linux 上）
npm run build:mac   # 打包 .dmg（只能在 macOS 上，Intel 與 Apple Silicon 通用）
```

### 用 GitHub Actions 打包

Mac 版只能在 Mac 上編譯。把專案推到 GitHub 之後，`.github/workflows/build.yml` 會用 GitHub 的機器同時打包 macOS 與 Debian 版：

- 在 GitHub 的 **Actions** 頁面手動執行「Build installers」，完成後在該次執行的 **Artifacts** 下載安裝檔
- 或推送版本標籤（例如 `git tag v0.3.0 && git push --tags`），會另外建立一個草稿 Release 並附上安裝檔

## 結構

- `ui/`：介面，純 HTML/CSS/JS，不需要打包工具
  - `scene.js`：場景繪製
  - `main.js`：編輯、存檔、浮動選單、各面板、小黑屋與快捷鍵
- `src-tauri/`：Rust 後端
  - `docs.rs`：讀寫文件與草稿
  - `audio.rs`：背景音與打字音效
- `src-tauri/sounds/`：打字機的錄音，取自 Tickeys（MIT），詳見該資料夾的 README
  - `journal.rs`：每日寫作統計
  - `lock.rs`：小黑屋狀態，鎖定期間擋下關閉與結束程式

## 授權

Kuma Write 以 [GNU General Public License v3.0](LICENSE) 授權。

打字機音效的錄音取自 [Tickeys](https://github.com/yingDev/Tickeys)（MIT 授權，Copyright (c) 2015 YingDev.com），條文見 `src-tauri/sounds/typewriter/LICENSE`。
