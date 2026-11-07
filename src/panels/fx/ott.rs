//! OTT's native Honeydew rack panel, using the shared panel widgets.

use honeydew_audio::ott as o;

use crate::panels::build::*;

pub(in crate::panels) fn panel() -> Panel {
    Panel::new(vstack([
        hstack([
            knob(1, "Depth").p(o::DEPTH),
            knob(2, "Time").p(o::TIME),
            knob(3, "Upward").p(o::UPWARD),
            knob(4, "Downward").p(o::DOWNWARD),
        ]),
        hstack([
            knob(5, "Input Gain").p(o::INPUT_GAIN),
            knob(6, "Output Gain").p(o::OUTPUT_GAIN),
            knob(7, "Low Thresh").p(o::LOW_THRESHOLD),
            knob(8, "Mid Thresh").p(o::MID_THRESHOLD),
        ]),
        hstack([
            knob(9, "High Thresh").p(o::HIGH_THRESHOLD),
            knob(10, "Low Gain").p(o::LOW_GAIN),
            knob(11, "Mid Gain").p(o::MID_GAIN),
            knob(12, "High Gain").p(o::HIGH_GAIN),
        ]),
    ]))
}
