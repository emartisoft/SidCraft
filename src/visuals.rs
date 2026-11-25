// SPDX-License-Identifier: GPL-3.0-only
// Copyright (c) 2026 Emarti

use std::collections::VecDeque;
use std::f32::consts::PI;
use std::time::Instant;

pub const SCOPE_DISPLAY_SAMPLES: usize = 256;
/// FFT window used by the spectrum analyser and the spectrogram.
pub const FFT_SIZE: usize = 1024;
/// Number of log-spaced bands drawn by the spectrum analyser.
pub const SPECTRUM_BANDS: usize = 72;
/// Spectrogram history length (about four seconds at 40 fps).
pub const SPECTROGRAM_COLUMNS: usize = 160;
const ATTACK_RATE: f32 = 0.7;
const DECAY_RATE: f32 = 0.92;
const PEAK_HOLD_MS: u128 = 500;
const SCOPE_PERSISTENCE: f32 = 0.6;
/// Everything quieter than this is drawn as silence.
const SPECTRUM_FLOOR_DB: f32 = -64.0;
const SPECTRUM_HOLD_MS: u128 = 650;
/// How fast a spectrum band falls back after a transient (per frame).
const SPECTRUM_RELEASE: f32 = 0.88;

pub struct VuMeter {
    pub levels: Vec<f32>,
    pub peaks: Vec<f32>,
    peak_hold: Vec<Instant>,
}

impl VuMeter {
    pub fn with_voice_count(voice_count: usize) -> Self {
        let now = Instant::now();
        Self {
            levels: vec![0.0; voice_count],
            peaks: vec![0.0; voice_count],
            peak_hold: vec![now; voice_count],
        }
    }

    pub fn update(&mut self, envelope: &[u8]) {
        self.resize_if_needed(envelope.len());
        let now = Instant::now();
        for (i, &env) in envelope.iter().enumerate() {
            let target = f32::from(env) / 255.0;
            self.levels[i] = if target > self.levels[i] {
                (target - self.levels[i]).mul_add(ATTACK_RATE, self.levels[i])
            } else {
                self.levels[i] * DECAY_RATE
            };

            if self.levels[i] >= self.peaks[i] {
                self.peaks[i] = self.levels[i];
                self.peak_hold[i] = now;
            } else if now.duration_since(self.peak_hold[i]).as_millis() > PEAK_HOLD_MS {
                self.peaks[i] *= 0.95;
            }
        }
    }

    fn resize_if_needed(&mut self, voice_count: usize) {
        if self.levels.len() != voice_count {
            let now = Instant::now();
            self.levels.resize(voice_count, 0.0);
            self.peaks.resize(voice_count, 0.0);
            self.peak_hold.resize(voice_count, now);
        }
    }

    pub fn voice_count(&self) -> usize {
        self.levels.len()
    }
}

pub struct VoiceScopes {
    pub samples: Vec<Vec<f32>>,
}

impl VoiceScopes {
    pub fn with_voice_count(voice_count: usize) -> Self {
        Self {
            samples: (0..voice_count)
                .map(|_| vec![0.0; SCOPE_DISPLAY_SAMPLES])
                .collect(),
        }
    }

    pub fn update(&mut self, raw_samples: &[Vec<f32>]) {
        self.resize_if_needed(raw_samples.len());
        for (display, raw) in self.samples.iter_mut().zip(raw_samples.iter()) {
            if raw.is_empty() {
                continue;
            }
            let step = raw.len() / SCOPE_DISPLAY_SAMPLES;
            if step == 0 {
                continue;
            }
            for (i, sample) in display.iter_mut().enumerate() {
                let new_val = raw.get(i * step).copied().unwrap_or(0.0);
                *sample = sample.mul_add(SCOPE_PERSISTENCE, new_val * (1.0 - SCOPE_PERSISTENCE));
            }
        }
    }

    fn resize_if_needed(&mut self, voice_count: usize) {
        if self.samples.len() != voice_count {
            self.samples
                .resize_with(voice_count, || vec![0.0; SCOPE_DISPLAY_SAMPLES]);
        }
    }

    pub fn voice_count(&self) -> usize {
        self.samples.len()
    }
}

/// Log-spaced FFT spectrum with peak hold.
pub struct Spectrum {
    /// Smoothed band magnitudes, 0 = floor, 1 = full scale.
    pub bands: Vec<f32>,
    /// Peak-hold values for each band.
    pub peaks: Vec<f32>,
    peak_hold: Vec<Instant>,
    window: Vec<f32>,
    re: Vec<f32>,
    im: Vec<f32>,
    twiddles: Vec<(f32, f32)>,
    band_edges: Vec<(usize, usize)>,
    sample_rate: u32,
}

impl Spectrum {
    pub fn new() -> Self {
        let window = (0..FFT_SIZE)
            .map(|i| {
                let x = 2.0 * PI * i as f32 / (FFT_SIZE - 1) as f32;
                0.5 - 0.5 * x.cos()
            })
            .collect();
        let mut twiddles = Vec::new();
        let mut len = 2;
        while len <= FFT_SIZE {
            let angle = -2.0 * PI / len as f32;
            twiddles.push((angle.cos(), angle.sin()));
            len <<= 1;
        }
        Self {
            bands: vec![0.0; SPECTRUM_BANDS],
            peaks: vec![0.0; SPECTRUM_BANDS],
            peak_hold: vec![Instant::now(); SPECTRUM_BANDS],
            window,
            re: vec![0.0; FFT_SIZE],
            im: vec![0.0; FFT_SIZE],
            twiddles,
            band_edges: Vec::new(),
            sample_rate: 0,
        }
    }

    /// Analyses the newest `FFT_SIZE` samples. `samples` may be shorter, in
    /// which case the missing history is treated as silence.
    pub fn update(&mut self, samples: &[f32], sample_rate: u32) {
        if sample_rate == 0 {
            return;
        }
        if self.sample_rate != sample_rate || self.band_edges.is_empty() {
            self.sample_rate = sample_rate;
            self.rebuild_bands(sample_rate);
        }

        let take = samples.len().min(FFT_SIZE);
        let offset = FFT_SIZE - take;
        for i in 0..FFT_SIZE {
            let sample = if i >= offset { samples[i - offset] } else { 0.0 };
            self.re[i] = sample * self.window[i];
            self.im[i] = 0.0;
        }
        fft(&mut self.re, &mut self.im, &self.twiddles);

        let now = Instant::now();
        let norm = 4.0 / FFT_SIZE as f32;
        for (band, &(lo, hi)) in self.band_edges.iter().enumerate() {
            let mut magnitude: f32 = 0.0;
            for k in lo..hi {
                magnitude = magnitude.max(self.re[k].hypot(self.im[k]) * norm);
            }
            let db = 20.0 * (magnitude + 1e-7).log10();
            let target = ((db - SPECTRUM_FLOOR_DB) / -SPECTRUM_FLOOR_DB).clamp(0.0, 1.0);
            self.bands[band] = if target > self.bands[band] {
                target
            } else {
                self.bands[band].mul_add(SPECTRUM_RELEASE, target * (1.0 - SPECTRUM_RELEASE))
            };
            if self.bands[band] >= self.peaks[band] {
                self.peaks[band] = self.bands[band];
                self.peak_hold[band] = now;
            } else if now.duration_since(self.peak_hold[band]).as_millis() > SPECTRUM_HOLD_MS {
                self.peaks[band] *= 0.94;
            }
        }
    }

    /// Maps each band to the FFT bins between two log-spaced frequencies.
    fn rebuild_bands(&mut self, sample_rate: u32) {
        self.band_edges.clear();
        let nyquist = sample_rate as f32 * 0.5;
        let f_min = 30.0_f32.min(nyquist * 0.5);
        let f_max = nyquist.min(16_000.0).max(f_min * 2.0);
        for band in 0..SPECTRUM_BANDS {
            let t0 = band as f32 / SPECTRUM_BANDS as f32;
            let t1 = (band + 1) as f32 / SPECTRUM_BANDS as f32;
            let f0 = f_min * (f_max / f_min).powf(t0);
            let f1 = f_min * (f_max / f_min).powf(t1);
            let lo = ((f0 / nyquist) * (FFT_SIZE / 2) as f32).floor() as usize;
            let hi = ((f1 / nyquist) * (FFT_SIZE / 2) as f32).ceil() as usize;
            let lo = lo.clamp(1, FFT_SIZE / 2 - 1);
            let hi = hi.clamp(lo + 1, FFT_SIZE / 2);
            self.band_edges.push((lo, hi));
        }
    }
}

/// Rolling history of spectrum columns for the waterfall display.
pub struct Spectrogram {
    /// Oldest column first, newest last.
    pub columns: VecDeque<Vec<f32>>,
}

impl Spectrogram {
    pub fn new() -> Self {
        Self {
            columns: VecDeque::new(),
        }
    }

    pub fn push(&mut self, bands: &[f32]) {
        if self.columns.len() >= SPECTROGRAM_COLUMNS {
            if let Some(mut recycled) = self.columns.pop_front() {
                recycled.resize(bands.len(), 0.0);
                recycled.copy_from_slice(bands);
                self.columns.push_back(recycled);
                return;
            }
        }
        self.columns.push_back(bands.to_vec());
    }

    pub fn clear(&mut self) {
        self.columns.clear();
    }
}

/// In-place iterative radix-2 Cooley-Tukey FFT using precomputed stage twiddle factors.
fn fft(re: &mut [f32], im: &mut [f32], twiddles: &[(f32, f32)]) {
    let n = re.len();
    if n <= 1 || !n.is_power_of_two() {
        return;
    }

    // Bit-reversal permutation.
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    let mut stage = 0;
    while len <= n {
        let (w_re, w_im) = twiddles
            .get(stage)
            .copied()
            .unwrap_or_else(|| {
                let angle = -2.0 * PI / len as f32;
                (angle.cos(), angle.sin())
            });
        let half = len / 2;
        let mut start = 0;
        while start < n {
            let (mut cur_re, mut cur_im) = (1.0_f32, 0.0_f32);
            for k in 0..half {
                let a = start + k;
                let b = a + half;
                let v_re = re[b].mul_add(cur_re, -im[b] * cur_im);
                let v_im = re[b].mul_add(cur_im, im[b] * cur_re);
                let u_re = re[a];
                let u_im = im[a];
                re[a] = u_re + v_re;
                im[a] = u_im + v_im;
                re[b] = u_re - v_re;
                im[b] = u_im - v_im;
                let next_re = cur_re.mul_add(w_re, -cur_im * w_im);
                cur_im = cur_re.mul_add(w_im, cur_im * w_re);
                cur_re = next_re;
            }
            start += len;
        }
        len <<= 1;
        stage += 1;
    }
}
