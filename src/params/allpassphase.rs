//! Parameter registry for Honeydew's AllPassPhase effect.
//! Place at src/params/allpassphase.rs

use honeydew_audio::allpassphase as a;

use super::{At, Display, ModuleSpec, ParamSpec as P};

static PARAM_SPECS: [P; a::PARAMS] = [
    P::ENABLE,
    // 20.5 Hz ... 20 kHz, exponential, as the original's knob.
    P::cont(
        "Frequency",
        a::DEFAULTS[a::FREQUENCY],
        Display::Freq {
            min: a::FREQ_MIN_HZ,
            max: a::FREQ_MAX_HZ,
            hz_decimals: 0,
            khz_decimals: 2,
        },
    ),
    // Q = knob * sqrt(2); the DSP floors it at 0.005.
    P::cont(
        "Q",
        a::DEFAULTS[a::Q],
        Display::linear(0.0, std::f64::consts::SQRT_2, 2, ""),
    ),
    // Stages 0..50; 0 is bypass.
    P::cont(
        "Intensity",
        a::DEFAULTS[a::INTENSITY],
        Display::Special {
            inner: &Display::linear(0.0, 50.0, 0, ""),
            at: At::Min,
            text: "BYPASS",
        },
    ),
    P::cont("Mix", a::DEFAULTS[a::MIX], Display::linear(0.0, 100.0, 0, " %")),
];

pub static MODULE: ModuleSpec = ModuleSpec::effect(
    a::ALLPASS_PHASE,
    "AllPassPhase",
    "AllPassPhase",
    false,
    &PARAM_SPECS,
    &[],
);