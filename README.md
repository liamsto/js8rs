# js8rs 

`js8rs` is a lightweight Rust library for JS8-compatible framing, encoding, modulation, detection, and decoding, with a focus on performance. The goal is to present a clean and easy to use API with which to interact with the protocol, while also making optimizations where possible to run on more constrained platforms. I am by no means a performance engineer, so if you have suggestions let me know!

The core receive API is synchronous. `Decoder` is `Send`, so applications can move it to their own thread or runtime and choose their own queueing, backpressure, priority, and shutdown behavior.

This library is experimental, and primarily for my own experiments, both with radio and performance engineering. If you want to use the JS8 protocol normally, you should obviously use JS8Call-improved, which this is adapted from. It is also a WIP, but is fully functional and compatible with JS8Call in its current state.

## Copyright

This program is licensed under the GPLv3. It is derived in large part from GPLv3-licensed JS8Call-improved code. All code remains a copyright of the original authors, and copyright appears as appropriate on all derived source files.

This project is an independent experiment and is not affiliated with nor endorsed by the JS8Call project. It is a derivative of the great work done by Jordan Sherer and the rest of the JS8Call/JS8Call-improved team. 

Please see the [LICENSE](./LICENSE) for more information. See the comment headers in each file for information on modifications made to the original source, where relevant.

## Quick Start

```rust
use js8rs::codec::{BuildFramesOptions, build_frames};
use js8rs::protocol::{FrameFlags, Submode};

let built = build_frames(&BuildFramesOptions::new("HELLO WORLD", Submode::Fast));
assert!(built.frames[0].flags.contains(FrameFlags::FIRST));

let encoded = built.encode()?;
assert_eq!(encoded[0].tones.len(), 79);
```

## Protocol And Codec

`protocol` contains the public types: `Submode`, `FrameType`, `FrameFlags`, and `DecodeModes`. `FrameFlags` handles the flags JS8 uses to mark frame position in a message, f.eks.  `FrameFlags::FIRST | FrameFlags::LAST` for a single frame message. Explicit raw conversion can be done with `bits`, `from_bits`, and `from_bits_truncate`.

`codec` provides `build_frames`, `BuildFramesResult::encode`, the low-level `encode_tones` primitive, parsing into `DecodedFrame`.

```rust
use js8rs::codec::{BuildFramesOptions, build_frames, parse_frame};
use js8rs::protocol::Submode;

let built = build_frames(
    &BuildFramesOptions::new("K1ABC K2XYZ MSG HELLO", Submode::Fast)
        .with_station("K1ABC", "EM73"),
);
let frame = &built.frames[0];
let parsed = parse_frame(&frame.encoded, frame.flags, built.submode);
println!("{}", parsed.message);
```

## Receive

`Detector` accepts 48 kHz PCM and decimates it into the fixed 12 kHz decoder buffer, not much is different from JS8Call-improved here aside from some optimization. `with_samples` borrows that buffer without copying, while `copy_samples` copies into caller-owned reusable storage if you cannot pause.

`DecodeConfig` contains only user configuration.

```rust
use js8rs::protocol::DecodeModes;
use js8rs::rx::{DecodeConfig, Decoder, Event, SAMPLE_BUFFER_SIZE};

let mut decoder = Decoder::with_modes(DecodeModes::FAST);
let config = DecodeConfig::default()
    .with_modes(DecodeModes::FAST)
    .with_nominal_frequency(1500)
    .with_frequency_range(200, 3000);
let samples = vec![0i16; SAMPLE_BUFFER_SIZE];

let count = decoder.decode(&samples, samples.len(), &config, |event| {
    if let Event::Decoded(decoded) = event {
        println!("{} at {} Hz", decoded.frame.message, decoded.frequency_hz);
    }
});
println!("decoded {count} frames");
```

RX `Decoded` places signal data into `codec::DecodedFrame`. `DecodeScheduler` and `MessageBufferAssembler` are available for slot scheduling (to meet the JS8 UTC alignment requirement) or buffered directed-command reassembly. Keep in mind you still have to sync your clock properly, this just gives the slots based on system time.

## Transmit

`Modulator::start` accepts an `EncodedFrame`. Rendering returns stereo frame count, and both typed and LE byte output avoid heap allocation.

```rust
use js8rs::codec::{BuildFramesOptions, build_frames};
use js8rs::protocol::Submode;
use js8rs::tx::{Channel, Modulator};
use std::time::Duration;

let frame = build_frames(&BuildFramesOptions::new("HELLO", Submode::Fast))
    .encode()?
    .remove(0);
let mut modulator = Modulator::new();
modulator.start(&frame, 0, 1500.0, Duration::ZERO, Channel::Mono);

let mut stereo = [0i16; 2048];
let frames = modulator.render_stereo(&mut stereo);
assert_eq!(frames, stereo.len() / 2);
# Ok::<(), js8rs::codec::EncodeError>(())
```

`timing::compute_slot` and `Modulator::start_tones` use the protocol properties exposed by `Submode`. Durations use `std::time::Duration` and Unix timestamps are in ms.

## Benchmarking

The library is optimized to make use of SIMD where possible. As such, you will see a huge performance gain if you run a native build on a platform with SIMD support. To enable native CPU instructions for local Criterion runs, use the Cargo alias:

```sh
cargo bench-native
```

Pass normal benchmark arguments after `--`, for example:

```sh
cargo bench-native --bench micro -- modulator_render
```

### Encode/Decode Benchmark Results

A handful of benchmark results from running on my Ryzen 7 9800X3D:

```text
command_parse_case/case/bare_cq
                        time:   [24.353 ns 24.489 ns 24.649 ns]
                        thrpt:  [502.98 MiB/s 506.25 MiB/s 509.09 MiB/s]
encode_tones_js8_normal time:   [49.172 ns 49.197 ns 49.223 ns]
decode_js8_normal       time:   [19.452 ms 19.458 ms 19.465 ms]
modulator_render/fast_stereo
                        time:   [7.3229 µs 7.3241 µs 7.3253 µs]
                        thrpt:  [559.16 Melem/s 559.25 Melem/s 559.34 Melem/s]
parse_compound          time:   [81.985 ns 82.213 ns 82.429 ns]
full_chain_js8_fast     time:   [13.515 ms 13.604 ms 13.696 ms]
```
