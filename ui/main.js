'use strict';

const { invoke } = window.__TAURI__.core;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const DEFAULTS = {
  theme: 'Snow',
  font_style: 'Serif',
  font_size: 22,
  line_spacing: 1.8,
  column_width: 720,
  focus_paragraph: false,
  typewriter: true,
  start_fullscreen: true,
  /** 開著的分頁：{ path } 或 { draft }，以及目前在哪一頁 */
  open_docs: [],
  active_doc: 0,
  last_dir: null,
  recent: [],
  ambient: 'Off',
  ambient_volume: 0.5,
  key_sound: 'Soft',
  key_volume: 0.5,
  daily_goal: 1000,
  lock_minutes: 30,
  lock_words: 500,
};

const FONTS = {
  Serif: '"Noto Serif CJK TC", "Noto Serif TC", "Source Han Serif TC", "Songti TC", "PMingLiU", serif',
  Sans: '"Noto Sans CJK TC", "Noto Sans TC", "Source Han Sans TC", "PingFang TC", "Microsoft JhengHei", sans-serif',
};

const CHROME_TIMEOUT = 2500;
const AUTOSAVE_DELAY = 1000;
/** 停筆超過這麼久就不算在寫作時間裡 */
const ACTIVE_WINDOW = 20_000;
/** 開啟緊急出口時需要一字不差輸入的句子 */
const EMERGENCY_PHRASE = '我決定放棄這次的小黑屋，下次會堅持到最後';

const $ = (s) => document.querySelector(s);
const editor = $('#editor');
const mirror = $('#mirror');

let cfg = { ...DEFAULTS };
let saveTimer = 0;
let configTimer = 0;
let chromeTimer = 0;
let pinned = false;

// ── 字數 ───────────────────────────────────────

const CJK = /[぀-ヿ㐀-䶿一-鿿가-힯豈-﫿\u{20000}-\u{3134f}]/u;
const WORD = /[\p{L}\p{N}]/u;

/** 中日韓文字每字算一，其他語言以連續的字母數字算一個詞。標點不計。 */
function countWords(text) {
  let n = 0;
  let inWord = false;
  for (const ch of text) {
    if (CJK.test(ch)) {
      n++;
      inWord = false;
    } else if (WORD.test(ch)) {
      if (!inWord) n++;
      inWord = true;
    } else if (!(inWord && (ch === "'" || ch === '’' || ch === '-'))) {
      inWord = false;
    }
  }
  return n;
}

function titleOf(text) {
  const line = text.split('\n').map((l) => l.trim().replace(/^#+/, '').trim()).find((l) => l);
  if (!line) return '（空白）';
  const chars = [...line];
  return chars.length > 24 ? chars.slice(0, 24).join('') + '…' : line;
}

const pad2 = (n) => String(n).padStart(2, '0');

function fmtDuration(secs) {
  const s = Math.ceil(Math.max(0, secs));
  const h = Math.floor(s / 3600);
  const m = Math.floor(s / 60) % 60;
  return h > 0 ? `${h}:${pad2(m)}:${pad2(s % 60)}` : `${pad2(m)}:${pad2(s % 60)}`;
}

function fmtMinutes(secs) {
  const m = Math.round(secs / 60);
  return m >= 60 ? `${Math.floor(m / 60)} 小時 ${m % 60} 分` : `${m} 分鐘`;
}

// ── 文件與分頁 ─────────────────────────────────
// 可以同時開好幾篇，用分頁切換。每篇的狀態存在 docs 裡，編輯區只放目前那篇（doc）。
// path 為 null 的是還沒選存檔位置的草稿：每篇有自己的草稿檔（draft 編號），
// 按 Ctrl+S 或關閉分頁時才問要存在哪裡。

const nameOf = (path) => path.slice(Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\')) + 1);
const dirOf = (path) => path.slice(0, Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'))) || null;

let docs = [];
let doc = null;

const newDraftId = () => Date.now().toString(36) + Math.floor(Math.random() * 1e6).toString(36);

function makeDoc(path, text, draft = null) {
  return {
    path,
    draft: path ? null : draft || newDraftId(),
    text,
    saved: text,
    startWords: countWords(text),
    sel: [text.length, text.length],
    scroll: null,
    undo: [],
    redo: [],
    lastKind: '',
    lastEdit: 0,
  };
}

const docText = (d) => (d === doc ? editor.value : d.text);

/** 把某篇放進編輯區。目前那篇的內容、游標與捲動位置先收回去 */
function show(d) {
  flushJournal();
  if (doc) {
    doc.text = editor.value;
    doc.sel = [editor.selectionStart, editor.selectionEnd];
    doc.scroll = editor.scrollTop;
  }
  doc = d;
  editor.value = d.text;
  editor.setSelectionRange(...d.sel);
  journalCount = countWords(d.text);
  if (d.scroll == null) caretWork(true);
  else {
    editor.scrollTop = d.scroll;
    caretWork(false);
  }
  updateHud();
  renderTabs();
  rememberTabs();
  editor.focus();
}

/** 記住開著哪些分頁，下次開啟時還原 */
function rememberTabs() {
  cfg.open_docs = docs.map((d) => (d.path ? { path: d.path } : { draft: d.draft }));
  cfg.active_doc = docs.indexOf(doc);
  saveConfig();
}

/** 記住最近的文件與資料夾，開檔、存檔對話框都從這裡開始 */
function rememberRecent(path) {
  cfg.last_dir = dirOf(path);
  cfg.recent = [path, ...(cfg.recent || []).filter((p) => p !== path)].slice(0, 20);
  saveConfig();
}

/** 存目前這篇。其他分頁在切走時都已經存好了 */
async function saveNow() {
  clearTimeout(saveTimer);
  const d = doc;
  const text = editor.value;
  if (text === d.saved) return;
  try {
    if (d.path) await invoke('save_doc', { path: d.path, text });
    else await invoke('save_draft', { id: d.draft, text });
    d.saved = text;
    updateHud();
    renderTabs();
  } catch (e) {
    toast(`儲存失敗：${e}`);
  }
}

/** 選位置存檔。使用者取消時回傳 false */
async function saveAs() {
  const path = await invoke('pick_save_path', { name: titleOf(editor.value), dir: cfg.last_dir || null });
  if (!path) return false;
  const text = editor.value;
  try {
    await invoke('save_doc', { path, text });
  } catch (e) {
    toast(`儲存失敗：${e}`);
    return false;
  }
  invoke('save_draft', { id: doc.draft, text: '' }).catch(() => {});
  Object.assign(doc, { path, draft: null, saved: text });
  rememberRecent(path);
  rememberTabs();
  renderTabs();
  updateHud();
  toast(`已儲存：${nameOf(path)}`);
  return true;
}

async function saveCommand() {
  if (doc.path) {
    await saveNow();
    toast('已儲存');
  } else if (lock) {
    // 存檔對話框會讓視窗失去焦點，小黑屋期間先存成草稿
    await saveNow();
    toast('已存成草稿，出關後再選存檔位置');
  } else {
    await saveAs();
  }
  editor.focus();
}

function lockedOut() {
  if (lock) toast('小黑屋期間不能切換文件');
  return !!lock;
}

/** 新分頁插在目前這篇後面；目前這篇若是沒動過的空白草稿，就直接取代它 */
function addDoc(d) {
  const blank = !doc.path && !editor.value && !doc.saved && !doc.undo.length;
  const i = docs.indexOf(doc);
  if (blank) {
    invoke('save_draft', { id: doc.draft, text: '' }).catch(() => {});
    docs.splice(i, 1, d);
    doc = null;
  } else {
    docs.splice(i + 1, 0, d);
  }
  show(d);
}

async function openDoc(path) {
  if (lockedOut()) return;
  await saveNow();
  const open = docs.find((d) => d.path === path);
  if (open) show(open);
  else {
    try {
      addDoc(makeDoc(path, await invoke('open_doc', { path })));
      rememberRecent(path);
    } catch (e) {
      toast(`無法開啟：${e}`);
    }
  }
  closePanels();
}

async function openOther() {
  if (lockedOut()) return;
  const path = await invoke('pick_open_path', { dir: cfg.last_dir || null });
  if (path) await openDoc(path);
  else editor.focus();
}

async function newDoc() {
  if (lockedOut()) return;
  await saveNow();
  addDoc(makeDoc(null, ''));
  closePanels();
}

async function switchTab(delta) {
  if (docs.length < 2 || lockedOut()) return;
  await saveNow();
  const n = docs.length;
  show(docs[(docs.indexOf(doc) + delta + n) % n]);
}

/** 關閉分頁。還沒存檔的草稿有內容的話，先請使用者選位置 */
async function closeDoc(d = doc) {
  if (lock) {
    toast('小黑屋期間不能關閉文件');
    return;
  }
  await saveNow();
  if (d !== doc) show(d);
  if (!doc.path && editor.value.trim()) {
    toast('這篇還沒存檔，先選個位置吧');
    if (!(await saveAs())) return;
  }
  if (!doc.path) invoke('save_draft', { id: doc.draft, text: '' }).catch(() => {});
  flushJournal();
  const i = docs.indexOf(doc);
  docs.splice(i, 1);
  if (!docs.length) docs.push(makeDoc(null, ''));
  doc = null;
  show(docs[Math.min(i, docs.length - 1)]);
}

/** 分頁列：只有一篇時不顯示，保持畫面乾淨 */
function renderTabs() {
  const bar = $('#tabs');
  bar.hidden = docs.length < 2;
  if (bar.hidden) return;
  bar.replaceChildren(
    ...docs.map((d) => {
      const tab = document.createElement('div');
      tab.className = 'tab' + (d === doc ? ' current' : '');
      tab.title = d.path || '草稿（還沒選存檔位置）';
      const name = document.createElement('button');
      const dirty = docText(d) !== d.saved || !d.path;
      name.textContent = (dirty ? '• ' : '') + titleOf(docText(d));
      name.addEventListener('click', async () => {
        if (d === doc || lockedOut()) return;
        await saveNow();
        show(d);
      });
      const close = document.createElement('button');
      close.className = 'x';
      close.textContent = '×';
      close.title = '關閉（Ctrl+W）';
      close.addEventListener('click', () => closeDoc(d));
      tab.append(name, close);
      return tab;
    }),
  );
}

// ── 復原與重做 ─────────────────────────────────
// 每篇有自己的紀錄。連續同一種編輯（一直打字或一直刪）合併成一步，
// 停頓超過一秒、換行或貼上就開新的一步。輸入法組一個詞算一步。

const UNDO_LIMIT = 200;

const snapshot = () => ({ text: editor.value, sel: [editor.selectionStart, editor.selectionEnd] });

function recordUndo(kind) {
  const now = Date.now();
  const merge = kind === doc.lastKind && kind !== 'break' && kind !== 'paste' && now - doc.lastEdit < 1000;
  doc.lastKind = kind;
  doc.lastEdit = now;
  if (merge) return;
  doc.undo.push(snapshot());
  if (doc.undo.length > UNDO_LIMIT) doc.undo.shift();
  doc.redo = [];
}

function restore(snap) {
  editor.value = snap.text;
  editor.setSelectionRange(...snap.sel);
  doc.lastKind = '';
  edited();
}

function undo() {
  if (!doc.undo.length) return;
  doc.redo.push(snapshot());
  restore(doc.undo.pop());
}

function redo() {
  if (!doc.redo.length) return;
  doc.undo.push(snapshot());
  restore(doc.redo.pop());
}

editor.addEventListener('beforeinput', (e) => {
  // macOS 編輯選單裡的「復原」會走瀏覽器自己的復原，也改用這裡的紀錄
  if (e.inputType === 'historyUndo' || e.inputType === 'historyRedo') {
    e.preventDefault();
    e.inputType === 'historyUndo' ? undo() : redo();
    return;
  }
  // 組字中的變化在 compositionstart 時已經記過
  if (e.isComposing) return;
  const t = e.inputType;
  let kind = 'type';
  if (t.startsWith('delete')) kind = 'delete';
  else if (t === 'insertLineBreak' || t === 'insertParagraph') kind = 'break';
  else if (t === 'insertFromPaste' || t === 'insertFromDrop') kind = 'paste';
  recordUndo(kind);
});

editor.addEventListener('compositionstart', () => recordUndo('type'));

function saveConfig() {
  clearTimeout(configTimer);
  configTimer = setTimeout(() => invoke('save_config', { config: cfg }).catch(() => {}), 300);
}

// ── 每日統計 ───────────────────────────────────
// 字數記的是淨增量：每隔一段時間把這段期間的增減送去累加。

let journalCount = 0; // 上次記進統計時的字數
let pendingSecs = 0;
let lastKeyAt = 0;
let todayWords = 0;

function flushJournal() {
  // 字數要在換文件之前同步算好，所以送出前不能有 await
  const count = countWords(editor.value);
  const words = count - journalCount;
  const secs = pendingSecs;
  journalCount = count;
  pendingSecs = 0;
  if (!words && !secs) return Promise.resolve();
  return invoke('journal_add', { words, secs })
    .then((today) => {
      todayWords = today;
    })
    .catch(() => {});
}

setInterval(flushJournal, 15_000);

// ── 編輯區 ─────────────────────────────────────
// 打字時要讓字盡快畫出來：字數、打字機置中與專注段落都延後、限制頻率，不在輸入當下做

let hudTimer = 0;
let caretTimer = 0;
let wantCenter = false;
let paraTop = 0;
let paraBottom = 0;

/** 內容改變之後（打字、復原、重做）：排定自動存檔、更新畫面 */
function edited() {
  if (document.body.classList.contains('chrome')) {
    clearTimeout(hudTimer);
    hudTimer = setTimeout(() => {
      updateHud();
      renderTabs();
    }, 200);
  }
  clearTimeout(saveTimer);
  saveTimer = setTimeout(saveNow, AUTOSAVE_DELAY);
  scheduleCaret(true);
}

editor.addEventListener('input', edited);

// 游標移動時，專注段落要跟著換
editor.addEventListener('keyup', () => scheduleCaret(false));
editor.addEventListener('mouseup', () => scheduleCaret(false));
editor.addEventListener('scroll', () => {
  if (cfg.focus_paragraph) updateMask();
});

function scheduleCaret(center) {
  if (center) wantCenter = true;
  if (caretTimer) return;
  caretTimer = setTimeout(() => {
    caretTimer = 0;
    caretWork(wantCenter);
    wantCenter = false;
  }, 60);
}

function marker() {
  const span = document.createElement('span');
  span.textContent = '​';
  return span;
}

/** 量出游標與目前段落的位置：打字機模式把游標行捲到中央，專注段落據此淡化其他文字 */
function caretWork(center) {
  const doCenter = center && cfg.typewriter;
  if (!doCenter && !cfg.focus_paragraph) {
    updateMask();
    return;
  }
  const text = editor.value;
  const pos = editor.selectionEnd;
  const start = text.lastIndexOf('\n', pos - 1) + 1;
  let end = text.indexOf('\n', pos);
  if (end < 0) end = text.length;
  const [a, c, b] = [marker(), marker(), marker()];
  mirror.replaceChildren(text.slice(0, start), a, text.slice(start, pos), c, text.slice(pos, end), b);
  const padTop = parseFloat(getComputedStyle(editor).paddingTop);
  if (doCenter) editor.scrollTop = padTop + c.offsetTop + c.offsetHeight / 2 - editor.clientHeight / 2;
  paraTop = padTop + a.offsetTop;
  paraBottom = padTop + b.offsetTop + b.offsetHeight;
  updateMask();
}

/** 專注段落：用遮罩讓目前段落以外的文字變淡，交界處柔和過渡 */
function updateMask() {
  let mask = '';
  if (cfg.focus_paragraph) {
    const top = paraTop - editor.scrollTop;
    const bottom = paraBottom - editor.scrollTop;
    const dim = 'rgba(0,0,0,0.22)';
    const soft = 16;
    mask = `linear-gradient(to bottom, ${dim} ${top - soft}px, #000 ${top}px, #000 ${bottom}px, ${dim} ${bottom + soft}px)`;
  }
  editor.style.webkitMaskImage = mask;
  editor.style.maskImage = mask;
}

/** 這個按鍵該響哪種聲音。用中文輸入法組字時，每按一個鍵也會響 */
function keySoundKind(e) {
  if (e.key === 'Enter') return 'Enter';
  if (e.key === 'Backspace' || e.key === 'Delete') return 'Back';
  if (e.key === ' ') return 'Space';
  // 輸入法組字中的按鍵是 Process（keyCode 229）
  if (e.key === 'Process' || e.keyCode === 229 || e.key.length === 1) return 'Char';
  return null;
}

editor.addEventListener('keydown', (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey) return;
  lastKeyAt = Date.now();
  // 按住不放的自動重複不響，免得變成一串連發
  if (cfg.key_sound !== 'Off' && !e.repeat) {
    const kind = keySoundKind(e);
    if (kind) invoke('audio_key', { kind }).catch(() => {});
  }
  if (pinned || panelOpen()) return;
  clearTimeout(chromeTimer);
  document.body.classList.remove('chrome');
  document.body.classList.add('typing');
});

// ── 浮動介面：滑鼠一動就浮現，開始打字就隱藏 ──────

function panelOpen() {
  return document.querySelector('.panel.open') !== null;
}

function showChrome() {
  if (!document.body.classList.contains('chrome')) {
    updateHud();
    renderTabs();
  }
  document.body.classList.add('chrome');
  document.body.classList.remove('typing');
  clearTimeout(chromeTimer);
  chromeTimer = setTimeout(hideChromeLater, CHROME_TIMEOUT);
}

function hideChromeLater() {
  // 滑鼠停在選單上、開著面板或釘住時都不隱藏
  if (pinned || panelOpen() || $('#bar').matches(':hover')) {
    chromeTimer = setTimeout(hideChromeLater, CHROME_TIMEOUT);
    return;
  }
  document.body.classList.remove('chrome');
}

window.addEventListener('mousemove', showChrome);

function updateHud() {
  const words = countWords(editor.value);
  const delta = words - doc.startWords;
  const today = Math.max(0, todayWords + words - journalCount);
  const now = new Date();
  const parts = [
    titleOf(editor.value),
    `${words} 字`,
    `本次 ${delta >= 0 ? '+' : ''}${delta}`,
    `今日 ${today} / ${cfg.daily_goal}`,
    `${pad2(now.getHours())}:${pad2(now.getMinutes())}`,
  ];
  if (!doc.path) parts.push(lock ? '草稿' : '草稿・Ctrl+S 選擇存檔位置');
  else if (editor.value !== doc.saved) parts.push('未儲存');
  $('#hud-line').textContent = parts.join('　　');
  $('#hud-lock').textContent = lock ? lockStatus() : '';
}

setInterval(() => {
  if (document.body.classList.contains('chrome')) updateHud();
}, 30_000);

let toastTimer = 0;
function toast(msg) {
  const el = $('#toast');
  el.textContent = msg;
  el.classList.add('show');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove('show'), 3000);
}

// ── 面板 ───────────────────────────────────────

function closePanels() {
  document.querySelectorAll('.panel.open').forEach((p) => p.classList.remove('open'));
  editor.focus();
}

function togglePanel(id) {
  const panel = document.getElementById(id);
  const open = !panel.classList.contains('open');
  closePanels();
  if (open) {
    panel.classList.add('open');
    showChrome();
  }
  return open;
}

async function showDocs() {
  if (lock) return;
  await saveNow();
  const list = $('#doc-list');
  list.replaceChildren();
  const docs = await invoke('recent_docs', { paths: cfg.recent || [] });
  if (docs.length === 0) {
    const li = document.createElement('li');
    li.className = 'empty';
    li.textContent = '還沒有最近的文件。';
    list.appendChild(li);
  }
  for (const d of docs) {
    const li = document.createElement('li');
    const btn = document.createElement('button');
    btn.className = d.path === doc.path ? 'current' : '';
    btn.title = d.path;
    const title = document.createElement('span');
    title.textContent = d.title;
    const time = document.createElement('span');
    time.className = 'time';
    time.textContent = d.modified;
    btn.append(title, time);
    btn.addEventListener('click', () => openDoc(d.path));
    li.appendChild(btn);
    list.appendChild(li);
  }
  togglePanel('panel-docs');
}

// ── 外觀 ───────────────────────────────────────

function applyLook() {
  const t = THEMES[cfg.theme] || THEMES.Snow;
  const root = document.documentElement.style;
  const bg = mixHex(t.sky[0], t.sky[1], 0.4);
  const text = hexToRgb(t.text);
  const base = t.dark ? mixHex(rgbHex(bg), t.text, 0.06) : [255, 255, 255];

  root.setProperty('--text', t.text);
  root.setProperty('--accent', t.accent);
  root.setProperty('--ui', rgba(text, 0.62));
  root.setProperty('--dim', rgba(text, 0.3));
  root.setProperty('--hover', rgba(text, 0.08));
  root.setProperty('--line', rgba(text, 0.14));
  root.setProperty('--panel', rgba(base, t.dark ? 0.55 : 0.5));
  root.setProperty('--panel-strong', rgba(base, t.dark ? 0.98 : 0.95));
  root.setProperty('--shadow', t.dark ? 0.45 : 0.12);
  document.body.style.background = rgbHex(bg);

  root.setProperty('--font', FONTS[cfg.font_style] || FONTS.Serif);
  root.setProperty('--size', `${cfg.font_size}px`);
  root.setProperty('--leading', cfg.line_spacing);
  root.setProperty('--width', `${cfg.column_width}px`);

  drawSceneOn($('#scene'), t, { finish: true });
  syncLookPanel();
  caretWork(true);
}

function rgbHex(rgb) {
  return '#' + rgb.map((v) => v.toString(16).padStart(2, '0')).join('');
}

function setTheme(name) {
  cfg.theme = name;
  applyLook();
  saveConfig();
}

function nextTheme() {
  const i = THEME_ORDER.indexOf(cfg.theme);
  setTheme(THEME_ORDER[(i + 1) % THEME_ORDER.length]);
}

function changeFontSize(delta) {
  cfg.font_size = Math.min(40, Math.max(14, cfg.font_size + delta));
  applyLook();
  saveConfig();
}

function buildScenePicker() {
  const box = $('#scenes');
  for (const key of THEME_ORDER) {
    const t = THEMES[key];
    const btn = document.createElement('button');
    btn.dataset.theme = key;
    const canvas = document.createElement('canvas');
    const label = document.createElement('span');
    label.textContent = t.name;
    btn.append(canvas, label);
    btn.addEventListener('click', () => setTheme(key));
    box.appendChild(btn);
    // 等排版完成才知道縮圖的實際大小
    requestAnimationFrame(() => {
      const ctx = drawSceneOn(canvas, t);
      // 用該場景的文字色畫兩行假字，預覽閱讀起來的感覺
      ctx.strokeStyle = rgba(hexToRgb(t.text), 0.8);
      ctx.lineWidth = 2;
      [[0.62, 18], [0.42, 26]].forEach(([w, y]) => {
        ctx.beginPath();
        ctx.moveTo(14, y);
        ctx.lineTo(14 + (78 - 28) * w, y);
        ctx.stroke();
      });
    });
  }
}

const percent = (v) => `${Math.round(v * 100)}%`;

// [選擇器, 設定鍵, 顯示格式, 改變後要做的事]
const sliders = [
  ['#font-size', 'font_size', (v) => v, applyLook],
  ['#line-spacing', 'line_spacing', (v) => v.toFixed(1), applyLook],
  ['#column-width', 'column_width', (v) => v, applyLook],
  ['#ambient-volume', 'ambient_volume', percent, applyAudio],
  ['#key-volume', 'key_volume', percent, applyAudio],
];

// [容器, 設定鍵, 改變後要做的事]
const choices = [
  ['#font-style', 'font_style', applyLook],
  ['#ambient', 'ambient', applyAudio],
  ['#key-sound', 'key_sound', applyAudio],
];

function syncLookPanel() {
  document.querySelectorAll('#scenes button').forEach((b) => {
    b.classList.toggle('current', b.dataset.theme === cfg.theme);
  });
  for (const [sel, key] of choices) {
    document.querySelectorAll(`${sel} button`).forEach((b) => {
      b.classList.toggle('current', (b.dataset.v ?? b.dataset.font) === cfg[key]);
    });
  }
  for (const [sel, key, fmt] of sliders) {
    const input = $(sel);
    input.value = cfg[key];
    input.nextElementSibling.textContent = fmt(cfg[key]);
  }
  $('#typewriter').checked = cfg.typewriter;
  $('#focus-paragraph').checked = cfg.focus_paragraph;
}

for (const [sel, key, , after] of sliders) {
  $(sel).addEventListener('input', (e) => {
    cfg[key] = Number(e.target.value);
    after();
    syncLookPanel();
    saveConfig();
  });
}

for (const [sel, key, after] of choices) {
  document.querySelectorAll(`${sel} button`).forEach((b) => {
    b.addEventListener('click', () => {
      cfg[key] = b.dataset.v ?? b.dataset.font;
      after();
      syncLookPanel();
      saveConfig();
    });
  });
}

function setTypewriter(on) {
  cfg.typewriter = on;
  syncLookPanel();
  saveConfig();
  toast(on ? '打字機模式：開' : '打字機模式：關');
}

function setFocusParagraph(on) {
  cfg.focus_paragraph = on;
  syncLookPanel();
  saveConfig();
  caretWork(false);
  toast(on ? '專注段落：開' : '專注段落：關');
}

$('#typewriter').addEventListener('change', (e) => setTypewriter(e.target.checked));
$('#focus-paragraph').addEventListener('change', (e) => setFocusParagraph(e.target.checked));

let resizeTimer = 0;
window.addEventListener('resize', () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    drawSceneOn($('#scene'), THEMES[cfg.theme] || THEMES.Snow, { finish: true });
    caretWork(true);
  }, 100);
});

// ── 聲音 ───────────────────────────────────────

function applyAudio() {
  invoke('audio_settings', {
    ambient: cfg.ambient,
    ambientVolume: cfg.ambient_volume,
    keySound: cfg.key_sound,
    keyVolume: cfg.key_volume,
  }).catch(() => {});
}

// 換打字音效時先響一聲讓人試聽
document.querySelectorAll('#key-sound button').forEach((b) => {
  b.addEventListener('click', () => {
    if (b.dataset.v !== 'Off') invoke('audio_key', { kind: 'Char' }).catch(() => {});
  });
});

// ── 統計 ───────────────────────────────────────

async function showStats() {
  if (!togglePanel('panel-stats')) return;
  await flushJournal();
  renderStats(await invoke('journal_summary'));
}

function renderStats(s) {
  const goal = cfg.daily_goal;
  const today = Math.max(0, s.today.words);
  todayWords = today;
  $('#st-today').textContent = today;
  $('#st-today-sub').textContent = `今日字數・寫了 ${fmtMinutes(s.today.secs)}`;
  $('#st-streak').textContent = `連續寫作 ${s.streak} 天`;
  $('#st-progress').style.width = `${Math.min(100, (today / Math.max(goal, 1)) * 100)}%`;
  $('#daily-goal').value = goal;
  $('#st-goal-done').textContent = today >= goal ? '今天的目標達成了' : '';

  const days = s.recent.map(([date, d]) => ({ date, words: Math.max(0, d.words), secs: d.secs }));
  const week = days.slice(-7).reduce((n, d) => n + d.words, 0);
  const month = days.reduce((n, d) => n + d.words, 0);
  const active = days.filter((d) => d.words > 0).length;
  let totals = `近 7 天 ${week} 字・近 30 天 ${month} 字・有寫字的日子平均 ${active ? Math.round(month / active) : 0} 字`;
  if (s.best) totals += `\n最高紀錄：${s.best[0].replaceAll('-', '/')} 寫了 ${s.best[1]} 字`;
  $('#st-totals').textContent = totals;
  $('#st-first').textContent = days.length ? days[0].date.slice(5).replace('-', '/') : '';
  drawChart(days, goal);
}

/** 近 30 天長條圖：單一數列用主題強調色，虛線為每日目標，滑過長條顯示明細 */
function drawChart(days, goal) {
  const svg = $('#st-chart');
  const NS = 'http://www.w3.org/2000/svg';
  const w = 600;
  const h = 140;
  const top = 8;
  svg.setAttribute('viewBox', `0 0 ${w} ${h}`);
  svg.replaceChildren();
  const max = Math.max(goal, ...days.map((d) => d.words), 1);
  const y = (v) => h - (h - top) * (v / max);
  const slot = w / days.length;
  const hover = $('#st-hover');

  const el = (name, attrs) => {
    const e = document.createElementNS(NS, name);
    for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
    svg.appendChild(e);
    return e;
  };

  el('line', { x1: 0, x2: w, y1: h - 0.5, y2: h - 0.5, class: 'axis' });
  days.forEach((d, i) => {
    const x = i * slot;
    const hit = el('rect', { x, y: 0, width: slot, height: h, class: 'hit' });
    if (d.words > 0) {
      el('rect', { x: x + 1.5, y: y(d.words), width: Math.max(slot - 3, 1), height: h - y(d.words), rx: 2, class: 'bar' });
    }
    hit.addEventListener('mouseenter', () => {
      hover.textContent = `${d.date.replaceAll('-', '/')}：${d.words} 字・${fmtMinutes(d.secs)}`;
    });
    hit.addEventListener('mouseleave', () => {
      hover.textContent = '虛線為每日目標';
    });
  });
  // 目標線畫在長條上方，才不會被蓋住
  el('line', { x1: 0, x2: w, y1: y(goal), y2: y(goal), class: 'goal' });
}

$('#daily-goal').addEventListener('change', async (e) => {
  const v = Math.round(Number(e.target.value));
  if (!(v >= 50)) return;
  cfg.daily_goal = Math.min(v, 100_000);
  saveConfig();
  renderStats(await invoke('journal_summary'));
});

// ── 小黑屋 ─────────────────────────────────────
// 達成目標之前不能關閉視窗、離開全螢幕或切換文件。狀態會存進磁碟，
// 強制結束後重新開啟也會接著鎖；計時只在程式開著時累積。

let lock = null;
let lockSavedAt = 0;
let lastTick = performance.now();

const lockWordsWritten = () => Math.max(0, countWords(editor.value) - lock.start_words);
const lockTimeLeft = () =>
  lock.target_minutes == null ? null : Math.max(0, lock.target_minutes * 60 - lock.elapsed_secs);
const lockWordsLeft = () => (lock.target_words == null ? null : Math.max(0, lock.target_words - lockWordsWritten()));

/** 0–1，同時設定兩種目標時取較慢的那一個 */
function lockProgress() {
  const parts = [];
  if (lock.target_minutes != null) parts.push(lock.elapsed_secs / (lock.target_minutes * 60));
  if (lock.target_words != null) parts.push(lockWordsWritten() / Math.max(lock.target_words, 1));
  return parts.length ? Math.min(1, Math.max(0, Math.min(...parts))) : 1;
}

function lockStatus() {
  const parts = [];
  const t = lockTimeLeft();
  if (t != null) parts.push(t > 0 ? `剩 ${fmtDuration(t)}` : '時間已到');
  const w = lockWordsLeft();
  if (w != null) parts.push(w > 0 ? `還差 ${w} 字` : '字數已達成');
  return `小黑屋・${parts.join('・')}`;
}

function showLockPanel() {
  if (lock) return;
  $('#lock-minutes').value = cfg.lock_minutes;
  $('#lock-words').value = cfg.lock_words;
  syncLockForm();
  togglePanel('panel-lock');
}

function syncLockForm() {
  const time = $('#lock-use-time').checked;
  const words = $('#lock-use-words').checked;
  $('#lock-minutes').disabled = !time;
  $('#lock-words').disabled = !words;
  $('#lock-both').hidden = !(time && words);
}

['#lock-use-time', '#lock-use-words'].forEach((s) => $(s).addEventListener('change', syncLockForm));

async function enterLock() {
  const useTime = $('#lock-use-time').checked;
  const useWords = $('#lock-use-words').checked;
  if (!useTime && !useWords) {
    toast('請至少設定一個目標');
    return;
  }
  cfg.lock_minutes = Math.min(720, Math.max(1, Math.round(Number($('#lock-minutes').value) || 30)));
  cfg.lock_words = Math.min(200_000, Math.max(10, Math.round(Number($('#lock-words').value) || 500)));
  saveConfig();
  await saveNow();
  lock = {
    target_minutes: useTime ? cfg.lock_minutes : null,
    target_words: useWords ? cfg.lock_words : null,
    elapsed_secs: 0,
    start_words: countWords(editor.value),
    doc: doc.path,
    draft: doc.draft,
    escapes: 0,
    allow_emergency: $('#lock-emergency').checked,
  };
  await invoke('lock_save', { lock });
  lockSavedAt = Date.now();
  pinned = false;
  applyLockState();
  closePanels();
  toast('門已上鎖。寫吧。');
}

/** 進入或恢復小黑屋時：全螢幕、置頂、搶回焦點 */
function applyLockState() {
  document.body.classList.toggle('locked', !!lock);
  document.body.classList.toggle('no-exit', !!lock && !lock.allow_emergency);
  updateLockbar();
  if (!lock) return;
  appWindow.setFullscreen(true).catch(() => {});
  appWindow.setAlwaysOnTop(true).catch(() => {});
  appWindow.setFocus().catch(() => {});
}

function updateLockbar() {
  $('#lockbar').style.width = lock ? `${lockProgress() * 100}%` : '0';
}

function lockTick(dt) {
  lock.elapsed_secs += dt;
  updateLockbar();
  const t = lockTimeLeft();
  const w = lockWordsLeft();
  if ((t == null || t <= 0) && (w == null || w === 0)) {
    finishLock();
    return;
  }
  if (Date.now() - lockSavedAt > 5000) {
    lockSavedAt = Date.now();
    invoke('lock_save', { lock }).catch(() => {});
  }
  if (document.body.classList.contains('chrome')) $('#hud-lock').textContent = lockStatus();
}

async function leaveLock() {
  lock = null;
  await invoke('lock_clear').catch(() => {});
  appWindow.setAlwaysOnTop(false).catch(() => {});
  applyLockState();
  updateHud();
}

async function finishLock() {
  const done = lock;
  const written = lockWordsWritten();
  await leaveLock();
  await saveNow();
  let msg = `這次寫了 ${written} 字，歷時 ${fmtDuration(done.elapsed_secs)}。`;
  if (done.escapes > 0) msg += `\n中途試圖逃跑 ${done.escapes} 次。`;
  $('#done-msg').textContent = msg;
  closePanels();
  $('#panel-done').classList.add('open');
  showChrome();
}

function showEmergency() {
  if (!lock || !lock.allow_emergency) return;
  $('#phrase').textContent = EMERGENCY_PHRASE;
  $('#emergency-input').value = '';
  $('#emergency-go').disabled = true;
  if (togglePanel('panel-emergency')) $('#emergency-input').focus();
}

$('#emergency-input').addEventListener('input', (e) => {
  $('#emergency-go').disabled = e.target.value.trim() !== EMERGENCY_PHRASE;
});

$('#emergency-go').addEventListener('click', async () => {
  if ($('#emergency-input').value.trim() !== EMERGENCY_PHRASE) return;
  await leaveLock();
  closePanels();
  toast('已從緊急出口離開。');
});

let refocusTimer = 0;
appWindow.onFocusChanged(({ payload: focused }) => {
  if (!lock || focused) return;
  lock.escapes += 1;
  toast('想去哪？還沒寫完呢。');
  clearTimeout(refocusTimer);
  refocusTimer = setTimeout(async () => {
    if (!lock) return;
    await appWindow.unminimize().catch(() => {});
    await appWindow.setFullscreen(true).catch(() => {});
    await appWindow.setFocus().catch(() => {});
  }, 400);
});

appWindow.onResized(async () => {
  if (lock && !(await appWindow.isFullscreen())) appWindow.setFullscreen(true).catch(() => {});
});

// 每秒一次：累計寫作時間、推進小黑屋的計時
setInterval(() => {
  const now = performance.now();
  // 上限 3 秒：系統休眠時不算進去
  const dt = Math.min((now - lastTick) / 1000, 3);
  lastTick = now;
  if (document.hasFocus() && Date.now() - lastKeyAt < ACTIVE_WINDOW) pendingSecs += dt;
  if (lock) lockTick(dt);
}, 1000);

// ── 視窗 ───────────────────────────────────────

async function toggleFullscreen() {
  if (lock) return;
  const fs = await appWindow.isFullscreen();
  await appWindow.setFullscreen(!fs);
  cfg.start_fullscreen = !fs;
  saveConfig();
}

async function saveEverything() {
  await saveNow();
  await flushJournal();
  await invoke('save_config', { config: cfg }).catch(() => {});
}

async function quit() {
  if (lock) {
    toast(`門鎖著。${lockStatus()}`);
    return;
  }
  await saveEverything();
  await appWindow.destroy();
}

// macOS 的 Cmd+Q 會直接要求結束程式，後端先攔下來讓這裡存檔
window.__TAURI__.event.listen('quit-requested', quit);

appWindow.onCloseRequested(async (event) => {
  if (lock) {
    event.preventDefault();
    lock.escapes += 1;
    invoke('lock_save', { lock }).catch(() => {});
    toast(`門鎖著。${lockStatus()}`);
    return;
  }
  await saveEverything();
});

// ── 按鈕與快捷鍵 ───────────────────────────────

const actions = {
  docs: showDocs,
  new: newDoc,
  open: openOther,
  look: () => togglePanel('panel-look'),
  sound: () => togglePanel('panel-sound'),
  stats: showStats,
  lock: showLockPanel,
  'enter-lock': enterLock,
  emergency: showEmergency,
  fullscreen: toggleFullscreen,
  quit,
  close: closePanels,
};

document.querySelectorAll('[data-act]').forEach((el) => {
  el.addEventListener('click', () => actions[el.dataset.act]());
});

window.addEventListener('keydown', (e) => {
  if (e.isComposing) return;
  const mod = e.ctrlKey || e.metaKey;
  const key = e.key.toLowerCase();
  let handled = true;

  if (key === 'escape') {
    if (panelOpen()) closePanels();
    else {
      pinned = !pinned;
      pinned ? showChrome() : document.body.classList.remove('chrome');
    }
  } else if (mod && key === 'z' && document.activeElement === editor) {
    e.shiftKey ? redo() : undo();
  } else if (mod && key === 'y' && document.activeElement === editor) redo();
  else if (mod && key === 'tab') switchTab(e.shiftKey ? -1 : 1);
  else if (mod && key === 'pagedown') switchTab(1);
  else if (mod && key === 'pageup') switchTab(-1);
  else if (mod && key === 'w') closeDoc();
  else if (mod && key === 's') saveCommand();
  else if (mod && key === 'o') showDocs();
  else if (mod && key === 'n') newDoc();
  else if (mod && key === 'l') showLockPanel();
  else if (mod && key === 't') nextTheme();
  else if (mod && (key === '=' || key === '+')) changeFontSize(1);
  else if (mod && key === '-') changeFontSize(-1);
  else if (mod && key === 'q') quit();
  else if (key === 'f8') setFocusParagraph(!cfg.focus_paragraph);
  else if (key === 'f9') setTypewriter(!cfg.typewriter);
  else if (key === 'f11') toggleFullscreen();
  else handled = false;

  if (handled) e.preventDefault();
}, true);

// ── 啟動 ───────────────────────────────────────

/** 還原上次開著的分頁。檔案被移走或刪掉的就略過；小黑屋鎖住的那篇一定要開 */
async function restoreTabs(init) {
  let entries = Array.isArray(cfg.open_docs) ? cfg.open_docs : [];
  let legacy = null;
  if (!entries.length) {
    // 從只能開一篇的舊版本轉過來
    if (cfg.last_doc) entries = [{ path: cfg.last_doc }];
    if (init.legacy_draft) {
      legacy = { draft: newDraftId() };
      entries.push(legacy);
    }
  }
  if (init.lock) {
    const l = init.lock;
    const has = entries.some((e) => (l.doc ? e.path === l.doc : e.draft === l.draft));
    if (!has) entries.push(l.doc ? { path: l.doc } : { draft: l.draft || newDraftId() });
  }

  for (const e of entries) {
    try {
      if (e.path) {
        if (await invoke('doc_exists', { path: e.path })) docs.push(makeDoc(e.path, await invoke('open_doc', { path: e.path })));
      } else if (e.draft) {
        const text = e === legacy ? init.legacy_draft : await invoke('load_draft', { id: e.draft });
        if (e === legacy) {
          await invoke('save_draft', { id: e.draft, text });
          invoke('clear_legacy_draft').catch(() => {});
        }
        // 空白的草稿分頁不用還原
        if (text || (init.lock && init.lock.draft === e.draft)) docs.push(makeDoc(null, text, e.draft));
      }
    } catch (err) {
      toast(`無法開啟：${err}`);
    }
  }
  if (!docs.length) docs.push(makeDoc(null, ''));

  let active = docs[Math.min(Math.max(cfg.active_doc | 0, 0), docs.length - 1)];
  if (init.lock) {
    const l = init.lock;
    active = docs.find((d) => (l.doc ? d.path === l.doc : d.draft === l.draft)) || active;
  }
  show(active);
}

async function boot() {
  buildScenePicker();
  const init = await invoke('init');
  cfg = { ...DEFAULTS, ...init.config };
  applyLook();
  applyAudio();
  if (init.audio_error) {
    $('#audio-error').textContent = `音訊無法啟動：${init.audio_error}`;
    $('#audio-error').hidden = false;
  }
  await restoreTabs(init);
  invoke('journal_summary')
    .then((s) => {
      todayWords = Math.max(0, s.today.words);
    })
    .catch(() => {});
  if (init.lock) {
    lock = init.lock;
    lockSavedAt = Date.now();
    applyLockState();
    toast('小黑屋還沒結束，繼續寫吧。');
  }
  editor.focus();
}

boot().catch((e) => toast(`啟動失敗：${e}`));
