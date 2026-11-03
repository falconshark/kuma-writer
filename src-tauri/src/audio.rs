//! 背景環境音與打字音效。
//!
//! 所有聲音都即時合成，不需要任何音檔。UI 執行緒只透過 channel 送指令，
//! 實際混音在 cpal 的音訊回呼裡進行。

use std::f32::consts::{PI, TAU};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Ambient {
    #[default]
    Off,
    Rain,
    Waves,
    Wind,
    Fire,
    Drone,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KeySound {
    Off,
    #[default]
    Soft,
    Typewriter,
    Drop,
}

#[derive(Clone, Copy, Deserialize)]
pub enum KeyKind {
    Char,
    Space,
    Enter,
    Back,
}

enum Cmd {
    Ambient(Ambient),
    AmbientVolume(f32),
    KeySound(KeySound),
    KeyVolume(f32),
    Key(KeyKind),
}

pub struct Audio {
    tx: Sender<Cmd>,
    pub error: Option<String>,
}

impl Audio {
    /// 音訊串流不能在執行緒之間傳遞，所以交給專門的執行緒持有，其他地方只送指令
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let (ready_tx, ready_rx) = channel();
        let spawned = thread::Builder::new().name("audio".into()).spawn(move || match start(rx) {
            Ok(_stream) => {
                let _ = ready_tx.send(None);
                loop {
                    thread::park();
                }
            }
            Err(e) => {
                let _ = ready_tx.send(Some(e));
            }
        });
        let error = match spawned {
            Ok(_) => ready_rx.recv().unwrap_or_else(|_| Some("音訊無法啟動".to_owned())),
            Err(e) => Some(e.to_string()),
        };
        Self { tx, error }
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn set_ambient(&self, a: Ambient) {
        self.send(Cmd::Ambient(a));
    }

    pub fn set_ambient_volume(&self, v: f32) {
        self.send(Cmd::AmbientVolume(v));
    }

    pub fn set_key_sound(&self, k: KeySound) {
        self.send(Cmd::KeySound(k));
    }

    pub fn set_key_volume(&self, v: f32) {
        self.send(Cmd::KeyVolume(v));
    }

    pub fn key(&self, k: KeyKind) {
        self.send(Cmd::Key(k));
    }
}

fn start(rx: Receiver<Cmd>) -> Result<cpal::Stream, String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or("找不到音訊輸出裝置")?;
    let supported = device.default_output_config().map_err(|e| e.to_string())?;
    let mut config = supported.config();
    // 預設緩衝交給系統決定，PulseAudio 會給到約 2 秒，打字音效就會慢半拍。
    // 改成每次約 10 毫秒（實際延遲約兩倍）
    let want = config.sample_rate / 100;
    if let cpal::SupportedBufferSize::Range { min, max } = *supported.buffer_size() {
        config.buffer_size = cpal::BufferSize::Fixed(want.clamp(min, max));
    }
    let synth = Synth::new(config.sample_rate as f32, rx);
    let channels = config.channels as usize;
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, synth, channels),
        SampleFormat::I16 => build::<i16>(&device, config, synth, channels),
        SampleFormat::U16 => build::<u16>(&device, config, synth, channels),
        SampleFormat::I32 => build::<i32>(&device, config, synth, channels),
        f => return Err(format!("不支援的取樣格式 {f:?}")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    mut synth: Synth,
    channels: usize,
) -> Result<cpal::Stream, String> {
    device
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                synth.poll();
                for frame in data.chunks_mut(channels) {
                    let (l, r) = synth.next();
                    for (i, s) in frame.iter_mut().enumerate() {
                        *s = T::from_sample(if i % 2 == 0 { l } else { r });
                    }
                }
            },
            |e| eprintln!("音訊錯誤：{e}"),
            None,
        )
        .map_err(|e| e.to_string())
}

// ── 基本元件 ─────────────────────────────────────────

struct Rng(u32);

impl Rng {
    /// -1.0..1.0
    fn noise(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }

    /// 0.0..1.0
    fn unit(&mut self) -> f32 {
        self.noise() * 0.5 + 0.5
    }

    fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

fn lp_coef(fc: f32, sr: f32) -> f32 {
    1.0 - (-TAU * fc / sr).exp()
}

/// 時間常數為 secs 的指數衰減，每個取樣乘一次
fn decay_mult(secs: f32, sr: f32) -> f32 {
    (-1.0 / (secs * sr)).exp()
}

#[derive(Default, Clone, Copy)]
struct Lp(f32);

impl Lp {
    fn run(&mut self, x: f32, a: f32) -> f32 {
        self.0 += a * (x - self.0);
        self.0
    }
}

/// Chamberlin state-variable filter，取帶通輸出
#[derive(Default, Clone, Copy)]
struct Svf {
    low: f32,
    band: f32,
}

impl Svf {
    fn coef(fc: f32, sr: f32) -> f32 {
        2.0 * (PI * fc.clamp(20.0, sr / 7.0) / sr).sin()
    }

    fn bandpass(&mut self, x: f32, f: f32, damping: f32) -> f32 {
        self.low += f * self.band;
        let high = x - self.low - damping * self.band;
        self.band += f * high;
        self.band
    }
}

#[derive(Default, Clone, Copy)]
struct Pink([f32; 3]);

impl Pink {
    fn run(&mut self, w: f32) -> f32 {
        let b = &mut self.0;
        b[0] = 0.99765 * b[0] + w * 0.099_046;
        b[1] = 0.963 * b[1] + w * 0.296_516_4;
        b[2] = 0.57 * b[2] + w * 1.052_691_3;
        (b[0] + b[1] + b[2] + w * 0.1848) * 0.22
    }
}

#[derive(Default, Clone, Copy)]
struct Brown(f32);

impl Brown {
    fn run(&mut self, w: f32) -> f32 {
        self.0 = (self.0 + 0.02 * w) / 1.02;
        self.0 * 3.5
    }
}

fn pan_gains(pan: f32) -> (f32, f32) {
    let a = pan.clamp(0.0, 1.0) * PI / 2.0;
    (a.cos(), a.sin())
}

// ── 單一聲響：噪音爆發 + 音高滑動的正弦波 ────────────

struct Voice {
    delay: u32,
    age: u32,
    len: u32,
    noise_amp: f32,
    noise_mul: f32,
    svf: Svf,
    svf_f: f32,
    svf_damp: f32,
    tone_amp: f32,
    tone_mul: f32,
    freq: f32,
    freq_end: f32,
    glide: f32,
    phase: f32,
    gl: f32,
    gr: f32,
    sr: f32,
}

impl Voice {
    fn new(sr: f32) -> Self {
        let (gl, gr) = pan_gains(0.5);
        Self {
            delay: 0,
            age: 0,
            len: 0,
            noise_amp: 0.0,
            noise_mul: 0.0,
            svf: Svf::default(),
            svf_f: 0.0,
            svf_damp: 1.0,
            tone_amp: 0.0,
            tone_mul: 0.0,
            freq: 0.0,
            freq_end: 0.0,
            glide: 0.0,
            phase: 0.0,
            gl,
            gr,
            sr,
        }
    }

    fn extend(&mut self, secs: f32) {
        self.len = self.len.max((secs * 7.0 * self.sr) as u32);
    }

    fn noise(mut self, amp: f32, decay: f32, fc: f32, q: f32) -> Self {
        self.noise_amp = amp;
        self.noise_mul = decay_mult(decay, self.sr);
        self.svf_f = Svf::coef(fc, self.sr);
        self.svf_damp = 1.0 / q;
        self.extend(decay);
        self
    }

    fn tone(mut self, amp: f32, decay: f32, f0: f32, f1: f32, glide: f32) -> Self {
        self.tone_amp = amp;
        self.tone_mul = decay_mult(decay, self.sr);
        self.freq = f0;
        self.freq_end = f1;
        self.glide = 1.0 - decay_mult(glide.max(1e-4), self.sr);
        self.extend(decay);
        self
    }

    fn delay(mut self, secs: f32) -> Self {
        self.delay = (secs * self.sr) as u32;
        self
    }

    fn pan(mut self, pan: f32) -> Self {
        (self.gl, self.gr) = pan_gains(pan);
        self
    }

    fn done(&self) -> bool {
        self.delay == 0 && self.age > self.len
    }

    fn next(&mut self, rng: &mut Rng) -> (f32, f32) {
        if self.delay > 0 {
            self.delay -= 1;
            return (0.0, 0.0);
        }
        self.age += 1;
        let mut s = 0.0;
        if self.noise_amp > 1e-5 {
            s += self.svf.bandpass(rng.noise() * self.noise_amp, self.svf_f, self.svf_damp);
            self.noise_amp *= self.noise_mul;
        }
        if self.tone_amp > 1e-5 {
            self.freq += (self.freq_end - self.freq) * self.glide;
            self.phase = (self.phase + self.freq / self.sr).fract();
            s += (self.phase * TAU).sin() * self.tone_amp;
            self.tone_amp *= self.tone_mul;
        }
        (s * self.gl, s * self.gr)
    }
}

// ── 錄音 ─────────────────────────────────────────────

// 打字機的錄音取自 Tickeys（https://github.com/yingDev/Tickeys，MIT），見 sounds/README.md
static TYPEWRITER_KEYS: [&[u8]; 5] = [
    include_bytes!("../sounds/typewriter/key1.wav"),
    include_bytes!("../sounds/typewriter/key2.wav"),
    include_bytes!("../sounds/typewriter/key3.wav"),
    include_bytes!("../sounds/typewriter/key4.wav"),
    include_bytes!("../sounds/typewriter/key5.wav"),
];
static TYPEWRITER_SPACE: &[u8] = include_bytes!("../sounds/typewriter/space.wav");
static TYPEWRITER_BACK: &[u8] = include_bytes!("../sounds/typewriter/backspace.wav");
static TYPEWRITER_RETURN: &[u8] = include_bytes!("../sounds/typewriter/return.wav");

/// 解碼後的錄音：立體聲的取樣與原始取樣率
struct Recording {
    frames: Vec<[f32; 2]>,
    rate: f32,
}

/// 只支援 16 位元 PCM 的 WAV，程式內建的檔案都是這個格式
fn decode_wav(bytes: &[u8]) -> Recording {
    let u16_at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]);
    let u32_at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]);
    assert!(&bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE", "不是 WAV 檔");
    let (mut channels, mut rate, mut frames) = (2usize, 44_100.0, Vec::new());
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let size = u32_at(i + 4) as usize;
        let body = &bytes[i + 8..(i + 8 + size).min(bytes.len())];
        match &bytes[i..i + 4] {
            b"fmt " => {
                channels = u16_at(i + 10) as usize;
                rate = u32_at(i + 12) as f32;
                assert_eq!(u16_at(i + 22), 16, "只支援 16 位元 PCM");
            }
            b"data" => {
                frames = body
                    .chunks_exact(2 * channels)
                    .map(|f| {
                        let ch = |c: usize| i16::from_le_bytes([f[2 * c], f[2 * c + 1]]) as f32 / 32768.0;
                        [ch(0), ch(channels.min(2) - 1)]
                    })
                    .collect();
            }
            _ => {}
        }
        // 區塊長度是奇數時會補一個位元組
        i += 8 + size + (size & 1);
    }
    Recording { frames, rate }
}

impl Recording {
    /// 峰值等化（RBJ biquad）：在 f0 附近加減 gain_db
    fn eq(mut self, f0: f32, gain_db: f32, q: f32) -> Self {
        let a = 10f32.powf(gain_db / 40.0);
        let w = TAU * f0 / self.rate;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        let (b0, b1, b2) = ((1.0 + alpha * a) / a0, -2.0 * cos / a0, (1.0 - alpha * a) / a0);
        let (a1, a2) = (-2.0 * cos / a0, (1.0 - alpha / a) / a0);
        for ch in 0..2 {
            let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
            for f in &mut self.frames {
                let x = f[ch];
                let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                f[ch] = y;
            }
        }
        self
    }

    /// 這組錄音幾乎只有 2.5–10 kHz 的成分，聽起來偏輕：補上中低頻的機身聲，收一點尖銳
    fn heavier(self) -> Self {
        self.eq(220.0, 12.0, 0.8).eq(700.0, 6.0, 1.0).eq(7000.0, -6.0, 0.5)
    }

    /// 裁掉開頭的空白（錄音開頭有 5–20 ms 的靜音，會讓聲音慢半拍），
    /// 並把峰值拉到 peak，各段錄音的音量才一致。放大倍數有上限，免得把底噪也放大
    fn prepared(mut self, peak: f32) -> Self {
        let max = self.frames.iter().map(|f| f[0].abs().max(f[1].abs())).fold(0.0, f32::max);
        let start = self
            .frames
            .iter()
            .position(|f| f[0].abs().max(f[1].abs()) > max * 0.02)
            .unwrap_or(0);
        // 留 1 ms 讓起音不會太硬
        self.frames.drain(..start.saturating_sub((self.rate / 1000.0) as usize));
        let g = (peak / max.max(1e-6)).min(8.0);
        for f in &mut self.frames {
            f[0] *= g;
            f[1] *= g;
        }
        self
    }
}

/// 打字機的一組錄音
struct Typewriter {
    keys: Vec<Recording>,
    space: Recording,
    back: Recording,
    ret: Recording,
}

impl Typewriter {
    fn load() -> Self {
        Self {
            keys: TYPEWRITER_KEYS.iter().map(|b| decode_wav(b).heavier().prepared(0.9)).collect(),
            space: decode_wav(TYPEWRITER_SPACE).heavier().prepared(0.75),
            back: decode_wav(TYPEWRITER_BACK).prepared(0.6),
            ret: decode_wav(TYPEWRITER_RETURN).prepared(0.75),
        }
    }

    fn get(&self, which: Clip) -> &Recording {
        match which {
            Clip::Key(i) => &self.keys[i],
            Clip::Space => &self.space,
            Clip::Back => &self.back,
            Clip::Return => &self.ret,
        }
    }
}

#[derive(Clone, Copy)]
enum Clip {
    Key(usize),
    Space,
    Back,
    Return,
}

/// 正在播放的一段錄音。可以同時播好幾段，快打時前一聲不會被切掉
struct Playback {
    clip: Clip,
    pos: f64,
    step: f64,
    gain: f32,
}

// ── 合成器 ───────────────────────────────────────────

const PENTATONIC: [f32; 6] = [523.25, 587.33, 659.25, 783.99, 880.0, 1046.5];

/// 冥想模式的和弦進行：Fmaj7 → Am7 → Cmaj7 → Gsus2
const CHORDS: [[f32; 4]; 4] = [
    [87.31, 130.81, 164.81, 220.0],
    [110.0, 164.81, 196.0, 261.63],
    [130.81, 196.0, 246.94, 329.63],
    [98.0, 146.83, 220.0, 293.66],
];
const CHORD_SECS: f32 = 18.0;

#[derive(Default)]
struct AmbientState {
    t: f32,
    pink: [Pink; 2],
    brown: [Brown; 2],
    lp: [Lp; 4],
    svf: [Svf; 2],
    drone_freq: [f32; 4],
    drone_phase: [f32; 8],
}

struct Synth {
    rx: Receiver<Cmd>,
    sr: f32,
    rng: Rng,
    ambient: Ambient,
    target: Ambient,
    ambient_vol: f32,
    level: f32,
    fade: f32,
    key_sound: KeySound,
    key_vol: f32,
    st: AmbientState,
    /// 環境音裡的雨滴、火花
    grains: Vec<Voice>,
    /// 打字音效
    keys: Vec<Voice>,
    typewriter: Typewriter,
    plays: Vec<Playback>,
    /// 上一次用的按鍵錄音，避免連續兩次挑到同一個
    last_key: usize,
}

impl Synth {
    fn new(sr: f32, rx: Receiver<Cmd>) -> Self {
        Self {
            rx,
            sr,
            rng: Rng(0x9E37_79B9),
            ambient: Ambient::Off,
            target: Ambient::Off,
            ambient_vol: 0.5,
            level: 0.0,
            fade: lp_coef(1.0 / (TAU * 0.35), sr),
            key_sound: KeySound::Soft,
            key_vol: 0.5,
            st: AmbientState::default(),
            grains: Vec::new(),
            keys: Vec::new(),
            typewriter: Typewriter::load(),
            plays: Vec::new(),
            last_key: 0,
        }
    }

    fn poll(&mut self) {
        while let Ok(cmd) = self.rx.try_recv() {
            match cmd {
                Cmd::Ambient(a) => self.target = a,
                Cmd::AmbientVolume(v) => self.ambient_vol = v,
                Cmd::KeySound(k) => self.key_sound = k,
                Cmd::KeyVolume(v) => self.key_vol = v,
                Cmd::Key(k) => self.trigger_key(k),
            }
        }
    }

    fn next(&mut self) -> (f32, f32) {
        // 切換環境音時先淡出，再換成新的聲音淡入
        let goal = if self.ambient == self.target { self.ambient_vol } else { 0.0 };
        self.level += (goal - self.level) * self.fade;
        if self.ambient != self.target && self.level < 1e-4 {
            self.ambient = self.target;
            self.st = AmbientState::default();
            self.grains.clear();
        }

        let (mut l, mut r) = (0.0, 0.0);
        if self.ambient != Ambient::Off && self.level > 1e-5 {
            let (al, ar) = self.ambient_sample();
            l += al * self.level;
            r += ar * self.level;
        }
        let (mut kl, mut kr) = (0.0, 0.0);
        for v in &mut self.keys {
            let (a, b) = v.next(&mut self.rng);
            kl += a;
            kr += b;
        }
        self.keys.retain(|v| !v.done());
        for p in &mut self.plays {
            let frames = &self.typewriter.get(p.clip).frames;
            let i = p.pos as usize;
            if i + 1 < frames.len() {
                // 錄音與輸出的取樣率不同，用線性內插取值
                let t = (p.pos - i as f64) as f32;
                let (a, b) = (frames[i], frames[i + 1]);
                kl += (a[0] + (b[0] - a[0]) * t) * p.gain;
                kr += (a[1] + (b[1] - a[1]) * t) * p.gain;
            }
            p.pos += p.step;
        }
        let tw = &self.typewriter;
        self.plays.retain(|p| (p.pos as usize) + 1 < tw.get(p.clip).frames.len());
        l += kl * self.key_vol;
        r += kr * self.key_vol;
        (soft_clip(l), soft_clip(r))
    }

    fn spawn_grain(&mut self, v: Voice) {
        if self.grains.len() < 48 {
            self.grains.push(v);
        }
    }

    fn grains_sample(&mut self) -> (f32, f32) {
        let (mut l, mut r) = (0.0, 0.0);
        for g in &mut self.grains {
            let (a, b) = g.next(&mut self.rng);
            l += a;
            r += b;
        }
        self.grains.retain(|g| !g.done());
        (l, r)
    }

    fn ambient_sample(&mut self) -> (f32, f32) {
        let sr = self.sr;
        self.st.t += 1.0 / sr;
        let t = self.st.t;
        let w = [self.rng.noise(), self.rng.noise()];

        match self.ambient {
            Ambient::Off => (0.0, 0.0),
            Ambient::Rain => {
                let a = lp_coef(3200.0, sr);
                let base = [
                    self.st.lp[0].run(self.st.pink[0].run(w[0]), a) * 0.42,
                    self.st.lp[1].run(self.st.pink[1].run(w[1]), a) * 0.42,
                ];
                if self.rng.unit() < 45.0 / sr {
                    let rng = &mut self.rng;
                    let v = Voice::new(sr)
                        .noise(rng.range(0.08, 0.35), rng.range(0.002, 0.007), rng.range(1800.0, 5000.0), 1.5)
                        .pan(rng.unit());
                    self.spawn_grain(v);
                }
                let (gl, gr) = self.grains_sample();
                (base[0] + gl, base[1] + gr)
            }
            Ambient::Waves => {
                let mut out = [0.0; 2];
                for (i, o) in out.iter_mut().enumerate() {
                    let off = i as f32 * 0.9;
                    let swell = 0.5 - 0.5 * (TAU * (t + off) / 9.5).cos()
                        + 0.15 * (TAU * (t + off) / 23.0).sin();
                    let e = swell.clamp(0.0, 1.0).powf(1.6);
                    let n = self.st.brown[i].run(w[i]) * 0.7 + self.st.pink[i].run(w[i]) * 0.3;
                    let cut = lp_coef(180.0 + 1700.0 * e, sr);
                    *o = self.st.lp[i].run(n, cut) * (0.12 + 0.88 * e) * 0.9;
                }
                (out[0], out[1])
            }
            Ambient::Wind => {
                let gust = 0.55 + 0.25 * (TAU * 0.071 * t).sin() + 0.2 * (TAU * 0.113 * t + 1.3).sin();
                let mut out = [0.0; 2];
                for (i, o) in out.iter_mut().enumerate() {
                    let off = i as f32 * 2.1;
                    let fc = 520.0 + 260.0 * (TAU * 0.05 * t + off).sin() + 120.0 * (TAU * 0.17 * t + off).sin();
                    let f = Svf::coef(fc * (0.6 + 0.6 * gust), sr);
                    let b = self.st.svf[i].bandpass(self.st.pink[i].run(w[i]), f, 0.25);
                    *o = b * gust * 0.6;
                }
                (out[0], out[1])
            }
            Ambient::Fire => {
                let flicker = 0.8 + 0.12 * (TAU * 0.7 * t).sin() + 0.08 * (TAU * 1.9 * t).sin();
                let a = lp_coef(380.0, sr);
                let rumble = [
                    self.st.lp[0].run(self.st.brown[0].run(w[0]), a) * flicker * 1.1,
                    self.st.lp[1].run(self.st.brown[1].run(w[1]), a) * flicker * 1.1,
                ];
                let hiss = self.st.lp[2].run(self.st.pink[0].run(w[1]), lp_coef(1500.0, sr)) * 0.06;
                if self.rng.unit() < 7.0 / sr {
                    let rng = &mut self.rng;
                    let pan = rng.unit();
                    let n = if rng.unit() < 0.35 { 1 + (rng.unit() * 3.0) as usize } else { 0 };
                    let pops: Vec<Voice> = (0..=n)
                        .map(|k| {
                            Voice::new(sr)
                                .noise(rng.range(0.15, 0.6), rng.range(0.0008, 0.003), rng.range(900.0, 3500.0), 1.2)
                                .delay(k as f32 * rng.range(0.01, 0.04))
                                .pan(pan)
                        })
                        .collect();
                    for p in pops {
                        self.spawn_grain(p);
                    }
                }
                let (gl, gr) = self.grains_sample();
                (rumble[0] + hiss + gl, rumble[1] + hiss + gr)
            }
            Ambient::Drone => {
                let chord = CHORDS[(t / CHORD_SECS) as usize % CHORDS.len()];
                let glide = lp_coef(1.0 / (TAU * 2.5), sr);
                let (mut l, mut r) = (0.0, 0.0);
                for i in 0..4 {
                    let f = &mut self.st.drone_freq[i];
                    if *f == 0.0 {
                        *f = chord[i];
                    }
                    *f += (chord[i] - *f) * glide;
                    let f = *f;
                    // 兩個略微走音的振盪器疊在一起，聲音比較溫暖
                    let mut s = 0.0;
                    for (k, detune) in [1.0, 1.003].into_iter().enumerate() {
                        let ph = &mut self.st.drone_phase[i * 2 + k];
                        *ph = (*ph + f * detune / sr).fract();
                        s += (*ph * TAU).sin() + 0.12 * (*ph * TAU * 2.0).sin();
                    }
                    let trem = 0.65 + 0.35 * (TAU * (0.04 + i as f32 * 0.023) * t + i as f32).sin();
                    let (gl, gr) = pan_gains(0.2 + 0.2 * i as f32);
                    s *= 0.11 * trem;
                    l += s * gl;
                    r += s * gr;
                }
                let air = self.st.lp[0].run(self.st.pink[0].run(w[0]), lp_coef(900.0, sr)) * 0.03;
                (l + air, r + air)
            }
        }
    }

    fn trigger_key(&mut self, kind: KeyKind) {
        let sr = self.sr;
        let rng = &mut self.rng;
        let pan = rng.range(0.4, 0.6);
        let j = |rng: &mut Rng, x: f32| x * rng.range(0.85, 1.15);
        let mut v: Vec<Voice> = Vec::new();

        match (self.key_sound, kind) {
            (KeySound::Off, _) => {}
            (KeySound::Soft, k) => {
                let (fc, decay, tone, amp) = match k {
                    KeyKind::Char => (2600.0, 0.010, 190.0, 0.22),
                    KeyKind::Back => (3200.0, 0.008, 220.0, 0.16),
                    KeyKind::Space => (1500.0, 0.016, 140.0, 0.28),
                    KeyKind::Enter => (1200.0, 0.022, 115.0, 0.32),
                };
                v.push(
                    Voice::new(sr)
                        .noise(0.28, j(rng, decay), j(rng, fc), 0.9)
                        .tone(amp, decay * 1.4, j(rng, tone), tone * 0.7, 0.02),
                );
            }
            (KeySound::Typewriter, k) => {
                // 一般按鍵從五段錄音裡隨機挑一段，不連續重複，連打時才不會像同一聲在重播
                let clip = match k {
                    KeyKind::Char => {
                        let n = self.typewriter.keys.len();
                        let mut i = (rng.unit() * n as f32) as usize % n;
                        if i == self.last_key {
                            i = (i + 1 + (rng.unit() * (n - 1) as f32) as usize % (n - 1)) % n;
                        }
                        self.last_key = i;
                        Clip::Key(i)
                    }
                    KeyKind::Space => Clip::Space,
                    KeyKind::Back => Clip::Back,
                    KeyKind::Enter => Clip::Return,
                };
                if self.plays.len() < 16 {
                    self.plays.push(Playback {
                        clip,
                        pos: 0.0,
                        step: (self.typewriter.get(clip).rate / sr) as f64,
                        gain: rng.range(0.85, 1.0),
                    });
                }
            }
            (KeySound::Drop, k) => {
                let f0 = match k {
                    KeyKind::Space => j(rng, 380.0),
                    KeyKind::Back => j(rng, 1000.0),
                    _ => j(rng, 700.0),
                };
                let (rise, decay) = if matches!(k, KeyKind::Back) { (0.6, 0.05) } else { (2.2, 0.07) };
                v.push(Voice::new(sr).tone(0.28, decay, f0, f0 * rise, 0.012));
                if matches!(k, KeyKind::Enter) {
                    let note = PENTATONIC[(rng.unit() * PENTATONIC.len() as f32) as usize % PENTATONIC.len()];
                    v.push(Voice::new(sr).tone(0.14, 0.35, note, note, 1.0).delay(0.06));
                }
            }
        }
        for voice in v {
            if self.keys.len() < 48 {
                self.keys.push(voice.pan(pan));
            }
        }
    }
}

fn soft_clip(x: f32) -> f32 {
    if x.abs() < 0.8 {
        x
    } else {
        x.signum() * (0.8 + 0.2 * ((x.abs() - 0.8) / 0.2).tanh())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMBIENTS: [(Ambient, &str); 5] = [
        (Ambient::Rain, "雨"),
        (Ambient::Waves, "海浪"),
        (Ambient::Wind, "風"),
        (Ambient::Fire, "爐火"),
        (Ambient::Drone, "冥想"),
    ];

    const KEY_SOUNDS: [(KeySound, &str); 3] = [
        (KeySound::Soft, "輕觸"),
        (KeySound::Typewriter, "打字機"),
        (KeySound::Drop, "水滴"),
    ];

    fn render(synth: &mut Synth, secs: f32) -> (f32, f32) {
        let n = (secs * synth.sr) as usize;
        let (mut peak, mut sum) = (0.0f32, 0.0f64);
        for _ in 0..n {
            let (l, r) = synth.next();
            assert!(l.is_finite() && r.is_finite());
            peak = peak.max(l.abs()).max(r.abs());
            sum += (l * l + r * r) as f64 / 2.0;
        }
        (peak, (sum / n as f64).sqrt() as f32)
    }

    #[test]
    fn typewriter_recordings_load() {
        let tw = Typewriter::load();
        for rec in tw.keys.iter().chain([&tw.space, &tw.back, &tw.ret]) {
            assert_eq!(rec.rate, 44_100.0);
            let peak = rec.frames.iter().map(|f| f[0].abs().max(f[1].abs())).fold(0.0, f32::max);
            assert!(peak > 0.1 && peak <= 0.91, "峰值 {peak}");
            // 開頭的靜音已經裁掉，1.5 ms 內就要出聲
            let start = rec.frames.iter().position(|f| f[0].abs().max(f[1].abs()) > peak * 0.02).unwrap();
            assert!(start < 66, "開頭還有 {start} 個取樣的靜音");
        }
    }

    /// 一段聲音裡 1200 Hz 以下的能量佔比與頻譜重心。只看開頭 4096 個取樣，直接做 DFT
    fn low_share_and_centroid(rec: &Recording) -> (f32, f32) {
        let x: Vec<f64> = rec.frames.iter().take(4096).map(|f| ((f[0] + f[1]) / 2.0) as f64).collect();
        let n = x.len();
        let (mut low, mut total, mut mag, mut mag_f) = (0.0, 0.0, 0.0, 0.0);
        for k in 1..n / 2 {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (i, &v) in x.iter().enumerate() {
                let a = TAU as f64 * ((k * i) % n) as f64 / n as f64;
                re += v * a.cos();
                im -= v * a.sin();
            }
            let p = re * re + im * im;
            let f = k as f64 * rec.rate as f64 / n as f64;
            total += p;
            mag += p.sqrt();
            mag_f += p.sqrt() * f;
            if f < 1200.0 {
                low += p;
            }
        }
        ((low / total) as f32, (mag_f / mag) as f32)
    }

    #[test]
    fn typewriter_keys_are_heavier() {
        let raw = decode_wav(TYPEWRITER_KEYS[0]).prepared(0.9);
        let heavy = decode_wav(TYPEWRITER_KEYS[0]).heavier().prepared(0.9);
        let (rl, rc) = low_share_and_centroid(&raw);
        let (hl, hc) = low_share_and_centroid(&heavy);
        println!("1200 Hz 以下 {:.0}% → {:.0}%，重心 {rc:.0} → {hc:.0} Hz", rl * 100.0, hl * 100.0);
        assert!(hl > rl * 3.0 && hl > 0.15, "中低頻沒有補起來");
        assert!(hc < rc * 0.9, "音色還是太尖");
    }

    #[test]
    fn typewriter_keys_do_not_repeat() {
        let (_tx, rx) = channel();
        let mut s = Synth::new(48_000.0, rx);
        s.key_sound = KeySound::Typewriter;
        let mut last = None;
        for _ in 0..200 {
            s.trigger_key(KeyKind::Char);
            let Clip::Key(i) = s.plays.pop().unwrap().clip else { panic!() };
            assert_ne!(Some(i), last, "連續兩次挑到同一段錄音");
            last = Some(i);
        }
    }

    #[test]
    fn ambients_are_audible_and_bounded() {
        for (a, name) in AMBIENTS {
            let (tx, rx) = channel();
            let mut s = Synth::new(48_000.0, rx);
            tx.send(Cmd::Ambient(a)).unwrap();
            s.poll();
            let (peak, rms) = render(&mut s, 60.0);
            println!("{name}: peak {peak:.3} rms {rms:.3}");
            assert!(rms > 0.01, "{name} 太小聲");
            assert!(peak <= 1.0);
        }
    }

    #[test]
    fn key_sounds_are_audible_and_bounded() {
        for (k, name) in KEY_SOUNDS {
            for kind in [KeyKind::Char, KeyKind::Space, KeyKind::Enter, KeyKind::Back] {
                let (tx, rx) = channel();
                let mut s = Synth::new(48_000.0, rx);
                tx.send(Cmd::KeySound(k)).unwrap();
                tx.send(Cmd::Key(kind)).unwrap();
                s.poll();
                let (peak, _) = render(&mut s, 8.0);
                println!("{name} {:?}: peak {peak:.3}", kind as u8);
                assert!(peak > 0.02 && peak <= 1.0);
                assert!(s.keys.is_empty() && s.plays.is_empty(), "聲音沒有結束");
            }
        }
    }
}
