// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use js8rs::{
    codec::{BuildFramesOptions, EncodedFrame, build_frames},
    protocol::Submode,
    timing::{TxTimingConfig, compute_slot, unix_time_ms},
    tx::{Channel, Modulator},
};
use std::{
    collections::VecDeque,
    env,
    error::Error,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

const JS8_SAMPLE_RATE_HZ: u32 = 48_000;
const POLL_INTERVAL: Duration = Duration::from_millis(50);
const RENDER_BLOCK_FRAMES: usize = 4096;

fn main() -> Result<(), Box<dyn Error>> {
    let args = parse_args(env::args().skip(1).collect())?;

    let period_seconds = args.submode.period_seconds();
    let tx_delay = Duration::from_secs_f64(args.tx_delay_seconds);
    let timing_cfg = TxTimingConfig::new(tx_delay, Duration::from_secs(period_seconds));

    let built = build_frames(&BuildFramesOptions::new(args.message, args.submode));
    if built.frames.is_empty() {
        return Err("message produced no transmittable frames".into());
    }

    let tx_frames = built.encode()?;

    println!(
        "Prepared {} frame(s) in {} mode (period={}s, tx_delay={}s).",
        tx_frames.len(),
        args.submode.name(),
        period_seconds,
        args.tx_delay_seconds
    );

    let output = AudioOutput::open_default()?;
    let mut modulator = Modulator::new();

    for (index, encoded_frame) in tx_frames.iter().enumerate() {
        let frame = &encoded_frame.frame;
        let msg_len = frame.encoded.trim().len();
        let start_ms = wait_for_tx_gate(args.submode, timing_cfg, msg_len);
        let slot = compute_slot(args.submode, start_ms, timing_cfg);

        println!(
            "Frame {}/{}: [{}] bits={} at {:.3}s into slot.",
            index + 1,
            tx_frames.len(),
            frame.encoded.trim_end(),
            frame.flags.bits(),
            slot.seconds_into_slot
        );

        let pcm = render_frame_audio(
            &mut modulator,
            encoded_frame,
            args.audio_frequency_hz,
            tx_delay,
            start_ms,
        );

        output.enqueue(&pcm);
        output.wait_until_empty();
    }

    println!("Transmission complete.");
    Ok(())
}

struct Args {
    message: String,
    submode: Submode,
    audio_frequency_hz: f64,
    tx_delay_seconds: f64,
}

fn parse_args(args: Vec<String>) -> Result<Args, Box<dyn Error>> {
    let mut message = String::from("CQ CQ DE N0CALL");
    let mut submode = Submode::Normal;
    let mut audio_frequency_hz = 1500.0;
    let mut tx_delay_seconds = 0.2;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "-m" | "--message" => {
                i += 1;
                message = args
                    .get(i)
                    .ok_or("missing value for --message")?
                    .to_uppercase();
            }
            "-s" | "--submode" => {
                i += 1;
                let value = args.get(i).ok_or("missing value for --submode")?;
                submode = parse_submode(value)?;
            }
            "-f" | "--freq" => {
                i += 1;
                let value = args.get(i).ok_or("missing value for --freq")?;
                audio_frequency_hz = value.parse::<f64>()?;
            }
            "-d" | "--tx-delay" => {
                i += 1;
                let value = args.get(i).ok_or("missing value for --tx-delay")?;
                tx_delay_seconds = value.parse::<f64>()?;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => {
                return Err(format!("unknown argument: {other}").into());
            }
        }
        i += 1;
    }

    Ok(Args {
        message,
        submode,
        audio_frequency_hz,
        tx_delay_seconds,
    })
}

fn parse_submode(value: &str) -> Result<Submode, Box<dyn Error>> {
    let mode = match value.to_ascii_lowercase().as_str() {
        "normal" | "n" => Submode::Normal,
        "fast" | "f" => Submode::Fast,
        "turbo" | "t" => Submode::Turbo,
        "slow" | "s" => Submode::Slow,
        "ultra" | "u" => Submode::Ultra,
        _ => {
            return Err(format!("invalid submode '{value}'").into());
        }
    };
    Ok(mode)
}

fn print_usage() {
    println!("JS8 end-to-end TX example");
    println!("Usage:");
    println!("  cargo run --example e2e_tx -- [options]");
    println!("Options:");
    println!("  -m, --message <text>      Text to encode (default: \"CQ CQ DE N0CALL\")");
    println!("  -s, --submode <mode>      normal|fast|turbo|slow|ultra (default: normal)");
    println!("  -f, --freq <hz>           Audio base frequency in Hz (default: 1500)");
    println!("  -d, --tx-delay <seconds>  TX delay in seconds (default: 0.2)");
    println!("  -h, --help                Show help");
}

fn wait_for_tx_gate(submode: Submode, cfg: TxTimingConfig, msg_len: usize) -> u64 {
    loop {
        let now_ms = unix_time_ms();
        let slot = compute_slot(submode, now_ms, cfg);
        if slot.should_start_tx_now(false, msg_len) {
            return now_ms;
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn render_frame_audio(
    modulator: &mut Modulator,
    frame: &EncodedFrame,
    audio_frequency_hz: f64,
    tx_delay: Duration,
    now_ms: u64,
) -> Vec<i16> {
    let mut frame_audio = Vec::new();
    let mut scratch = vec![0i16; RENDER_BLOCK_FRAMES * 2];

    modulator.start(frame, now_ms, audio_frequency_hz, tx_delay, Channel::Mono);

    loop {
        let frames = modulator.render_stereo(&mut scratch);
        if frames == 0 {
            break;
        }

        let samples = frames * 2;
        if modulator.is_idle() {
            let trimmed = trim_trailing_stereo_silence(&scratch[..samples]);
            frame_audio.extend_from_slice(&scratch[..trimmed]);
            break;
        }

        frame_audio.extend_from_slice(&scratch[..samples]);
    }

    frame_audio
}

fn trim_trailing_stereo_silence(samples: &[i16]) -> usize {
    let mut used = samples.len() - (samples.len() % 2);
    while used >= 2 && samples[used - 2] == 0 && samples[used - 1] == 0 {
        used -= 2;
    }
    used
}

struct AudioOutput {
    _stream: cpal::Stream,
    queue: Arc<Mutex<VecDeque<i16>>>,
}

impl AudioOutput {
    fn open_default() -> Result<Self, Box<dyn Error>> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or("no default output audio device found")?;
        let supported = choose_supported_config(&device)?;
        let stream_cfg = supported.config();
        let sample_format = supported.sample_format();

        println!(
            "Output device: {} | {:?}, {} channel(s) @ {} Hz",
            device.name()?,
            sample_format,
            stream_cfg.channels,
            stream_cfg.sample_rate.0
        );

        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let stream = build_output_stream(&device, &stream_cfg, sample_format, Arc::clone(&queue))?;
        stream.play()?;

        Ok(Self {
            _stream: stream,
            queue,
        })
    }

    fn enqueue(&self, pcm: &[i16]) {
        let mut queue = self.queue.lock().expect("audio queue lock poisoned");
        queue.extend(pcm.iter().copied());
    }

    fn wait_until_empty(&self) {
        while !self
            .queue
            .lock()
            .expect("audio queue lock poisoned")
            .is_empty()
        {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn choose_supported_config(
    device: &cpal::Device,
) -> Result<cpal::SupportedStreamConfig, Box<dyn Error>> {
    let mut single_channel_48k = None;

    for cfg in device.supported_output_configs()? {
        if cfg.min_sample_rate().0 <= JS8_SAMPLE_RATE_HZ
            && JS8_SAMPLE_RATE_HZ <= cfg.max_sample_rate().0
        {
            let selected = cfg.with_sample_rate(cpal::SampleRate(JS8_SAMPLE_RATE_HZ));
            if selected.channels() >= 2 {
                return Ok(selected);
            }
            if single_channel_48k.is_none() {
                single_channel_48k = Some(selected);
            }
        }
    }

    single_channel_48k.ok_or_else(|| {
        "default output device has no 48 kHz mode; JS8 TX timing requires 48 kHz playback".into()
    })
}

fn build_output_stream(
    device: &cpal::Device,
    cfg: &cpal::StreamConfig,
    sample_format: SampleFormat,
    queue: Arc<Mutex<VecDeque<i16>>>,
) -> Result<cpal::Stream, Box<dyn Error>> {
    let stream = match sample_format {
        SampleFormat::I8 => build_output_stream_t::<i8>(device, cfg, queue)?,
        SampleFormat::F32 => build_output_stream_t::<f32>(device, cfg, queue)?,
        SampleFormat::F64 => build_output_stream_t::<f64>(device, cfg, queue)?,
        SampleFormat::I16 => build_output_stream_t::<i16>(device, cfg, queue)?,
        SampleFormat::I32 => build_output_stream_t::<i32>(device, cfg, queue)?,
        SampleFormat::I64 => build_output_stream_t::<i64>(device, cfg, queue)?,
        SampleFormat::U8 => build_output_stream_t::<u8>(device, cfg, queue)?,
        SampleFormat::U16 => build_output_stream_t::<u16>(device, cfg, queue)?,
        SampleFormat::U32 => build_output_stream_t::<u32>(device, cfg, queue)?,
        SampleFormat::U64 => build_output_stream_t::<u64>(device, cfg, queue)?,
        other => {
            return Err(format!("unsupported output sample format: {other:?}").into());
        }
    };
    Ok(stream)
}

fn build_output_stream_t<T>(
    device: &cpal::Device,
    cfg: &cpal::StreamConfig,
    queue: Arc<Mutex<VecDeque<i16>>>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<i16>,
{
    let channels = cfg.channels as usize;
    device.build_output_stream(
        cfg,
        move |output: &mut [T], _| fill_output_buffer(output, channels, &queue),
        move |err| eprintln!("audio output error: {err}"),
        None,
    )
}

fn fill_output_buffer<T>(output: &mut [T], channels: usize, queue: &Arc<Mutex<VecDeque<i16>>>)
where
    T: SizedSample + FromSample<i16>,
{
    let mut samples = queue.lock().expect("audio queue lock poisoned");
    for frame in output.chunks_mut(channels) {
        let left = samples.pop_front().unwrap_or(0);
        let right = samples.pop_front().unwrap_or(0);
        let mono = i32::midpoint(i32::from(left), i32::from(right)) as i16;

        for (chan, sample) in frame.iter_mut().enumerate() {
            let v = if channels == 1 {
                mono
            } else if chan == 0 {
                left
            } else if chan == 1 {
                right
            } else {
                0
            };
            *sample = T::from_sample(v);
        }
    }
}
