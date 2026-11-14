//! AllPassPhase: phase dispersion by running audio through a cascade of identical
//! second-order all-pass filters, for Honeydew.
//!
//! Re-implementation of the effect from enummusic's AllPassPhase VST (the repo
//! publishes no license; see the notes delivered with this file). Same sound
//! parameters (Frequency, Q, Intensity, Mix), written as a stereo-frame Rust
//! module: no allocation anywhere, filter coefficients shared by all stages,
//! stages run per sample so no block buffers are copied, coefficients glide
//! toward their targets, and the host's real sample rate is used.
//!
//! Each stage is the RBJ cookbook all-pass biquad with alpha = 2 * q * sin(w0)
//! (so a smaller Q gives a more pronounced dispersion, as in the original),
//! run in transposed direct form II with f64 state:
//!   y  = c0 * x + w1
//!   w1 = c1 * (x - y) + w2
//!   w2 = x - c0 * y
//! with c0 = (1 - alpha) / (1 + alpha) and c1 = -2 cos(w0) / (1 + alpha).
//! At exactly the center frequency each stage inverts the signal, so an odd
//! stage count inverts polarity there (used as a test).

use std::f64::consts::{SQRT_2, TAU};

/// Honeydew module type ID. OTT is 1009; 1010 is reserved for the Harmonizer.
pub const ALLPASS_PHASE: u16 = 1011;

/// Parameter indexes in the saved module; index 0 is always Enable.
pub const PARAMS: usize = 5;
pub const FREQUENCY: usize = 1;
pub const Q: usize = 2;
pub const INTENSITY: usize = 3;
pub const MIX: usize = 4;

/// The most stages Intensity can run.
pub const MAX_STAGES: usize = 50;

/// Frequency knob range in Hz, for the registry's display (knob 0 and knob 1).
pub const FREQ_MIN_HZ: f64 = 20.462_168_93;
pub const FREQ_MAX_HZ: f64 = 20_000.053_74;

/// Normalized defaults, the original's first program: Frequency 700 Hz,
/// Q knob 0.5 (Q 0.71), Intensity 25 stages, Mix 100 %.
pub const DEFAULTS: [f32; PARAMS] = [
    1.0,    // Enable
    0.5131, // Frequency: about 700 Hz
    0.5,    // Q
    0.5,    // Intensity: 25 stages
    1.0,    // Mix
];

/// Q is never allowed below this (no self-oscillation, as in the original).
const Q_MIN: f64 = 0.005;
/// Samples below this on both channels count as silence (about -103 dBFS).
const NOISE_FLOOR: f32 = 7e-6;
/// Coefficients are recomputed every this many frames while they glide.
const COEF_INTERVAL: u32 = 32;
/// Time constant of the frequency and Q glide.
const GLIDE_SECONDS: f64 = 0.015;
/// Wet output beyond this (about +36 dBFS) means a filter has run away.
const RUNAWAY: f64 = 64.0;

/// Frequency knob (0..1) to Hz: the original's note-like exponential law,
/// 20.5 Hz to 20 kHz, without its rounding to whole Hz.
pub fn knob_to_hz(v: f32) -> f64 {
    ((16.0 + f64::from(v) * 100.0 * 1.20103) * 1.059_f64.ln()).exp() * 8.17742
}

#[derive(Clone, Copy, Default)]
struct Stage {
    w1: [f64; 2],
    w2: [f64; 2],
}

/// Per-module AllPassPhase state. Fixed-size; `new` and `process_frame` allocate nothing.
pub struct AllPassPhaseState {
    sample_rate: f64,

    // Targets from the parameters
    target_log_hz: f64,
    target_log_q: f64,
    stages: usize,
    mix: f32,

    // Smoothed values and the coefficients they give
    cur_log_hz: f64,
    cur_log_q: f64,
    glide: f64,
    c0: f64,
    c1: f64,
    coef_countdown: u32,
    coef_stale: bool,

    // Filter state, one pair of channels per stage
    state: [Stage; MAX_STAGES],

    // Idle gate: stop processing once the input has been silent for longer than
    // the filters ring, and clear their state once.
    silent_run: u32,
    tail_limit: u32,
    dirty: bool,
}

impl AllPassPhaseState {
    pub fn new(params: &[f32], sample_rate: f32) -> Self {
        let mut s = Self {
            sample_rate: 0.0,
            target_log_hz: 700.0_f64.ln(),
            target_log_q: 0.5_f64.ln(),
            stages: 0,
            mix: 1.0,
            cur_log_hz: 0.0,
            cur_log_q: 0.0,
            glide: 0.01,
            c0: 0.0,
            c1: 0.0,
            coef_countdown: 1,
            coef_stale: true,
            state: [Stage::default(); MAX_STAGES],
            silent_run: 0,
            tail_limit: 1,
            dirty: false,
        };
        s.refresh(params, sample_rate);
        // Start on the targets, not gliding from the placeholders.
        s.cur_log_hz = s.target_log_hz;
        s.cur_log_q = s.target_log_q;
        s.update_coefficients();
        s.reset(params);
        s
    }

    /// Refreshes cached targets from normalized module parameters.
    pub fn refresh(&mut self, params: &[f32], sample_rate: f32) {
        let value = |index: usize| {
            params
                .get(index)
                .copied()
                .unwrap_or(DEFAULTS[index])
                .clamp(0.0, 1.0)
        };

        let rate = f64::from(sample_rate).max(8_000.0);
        if rate != self.sample_rate {
            self.sample_rate = rate;
            self.glide = 1.0 - (-(f64::from(COEF_INTERVAL)) / (GLIDE_SECONDS * rate)).exp();
            self.coef_stale = true;
        }

        let hz = knob_to_hz(value(FREQUENCY)).clamp(10.0, 0.49 * rate);
        self.target_log_hz = hz.ln();
        self.target_log_q = (f64::from(value(Q)) * SQRT_2).max(Q_MIN).ln();

        // The original's int(knob * 50); the epsilon absorbs float error from a snapped control.
        let stages = ((value(INTENSITY) * MAX_STAGES as f32 + 1e-3).floor() as usize).min(MAX_STAGES);
        if stages > self.stages {
            // Newly running stages start from rest, not from stale state.
            for stage in &mut self.state[self.stages..stages] {
                *stage = Stage::default();
            }
        }
        if stages != self.stages {
            self.stages = stages;
            self.coef_stale = true; // the idle limit depends on the stage count
        }

        self.mix = value(MIX);
        self.coef_countdown = 1;
    }

    /// Clears the filters while keeping current settings.
    pub fn reset(&mut self, _params: &[f32]) {
        self.clear();
        self.silent_run = self.tail_limit;
    }

    fn clear(&mut self) {
        self.state = [Stage::default(); MAX_STAGES];
        self.dirty = false;
    }

    /// Moves the smoothed frequency and Q toward their targets and recomputes
    /// the shared coefficients and the idle limit when anything changed.
    fn update_coefficients(&mut self) {
        let dh = self.target_log_hz - self.cur_log_hz;
        let dq = self.target_log_q - self.cur_log_q;
        if dh == 0.0 && dq == 0.0 && !self.coef_stale {
            return;
        }
        self.cur_log_hz = if dh.abs() < 1e-5 { self.target_log_hz } else { self.cur_log_hz + dh * self.glide };
        self.cur_log_q = if dq.abs() < 1e-5 { self.target_log_q } else { self.cur_log_q + dq * self.glide };
        self.coef_stale = false;

        let w0 = TAU * self.cur_log_hz.exp() / self.sample_rate;
        let alpha = w0.sin() * 2.0 * self.cur_log_q.exp();
        let a0 = 1.0 + alpha;
        self.c0 = (1.0 - alpha) / a0;
        self.c1 = -2.0 * w0.cos() / a0;

        // How long the cascade rings: the slower pole's decay, stretched by the
        // stage count, clamped to between 0.37 s (the original's 16384 samples
        // at 44.1 kHz) and 6 s.
        let disc = self.c1 * self.c1 - 4.0 * self.c0;
        let radius = if disc >= 0.0 {
            0.5 * (self.c1.abs() + disc.sqrt())
        } else {
            self.c0.max(0.0).sqrt()
        };
        let decay = -(radius.clamp(1e-9, 0.999_999_9)).ln();
        let samples = (12.0 + self.stages as f64) / decay;
        self.tail_limit = samples.clamp(0.37 * self.sample_rate, 6.0 * self.sample_rate) as u32;
    }

    /// Processes one stereo frame.
    #[inline]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        // Bypassed: exact dry. Clear once so re-engaging starts from rest.
        if self.stages == 0 || self.mix <= 0.0 {
            if self.dirty {
                self.clear();
            }
            return (left, right);
        }

        // Idle gate. Past the ringing time of a silent input the wet signal is
        // silence too, so pass the (silent) input and clear the state once.
        if left.abs() >= NOISE_FLOOR || right.abs() >= NOISE_FLOOR {
            self.silent_run = 0;
        } else if self.silent_run < self.tail_limit {
            self.silent_run += 1;
        }
        if self.silent_run >= self.tail_limit {
            if self.dirty {
                self.clear();
            }
            return (left, right);
        }

        self.coef_countdown -= 1;
        if self.coef_countdown == 0 {
            self.coef_countdown = COEF_INTERVAL;
            self.update_coefficients();
        }

        self.dirty = true;
        let (c0, c1) = (self.c0, self.c1);
        let mut x = [f64::from(left), f64::from(right)];
        for stage in &mut self.state[..self.stages] {
            let y = [c0 * x[0] + stage.w1[0], c0 * x[1] + stage.w1[1]];
            stage.w1 = [
                c1 * (x[0] - y[0]) + stage.w2[0],
                c1 * (x[1] - y[1]) + stage.w2[1],
            ];
            stage.w2 = [x[0] - c0 * y[0], x[1] - c0 * y[1]];
            x = y;
        }

        // A runaway filter (or NaN, which fails the comparison) is cut and restarted.
        if !(x[0].abs() <= RUNAWAY && x[1].abs() <= RUNAWAY) {
            self.clear();
            return (left, right);
        }

        let mix = self.mix;
        let out_l = x[0] as f32 * mix + left * (1.0 - mix);
        let out_r = x[1] as f32 * mix + right * (1.0 - mix);
        (out_l, out_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn run(state: &mut AllPassPhaseState, frames: usize, signal: impl Fn(usize) -> f32) -> (Vec<f32>, Vec<f32>) {
        let mut input = Vec::with_capacity(frames);
        let mut output = Vec::with_capacity(frames);
        for i in 0..frames {
            let x = signal(i);
            let (l, r) = state.process_frame(x, x);
            assert_eq!(l, r);
            input.push(x);
            output.push(l);
        }
        (input, output)
    }

    fn sine(hz: f64, amplitude: f32) -> impl Fn(usize) -> f32 {
        move |i| amplitude * (TAU * hz * i as f64 / f64::from(RATE)).sin() as f32
    }

    #[test]
    fn zero_mix_is_exact_dry_passthrough() {
        let mut params = DEFAULTS;
        params[MIX] = 0.0;
        let mut state = AllPassPhaseState::new(&params, RATE);
        for i in 0..4096 {
            let l = (i as f32 * 0.031).sin() * 0.7;
            let r = (i as f32 * 0.047).cos() * 0.4;
            assert_eq!(state.process_frame(l, r), (l, r));
        }
    }

    #[test]
    fn zero_intensity_is_exact_dry_passthrough() {
        let mut params = DEFAULTS;
        params[INTENSITY] = 0.0;
        let mut state = AllPassPhaseState::new(&params, RATE);
        for i in 0..4096 {
            let l = (i as f32 * 0.031).sin() * 0.7;
            assert_eq!(state.process_frame(l, -l), (l, -l));
        }
    }

    #[test]
    fn frequency_knob_covers_20_hz_to_20_khz() {
        assert!((knob_to_hz(0.0) - FREQ_MIN_HZ).abs() < 0.01);
        assert!((knob_to_hz(1.0) - FREQ_MAX_HZ).abs() < 1.0);
        assert!((knob_to_hz(DEFAULTS[FREQUENCY]) - 700.0).abs() < 1.0);
    }

    #[test]
    fn an_all_pass_cascade_keeps_the_level() {
        // All-pass: the steady-state level of a tone does not change, at the center frequency or away from it.
        for hz in [300.0, 700.0, 3000.0] {
            let mut state = AllPassPhaseState::new(&DEFAULTS, RATE);
            let (input, output) = run(&mut state, 24_000, sine(hz, 0.5));
            let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len() as f32).sqrt();
            let (a, b) = (rms(&input[19_200..]), rms(&output[19_200..]));
            assert!((b / a - 1.0).abs() < 0.01, "{hz} Hz: {a} {b}");
        }
    }

    #[test]
    fn an_odd_stage_count_inverts_the_center_frequency_and_an_even_one_does_not() {
        let f0 = knob_to_hz(DEFAULTS[FREQUENCY]);
        for (intensity, want) in [(0.5, -1.0), (0.52, 1.0)] {
            // 25 stages, then 26.
            let mut params = DEFAULTS;
            params[INTENSITY] = intensity;
            let mut state = AllPassPhaseState::new(&params, RATE);
            let (input, output) = run(&mut state, 24_000, sine(f0, 0.5));
            let dot: f32 = input[19_200..].iter().zip(&output[19_200..]).map(|(a, b)| a * b).sum();
            let energy: f32 = input[19_200..].iter().map(|a| a * a).sum();
            assert!((dot / energy - want).abs() < 0.02, "{intensity}: {}", dot / energy);
        }
    }

    #[test]
    fn the_idle_gate_clears_the_filters_after_the_input_goes_silent() {
        let mut state = AllPassPhaseState::new(&DEFAULTS, RATE);
        run(&mut state, 4800, sine(500.0, 0.5));
        assert!(state.dirty);
        let silence = state.tail_limit as usize + 100;
        let (_, output) = run(&mut state, silence, |_| 0.0);
        assert!(!state.dirty);
        assert_eq!(*output.last().unwrap(), 0.0);
        // And it wakes up again.
        let (_, output) = run(&mut state, 4800, sine(500.0, 0.5));
        assert!(output.iter().any(|x| x.abs() > 0.1));
    }

    #[test]
    fn parameter_jumps_stay_finite_and_bounded() {
        let mut state = AllPassPhaseState::new(&DEFAULTS, RATE);
        let mut seed = 7u32;
        let mut rand = move || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 8) as f32 / (1u32 << 24) as f32
        };
        let mut params = DEFAULTS;
        for i in 0..96_000 {
            if i % 64 == 0 {
                params[FREQUENCY] = rand();
                params[Q] = rand();
                params[INTENSITY] = rand();
                state.refresh(&params, RATE);
            }
            let x = 0.5 * (rand() - 0.5);
            let (l, r) = state.process_frame(x, -x);
            assert!(l.is_finite() && r.is_finite() && l.abs() < RUNAWAY as f32 && r.abs() < RUNAWAY as f32);
        }
    }
}