// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Liam Storgaard <liam-git@aqrx.net>

use js8rs::protocol::DecodeModes;
use js8rs::rx::{DecodeConfig, Decoder, Event, SAMPLE_BUFFER_SIZE};

fn config() -> DecodeConfig {
    DecodeConfig::default().with_modes(DecodeModes::ALL)
}

#[test]
fn sync_decoder_emits_started_and_finished_events() {
    let mut decoder = Decoder::new();
    let config = config();
    let samples = vec![0i16; SAMPLE_BUFFER_SIZE];

    let mut saw_started = false;
    let mut saw_finished = false;
    let _ = decoder.decode(&samples, SAMPLE_BUFFER_SIZE, &config, |event| match event {
        Event::DecodeStarted(started) => {
            saw_started = true;
            assert_eq!(started.modes, DecodeModes::ALL);
        }
        Event::DecodeFinished(_) => saw_finished = true,
        _ => {}
    });

    assert!(saw_started);
    assert!(saw_finished);
}

#[test]
fn decoder_is_send_for_caller_managed_workers() {
    fn assert_send<T: Send>() {}
    assert_send::<Decoder>();
}
