//! OTT: Rust DSP port adapted from Rui-727/OTT's MIT-licensed C implementation.
//! Copyright (c) 2026 Rui-727. See `vendor/ott/LICENSE`.
//! Modified for Honeydew: Rust port, native AudioEffect parameters, and stereo-frame processing.
//!
//! Three-band Linkwitz–Riley 4th-order crossover (88.3 Hz / 2.5 kHz) followed
//! by downward and upward compression in each band. No allocation occurs in
//! `process_frame`.

/// Honeydew module type ID. This is outside the FL Studio Mobile effect IDs.
pub const OTT: u16 = 1009;

/// Parameter indexes in the saved module; index 0 is always Enable.
pub const PARAMS: usize = 13;
pub const DEPTH: usize = 1;
pub const TIME: usize = 2;
pub const UPWARD: usize = 3;
pub const DOWNWARD: usize = 4;
pub const INPUT_GAIN: usize = 5;
pub const OUTPUT_GAIN: usize = 6;
pub const LOW_THRESHOLD: usize = 7;
pub const MID_THRESHOLD: usize = 8;
pub const HIGH_THRESHOLD: usize = 9;
pub const LOW_GAIN: usize = 10;
pub const MID_GAIN: usize = 11;
pub const HIGH_GAIN: usize = 12;

/// Normalized defaults; parameter 0 is Enable. The band gains and +5.2 dB
/// input gain follow the reference project's Ableton OTT preset defaults.
pub const DEFAULTS: [f32; PARAMS] = [
    1.0,          // Enable
    1.0,          // Depth
    0.5,          // Time
    1.0,          // Upward
    1.0,          // Downward
    29.2 / 48.0,  // Input Gain: +5.2 dB in -24..+24 dB
    0.5,          // Output Gain: 0 dB
    0.5,          // Low Threshold: -30 dB in -60..0 dB
    0.5,          // Mid Threshold: -30 dB
    0.5,          // High Threshold: -30 dB
    34.3 / 48.0,  // Low Gain: +10.3 dB
    29.7 / 48.0,  // Mid Gain: +5.7 dB
    34.3 / 48.0,  // High Gain: +10.3 dB
];

const CROSSOVER_LOW_HZ: f32 = 88.3;
const CROSSOVER_HIGH_HZ: f32 = 2_500.0;
const ATTACK_MS: f32 = 5.0;
const RELEASE_MS: f32 = 50.0;
const RMS_MS: f32 = 2.5;
const KNEE_DB: f32 = 6.0;
const DOWN_RATIO_LOW_MID: f32 = 66.7;
const UP_RATIO: f32 = 4.0;
const DB_FLOOR: f32 = 1e-10;
const DENORMAL_FLOOR: f32 = 1e-20;

/// Butterworth biquad in direct form I. The coefficients and delay state are
/// kept together because each crossover branch has independent history.
#[derive(Clone, Copy, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl Biquad {
    fn set(&mut self, cutoff_hz: f32, sample_rate: f32, high_pass: bool) {
        let fs = sample_rate.max(1.0);
        let fc = cutoff_hz.clamp(1.0, 0.49 * fs);
        let omega = std::f32::consts::TAU * fc / fs;
        let cos_o = omega.cos();
        let sin_o = omega.sin();
        let alpha = sin_o / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;

        if high_pass {
            self.b0 = (1.0 + cos_o) * 0.5 / a0;
            self.b1 = -(1.0 + cos_o) / a0;
            self.b2 = (1.0 + cos_o) * 0.5 / a0;
        } else {
            self.b0 = (1.0 - cos_o) * 0.5 / a0;
            self.b1 = (1.0 - cos_o) / a0;
            self.b2 = (1.0 - cos_o) * 0.5 / a0;
        }

        self.a1 = -2.0 * cos_o / a0;
        self.a2 = (1.0 - alpha) / a0;
        self.clear();
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = x;
        self.y2 = self.y1;
        self.y1 = flush(y);
        self.y1
    }

    fn clear(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// Two cascaded Butterworth sections form an LR4 crossover branch.
#[derive(Clone, Copy, Default)]
struct Lr4 {
    first: Biquad,
    second: Biquad,
}

impl Lr4 {
    fn set(&mut self, cutoff_hz: f32, sample_rate: f32, high_pass: bool) {
        self.first.set(cutoff_hz, sample_rate, high_pass);
        self.second.set(cutoff_hz, sample_rate, high_pass);
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        self.second.process(self.first.process(x))
    }

    fn clear(&mut self) {
        self.first.clear();
        self.second.clear();
    }
}

#[derive(Clone, Copy, Default)]
struct ChannelFilters {
    low_lp: Lr4,
    low_hp: Lr4,
    high_lp: Lr4,
    high_hp: Lr4,
}

impl ChannelFilters {
    fn new(sample_rate: f32) -> Self {
        let mut filters = Self::default();
        filters.set(sample_rate);
        filters
    }

    fn set(&mut self, sample_rate: f32) {
        self.low_lp.set(CROSSOVER_LOW_HZ, sample_rate, false);
        self.low_hp.set(CROSSOVER_LOW_HZ, sample_rate, true);
        self.high_lp.set(CROSSOVER_HIGH_HZ, sample_rate, false);
        self.high_hp.set(CROSSOVER_HIGH_HZ, sample_rate, true);
    }

    #[inline]
    fn split(&mut self, input: f32) -> [f32; 3] {
        let low = self.low_lp.process(input);
        let mid_high = self.low_hp.process(input);
        let mid = self.high_lp.process(mid_high);
        let high = self.high_hp.process(mid_high);
        [low, mid, high]
    }

    fn clear(&mut self) {
        self.low_lp.clear();
        self.low_hp.clear();
        self.high_lp.clear();
        self.high_hp.clear();
    }
}

#[derive(Clone, Copy, Default)]
struct BandState {
    rms_down_sq: [f32; 2],
    env_down_attack: f32,
    env_down_release: f32,
    rms_up_sq: [f32; 2],
    env_up_attack: f32,
    env_up_release: f32,
    net_gr_db: f32,
}

impl BandState {
    fn clear(&mut self, threshold_db: f32) {
        *self = Self::default();
        // Starting the RMS followers at threshold avoids a large upward boost
        // when the first samples after silence arrive.
        let threshold = db_to_lin(threshold_db);
        let threshold_sq = threshold * threshold;
        self.rms_down_sq = [threshold_sq; 2];
        self.rms_up_sq = [threshold_sq; 2];
    }
}

/// Per-module OTT state. All buffers are fixed-size; processing allocates none.
pub struct OttState {
    sample_rate: f32,
    filters: [ChannelFilters; 2],
    bands: [BandState; 3],
    depth: f32,
    alpha_attack: f32,
    alpha_release: f32,
    alpha_rms: f32,
    input_gain: f32,
    output_gain: f32,
    thresholds: [f32; 3],
    band_gains: [f32; 3],
    slope_down: [f32; 3],
    slope_up: [f32; 3],
}

impl OttState {
    pub fn new(params: &[f32], sample_rate: f32) -> Self {
        let mut state = Self {
            sample_rate,
            filters: [
                ChannelFilters::new(sample_rate),
                ChannelFilters::new(sample_rate),
            ],
            bands: [BandState::default(); 3],
            depth: 1.0,
            alpha_attack: 0.0,
            alpha_release: 0.0,
            alpha_rms: 0.0,
            input_gain: 1.0,
            output_gain: 1.0,
            thresholds: [-30.0; 3],
            band_gains: [1.0; 3],
            slope_down: [0.0; 3],
            slope_up: [0.0; 3],
        };
        state.refresh(params, sample_rate);
        state.reset(params);
        state
    }

    /// Refreshes cached coefficients from normalized module parameters.
    pub fn refresh(&mut self, params: &[f32], sample_rate: f32) {
        if sample_rate != self.sample_rate {
            self.sample_rate = sample_rate;
            for filters in &mut self.filters {
                filters.set(sample_rate);
            }
        }

        let value = |index: usize| {
            params
                .get(index)
                .copied()
                .unwrap_or(DEFAULTS[index])
                .clamp(0.0, 1.0)
        };

        self.depth = value(DEPTH);
        let time_scale = 0.1 + 1.9 * value(TIME);
        let rate = sample_rate.max(1.0);
        self.alpha_attack = (-1.0 / (ATTACK_MS * 0.001 * time_scale * rate)).exp();
        self.alpha_release = (-1.0 / (RELEASE_MS * 0.001 * time_scale * rate)).exp();
        self.alpha_rms = (-1.0 / (RMS_MS * 0.001 * rate)).exp();

        self.input_gain = db_to_lin(-24.0 + 48.0 * value(INPUT_GAIN));
        self.output_gain = db_to_lin(-24.0 + 48.0 * value(OUTPUT_GAIN));

        let threshold_indices = [LOW_THRESHOLD, MID_THRESHOLD, HIGH_THRESHOLD];
        let gain_indices = [LOW_GAIN, MID_GAIN, HIGH_GAIN];
        for band in 0..3 {
            self.thresholds[band] = -60.0 + 60.0 * value(threshold_indices[band]);
            self.band_gains[band] = db_to_lin(-24.0 + 48.0 * value(gain_indices[band]));
        }

        let downward = value(DOWNWARD);
        let upward = value(UPWARD);
        let low_mid_slope = 1.0 - 1.0 / DOWN_RATIO_LOW_MID;
        let up_slope = 1.0 - 1.0 / UP_RATIO;
        self.slope_down = [low_mid_slope * downward, low_mid_slope * downward, downward];
        self.slope_up = [up_slope * upward; 3];
    }

    /// Clears crossover and envelope histories while keeping current settings.
    pub fn reset(&mut self, _params: &[f32]) {
        for filters in &mut self.filters {
            filters.clear();
        }
        for (band, &threshold) in self.bands.iter_mut().zip(&self.thresholds) {
            band.clear(threshold);
        }
    }

    /// Processes one stereo frame with linked gain detection.
    #[inline]
    pub fn process_frame(&mut self, left: f32, right: f32) -> (f32, f32) {
        let input_l = flush(left * self.input_gain);
        let input_r = flush(right * self.input_gain);
        let bands_l = self.filters[0].split(input_l);
        let bands_r = self.filters[1].split(input_r);
        let mut wet_l = 0.0;
        let mut wet_r = 0.0;

        for band_index in 0..3 {
            let state = &mut self.bands[band_index];
            let mut l = bands_l[band_index];
            let mut r = bands_r[band_index];

            // Separate channel RMS followers, shared max-level envelope/gain.
            state.rms_down_sq[0] = self.alpha_rms.mul_add(
                state.rms_down_sq[0],
                (1.0 - self.alpha_rms) * l * l,
            );
            state.rms_down_sq[1] = self.alpha_rms.mul_add(
                state.rms_down_sq[1],
                (1.0 - self.alpha_rms) * r * r,
            );
            let level_down = state.rms_down_sq[0]
                .max(state.rms_down_sq[1])
                .sqrt();
            let level_down_db = 20.0 * (level_down + DB_FLOOR).log10();
            let down_raw = gain_computer_down(
                level_down_db,
                self.thresholds[band_index],
                self.slope_down[band_index],
            );
            let down_db = ballistics(
                down_raw,
                &mut state.env_down_attack,
                &mut state.env_down_release,
                self.alpha_attack,
                self.alpha_release,
            )
            .clamp(0.0, 60.0);
            let down_gain = db_to_lin(-down_db);
            l = flush(l * down_gain);
            r = flush(r * down_gain);

            state.rms_up_sq[0] = self.alpha_rms.mul_add(
                state.rms_up_sq[0],
                (1.0 - self.alpha_rms) * l * l,
            );
            state.rms_up_sq[1] = self.alpha_rms.mul_add(
                state.rms_up_sq[1],
                (1.0 - self.alpha_rms) * r * r,
            );
            let level_up = state.rms_up_sq[0].max(state.rms_up_sq[1]).sqrt();
            let level_up_db = 20.0 * (level_up + DB_FLOOR).log10();
            let up_raw = gain_computer_up(
                level_up_db,
                self.thresholds[band_index],
                self.slope_up[band_index],
            );
            let up_db = ballistics(
                up_raw,
                &mut state.env_up_attack,
                &mut state.env_up_release,
                self.alpha_attack,
                self.alpha_release,
            )
            .clamp(-24.0, 0.0);
            let up_gain = db_to_lin(-up_db);
            l = flush(l * up_gain * self.band_gains[band_index]);
            r = flush(r * up_gain * self.band_gains[band_index]);

            state.net_gr_db = down_db + up_db;
            wet_l += l;
            wet_r += r;
        }

        wet_l *= self.output_gain;
        wet_r *= self.output_gain;
        let out_l = left * (1.0 - self.depth) + wet_l * self.depth;
        let out_r = right * (1.0 - self.depth) + wet_r * self.depth;
        (finite_or_zero(out_l), finite_or_zero(out_r))
    }

    /// Largest positive net attenuation, for Honeydew's standard GR meter.
    pub fn gain_reduction_db(&self) -> f32 {
        self.bands
            .iter()
            .map(|band| band.net_gr_db)
            .fold(0.0, f32::max)
    }
}

fn gain_computer_down(level_db: f32, threshold_db: f32, slope: f32) -> f32 {
    let over = level_db - threshold_db;
    let half_knee = KNEE_DB * 0.5;
    if over <= -half_knee {
        0.0
    } else if over < half_knee {
        let x = over + half_knee;
        0.5 * slope * x * x / KNEE_DB
    } else {
        slope * over
    }
}

/// Result is negative dB: a boost is applied as `10^(-result/20)`.
fn gain_computer_up(level_db: f32, threshold_db: f32, slope: f32) -> f32 {
    let under = threshold_db - level_db;
    if under <= 0.0 || under > 30.0 {
        return 0.0;
    }
    let half_knee = KNEE_DB * 0.5;
    if under < half_knee {
        let x = under + half_knee;
        -0.5 * slope * x * x / KNEE_DB
    } else {
        -slope * under
    }
}

fn ballistics(target: f32, attack: &mut f32, release: &mut f32, alpha_a: f32, alpha_r: f32) -> f32 {
    let released = alpha_r * *release + (1.0 - alpha_r) * target;
    *release = target.max(released);
    *attack = alpha_a * *attack + (1.0 - alpha_a) * *release;
    flush(*attack)
}

fn db_to_lin(db: f32) -> f32 {
    10.0_f32.powf(db / 20.0)
}

fn flush(value: f32) -> f32 {
    if value.abs() < DENORMAL_FLOOR { 0.0 } else { value }
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { flush(value) } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_depth_is_exact_dry_passthrough() {
        let mut params = DEFAULTS;
        params[DEPTH] = 0.0;
        let mut state = OttState::new(&params, 48_000.0);
        for i in 0..2048 {
            let left = (i as f32 * 0.031).sin() * 0.7;
            let right = (i as f32 * 0.047).cos() * 0.4;
            let (out_l, out_r) = state.process_frame(left, right);
            assert_eq!(out_l, left);
            assert_eq!(out_r, right);
        }
    }

    #[test]
    fn default_processing_stays_finite() {
        let mut state = OttState::new(&DEFAULTS, 48_000.0);
        for i in 0..20_000 {
            let left = 0.7 * (i as f32 * 0.037).sin();
            let right = 0.4 * (i as f32 * 0.051).cos();
            let (out_l, out_r) = state.process_frame(left, right);
            assert!(out_l.is_finite() && out_r.is_finite());
        }
    }
}
