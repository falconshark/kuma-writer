//! 背景環境音與打字音效。
//!
//! 所有聲音都即時合成，不需要任何音檔。UI 執行緒只透過 channel 送指令，
//! 實際混音在 cpal 的音訊回呼裡進行。

use std::f32::consts::{PI, TAU};
use std::sync::mpsc::{channel, Receiver, Sender};

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

impl Ambient {
    pub const ALL: [Ambient; 6] = [
        Ambient::Off,
        Ambient::Rain,
        Ambient::Waves,
        Ambient::Wind,
        Ambient::Fire,
        Ambient::Drone,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Ambient::Off => "無",
            Ambient::Rain => "雨",
            Ambient::Waves => "海浪",
            Ambient::Wind => "風",
            Ambient::Fire => "爐火",
            Ambient::Drone => "冥想",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum KeySound {
    Off,
    #[default]
    Soft,
    Typewriter,
    Drop,
}

impl KeySound {
    pub const ALL: [KeySound; 4] = [KeySound::Off, KeySound::Soft, KeySound::Typewriter, KeySound::Drop];

    pub fn name(self) -> &'static str {
        match self {
            KeySound::Off => "無",
            KeySound::Soft => "輕觸",
            KeySound::Typewriter => "打字機",
            KeySound::Drop => "水滴",
        }
    }
}

#[derive(Clone, Copy)]
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
    _stream: Option<cpal::Stream>,
    pub error: Option<String>,
}

impl Audio {
    pub fn new(ambient: Ambient, ambient_vol: f32, keys: KeySound, key_vol: f32) -> Self {
        let (tx, rx) = channel();
        let (stream, error) = match start(rx) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        };
        let audio = Self { tx, _stream: stream, error };
        audio.set_ambient(ambient);
        audio.set_ambient_volume(ambient_vol);
        audio.set_key_sound(keys);
        audio.set_key_volume(key_vol);
        audio
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
    let config = supported.config();
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
            (KeySound::Typewriter, KeyKind::Enter) => {
                // 捲紙的棘輪聲，接著一聲鈴響
                for i in 0..6 {
                    v.push(Voice::new(sr).noise(0.18, 0.003, j(rng, 4000.0), 1.5).delay(i as f32 * 0.028));
                }
                v.push(Voice::new(sr).tone(0.16, 0.9, 2093.0, 2093.0, 1.0).delay(0.2));
                v.push(Voice::new(sr).tone(0.05, 0.3, 5777.0, 5777.0, 1.0).delay(0.2));
            }
            (KeySound::Typewriter, k) => {
                let (amp, fc) = match k {
                    KeyKind::Space => (0.35, 700.0),
                    KeyKind::Back => (0.35, 2800.0),
                    _ => (0.55, 3600.0),
                };
                v.push(Voice::new(sr).noise(amp, 0.005, j(rng, fc), 0.6));
                v.push(
                    Voice::new(sr)
                        .noise(0.25, 0.018, j(rng, 900.0), 0.8)
                        .tone(0.12, 0.01, j(rng, 320.0), 240.0, 0.01)
                        .delay(j(rng, 0.012)),
                );
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
    fn ambients_are_audible_and_bounded() {
        for a in Ambient::ALL.into_iter().skip(1) {
            let (tx, rx) = channel();
            let mut s = Synth::new(48_000.0, rx);
            tx.send(Cmd::Ambient(a)).unwrap();
            s.poll();
            let (peak, rms) = render(&mut s, 60.0);
            println!("{}: peak {peak:.3} rms {rms:.3}", a.name());
            assert!(rms > 0.01, "{} 太小聲", a.name());
            assert!(peak <= 1.0);
        }
    }

    #[test]
    fn key_sounds_are_audible_and_bounded() {
        for k in KeySound::ALL.into_iter().skip(1) {
            for kind in [KeyKind::Char, KeyKind::Space, KeyKind::Enter, KeyKind::Back] {
                let (tx, rx) = channel();
                let mut s = Synth::new(48_000.0, rx);
                tx.send(Cmd::KeySound(k)).unwrap();
                tx.send(Cmd::Key(kind)).unwrap();
                s.poll();
                let (peak, _) = render(&mut s, 8.0);
                println!("{} {:?}: peak {peak:.3}", k.name(), kind as u8);
                assert!(peak > 0.02 && peak <= 1.0);
                assert!(s.keys.is_empty(), "聲音沒有結束");
            }
        }
    }
}
