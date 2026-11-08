# Kuma Write

A quiet, full-screen writing app. Kuma Write gives you a soft painted landscape, a column of text, and nothing else. Menus fade away when you start typing and come back when you move the mouse.

It draws on OmmWriter's calm atmosphere, FocusWriter's practical writing tools, and the "little black room" (小黑屋) apps popular with Chinese writers, which lock you in until you reach your goal.

Built with [Tauri 2](https://tauri.app): a Rust backend with an HTML/CSS interface. Runs on Linux, macOS and Windows. The interface is in Traditional Chinese.

## Features

- **Five scenes**: snow, night, paper, forest and dusk. Each one is a landscape painted live on a canvas (gradient sky, sun or moon glow, layered hills, fog, a little grain), so the app ships no background images.
- **Tabs**: keep several documents open and switch between them. Open tabs, cursor positions and scroll positions come back the next time you launch.
- **Focus tools**: paragraph focus dims everything except the paragraph you're writing in, and typewriter mode keeps the current line centred on screen.
- **Sound**: ambient backgrounds (rain, waves, wind, fireplace, a slow meditative drone) and typing sounds (soft tap, typewriter, water drop). Ambient sounds are generated in real time; the typewriter uses recorded samples, with five variations for regular keys so fast typing never sounds like a loop.
- **Writing stats**: today's word count, time spent writing, your current streak, a daily goal, and a 30-day bar chart.
- **Lock-in mode**: set a goal ("write for 30 minutes" or "add 500 words"). Until you reach it, the window can't be closed, can't leave full screen, and won't let you switch documents.
- **Undo and redo**, kept separately for each tab.

Word counts treat each CJK character as one word and count other languages by whole words. Punctuation doesn't count.

## Saving

- Documents are plain-text `.txt` files. You can also open `.md` files.
- Files keep their original format. A file saved by Windows Notepad with CRLF line endings or a byte-order mark is written back the same way.
- A new document starts as a draft. Drafts are autosaved inside the app's data folder, so nothing is lost if you close the app or it crashes.
- The first time you press `Ctrl+S`, Kuma Write asks where to save. You can choose any folder, such as one inside Dropbox. After that, it saves back to that file about a second after you stop typing.
- If you close a draft tab that has text in it, Kuma Write asks you where to save it first.
- The save and open dialogs start in the last folder you used, or your Documents folder the first time.

Settings, drafts, stats and lock-in state live in the app's data folder:

- Linux: `~/.local/share/kuma-write/`
- macOS: `~/Library/Application Support/kuma-write/`
- Windows: `%APPDATA%\kuma-write\`

## Keyboard shortcuts

On macOS, use `Cmd` instead of `Ctrl`.

| Shortcut | Action |
| --- | --- |
| `Ctrl+N` | New document in a new tab |
| `Ctrl+O` | Recent documents |
| `Ctrl+S` | Save now (asks for a location if the document is still a draft) |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` / `Ctrl+Y` | Redo |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab (`Ctrl+PageDown` / `Ctrl+PageUp` also work) |
| `Ctrl+W` | Close tab |
| `Ctrl+L` | Lock-in mode |
| `Ctrl+T` | Next scene |
| `Ctrl+=` / `Ctrl+-` | Larger / smaller text |
| `F8` | Paragraph focus |
| `F9` | Typewriter mode |
| `F11` | Full screen |
| `Esc` | Close the open panel, or keep the menu visible |
| `Ctrl+Q` | Quit |

## Lock-in mode

- The window stays on top. If you switch to another window or minimise it, Kuma Write takes focus back and counts it as an escape attempt.
- The lock is saved to disk. If you force-quit and reopen the app, it locks onto the same document again. The timer only runs while the app is open, so quitting doesn't help the clock.
- You can't switch, open or close tabs while locked.
- If you enabled the emergency exit before starting, you can leave early by typing out a fixed sentence exactly. Copying and pasting it won't work.
- If the locked document is still a draft, `Ctrl+S` keeps it as a draft, because a save dialog would take focus away from the window. Choose a save location after you finish.

A normal application can't block everything the operating system allows, such as switching virtual desktops or ending the process from a task manager. Kuma Write doesn't try to. On Windows, the system may also refuse to hand focus back and flash the taskbar button instead. Either way, the lock is still waiting when you come back.

## Installing

### Debian / Ubuntu

```sh
sudo apt install ./Kuma\ Write_0.3.0_amd64.deb
```

### Windows

Run `Kuma Write_0.3.0_x64-setup.exe`. It installs for the current user only, so it doesn't need administrator rights. Kuma Write uses Microsoft Edge WebView2, which is built into Windows 10 and 11; on the rare machine without it, the installer downloads it.

The installer isn't code-signed, so Windows SmartScreen may warn you. Click **More info**, then **Run anyway**.

### macOS

Open the `.dmg` and drag Kuma Write into Applications.

The app isn't signed with an Apple Developer ID, so macOS blocks it the first time. Right-click Kuma Write in Applications and choose **Open**, or run:

```sh
xattr -cr "/Applications/Kuma Write.app"
```

## Building from source

On Debian or Ubuntu, install the build dependencies first:

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev \
  libayatana-appindicator3-dev librsvg2-dev libasound2-dev
npm install
```

Then:

```sh
npm run dev         # run in development mode
npm run build:deb   # build a .deb (Linux)
npm run build:mac   # build a universal .dmg for Intel and Apple Silicon (macOS only)
npm run build:win   # build a Windows installer (Windows only)
```

### Building installers on GitHub

macOS and Windows installers have to be built on their own systems. The workflow in `.github/workflows/build.yml` builds all three installers (macOS, Windows and Debian) on GitHub's runners:

- Run **Build installers** manually from the **Actions** tab, then download the installers from that run's **Artifacts**.
- Or push a version tag, for example `git tag v0.3.0 && git push --tags`. This also creates a draft release with the installers attached.

## Project layout

- `ui/`: the interface, plain HTML/CSS/JS with no bundler
  - `scene.js`: draws the scenes
  - `main.js`: editor, tabs, saving, undo, panels, lock-in mode and shortcuts
- `src-tauri/`: the Rust backend
  - `docs.rs`: reading and writing documents and drafts
  - `audio.rs`: ambient sound and typing sounds
  - `journal.rs`: daily writing stats
  - `lock.rs`: lock-in state; blocks closing and quitting while locked
- `src-tauri/sounds/`: typewriter recordings and where they come from

### Why Tauri

Kuma Write started out with egui. Starting with eframe 0.33, some Linux desktops (MATE on X11, for example) stopped delivering keyboard and mouse input to the app at all, and egui's support for Chinese input methods was weak anyway. Tauri hands text input to the platform's web engine, which handles input methods properly on every platform. The egui version is still in the first commit of the git history.

## License

Kuma Write is released under the [GNU General Public License v3.0](LICENSE).

The typewriter recordings come from [Tickeys](https://github.com/yingDev/Tickeys) (MIT License, Copyright (c) 2015 YingDev.com). See `src-tauri/sounds/typewriter/LICENSE`.
