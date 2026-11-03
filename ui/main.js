'use strict';

const { invoke } = window.__TAURI__.core;
const appWindow = window.__TAURI__.window.getCurrentWindow();

const DEFAULTS = {
  theme: 'Snow',
  font_style: 'Serif',
  font_size: 22,
  line_spacing: 1.8,
  column_width: 720,
  typewriter: true,
  start_fullscreen: true,
  last_doc: null,
};

const FONTS = {
  Serif: '"Noto Serif CJK TC", "Noto Serif TC", "Source Han Serif TC", "Songti TC", "PMingLiU", serif',
  Sans: '"Noto Sans CJK TC", "Noto Sans TC", "Source Han Sans TC", "PingFang TC", "Microsoft JhengHei", sans-serif',
};

const CHROME_TIMEOUT = 2500;
const AUTOSAVE_DELAY = 1000;

const $ = (s) => document.querySelector(s);
const editor = $('#editor');
const page = $('#page');
const mirror = $('#mirror');
const scroller = $('#scroller');

let cfg = { ...DEFAULTS };
let docPath = '';
let savedText = '';
let sessionStartWords = 0;
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

// ── 文件 ───────────────────────────────────────

function loadText(path, text) {
  docPath = path;
  editor.value = text;
  savedText = text;
  sessionStartWords = countWords(text);
  editor.setSelectionRange(text.length, text.length);
  textChanged();
  if (cfg.last_doc !== path) {
    cfg.last_doc = path;
    saveConfig();
  }
}

async function saveNow() {
  clearTimeout(saveTimer);
  const text = editor.value;
  if (text === savedText) return;
  try {
    await invoke('save_doc', { path: docPath, text });
    savedText = text;
    updateHud();
  } catch (e) {
    toast(`儲存失敗：${e}`);
  }
}

async function openDoc(path) {
  await saveNow();
  try {
    loadText(path, await invoke('open_doc', { path }));
  } catch (e) {
    toast(`無法開啟：${e}`);
  }
  closePanels();
}

async function newDoc() {
  await saveNow();
  loadText(await invoke('new_doc'), '');
  closePanels();
}

async function exportTxt() {
  try {
    const path = await invoke('export_txt', { text: editor.value, name: titleOf(editor.value) });
    toast(`已匯出：${path}`);
  } catch (e) {
    toast(`匯出失敗：${e}`);
  }
}

function saveConfig() {
  clearTimeout(configTimer);
  configTimer = setTimeout(() => invoke('save_config', { config: cfg }).catch(() => {}), 300);
}

// ── 編輯區 ─────────────────────────────────────

let hudFrame = 0;

function textChanged() {
  page.dataset.value = editor.value;
  cancelAnimationFrame(hudFrame);
  hudFrame = requestAnimationFrame(updateHud);
}

editor.addEventListener('input', () => {
  textChanged();
  clearTimeout(saveTimer);
  saveTimer = setTimeout(saveNow, AUTOSAVE_DELAY);
  centerCaret();
});

/** 打字機模式：讓游標所在的行保持在畫面中央 */
function centerCaret() {
  if (!cfg.typewriter) return;
  mirror.textContent = editor.value.slice(0, editor.selectionEnd);
  const mark = document.createElement('span');
  mark.textContent = '​';
  mirror.appendChild(mark);
  const y = page.offsetTop + mirror.offsetTop + mark.offsetTop + mark.offsetHeight / 2;
  scroller.scrollTo({ top: y - scroller.clientHeight / 2, behavior: 'smooth' });
}

// 點到文字以外的地方也回到編輯區
scroller.addEventListener('mousedown', (e) => {
  if (e.target !== editor) {
    e.preventDefault();
    editor.focus();
  }
});

// ── 浮動介面：滑鼠一動就浮現，開始打字就隱藏 ──────

function panelOpen() {
  return document.querySelector('.panel.open') !== null;
}

function showChrome() {
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

editor.addEventListener('keydown', (e) => {
  if (e.ctrlKey || e.metaKey || e.altKey || pinned || panelOpen()) return;
  clearTimeout(chromeTimer);
  document.body.classList.remove('chrome');
  document.body.classList.add('typing');
});

function updateHud() {
  const words = countWords(editor.value);
  const delta = words - sessionStartWords;
  const now = new Date();
  const time = `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}`;
  const parts = [titleOf(editor.value), `${words} 字`, `本次 ${delta >= 0 ? '+' : ''}${delta}`, time];
  if (editor.value !== savedText) parts.push('未儲存');
  $('#hud').textContent = parts.join('　　');
}
setInterval(updateHud, 30_000);

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
}

async function showDocs() {
  await saveNow();
  const list = $('#doc-list');
  list.replaceChildren();
  const docs = await invoke('list_docs');
  if (docs.length === 0) {
    const li = document.createElement('li');
    li.className = 'empty';
    li.textContent = '還沒有文件。';
    list.appendChild(li);
  }
  for (const d of docs) {
    const li = document.createElement('li');
    const btn = document.createElement('button');
    btn.className = d.path === docPath ? 'current' : '';
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
  root.setProperty('--panel-strong', rgba(base, t.dark ? 0.82 : 0.78));
  root.setProperty('--shadow', t.dark ? 0.45 : 0.12);
  root.setProperty('--vignette', t.vignette);
  root.setProperty('--grain', t.grain);
  document.body.style.background = rgbHex(bg);

  root.setProperty('--font', FONTS[cfg.font_style] || FONTS.Serif);
  root.setProperty('--size', `${cfg.font_size}px`);
  root.setProperty('--leading', cfg.line_spacing);
  root.setProperty('--width', `${cfg.column_width}px`);

  drawSceneOn($('#scene'), t);
  syncLookPanel();
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

const sliders = [
  ['#font-size', 'font_size', (v) => v],
  ['#line-spacing', 'line_spacing', (v) => v.toFixed(1)],
  ['#column-width', 'column_width', (v) => v],
];

function syncLookPanel() {
  document.querySelectorAll('#scenes button').forEach((b) => {
    b.classList.toggle('current', b.dataset.theme === cfg.theme);
  });
  document.querySelectorAll('#font-style button').forEach((b) => {
    b.classList.toggle('current', b.dataset.font === cfg.font_style);
  });
  for (const [sel, key, fmt] of sliders) {
    const input = $(sel);
    input.value = cfg[key];
    input.nextElementSibling.textContent = fmt(cfg[key]);
  }
  $('#typewriter').checked = cfg.typewriter;
}

for (const [sel, key] of sliders) {
  $(sel).addEventListener('input', (e) => {
    cfg[key] = Number(e.target.value);
    applyLook();
    saveConfig();
  });
}

document.querySelectorAll('#font-style button').forEach((b) => {
  b.addEventListener('click', () => {
    cfg.font_style = b.dataset.font;
    applyLook();
    saveConfig();
  });
});

function setTypewriter(on) {
  cfg.typewriter = on;
  syncLookPanel();
  saveConfig();
  toast(on ? '打字機模式：開' : '打字機模式：關');
}

$('#typewriter').addEventListener('change', (e) => setTypewriter(e.target.checked));

let resizeTimer = 0;
window.addEventListener('resize', () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => drawSceneOn($('#scene'), THEMES[cfg.theme] || THEMES.Snow), 100);
});

// ── 視窗 ───────────────────────────────────────

async function toggleFullscreen() {
  const fs = await appWindow.isFullscreen();
  await appWindow.setFullscreen(!fs);
  cfg.start_fullscreen = !fs;
  saveConfig();
}

async function quit() {
  await saveNow();
  await invoke('save_config', { config: cfg }).catch(() => {});
  await appWindow.destroy();
}

appWindow.onCloseRequested(async () => {
  await saveNow();
  await invoke('save_config', { config: cfg }).catch(() => {});
});

// ── 按鈕與快捷鍵 ───────────────────────────────

const actions = {
  docs: showDocs,
  new: newDoc,
  export: exportTxt,
  look: () => togglePanel('panel-look'),
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
  } else if (mod && key === 's') {
    saveNow().then(() => toast('已儲存'));
  } else if (mod && key === 'o') showDocs();
  else if (mod && key === 'n') newDoc();
  else if (mod && key === 'e') exportTxt();
  else if (mod && key === 't') nextTheme();
  else if (mod && (key === '=' || key === '+')) changeFontSize(1);
  else if (mod && key === '-') changeFontSize(-1);
  else if (mod && key === 'q') quit();
  else if (key === 'f9') setTypewriter(!cfg.typewriter);
  else if (key === 'f11') toggleFullscreen();
  else handled = false;

  if (handled) e.preventDefault();
}, true);

// ── 啟動 ───────────────────────────────────────

async function boot() {
  document.documentElement.style.setProperty('--grain-img', `url(${grainDataUrl()})`);
  buildScenePicker();
  const init = await invoke('init');
  cfg = { ...DEFAULTS, ...init.config };
  $('#docs-dir').textContent = init.docs_dir;
  applyLook();
  loadText(init.doc_path, init.text);
  editor.focus();
}

boot().catch((e) => toast(`啟動失敗：${e}`));
