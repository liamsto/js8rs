# js8rs 

A Rust library for the JS8 radio protocol. It is an experiment. If you want to use the JS8 protocol normally, obviously use JS8Call-improved, which this is adapted from. It is a WIP, but is functional and compatible with JS8Call. You should be able to set up a full transmit and decode chain using this code. 

It also includes some very experimental  code for transmitting and decoding without UTC alignment. This has been discussed in JS8Call before, but last I checked it has been #ifdefd out of the build. The basic idea, as I understand it, is that FT8, and by derivation JS8, use the timeslot alignment as a correlator for the message to reduce the search time. This alignment can be removed at the cost of CPU cycles, but it allows you to transmit whenever you want, and also to chain transmissions together. To try it out, enable `experimental-time`. 

## Why Rust?

Cuz idk C++ very well

## Copyright

This program is licensed under the GPLv3, and is a derivative work. All code remains a copyright of the original authors, and copyright appears as appropriate on all derived source files. This project is an independent experiment and is not affiliated with nor endorsed by the JS8Call project. It is a derivative of the great work done by Jordan Sherer and the rest of the JS8Call/JS8Call-improved team. 

Please see the [LICENSE](./LICENSE) for more information. See the comment headers in each file for information on modifications made to the original source, where relevant.

## Benchmarking

The library is optimized to make use of SIMD where possible. As such, you will see a huge performance gain if you run a native build on a platform with SIMD support. To enable native CPU instructions for local Criterion runs, use the Cargo alias:

```sh
cargo bench-native
```

I have tried to optimize the code where possible to improve performance, mainly by allowing for vectorization and whatnot. I am by no means a performance engineer, so if you have suggestions let me know!


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
