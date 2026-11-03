// 背景的景：漸層天空、日月光暈、星星、層層遠山與霧。全部即時繪製，不用圖檔。
'use strict';

const THEMES = {
  Snow: {
    name: '雪', dark: false, text: '#424a56', accent: '#688aaa',
    sky: ['#c9d4e0', '#eaedf0'], horizon: 0.66,
    glow: { rgb: [255, 250, 240], alpha: 0.6, x: 0.74, y: 0.4, r: 0.5 },
    hills: ['#cbd3dc', '#dbe1e7', '#eceff2'], fog: '#eef0f3',
    stars: false, grain: 0.081, vignette: 0.08, seed: 7,
  },
  Night: {
    name: '夜', dark: true, text: '#c8ceda', accent: '#8caae6',
    sky: ['#0b0e18', '#1e2538'], horizon: 0.7,
    glow: { rgb: [150, 172, 215], alpha: 0.24, x: 0.8, y: 0.2, r: 0.4 },
    hills: ['#1e2436', '#161b2a', '#0f121d'], fog: '#22293c',
    stars: true, grain: 0.063, vignette: 0.35, seed: 3,
  },
  Paper: {
    name: '紙', dark: false, text: '#3c3630', accent: '#b06c3e',
    sky: ['#f7f3eb', '#eee7da'], horizon: 1,
    glow: { rgb: [255, 252, 245], alpha: 0.5, x: 0.5, y: 0.3, r: 0.7 },
    hills: [], fog: '#eee7da',
    stars: false, grain: 0.135, vignette: 0.12, seed: 1,
  },
  Forest: {
    name: '森', dark: true, text: '#ced8ca', accent: '#9cc48a',
    sky: ['#141d1b', '#34443c'], horizon: 0.68,
    glow: { rgb: [214, 222, 170], alpha: 0.18, x: 0.76, y: 0.64, r: 0.45 },
    hills: ['#2c3c35', '#212e28', '#17211d'], fog: '#384840',
    stars: false, grain: 0.063, vignette: 0.3, seed: 11,
  },
  Dusk: {
    name: '暮', dark: true, text: '#ece0e8', accent: '#f0a892',
    sky: ['#262038', '#845c6a'], horizon: 0.7,
    glow: { rgb: [255, 186, 150], alpha: 0.36, x: 0.66, y: 0.68, r: 0.5 },
    hills: ['#684c62', '#4a374e', '#2e2436'], fog: '#805c6c',
    stars: false, grain: 0.063, vignette: 0.3, seed: 5,
  },
};

const THEME_ORDER = ['Snow', 'Night', 'Paper', 'Forest', 'Dusk'];

function xorshift(seed) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13; s >>>= 0;
    s ^= s >>> 17;
    s ^= s << 5; s >>>= 0;
    return s / 4294967296;
  };
}

function hexToRgb(hex) {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function mixHex(a, b, t) {
  const [x, y] = [hexToRgb(a), hexToRgb(b)];
  return x.map((v, i) => Math.round(v + (y[i] - v) * t));
}

const rgba = (rgb, a = 1) => `rgba(${rgb[0]},${rgb[1]},${rgb[2]},${a})`;

/** 在 (0,0)–(w,h) 範圍畫出整個場景 */
function paintScene(ctx, w, h, t) {
  const horizon = h * t.horizon;

  const sky = ctx.createLinearGradient(0, 0, 0, h);
  sky.addColorStop(0, t.sky[0]);
  sky.addColorStop(Math.min(t.horizon, 1), t.sky[1]);
  if (t.horizon < 1) sky.addColorStop(1, t.fog);
  ctx.fillStyle = sky;
  ctx.fillRect(0, 0, w, h);

  if (t.glow) {
    const g = t.glow;
    const [cx, cy, r] = [w * g.x, h * g.y, h * g.r];
    const grad = ctx.createRadialGradient(cx, cy, 0, cx, cy, r);
    // 立方衰減，中心亮、邊緣柔
    for (let i = 0; i <= 8; i++) {
      const s = i / 8;
      grad.addColorStop(s, rgba(g.rgb, g.alpha * (1 - s) ** 3));
    }
    ctx.fillStyle = grad;
    ctx.fillRect(0, 0, w, h);
  }

  if (t.stars) {
    const rand = xorshift(Math.imul(t.seed, 2654435761));
    const count = Math.floor((w * h) / 9000);
    for (let i = 0; i < count; i++) {
      const x = rand() * w;
      // 越接近地平線越稀疏
      const y = rand() * rand() * horizon * 0.9;
      const size = 0.4 + rand() * rand() * 1.1;
      const alpha = 0.08 + rand() * rand() * 0.4;
      ctx.fillStyle = `rgba(255,255,255,${alpha})`;
      ctx.beginPath();
      ctx.arc(x, y, size, 0, Math.PI * 2);
      ctx.fill();
    }
  }

  t.hills.forEach((color, i) => {
    // 越近的山越低、起伏越大，山腳淡入霧中
    const depth = i / t.hills.length;
    const base = horizon + h * (0.02 + 0.09 * depth);
    const amp = h * (0.05 + 0.035 * depth);
    ridge(ctx, w, h, base, amp, t.seed + i * 17, color, t.fog);
  });
}

/** 一道山稜：幾個不同頻率的正弦波疊在一起 */
function ridge(ctx, w, h, base, amp, seed, color, fog) {
  const rand = xorshift(Math.imul(seed, 2246822519));
  const TAU = Math.PI * 2;
  const waves = [
    [0.55, 0.6 + rand() * 0.8, rand() * TAU],
    [0.25, 1.6 + rand() * 1.4, rand() * TAU],
    [0.14, 3.5 + rand() * 2.5, rand() * TAU],
    [0.06, 9 + rand() * 6, rand() * TAU],
  ];
  const height = (x) => waves.reduce((sum, [a, f, p]) => sum + a * Math.sin(x * f * TAU + p), 0);

  ctx.beginPath();
  ctx.moveTo(0, h);
  const step = Math.max(w / 200, 2);
  for (let x = 0; x <= w + step; x += step) {
    ctx.lineTo(x, Math.min(base - amp * height(x / w), h));
  }
  ctx.lineTo(w, h);
  ctx.closePath();
  const grad = ctx.createLinearGradient(0, base - amp, 0, h);
  grad.addColorStop(0, color);
  grad.addColorStop(1, rgba(mixHex(color, fog, 0.45)));
  ctx.fillStyle = grad;
  ctx.fill();
}

/** 灰階雜訊，讓漸層不出現色帶，也多一點紙張般的質感 */
function grainTile() {
  const size = 160;
  const c = document.createElement('canvas');
  c.width = c.height = size;
  const g = c.getContext('2d');
  const img = g.createImageData(size, size);
  const rand = xorshift(0x9e3779b9);
  for (let i = 0; i < img.data.length; i += 4) {
    const v = Math.floor(rand() * 256);
    img.data[i] = img.data[i + 1] = img.data[i + 2] = v;
    img.data[i + 3] = 255;
  }
  g.putImageData(img, 0, 0);
  return c;
}

let grainCanvas = null;

/** 四周微微變暗，再疊上一層顆粒。直接畫進背景，打字時不用每次重新混色 */
function paintFinish(ctx, w, h, t) {
  const r = Math.hypot(w, h) / 2;
  const v = ctx.createRadialGradient(w / 2, h / 2, r * 0.45, w / 2, h / 2, r);
  v.addColorStop(0, 'rgba(0,0,0,0)');
  v.addColorStop(1, `rgba(0,0,0,${t.vignette})`);
  ctx.fillStyle = v;
  ctx.fillRect(0, 0, w, h);

  grainCanvas ??= grainTile();
  ctx.save();
  // 顆粒以實際像素為單位，高解析度螢幕上才夠細
  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.globalCompositeOperation = 'overlay';
  ctx.globalAlpha = t.grain;
  ctx.fillStyle = ctx.createPattern(grainCanvas, 'repeat');
  ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  ctx.restore();
}

/** 把 canvas 調整成和顯示大小一致（考慮高解析度螢幕）並畫上場景 */
function drawSceneOn(canvas, t, { finish = false } = {}) {
  const dpr = window.devicePixelRatio || 1;
  const { width, height } = canvas.getBoundingClientRect();
  canvas.width = Math.round(width * dpr);
  canvas.height = Math.round(height * dpr);
  const ctx = canvas.getContext('2d');
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  paintScene(ctx, width, height, t);
  if (finish) paintFinish(ctx, width, height, t);
  return ctx;
}
