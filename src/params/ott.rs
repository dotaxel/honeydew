//! Parameter registry for Honeydew's OTT effect.
//! Copyright (c) 2026 Rui-727 for the adapted DSP; see `vendor/ott/LICENSE`.

use honeydew_audio::ott as o;

use super::{Display, ModuleSpec, ParamSpec as P};

static PARAM_SPECS: [P; o::PARAMS] = [
    P::ENABLE,
    P::cont("Depth", o::DEFAULTS[o::DEPTH], Display::linear(0.0, 100.0, 0, " %")),
    P::cont("Time", o::DEFAULTS[o::TIME], Display::linear(0.0, 100.0, 0, " %")),
    P::cont("Upward", o::DEFAULTS[o::UPWARD], Display::linear(0.0, 100.0, 0, " %")),
    P::cont("Downward", o::DEFAULTS[o::DOWNWARD], Display::linear(0.0, 100.0, 0, " %")),
    P::cont(
        "Input Gain",
        o::DEFAULTS[o::INPUT_GAIN],
        Display::linear(-24.0, 24.0, 1, " dB"),
    ),
    P::cont(
        "Output Gain",
        o::DEFAULTS[o::OUTPUT_GAIN],
        Display::linear(-24.0, 24.0, 1, " dB"),
    ),
    P::cont(
        "Low Threshold",
        o::DEFAULTS[o::LOW_THRESHOLD],
        Display::linear(-60.0, 0.0, 1, " dB"),
    ),
    P::cont(
        "Mid Threshold",
        o::DEFAULTS[o::MID_THRESHOLD],
        Display::linear(-60.0, 0.0, 1, " dB"),
    ),
    P::cont(
        "High Threshold",
        o::DEFAULTS[o::HIGH_THRESHOLD],
        Display::linear(-60.0, 0.0, 1, " dB"),
    ),
    P::cont(
        "Low Gain",
        o::DEFAULTS[o::LOW_GAIN],
        Display::linear(-24.0, 24.0, 1, " dB"),
    ),
    P::cont(
        "Mid Gain",
        o::DEFAULTS[o::MID_GAIN],
        Display::linear(-24.0, 24.0, 1, " dB"),
    ),
    P::cont(
        "High Gain",
        o::DEFAULTS[o::HIGH_GAIN],
        Display::linear(-24.0, 24.0, 1, " dB"),
    ),
];

pub static MODULE: ModuleSpec =
    ModuleSpec::effect(o::OTT, "OTT", "OTT", false, &PARAM_SPECS, &[]);
