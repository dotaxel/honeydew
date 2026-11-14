//! Rack panel for AllPassPhase.
//! Place at src/panels/fx/allpassphase.rs

use crate::panels::build::*;
use honeydew_audio::allpassphase as a;

pub(in crate::panels) fn panel() -> Panel {
    Panel::new(hstack([
        knob(1, "Frequency").p(a::FREQUENCY),
        knob(2, "Q").p(a::Q),
        knob(3, "Intensity").p(a::INTENSITY),
        knob(4, "Mix").p(a::MIX),
    ]))
}