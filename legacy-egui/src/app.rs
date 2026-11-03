use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CursorIcon, FontId, Id, Key, KeyboardShortcut, LayerId, Modifiers,
    Order, RichText, Stroke, TextureHandle, ViewportCommand, WindowLevel,
};
use egui::text::{LayoutJob, TextFormat};

use crate::audio::{Ambient, Audio, KeyKind, KeySound};
use crate::config::{self, Config};
use crate::docs::{self, DocEntry, DocStore};
use crate::fonts;
use crate::journal::{self, Journal};
use crate::lock::{LockSession, EMERGENCY_PHRASE};
use crate::scene;
use crate::stats;
use crate::theme::{self, Palette, ThemeKind};

const CHROME_TIMEOUT: Duration = Duration::from_millis(2500);
const AUTOSAVE_DELAY: Duration = Duration::from_millis(1200);

fn editor_id() -> Id {
    Id::unique("kuma-editor")
}

pub struct App {
    cfg: Config,
    cfg_changed: bool,
    store: DocStore,
    palette: Palette,
    grain: TextureHandle,

    doc_path: PathBuf,
    text: String,
    words: usize,
    session_start_words: usize,
    dirty: bool,
    last_edit: Instant,

    // 介面顯示：滑鼠一動就浮現選單，開始打字就隱藏
    last_pointer: Instant,
    last_key: Instant,
    typed_this_frame: bool,
    menu_pinned: bool,
    last_tick: Instant,

    show_docs: bool,
    show_look: bool,
    doc_list: Vec<DocEntry>,
    show_lock_dialog: bool,
    form_use_time: bool,
    form_use_words: bool,
    form_allow_emergency: bool,
    show_emergency: bool,
    emergency_input: String,
    completed: Option<String>,
    toast: Option<(String, Instant)>,

    audio: Audio,
    show_sound: bool,
    journal: Journal,
    show_stats: bool,
    last_journal_save: Instant,

    lock: Option<LockSession>,
    lock_window_applied: bool,
    was_focused: bool,
    last_refocus: Instant,
    last_lock_save: Instant,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, cfg: Config) -> Self {
        let ctx = &cc.egui_ctx;
        fonts::install(ctx, cfg.font_style, cfg.font_path.as_deref());
        let palette = cfg.theme.palette();
        theme::apply(ctx, &palette);
        ctx.all_styles_mut(|s| {
            s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(15.0));
            s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(15.0));
            s.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(17.0));
            s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.0));
            s.spacing.item_spacing = egui::vec2(10.0, 10.0);
            s.spacing.button_padding = egui::vec2(10.0, 4.0);
            s.spacing.window_margin = egui::Margin::same(22);
            s.spacing.slider_width = 200.0;
        });
        let grain = scene::grain_texture(ctx);

        let store = DocStore::new();
        let lock = LockSession::load();
        let doc_path = match &lock {
            Some(l) => l.doc.clone(),
            None => cfg
                .last_doc
                .clone()
                .filter(|p| p.exists())
                .unwrap_or_else(|| store.new_doc_path()),
        };
        let text = docs::load(&doc_path).unwrap_or_default();
        let words = stats::count_words(&text);
        let now = Instant::now();
        let past = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);

        let audio = Audio::new(cfg.ambient, cfg.ambient_volume, cfg.key_sound, cfg.key_volume);
        let mut app = Self {
            cfg,
            cfg_changed: false,
            store,
            palette,
            grain,
            doc_path,
            text,
            words,
            session_start_words: words,
            dirty: false,
            last_edit: now,
            last_pointer: past,
            last_key: past,
            typed_this_frame: false,
            menu_pinned: false,
            last_tick: now,
            show_docs: false,
            show_look: false,
            doc_list: Vec::new(),
            show_lock_dialog: false,
            form_use_time: true,
            form_use_words: false,
            form_allow_emergency: true,
            show_emergency: false,
            emergency_input: String::new(),
            completed: None,
            toast: None,
            audio,
            show_sound: false,
            journal: Journal::load(),
            show_stats: false,
            last_journal_save: now,
            lock,
            lock_window_applied: false,
            was_focused: false,
            last_refocus: past,
            last_lock_save: now,
        };
        if app.lock.is_some() {
            app.notify("小黑屋還沒結束，繼續寫吧。");
        } else if let Some(e) = &app.audio.error {
            eprintln!("音訊無法啟動：{e}");
        }
        app
    }

    fn notify(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    fn locked(&self) -> bool {
        self.lock.is_some()
    }

    fn dialog_open(&self) -> bool {
        self.show_docs
            || self.show_look
            || self.show_lock_dialog
            || self.show_emergency
            || self.show_sound
            || self.show_stats
            || self.completed.is_some()
    }

    fn chrome_visible(&self) -> bool {
        self.menu_pinned
            || self.dialog_open()
            || (self.last_pointer > self.last_key && self.last_pointer.elapsed() < CHROME_TIMEOUT)
    }

    // ── 文件 ─────────────────────────────────────────

    fn save_now(&mut self) {
        if !self.dirty {
            return;
        }
        if self.text.is_empty() && !self.doc_path.exists() {
            // 空白的新文件不寫檔，免得留下一堆空檔案
            self.dirty = false;
            return;
        }
        match docs::save(&self.doc_path, &self.text) {
            Ok(()) => {
                self.dirty = false;
                if self.cfg.last_doc.as_ref() != Some(&self.doc_path) {
                    self.cfg.last_doc = Some(self.doc_path.clone());
                    self.cfg_changed = true;
                }
            }
            Err(e) => self.notify(format!("儲存失敗：{e}")),
        }
    }

    fn open_doc(&mut self, path: PathBuf) {
        self.save_now();
        match docs::load(&path) {
            Ok(text) => self.text = text,
            Err(e) => {
                self.notify(format!("無法開啟：{e}"));
                return;
            }
        }
        self.doc_path = path;
        self.after_doc_switch();
    }

    fn new_doc(&mut self) {
        self.save_now();
        self.text.clear();
        self.doc_path = self.store.new_doc_path();
        self.after_doc_switch();
    }

    fn after_doc_switch(&mut self) {
        self.words = stats::count_words(&self.text);
        self.session_start_words = self.words;
        self.dirty = false;
        self.cfg.last_doc = Some(self.doc_path.clone());
        self.cfg_changed = true;
    }

    fn autosave(&mut self) {
        if self.dirty && self.last_edit.elapsed() > AUTOSAVE_DELAY {
            self.save_now();
        }
        if self.cfg_changed {
            self.cfg.save();
            self.cfg_changed = false;
        }
        if self.last_journal_save.elapsed() > Duration::from_secs(15) {
            self.journal.save();
            self.last_journal_save = Instant::now();
        }
    }

    /// 匯出成 TXT，放在文件資料夾下的「匯出」
    fn export_txt(&mut self) {
        let title = docs::title_of(&self.text);
        let mut name: String = title
            .chars()
            .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
            .collect();
        if self.text.trim().is_empty() || name.trim().is_empty() {
            name = self
                .doc_path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "未命名".to_owned());
        }
        let path = self.store.dir.join("匯出").join(format!("{}.txt", name.trim()));
        let mut text = self.text.clone();
        if !text.ends_with('\n') {
            text.push('\n');
        }
        match config::write_atomic(&path, &text) {
            Ok(()) => self.notify(format!("已匯出：{}", path.display())),
            Err(e) => self.notify(format!("匯出失敗：{e}")),
        }
    }

    // ── 外觀 ─────────────────────────────────────────

    fn set_theme(&mut self, ctx: &egui::Context, kind: theme::ThemeKind) {
        self.cfg.theme = kind;
        self.palette = kind.palette();
        theme::apply(ctx, &self.palette);
        self.cfg_changed = true;
    }

    fn toggle_font(&mut self, ctx: &egui::Context) {
        self.cfg.font_style = self.cfg.font_style.toggled();
        fonts::install(ctx, self.cfg.font_style, self.cfg.font_path.as_deref());
        self.cfg_changed = true;
    }

    fn change_font_size(&mut self, delta: f32) {
        self.cfg.font_size = (self.cfg.font_size + delta).clamp(12.0, 48.0);
        self.cfg_changed = true;
    }

    fn toggle_fullscreen(&mut self, ctx: &egui::Context) {
        let fs = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
        ctx.send_viewport_cmd(ViewportCommand::Fullscreen(!fs));
        self.cfg.start_fullscreen = !fs;
        self.cfg_changed = true;
    }

    // ── 小黑屋 ───────────────────────────────────────

    fn start_lock(&mut self) {
        let minutes = self.form_use_time.then_some(self.cfg.lock_minutes);
        let words = self.form_use_words.then_some(self.cfg.lock_words);
        if minutes.is_none() && words.is_none() {
            self.notify("請至少設定一個目標。");
            return;
        }
        self.dirty = true;
        self.save_now();
        let lock = LockSession {
            target_minutes: minutes,
            target_words: words,
            elapsed_secs: 0.0,
            start_words: self.words,
            doc: self.doc_path.clone(),
            escapes: 0,
            allow_emergency: self.form_allow_emergency,
        };
        lock.save();
        self.lock = Some(lock);
        self.lock_window_applied = false;
        self.show_lock_dialog = false;
        self.show_docs = false;
        self.menu_pinned = false;
        self.cfg_changed = true;
        self.notify("門已上鎖。寫吧。");
    }

    fn end_lock(&mut self, ctx: &egui::Context) {
        self.lock = None;
        LockSession::clear();
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::Normal));
    }

    fn enforce_lock(&mut self, ctx: &egui::Context, dt: f64) {
        let words = self.words;
        let Some(lock) = &mut self.lock else {
            return;
        };
        lock.elapsed_secs += dt;

        if !self.lock_window_applied {
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
            ctx.send_viewport_cmd(ViewportCommand::WindowLevel(WindowLevel::AlwaysOnTop));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.lock_window_applied = true;
        }

        let (focused, minimized, fullscreen) = ctx.input(|i| {
            let v = i.viewport();
            (
                v.focused.unwrap_or(true),
                v.minimized.unwrap_or(false),
                v.fullscreen.unwrap_or(true),
            )
        });
        let present = focused && !minimized;
        let mut escaped = false;
        if !present && self.was_focused {
            lock.escapes += 1;
            escaped = true;
        }
        if (!present || !fullscreen) && self.last_refocus.elapsed() > Duration::from_millis(400) {
            ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.last_refocus = Instant::now();
        }
        self.was_focused = present;

        let complete = lock.is_complete(words);
        if !complete && self.last_lock_save.elapsed() > Duration::from_secs(5) {
            lock.save();
            self.last_lock_save = Instant::now();
        }

        if escaped {
            self.notify("想去哪？還沒寫完呢。");
        }
        if complete {
            let lock = self.lock.clone().unwrap();
            let mut msg = format!(
                "出關！這次寫了 {} 字，歷時 {}。",
                lock.words_written(words),
                stats::fmt_duration(lock.elapsed_secs)
            );
            if lock.escapes > 0 {
                msg.push_str(&format!("\n中途試圖逃跑 {} 次。", lock.escapes));
            }
            self.dirty = true;
            self.save_now();
            self.end_lock(ctx);
            self.completed = Some(msg);
        }
    }

    fn lock_status(&self) -> Option<String> {
        let lock = self.lock.as_ref()?;
        let mut parts = Vec::new();
        if let Some(t) = lock.time_left() {
            parts.push(if t > 0.0 {
                format!("剩 {}", stats::fmt_duration(t))
            } else {
                "時間已到".to_owned()
            });
        }
        if let Some(w) = lock.words_left(self.words) {
            parts.push(if w > 0 { format!("還差 {w} 字") } else { "字數已達成".to_owned() });
        }
        Some(format!("小黑屋 · {}", parts.join(" · ")))
    }

    // ── 輸入 ─────────────────────────────────────────

    fn track_input(&mut self, ctx: &egui::Context, now: Instant) {
        let (moved, typed) = ctx.input(|i| {
            (
                i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_pressed(),
                i.events.iter().any(|e| {
                    matches!(
                        e,
                        egui::Event::Text(_) | egui::Event::Ime(_) | egui::Event::Key { pressed: true, .. }
                    )
                }),
            )
        });
        if moved {
            self.last_pointer = now;
        }
        if typed {
            self.last_key = now;
        }
        // 滑鼠停在選單上時別讓它消失
        if ctx.is_pointer_over_egui() && self.last_pointer > self.last_key && !typed {
            if ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| {
                ctx.layer_id_at(p).is_some_and(|l| l.order != Order::Background)
            }) {
                self.last_pointer = now;
            }
        }
        self.typed_this_frame = typed;
        if typed && !self.dialog_open() {
            if let Some(k) = ctx.input(|i| key_kind(&i.events)) {
                self.audio.key(k);
            }
        }
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) {
            return;
        }
        if let Some(lock) = &mut self.lock {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            lock.escapes += 1;
            let status = self.lock_status().unwrap_or_default();
            self.notify(format!("門鎖著。{status}"));
        } else {
            self.save_now();
            self.cfg.save();
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
        let plain = |key| KeyboardShortcut::new(Modifiers::NONE, key);
        let pressed = |s: KeyboardShortcut| ctx.input_mut(|i| i.consume_shortcut(&s));
        let locked = self.locked();

        if pressed(plain(Key::Escape)) {
            if self.dialog_open() {
                self.show_docs = false;
                self.show_look = false;
                self.show_lock_dialog = false;
                self.show_emergency = false;
                self.show_sound = false;
                self.show_stats = false;
                self.completed = None;
            } else {
                self.menu_pinned = !self.menu_pinned;
            }
        }
        if pressed(cmd(Key::S)) {
            self.dirty = true;
            self.save_now();
            self.notify("已儲存");
        }
        if pressed(cmd(Key::Equals)) || pressed(cmd(Key::Plus)) {
            self.change_font_size(1.0);
        }
        if pressed(cmd(Key::Minus)) {
            self.change_font_size(-1.0);
        }
        if pressed(cmd(Key::E)) {
            self.export_txt();
        }
        if pressed(cmd(Key::T)) {
            self.set_theme(ctx, self.cfg.theme.next());
        }
        if pressed(plain(Key::F8)) {
            self.cfg.focus_paragraph = !self.cfg.focus_paragraph;
            self.cfg_changed = true;
        }
        if pressed(plain(Key::F9)) {
            self.cfg.typewriter = !self.cfg.typewriter;
            self.cfg_changed = true;
        }
        if locked {
            return;
        }
        if pressed(cmd(Key::O)) {
            self.open_docs_window();
        }
        if pressed(cmd(Key::N)) {
            self.new_doc();
        }
        if pressed(cmd(Key::L)) {
            self.show_lock_dialog = true;
        }
        if pressed(plain(Key::F11)) {
            self.toggle_fullscreen(ctx);
        }
        if pressed(cmd(Key::Q)) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }

    fn open_docs_window(&mut self) {
        self.save_now();
        self.doc_list = self.store.list();
        self.show_docs = true;
    }

    // ── 畫面 ─────────────────────────────────────────

    fn editor(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        let p = self.palette;
        let size = self.cfg.font_size;
        let line_h = size * self.cfg.line_spacing;
        let focus_cursor = if self.cfg.focus_paragraph {
            egui::TextEdit::load_state(ctx, editor_id())
                .and_then(|s| s.cursor.char_range())
                .map(|r| r.primary.index.0)
        } else {
            None
        };

        let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            let text = buf.as_str();
            let fmt = |color| TextFormat {
                font_id: FontId::proportional(size),
                color,
                line_height: Some(line_h),
                ..Default::default()
            };
            let mut job = LayoutJob::default();
            match focus_cursor.map(|ci| stats::paragraph_range(text, ci)) {
                Some((a, b)) => {
                    job.append(&text[..a], 0.0, fmt(p.dim));
                    job.append(&text[a..b], 0.0, fmt(p.text));
                    job.append(&text[b..], 0.0, fmt(p.dim));
                }
                None => job.append(text, 0.0, fmt(p.text)),
            }
            job.wrap.max_width = wrap_width;
            ui.fonts_mut(|f| f.layout_job(job))
        };

        let block_focus = self.dialog_open();
        let typewriter = self.cfg.typewriter && self.typed_this_frame;
        let column_width = self.cfg.column_width;

        // 景畫在最底層，固定不隨文字捲動
        let screen = ctx.content_rect();
        let bg = ctx.layer_painter(LayerId::background());
        scene::paint(&bg, screen, &p.scene, &self.grain);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let avail = ui.available_size();
                let col_w = column_width.min(avail.x - 48.0).max(200.0);
                egui::ScrollArea::vertical()
                    .id_salt("editor-scroll")
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.add_space(avail.y * 0.2);
                        ui.horizontal(|ui| {
                            ui.add_space(((ui.available_width() - col_w) / 2.0).max(0.0));
                            let out = egui::TextEdit::multiline(&mut self.text)
                                .id(editor_id())
                                .frame(egui::Frame::NONE)
                                .background_color(Color32::TRANSPARENT)
                                .desired_width(col_w)
                                .desired_rows(1)
                                .lock_focus(true)
                                .hint_text(RichText::new("開始寫吧……").color(p.dim).size(size))
                                .layouter(&mut layouter)
                                .show(ui);

                            if out.response.changed() {
                                self.dirty = true;
                                self.last_edit = Instant::now();
                                let words = stats::count_words(&self.text);
                                self.journal.add_words(words as i64 - self.words as i64);
                                self.words = words;
                            }
                            if !block_focus && !out.response.has_focus() {
                                out.response.request_focus();
                            }
                            // 打字機模式：游標所在行保持在畫面中央
                            if typewriter {
                                if let Some(cr) = out.cursor_range {
                                    let r = out
                                        .galley
                                        .pos_from_cursor(cr.primary)
                                        .translate(out.galley_pos.to_vec2());
                                    ui.scroll_to_rect(r, Some(Align::Center));
                                }
                            }
                        });
                        ui.add_space(avail.y * 0.5);
                    });
            });
    }

    fn top_bar(&mut self, ctx: &egui::Context, alpha: f32) {
        if alpha <= 0.01 {
            return;
        }
        let p = self.palette;
        egui::Area::new(Id::unique("topbar"))
            .anchor(Align2::CENTER_TOP, [0.0, 18.0])
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                egui::Frame::new()
                    .fill(p.panel)
                    .corner_radius(20)
                    .shadow(ctx.global_style().visuals.popup_shadow)
                    .inner_margin(egui::Margin::symmetric(18, 6))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.horizontal(|ui| self.top_bar_contents(ui));
                    });
            });
    }

    fn top_bar_contents(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let locked = self.locked();
        // 安靜的文字按鈕：平常是淡色，滑過才浮現底色
        let btn = |ui: &mut egui::Ui, label: &str, enabled: bool, hint: &str| {
            ui.add_enabled(enabled, egui::Button::new(label).frame_when_inactive(false))
                .on_hover_text(hint)
                .clicked()
        };
        let dot = self.palette.dim;
        let gap = |ui: &mut egui::Ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("·").color(dot));
            ui.add_space(6.0);
        };

        if btn(ui, "文件", !locked, "Ctrl+O") {
            self.open_docs_window();
        }
        if btn(ui, "新文件", !locked, "Ctrl+N") {
            self.new_doc();
        }
        if btn(ui, "匯出", true, "匯出 TXT（Ctrl+E）") {
            self.export_txt();
        }
        gap(ui);
        if btn(ui, "外觀", true, "場景、字體與版面（Ctrl+T 切換場景）") {
            self.show_look = !self.show_look;
        }
        if btn(ui, "聲音", true, "背景音與打字音效") {
            self.show_sound = !self.show_sound;
        }
        if btn(ui, "統計", true, "每日寫作統計") {
            self.show_stats = !self.show_stats;
        }
        gap(ui);
        if locked {
            ui.label(RichText::new("已上鎖").color(self.palette.accent));
            if self.lock.as_ref().is_some_and(|l| l.allow_emergency)
                && btn(ui, "緊急出口", true, "輸入指定句子後離開小黑屋")
            {
                self.emergency_input.clear();
                self.show_emergency = true;
            }
        } else {
            if btn(ui, "小黑屋", true, "Ctrl+L") {
                self.show_lock_dialog = true;
            }
            if btn(ui, "全螢幕", true, "F11") {
                self.toggle_fullscreen(&ctx);
            }
            if btn(ui, "離開", true, "Ctrl+Q") {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
    }

    fn hud(&self, ctx: &egui::Context, alpha: f32) {
        let p = self.palette;
        let screen = ctx.content_rect();

        // 小黑屋進度條：一直顯示在畫面最底下
        if let Some(lock) = &self.lock {
            let frac = lock.progress(self.words);
            let painter = ctx.layer_painter(LayerId::new(Order::Foreground, Id::unique("lockbar")));
            let bar = egui::Rect::from_min_size(
                egui::pos2(screen.left(), screen.bottom() - 3.0),
                egui::vec2(screen.width() * frac, 3.0),
            );
            painter.rect_filled(bar, 0.0, p.accent.gamma_multiply(0.7));
        }

        if alpha <= 0.01 {
            return;
        }
        let delta = self.words as i64 - self.session_start_words as i64;
        let today = self.journal.day(journal::today()).shown_words();
        let mut line = format!(
            "{}　　{} 字　　本次 {:+}　　今日 {} / {}　　{}",
            docs::title_of(&self.text),
            self.words,
            delta,
            today,
            self.cfg.daily_goal,
            chrono::Local::now().format("%H:%M"),
        );
        if self.dirty {
            line.push_str("　　未儲存");
        }
        egui::Area::new(Id::unique("hud"))
            .anchor(Align2::CENTER_BOTTOM, [0.0, -22.0])
            .order(Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_opacity(alpha);
                ui.vertical_centered(|ui| {
                    if let Some(s) = self.lock_status() {
                        ui.label(RichText::new(s).color(p.accent).size(13.0));
                    }
                    ui.label(RichText::new(line).color(p.text.gamma_multiply(0.5)).size(13.0));
                });
            });
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let window = |title: &str| {
            egui::Window::new(title.to_owned())
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .order(Order::Foreground)
        };

        if self.show_docs {
            let mut to_open = None;
            let mut new = false;
            let current = &self.doc_path;
            window("文件").open(&mut self.show_docs).show(ctx, |ui| {
                ui.set_min_width(380.0);
                if ui.button("＋ 新文件").clicked() {
                    new = true;
                }
                ui.separator();
                egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                    if self.doc_list.is_empty() {
                        ui.label("還沒有文件。");
                    }
                    for e in &self.doc_list {
                        let label = format!("{}　　{}", e.title, e.modified);
                        if ui.selectable_label(&e.path == current, label).clicked() {
                            to_open = Some(e.path.clone());
                        }
                    }
                });
                ui.separator();
                ui.label(RichText::new(self.store.dir.display().to_string()).small());
            });
            if new {
                self.new_doc();
                self.show_docs = false;
            } else if let Some(path) = to_open {
                self.open_doc(path);
                self.show_docs = false;
            }
        }

        if self.show_look {
            let mut pick = None;
            let mut font = None;
            let current = self.cfg.theme;
            let cfg = &mut self.cfg;
            let grain = &self.grain;
            let p = self.palette;
            let mut changed = false;
            window("外觀").open(&mut self.show_look).show(ctx, |ui| {
                pick = scene_picker(ui, current, grain, &p);
                ui.add_space(8.0);
                egui::Grid::new("look-grid")
                    .num_columns(2)
                    .spacing([24.0, 14.0])
                    .show(ui, |ui| {
                        ui.label("字體");
                        ui.horizontal(|ui| {
                            for f in [fonts::FontStyle::Serif, fonts::FontStyle::Sans] {
                                if ui.selectable_label(cfg.font_style == f, f.name()).clicked() && cfg.font_style != f {
                                    font = Some(f);
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("字級");
                        changed |= ui.add(egui::Slider::new(&mut cfg.font_size, 12.0..=48.0).step_by(1.0).fixed_decimals(0)).changed();
                        ui.end_row();
                        ui.label("行距");
                        changed |= ui.add(egui::Slider::new(&mut cfg.line_spacing, 1.3..=2.6).step_by(0.1).fixed_decimals(1)).changed();
                        ui.end_row();
                        ui.label("欄寬");
                        changed |= ui
                            .add(egui::Slider::new(&mut cfg.column_width, 480.0..=1400.0).step_by(10.0).fixed_decimals(0))
                            .changed();
                        ui.end_row();
                        ui.label("專注");
                        ui.vertical(|ui| {
                            changed |= ui
                                .checkbox(&mut cfg.focus_paragraph, "淡化目前段落以外的文字")
                                .on_hover_text("F8")
                                .changed();
                            changed |= ui
                                .checkbox(&mut cfg.typewriter, "打字機：正在寫的那一行保持在中央")
                                .on_hover_text("F9")
                                .changed();
                        });
                        ui.end_row();
                    });
            });
            self.cfg_changed |= changed;
            if let Some(kind) = pick {
                self.set_theme(ctx, kind);
            }
            if font.is_some() {
                self.toggle_font(ctx);
            }
        }

        if self.show_lock_dialog {
            let mut start = false;
            let mut cancel = false;
            let cfg = &mut self.cfg;
            window("小黑屋").show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.label("進入後，在達成目標之前不能關閉視窗、離開全螢幕或切換文件。");
                ui.label("關掉程式也沒用——下次開啟時會接著鎖，計時只在程式開著時累積。");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.form_use_time, "寫滿");
                    ui.add_enabled(
                        self.form_use_time,
                        egui::DragValue::new(&mut cfg.lock_minutes).range(1..=720).suffix(" 分鐘"),
                    );
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.form_use_words, "新增");
                    ui.add_enabled(
                        self.form_use_words,
                        egui::DragValue::new(&mut cfg.lock_words)
                            .range(10..=200_000)
                            .speed(10)
                            .suffix(" 字"),
                    );
                });
                if self.form_use_time && self.form_use_words {
                    ui.label(RichText::new("兩個目標都要達成才能出關。").small());
                }
                ui.checkbox(&mut self.form_allow_emergency, "保留緊急出口（要抄寫一段句子才能離開）");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("進入小黑屋").clicked() {
                        start = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                });
            });
            if start {
                self.start_lock();
            } else if cancel {
                self.show_lock_dialog = false;
            }
        }

        if self.show_emergency {
            let mut confirm = false;
            let mut cancel = false;
            let input = &mut self.emergency_input;
            window("緊急出口").show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.label("請一字不差地輸入下面這句話：");
                ui.label(RichText::new(EMERGENCY_PHRASE).strong());
                let r = ui.add(egui::TextEdit::singleline(input).desired_width(420.0));
                r.request_focus();
                let ok = input.trim() == EMERGENCY_PHRASE;
                ui.horizontal(|ui| {
                    if ui.add_enabled(ok, egui::Button::new("離開小黑屋")).clicked() {
                        confirm = true;
                    }
                    if ui.button("繼續寫").clicked() {
                        cancel = true;
                    }
                });
            });
            if confirm {
                self.end_lock(ctx);
                self.show_emergency = false;
                self.notify("已從緊急出口離開。");
            } else if cancel {
                self.show_emergency = false;
            }
        }

        if self.show_sound {
            let cfg = &mut self.cfg;
            let audio = &self.audio;
            let mut changed = false;
            window("聲音").open(&mut self.show_sound).show(ctx, |ui| {
                ui.set_min_width(360.0);
                if let Some(e) = &audio.error {
                    ui.colored_label(egui::Color32::from_rgb(220, 120, 110), format!("音訊無法啟動：{e}"));
                }
                ui.label(RichText::new("背景").strong());
                ui.horizontal_wrapped(|ui| {
                    for a in Ambient::ALL {
                        if ui.selectable_value(&mut cfg.ambient, a, a.name()).changed() {
                            audio.set_ambient(a);
                            changed = true;
                        }
                    }
                });
                if ui.add(egui::Slider::new(&mut cfg.ambient_volume, 0.0..=1.0).show_value(false).text("音量")).changed() {
                    audio.set_ambient_volume(cfg.ambient_volume);
                    changed = true;
                }
                ui.add_space(6.0);
                ui.label(RichText::new("打字音效").strong());
                ui.horizontal_wrapped(|ui| {
                    for k in KeySound::ALL {
                        if ui.selectable_value(&mut cfg.key_sound, k, k.name()).changed() {
                            audio.set_key_sound(k);
                            audio.key(KeyKind::Char);
                            changed = true;
                        }
                    }
                });
                if ui.add(egui::Slider::new(&mut cfg.key_volume, 0.0..=1.0).show_value(false).text("音量")).changed() {
                    audio.set_key_volume(cfg.key_volume);
                    changed = true;
                }
            });
            self.cfg_changed |= changed;
        }

        if self.show_stats {
            let p = self.palette;
            let journal = &self.journal;
            let cfg = &mut self.cfg;
            let mut changed = false;
            window("寫作統計").open(&mut self.show_stats).show(ctx, |ui| {
                changed = stats_panel(ui, journal, &mut cfg.daily_goal, &p);
            });
            self.cfg_changed |= changed;
        }

        if let Some(msg) = &self.completed {
            let mut close = false;
            window("出關").show(ctx, |ui| {
                ui.label(RichText::new(msg).size(18.0));
                ui.add_space(6.0);
                if ui.button("好").clicked() {
                    close = true;
                }
            });
            if close {
                self.completed = None;
            }
        }
    }

    fn toast_ui(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = &self.toast else {
            return;
        };
        let age = at.elapsed().as_secs_f32();
        if age > 3.0 {
            self.toast = None;
            return;
        }
        let p = self.palette;
        egui::Area::new(Id::unique("toast"))
            .anchor(Align2::CENTER_TOP, [0.0, 72.0])
            .order(Order::Tooltip)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_opacity(((3.0 - age) / 0.6).min(1.0));
                egui::Frame::new()
                    .fill(p.panel)
                    .corner_radius(20)
                    .inner_margin(egui::Margin::symmetric(20, 8))
                    .show(ui, |ui| ui.label(RichText::new(msg).color(p.accent)));
            });
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = Instant::now();
        // 上限 3 秒：系統休眠時不算進小黑屋時間
        let dt = now.duration_since(self.last_tick).as_secs_f64().min(3.0);
        self.last_tick = now;

        self.track_input(&ctx, now);
        self.handle_close(&ctx);
        self.handle_shortcuts(&ctx);
        self.enforce_lock(&ctx, dt);
        // 最近 20 秒內有打字才算「正在寫」
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && self.last_key.elapsed() < Duration::from_secs(20) {
            self.journal.add_secs(dt);
        }

        self.editor(ui);
        let alpha = ctx.animate_bool_with_time(Id::unique("chrome"), self.chrome_visible(), 0.3);
        self.top_bar(&ctx, alpha);
        self.hud(&ctx, alpha);
        self.dialogs(&ctx);
        self.toast_ui(&ctx);
        self.autosave();

        // 打字時隱藏滑鼠游標
        if self.last_key > self.last_pointer && !self.chrome_visible() {
            ctx.set_cursor_icon(CursorIcon::None);
        }
        let interval = if self.locked() { 500 } else { 1000 };
        ctx.request_repaint_after(Duration::from_millis(interval));
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.save_now();
        self.cfg.save();
        self.journal.save();
        if let Some(lock) = &self.lock {
            lock.save();
        }
    }
}

/// 場景縮圖，點一下就換。回傳被點選的場景。
fn scene_picker(ui: &mut egui::Ui, current: ThemeKind, grain: &TextureHandle, p: &Palette) -> Option<ThemeKind> {
    let mut pick = None;
    let size = egui::vec2(84.0, 56.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        for kind in ThemeKind::ALL {
            let layout = egui::Layout::top_down(Align::Center);
            ui.allocate_ui_with_layout(egui::vec2(size.x, size.y + 26.0), layout, |ui| {
                let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                let kp = kind.palette();
                scene::paint(&ui.painter().with_clip_rect(rect), rect, &kp.scene, grain);
                // 用該場景的文字色畫兩行假字，預覽閱讀起來的感覺
                for (i, w) in [0.62_f32, 0.42].into_iter().enumerate() {
                    let y = rect.top() + 18.0 + i as f32 * 8.0;
                    let x0 = rect.left() + 14.0;
                    ui.painter().hline(x0..=x0 + (rect.width() - 28.0) * w, y, Stroke::new(2.0, kp.text.gamma_multiply(0.8)));
                }
                let stroke = if kind == current {
                    Stroke::new(2.0, p.accent)
                } else if resp.hovered() {
                    Stroke::new(1.0, p.text.gamma_multiply(0.5))
                } else {
                    Stroke::new(1.0, p.text.gamma_multiply(0.12))
                };
                ui.painter().rect_stroke(rect, 8, stroke, egui::StrokeKind::Outside);
                let label = RichText::new(kind.name());
                ui.label(if kind == current { label.color(p.accent) } else { label });
                if resp.on_hover_cursor(CursorIcon::PointingHand).clicked() {
                    pick = Some(kind);
                }
            });
        }
    });
    pick
}

/// 這一幀的按鍵該發出哪種音效。一幀最多一聲。
fn key_kind(events: &[egui::Event]) -> Option<KeyKind> {
    let mut kind = None;
    for e in events {
        let k = match e {
            egui::Event::Key { key, pressed: true, modifiers, .. } if !modifiers.command => match key {
                Key::Enter => Some(KeyKind::Enter),
                Key::Backspace | Key::Delete => Some(KeyKind::Back),
                _ => None,
            },
            egui::Event::Text(t) if t == " " || t == "\u{3000}" => Some(KeyKind::Space),
            egui::Event::Text(_) => Some(KeyKind::Char),
            // 中文輸入法：組字中每按一個鍵、以及選字送出時
            egui::Event::Ime(egui::ImeEvent::Preedit { text: t, .. }) if !t.is_empty() => Some(KeyKind::Char),
            egui::Event::Ime(egui::ImeEvent::Commit(_)) => Some(KeyKind::Char),
            _ => None,
        };
        // Enter 與退格優先於一般字元
        kind = match (kind, k) {
            (Some(KeyKind::Enter | KeyKind::Back), _) => kind,
            (_, Some(k)) => Some(k),
            _ => kind,
        };
    }
    kind
}

/// 統計視窗。回傳每日目標是否被修改。
fn stats_panel(ui: &mut egui::Ui, journal: &Journal, goal: &mut usize, p: &Palette) -> bool {
    let today = journal.day(journal::today());
    let recent = journal.recent(30);
    let week: usize = recent.iter().rev().take(7).map(|(_, d)| d.shown_words()).sum();
    let month: usize = recent.iter().map(|(_, d)| d.shown_words()).sum();
    let active = recent.iter().filter(|(_, d)| d.shown_words() > 0).count();

    ui.set_width(560.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new(today.shown_words().to_string()).size(34.0).color(p.text));
        ui.vertical(|ui| {
            ui.add_space(6.0);
            ui.label(format!("今日字數 · 寫了 {}", journal::fmt_minutes(today.secs)));
            ui.label(format!("連續寫作 {} 天", journal.streak()));
        });
    });
    let frac = (today.shown_words() as f32 / (*goal).max(1) as f32).min(1.0);
    ui.add(
        egui::ProgressBar::new(frac)
            .desired_height(6.0)
            .fill(p.accent)
            .corner_radius(3),
    );
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("每日目標");
        changed = ui
            .add(egui::DragValue::new(goal).range(50..=100_000).speed(10).suffix(" 字"))
            .changed();
        if frac >= 1.0 {
            ui.label(RichText::new("今天的目標達成了").color(p.accent));
        }
    });

    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("近 30 天字數").strong());
        ui.label(RichText::new(format!("虛線為每日目標 {} 字", *goal)).color(p.dim));
    });
    daily_chart(ui, &recent, *goal, p);

    ui.add_space(4.0);
    ui.label(format!(
        "近 7 天 {week} 字 · 近 30 天 {month} 字 · 有寫字的日子平均 {} 字",
        if active > 0 { month / active } else { 0 }
    ));
    if let Some((d, w)) = journal.best() {
        ui.label(format!("最高紀錄：{} 寫了 {w} 字", d.format("%Y/%m/%d")));
    }

    egui::CollapsingHeader::new("每日明細").show(ui, |ui| {
        egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
            egui::Grid::new("journal-table").striped(true).num_columns(3).show(ui, |ui| {
                ui.label(RichText::new("日期").strong());
                ui.label(RichText::new("字數").strong());
                ui.label(RichText::new("時間").strong());
                ui.end_row();
                for (d, day) in recent.iter().rev().filter(|(_, d)| d.words != 0 || d.secs > 0.0) {
                    ui.label(d.format("%m/%d").to_string());
                    ui.label(day.shown_words().to_string());
                    ui.label(journal::fmt_minutes(day.secs));
                    ui.end_row();
                }
            });
        });
    });
    changed
}

/// 近 30 天長條圖：單一數列用主題強調色，虛線為每日目標，滑過長條顯示明細
fn daily_chart(ui: &mut egui::Ui, days: &[(chrono::NaiveDate, journal::Day)], goal: usize, p: &Palette) {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 150.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let ink = theme::mix(p.hud, p.text, 0.45);
    let grid = theme::mix(p.hud, p.text, 0.15);
    let label_h = 18.0;
    let plot = egui::Rect::from_min_max(rect.min + egui::vec2(0.0, 14.0), rect.max - egui::vec2(0.0, label_h));

    let max = days.iter().map(|(_, d)| d.shown_words()).max().unwrap_or(0).max(goal).max(1) as f32;
    let y_of = |w: f32| plot.bottom() - plot.height() * (w / max);
    let font = FontId::proportional(12.0);

    painter.hline(plot.x_range(), plot.bottom(), egui::Stroke::new(1.0, grid));

    let n = days.len().max(1) as f32;
    let gap = 2.0;
    let slot = plot.width() / n;
    let bar_w = (slot - gap).max(1.0);
    let hover = resp.hover_pos().filter(|pos| rect.contains(*pos));
    let mut hovered = None;
    for (i, (date, day)) in days.iter().enumerate() {
        let x0 = plot.left() + i as f32 * slot + gap / 2.0;
        let col = egui::Rect::from_min_max(egui::pos2(x0, plot.top()), egui::pos2(x0 + bar_w, plot.bottom()));
        let is_hover = hover.is_some_and(|pos| pos.x >= col.left() - gap / 2.0 && pos.x < col.right() + gap / 2.0);
        if is_hover {
            hovered = Some((*date, *day));
            painter.rect_filled(col, 2.0, grid.gamma_multiply(0.5));
        }
        let w = day.shown_words() as f32;
        if w > 0.0 {
            let bar = egui::Rect::from_min_max(egui::pos2(x0, y_of(w).min(plot.bottom() - 2.0)), egui::pos2(x0 + bar_w, plot.bottom()));
            let r = 4.0_f32.min(bar_w / 2.0).min(bar.height()) as u8;
            painter.rect_filled(bar, egui::CornerRadius { nw: r, ne: r, sw: 0, se: 0 }, p.accent);
        }
    }
    // 目標線畫在長條上方，才不會被蓋住
    let gy = y_of(goal as f32);
    let mut x = plot.left();
    while x < plot.right() {
        painter.hline(x..=(x + 4.0).min(plot.right()), gy, egui::Stroke::new(1.0, p.text.gamma_multiply(0.7)));
        x += 8.0;
    }
    painter.text(egui::pos2(plot.left(), rect.bottom()), Align2::LEFT_BOTTOM, days.first().map(|(d, _)| d.format("%m/%d").to_string()).unwrap_or_default(), font.clone(), ink);
    painter.text(egui::pos2(plot.right(), rect.bottom()), Align2::RIGHT_BOTTOM, "今天", font, ink);

    if let Some((date, day)) = hovered {
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(RichText::new(date.format("%Y/%m/%d").to_string()).strong());
            ui.label(format!("{} 字 · {}", day.shown_words(), journal::fmt_minutes(day.secs)));
        });
    }
}
