# OTT DSP port for Honeydew

This directory records the upstream DSP attribution and its MIT license. The Rust implementation is in
`crates/audio/src/ott.rs`; it is a native Honeydew effect, not a loaded LV2 or CLAP plugin.

- Upstream: <https://github.com/Rui-727/OTT>
- Upstream DSP: `ott_dsp.c`, adapted from C to Rust and Honeydew's normalized module parameter interface.
- License: MIT; see [`LICENSE`](LICENSE).
- Original design credit in upstream: Steve Duda / Xfer Records and the Ableton Multiband Dynamics OTT preset. Honeydew is not affiliated with them.

The engine parameter order is Enable, Depth, Time, Upward, Downward, Input Gain, Output Gain, Low/Mid/High Threshold, then Low/Mid/High Gain. All values are normalized to 0..1 in the song; the parameter registry defines the display ranges.
