use super::*;
use symphonia::core::io::MediaSource;

pub(super) fn track(generation: u64, from: u64) -> Arc<Track> {
    Arc::new(Track {
        decoded: Default::default(),
        output: Default::default(),
        quality: Quality::Original,
        seekable: std::sync::atomic::AtomicBool::new(true),
        generation,
        from,
        occurrence: generation,
        context: super::super::scrobble::Context {
            occurrence: generation,
            ..Default::default()
        },
        song: Song::default(),
        offset: 0.0,
        samples: AtomicU64::new(0),
        gate: Arc::new(std::sync::atomic::AtomicU8::new(if from == generation {
            2
        } else {
            1
        })),
        worker: 0,
    })
}
fn input(track: Arc<Track>, blocks: Vec<Vec<f32>>) -> Input {
    let (send, receive) = mpsc::sync_channel(blocks.len().max(1));
    for block in blocks {
        send.send(Ok(block)).unwrap();
    }
    drop(send);
    Input::new(track, receive)
}
fn source(
    blocks: Vec<Vec<f32>>,
    eq: crate::eq::SharedEq,
    tap: Arc<crate::vis::AudioTap>,
) -> (
    ContinuousSource,
    Arc<Preparation>,
    mpsc::Receiver<Boundary>,
    Arc<Track>,
) {
    let first = track(1, 1);
    let preparation = Arc::new(Preparation::default());
    let (send, receive) = mpsc::channel();
    (
        ContinuousSource::new(
            input(first.clone(), blocks),
            preparation.clone(),
            Arc::new(AtomicU64::new(1)),
            send,
            eq,
            tap,
        ),
        preparation,
        receive,
        first,
    )
}
fn raw(bytes: Vec<u8>, suffix: &str, position: f64) -> Vec<Vec<f32>> {
    let mut blocks = Vec::new();
    decode::decode_raw(
        Box::new(std::io::Cursor::new(bytes)),
        suffix,
        position,
        |block| {
            blocks.push(block);
            true
        },
    )
    .unwrap();
    blocks
}
fn decode(
    input: Box<dyn MediaSource>,
    suffix: &str,
    position: f64,
    eq: &crate::eq::SharedEq,
    mut consume: impl FnMut(Vec<f32>) -> bool,
) -> Result<(), String> {
    let mut blocks = Vec::new();
    decode::decode_raw(input, suffix, position, |block| {
        blocks.push(block);
        true
    })?;
    let mut source = source(blocks, eq.clone(), crate::vis::AudioTap::new()).0;
    loop {
        let block: Vec<_> = source.by_ref().take(PCM_BLOCK_SAMPLES).collect();
        if block.is_empty() || !consume(block) {
            break;
        }
    }
    Ok(())
}

#[test]
fn pcm_blocks_have_the_same_duration_for_cd_and_high_resolution_sources() {
    for rate in [RATE, 96_000, 192_000] {
        let mut sizes = Vec::new();
        decode(
            Box::new(std::io::Cursor::new(wav(rate, 2))),
            "wav",
            0.0,
            &crate::eq::shared(),
            |block| {
                sizes.push(block.len());
                true
            },
        )
        .unwrap();
        let (tail, full) = sizes.split_last().unwrap();
        assert!(
            full.iter().all(|&size| size == PCM_BLOCK_SAMPLES),
            "{rate}: {sizes:?}"
        );
        assert!(*tail > 0 && *tail <= PCM_BLOCK_SAMPLES && tail.is_multiple_of(2));
        assert!((sizes.iter().sum::<usize>() as i64 - i64::from(RATE) * 2).abs() < 128);
    }
}

fn wav(rate: u32, channels: u16) -> Vec<u8> {
    let size = rate * u32::from(channels) * 2;
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(size + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&rate.to_le_bytes());
    bytes.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    bytes.extend_from_slice(&(channels * 2).to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&size.to_le_bytes());
    for index in 0..rate {
        let sample = ((f64::from(index) * 440.0 * std::f64::consts::TAU / f64::from(rate)).sin()
            * 4000.0) as i16;
        for _ in 0..channels {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    bytes
}

fn decoded(rate: u32, channels: u16, position: f64, preamp: Option<f32>) -> Vec<f32> {
    let settings = crate::eq::EqSettings {
        on: preamp.is_some(),
        preamp_db: preamp.unwrap_or(0.0),
        ..Default::default()
    };
    let eq = Arc::new(std::sync::Mutex::new(settings));
    let mut result = Vec::new();
    decode(
        Box::new(std::io::Cursor::new(wav(rate, channels))),
        "wav",
        position,
        &eq,
        |block| {
            result.extend(block);
            true
        },
    )
    .unwrap();
    result
}

#[test]
fn mono_and_high_rate_audio_use_the_existing_stereo_dsp_format() {
    for (rate, channels) in [(RATE, 2), (48_000, 1), (96_000, 2)] {
        let audio = decoded(rate, channels, 0.0, None);
        assert!(
            (audio.len() as i64 - i64::from(RATE) * 2).abs() < 128,
            "{rate}: {}",
            audio.len()
        );
        assert!(audio.iter().any(|s| s.abs() > 0.05));
        assert!(audio.as_chunks::<2>().0.iter().all(|s| s[0] == s[1]));
    }
}

#[test]
fn accurate_seek_discards_audio_before_the_requested_time() {
    let entire = decoded(RATE, 2, 0.0, None);
    let sought = decoded(RATE, 2, 0.5, None);
    assert_eq!(sought.len(), RATE as usize);
    assert_eq!(sought, entire[RATE as usize..]);
}

#[test]
fn equalizer_changes_pcm_and_muting_output_keeps_the_visualizer() {
    let flat = decoded(RATE, 2, 0.0, None);
    let quiet = decoded(RATE, 2, 0.0, Some(-6.0));
    let rms = |samples: &[f32]| {
        (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
    };
    assert!((rms(&quiet) / rms(&flat) - 0.5012).abs() < 0.005);
    let tap = crate::vis::AudioTap::new();
    let source = source(vec![quiet.clone()], crate::eq::shared(), tap.clone()).0;
    assert!(source.amplify(0.0).all(|s| s == 0.0));
    assert!(tap.window(512, 0).iter().any(|s| s.abs() > 0.02));
}

#[test]
fn decoder_stops_when_consumer_cancels() {
    let eq = Arc::new(std::sync::Mutex::new(Default::default()));
    let mut blocks = 0;
    decode::decode_raw(
        Box::new(std::io::Cursor::new(wav(RATE, 2))),
        "wav",
        0.0,
        |_| {
            blocks += 1;
            false
        },
    )
    .unwrap();
    assert_eq!(blocks, 1);
    assert!(
        decode(
            Box::new(std::io::Cursor::new(b"not audio".to_vec())),
            "mp3",
            0.0,
            &eq,
            |_| true
        )
        .is_err()
    );
}
/// Opt-in smoke test against a disposable local Navidrome, never a user account.
#[test]
#[ignore = "requires the disposable local Navidrome fixture"]
fn local_navidrome_stream_decodes_and_seeks() {
    let server =
        std::env::var("NAVIFAST_TEST_SERVER").expect("set a disposable local test server URL");
    let username = std::env::var("NAVIFAST_TEST_USER").unwrap();
    let password = std::env::var("NAVIFAST_TEST_PASSWORD").unwrap();
    let client = Client::new(
        &server,
        &username,
        password,
        &crate::settings::ProxyConfig::Off,
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let song = runtime.block_on(async {
        client.ping().await.unwrap();
        let albums = client.albums(0, true).await.unwrap();
        let album = client.album(&albums[0].id).await.unwrap();
        album.song[0].clone()
    });
    drop(runtime);
    for position in [0.0, 5.0] {
        let source = HttpSource::open(
            client.clone(),
            Client::stream_client(&crate::settings::ProxyConfig::Off).unwrap(),
            song.id.clone(),
            Arc::new(AtomicU64::new(1)),
            1,
        )
        .unwrap();
        let eq = crate::eq::shared();
        let mut count = 0;
        let mut peak = 0.0f32;
        decode(Box::new(source), &song.suffix, position, &eq, |block| {
            count += block.len();
            peak = block.iter().fold(peak, |p, sample| p.max(sample.abs()));
            true
        })
        .unwrap();
        assert!(
            peak > 0.001,
            "the server stream must contain actual decoded audio"
        );
        assert!((count as f64 / (2.0 * f64::from(RATE)) - (song.duration - position)).abs() < 0.2);
    }
}

fn wav_section(bytes: &[u8], start: usize, end: usize) -> Vec<u8> {
    let mut section = bytes[..44].to_vec();
    section.extend_from_slice(&bytes[44 + start * 4..44 + end * 4]);
    let size = (section.len() - 8) as u32;
    section[4..8].copy_from_slice(&size.to_le_bytes());
    section[40..44].copy_from_slice(&((end - start) as u32 * 4).to_le_bytes());
    section
}

#[test]
fn split_wav_matches_continuous_reference_with_eq_and_limiter_across_boundary() {
    let bytes = wav(RATE, 2);
    let split = 12_347; // Non-silent, not a codec packet or output block boundary.
    let eq = crate::eq::shared();
    {
        let mut settings = eq.lock().unwrap();
        settings.on = true;
        settings.bands_db[2] = 9.0;
        settings.preamp_db = 12.0;
    }
    let reference: Vec<_> = source(
        raw(bytes.clone(), "wav", 0.0),
        eq.clone(),
        crate::vis::AudioTap::new(),
    )
    .0
    .collect();
    let (mut joined, preparation, events, first) = source(
        raw(wav_section(&bytes, 0, split), "wav", 0.0),
        eq,
        crate::vis::AudioTap::new(),
    );
    let second = track(2, 1);
    assert!(preparation.put(
        0,
        input(
            second.clone(),
            raw(wav_section(&bytes, split, RATE as usize), "wav", 0.0)
        )
    ));
    let before: Vec<_> = joined.by_ref().take(split * 2).collect();
    assert!(
        events.try_recv().is_err(),
        "metadata stays on the first occurrence through its last sample"
    );
    assert_eq!(first.samples.load(Ordering::Relaxed), (split * 2) as u64);
    assert_eq!(second.samples.load(Ordering::Relaxed), 0);
    let first_new = joined.next().unwrap();
    assert!(matches!(events.try_recv(), Ok(Boundary::Started(t)) if t.generation == 2));
    let actual: Vec<_> = before
        .into_iter()
        .chain([first_new])
        .chain(joined)
        .collect();
    assert_eq!(actual.len(), reference.len());
    assert_eq!(
        actual, reference,
        "no missing, repeated, overlapped or padded samples"
    );
    assert_eq!(
        second.samples.load(Ordering::Relaxed),
        ((RATE as usize - split) * 2) as u64
    );
}

#[test]
fn last_sample_queue_edit_cancels_already_decoded_successor() {
    let (mut joined, preparation, events, _) = source(
        vec![vec![0.2; 2000]],
        crate::eq::shared(),
        crate::vis::AudioTap::new(),
    );
    let second = track(2, 1);
    preparation.put(0, input(second.clone(), vec![vec![0.4; 2000]]));
    assert_eq!(joined.by_ref().take(2000).count(), 2000);
    preparation.cancel();
    assert_eq!(joined.next(), None);
    assert_eq!(second.samples.load(Ordering::Relaxed), 0);
    assert!(events.try_recv().is_err());
}

#[test]
fn late_preparation_buffers_whole_frames_without_advancing_or_repeating_audio() {
    let (mut joined, preparation, events, first) = source(
        vec![vec![0.2; 2000]],
        crate::eq::shared(),
        crate::vis::AudioTap::new(),
    );
    let second = track(2, 1);
    let (send, receive) = mpsc::sync_channel(1);
    preparation.put(0, Input::new(second.clone(), receive));
    assert!(joined.by_ref().take(2000).all(|s| s == 0.2));
    let consumed = first.samples.load(Ordering::Relaxed);
    assert_eq!(joined.next(), Some(0.0));
    send.send(Ok(vec![0.4; 1000])).unwrap();
    drop(send);
    assert_eq!(joined.next(), Some(0.0), "finish the silent stereo frame");
    assert_eq!(first.samples.load(Ordering::Relaxed), consumed);
    assert!(events.try_recv().is_err());
    assert!(joined.by_ref().all(|s| s == 0.4));
    assert_eq!(second.samples.load(Ordering::Relaxed), 1000);
    assert!(matches!(events.try_recv(), Ok(Boundary::Started(_))));
}

#[test]
fn seek_or_skip_cancels_entire_output_chain_even_after_natural_handoff() {
    let first = track(1, 1);
    let current = Arc::new(AtomicU64::new(1));
    let prep = Arc::new(Preparation::default());
    let second = track(2, 1);
    prep.put(0, input(second.clone(), vec![vec![0.4; 2000]]));
    let (send, _) = mpsc::channel();
    let mut joined = ContinuousSource::new(
        input(first, vec![vec![0.2; 2000]]),
        prep.clone(),
        current.clone(),
        send,
        crate::eq::shared(),
        crate::vis::AudioTap::new(),
    );
    assert_eq!(joined.by_ref().take(2002).count(), 2002);
    prep.cancel();
    assert_eq!(
        joined.next(),
        Some(0.4),
        "cancelling a future candidate cannot stop an already active occurrence"
    );
    current.store(3, Ordering::Release);
    assert_eq!(joined.next(), None);
}

#[test]
fn failed_preparation_reports_the_next_occurrence_after_current_tail() {
    let (mut joined, prep, events, _) = source(
        vec![vec![0.2; 2000]],
        crate::eq::shared(),
        crate::vis::AudioTap::new(),
    );
    let (send, receive) = mpsc::sync_channel(1);
    send.send(Err("The server refused this track.".into()))
        .unwrap();
    drop(send);
    prep.put(0, Input::new(track(2, 1), receive));
    assert!(joined.by_ref().take(2000).all(|s| s == 0.2));
    assert_eq!(joined.next(), None);
    assert!(matches!(events.try_recv(), Ok(Boundary::Failed(t, _)) if t.generation == 2));
}

#[test]
fn flac_and_lame_gap_metadata_preserve_track_lengths_and_continuous_output() {
    let cases: [(&str, &[u8], &[u8]); 2] = [
        (
            "flac",
            include_bytes!("../../../tests/fixtures/gapless/first.flac"),
            include_bytes!("../../../tests/fixtures/gapless/second.flac"),
        ),
        (
            "mp3",
            include_bytes!("../../../tests/fixtures/gapless/first.mp3"),
            include_bytes!("../../../tests/fixtures/gapless/second.mp3"),
        ),
    ];
    let original: Vec<_> = raw(wav(RATE, 2), "wav", 0.0)
        .into_iter()
        .flatten()
        .collect();
    for (suffix, first, second) in cases {
        let first = raw(first.to_vec(), suffix, 0.0);
        let second = raw(second.to_vec(), suffix, 0.0);
        assert_eq!(
            first.iter().map(Vec::len).sum::<usize>(),
            12347 * 2,
            "{suffix} encoder delay and padding"
        );
        assert_eq!(
            second.iter().map(Vec::len).sum::<usize>(),
            (RATE as usize - 12347) * 2
        );
        let decoded: Vec<_> = first.iter().chain(&second).flatten().copied().collect();
        if suffix == "flac" {
            assert_eq!(decoded, original);
        } else {
            let rms = (decoded
                .iter()
                .zip(&original)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f32>()
                / original.len() as f32)
                .sqrt();
            assert!(rms < 0.01, "lossy reference error {rms}");
        }
        // Exercise rodio's continuous conversion into a common 48 kHz device
        // format too. Separate per-track converters would reset at the split.
        let reference = source(
            vec![decoded],
            crate::eq::shared(),
            crate::vis::AudioTap::new(),
        )
        .0;
        let expected: Vec<_> =
            rodio::source::UniformSourceIterator::new(reference, 2, 48_000).collect();
        let (joined, prep, _, _) = source(first, crate::eq::shared(), crate::vis::AudioTap::new());
        prep.put(0, input(track(2, 1), second));
        let actual: Vec<_> = rodio::source::UniformSourceIterator::new(joined, 2, 48_000).collect();
        assert_eq!(actual, expected, "{suffix} 48 kHz output boundary");
    }
}

#[test]
fn selected_mp3_rejects_an_original_codec_before_emitting_audio() {
    let mut emitted = false;
    let result = decode::decode_mp3(Box::new(std::io::Cursor::new(wav(RATE, 2))), 0.0, |_| {
        emitted = true;
        true
    });
    assert!(result.is_err());
    assert!(!emitted);
}

#[test]
#[ignore = "requires the disposable local Navidrome fixture"]
fn local_navidrome_quality_decodes_seeks_and_reconnects() {
    let server = std::env::var("NAVIFAST_TEST_SERVER").unwrap();
    assert!(
        server.starts_with("http://127.0.0.1:"),
        "use only an isolated local fixture"
    );
    let client = Client::new(
        &server,
        &std::env::var("NAVIFAST_TEST_USER").unwrap(),
        std::env::var("NAVIFAST_TEST_PASSWORD").unwrap(),
        &crate::settings::ProxyConfig::Off,
    )
    .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let song = runtime.block_on(async {
        client.ping().await.unwrap();
        let albums = client.albums(0, true).await.unwrap();
        client.album(&albums[0].id).await.unwrap().song[0].clone()
    });
    for quality in Quality::ALL {
        for position in [0.0, 5.0, 17.0] {
            let plan = runtime
                .block_on(client.streaming_plan(&song.id, quality, position))
                .unwrap();
            assert!(plan.seekable);
            let current = Arc::new(AtomicU64::new(1));
            let http = Client::stream_client(&crate::settings::ProxyConfig::Off).unwrap();
            let source: Box<dyn MediaSource> = if plan.ranged {
                Box::new(HttpSource::open_url(http, plan.url, current, 1, None).unwrap())
            } else {
                Box::new(
                    super::super::stream::forward::ForwardSource::open(
                        http, plan.url, current, 1, None,
                    )
                    .unwrap(),
                )
            };
            let mut count = 0;
            let consume = |block: Vec<f32>| {
                count += block.len();
                true
            };
            let mut observation = None;
            let expected =
                (quality != Quality::Original).then_some(symphonia::core::codecs::CODEC_TYPE_MP3);
            decode::decode_observed(
                source,
                &song.suffix,
                plan.decode_position,
                expected,
                |facts| observation = Some(facts),
                consume,
            )
            .unwrap();
            let observed = observation.unwrap();
            assert!(observed.sample_rate > 0 && observed.channels > 0);
            if quality != Quality::Original {
                assert_eq!(observed.codec, "mp3");
            }
            let seconds = count as f64 / (2.0 * f64::from(RATE));
            assert!(
                (seconds - (song.duration - position)).abs() < 0.15,
                "{quality:?} at {position}: {seconds} decoded seconds"
            );
        }
    }
}

#[test]
#[ignore = "requires a native audio output device; opens a paused, muted sink"]
fn paused_native_output_reports_quality_failure_without_playing() {
    use crate::navidrome::test_server::{Server, response};
    let server = Server::spawn(|request| {
        let body = if request.contains("getOpenSubsonicExtensions") {
            serde_json::json!({"status":"ok","openSubsonicExtensions":[{"name":"transcoding","versions":[1]}]})
        } else {
            serde_json::json!({"status":"ok","transcodeDecision":{"canTranscode":false,"sourceStream":{"protocol":"http","container":"flac","codec":"flac","audioChannels":2,"audioBitrate":900000}}})
        };
        response(
            "200 OK",
            "Content-Type: application/json\r\n",
            &serde_json::to_vec(&serde_json::json!({"subsonic-response":body})).unwrap(),
        )
    });
    let root = std::env::current_dir()
        .unwrap()
        .join(".cache")
        .join(format!("paused-output-test-{}", next_generation()));
    let dirs = crate::paths::AppDirs {
        config: root.join("config"),
        state: root.join("state"),
        cache: root.join("cache"),
    };
    let backend = super::super::backend::Backend::spawn(Arc::new(|| {}), dirs);
    let (send, receive) = mpsc::channel();
    let player = Player::spawn(
        crate::settings::ProxyConfig::Off,
        backend.stream_resolver(),
        crate::eq::shared(),
        crate::vis::AudioTap::new(),
        Arc::new(move |event| {
            let _ = send.send(event);
        }),
    );
    player.send(Command::Volume(0.0));
    let generation = next_generation();
    player.send(Command::Load {
        quality: Quality::Mp3Standard,
        generation,
        listening: Default::default(),
        client: Client::new(
            &server.url,
            "fixture",
            "fixture-only".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap(),
        song: Box::new(Song {
            id: "fixture".into(),
            duration: 40.0,
            ..Default::default()
        }),
        position: 0.0,
        paused: true,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let event = receive
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("paused failure must arrive without pressing play");
        match event {
            Event::Failed {
                generation: actual,
                message,
            } => {
                assert_eq!(actual, generation);
                assert!(message.contains("cannot provide MP3"), "{message}");
                break;
            }
            Event::Position { started, .. } => assert!(!started),
            _ => {}
        }
    }
    drop(player);
    drop(backend);
    if root.exists() {
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn inspector_observes_96khz_before_stereo_44100_processing() {
    let mut observed = Vec::new();
    let mut samples = 0;
    decode::decode_observed(
        Box::new(std::io::Cursor::new(wav(96_000, 2))),
        "wav",
        0.0,
        None,
        |facts| observed.push(facts),
        |block| {
            samples += block.len();
            true
        },
    )
    .unwrap();
    assert_eq!(
        observed.len(),
        1,
        "publish changes, never one event per audio block"
    );
    assert_eq!(observed[0].sample_rate, 96_000);
    assert_eq!(observed[0].channels, 2);
    assert_eq!(observed[0].bit_depth, Some(16));
    assert!(observed[0].codec.contains("pcm"));
    assert!((samples as i64 - i64::from(RATE) * 2).abs() < 200);
}

#[test]
fn downloaded_audio_decodes_and_seeks_without_any_network_or_output_device() {
    use crate::{
        navidrome::downloads::{
            audio_path,
            worker::{Available, Shared},
        },
        settings::ProxyConfig,
    };
    let root = std::env::temp_dir().join(format!(
        "navifast-local-player-{:016x}",
        rand::random::<u64>()
    ));
    std::fs::create_dir(&root).unwrap();
    let client = Client::offline("http://127.0.0.1:1", "listener", &ProxyConfig::Off).unwrap();
    let audio = wav(44_100, 2);
    let song = Song {
        id: "downloaded".into(),
        suffix: "wav".into(),
        ..Default::default()
    };
    let path = audio_path(&root, &song.id);
    std::fs::write(&path, &audio).unwrap();
    let available = Shared::default();
    *available.write().unwrap() = Available {
        scope: client.scope.clone(),
        root: root.clone(),
        songs: [(song.id.clone(), song.clone())].into(),
    };
    let (requests, mut received) = tokio::sync::mpsc::unbounded_channel();
    let resolver =
        super::super::backend::StreamResolver::new(requests, available.clone(), Default::default());
    for offset in [0., 0.1, 0.5] {
        let mut next = track(1, 1);
        let details = Arc::get_mut(&mut next).unwrap();
        details.song = song.clone();
        details.offset = offset;
        let (samples, receive) = mpsc::sync_channel(512);
        let job = DecodeJob {
            chain: 1,
            track: next,
            client: client.clone(),
            samples,
        };
        decode_job(
            &job,
            &ProxyConfig::Off,
            &Arc::new(AtomicU64::new(1)),
            &resolver,
            &|_| {},
        )
        .unwrap();
        drop(job);
        let actual: Vec<f32> = receive.into_iter().flat_map(Result::unwrap).collect();
        let expected: Vec<f32> = raw(audio.clone(), "wav", offset)
            .into_iter()
            .flatten()
            .collect();
        assert!(!actual.is_empty());
        assert_eq!(actual, expected);
    }
    assert!(
        received.try_recv().is_err(),
        "local audio never asks for a stream"
    );
    // An account mismatch cannot fall through to unauthenticated networking.
    available.write().unwrap().scope = "another-account".into();
    let (samples, _) = mpsc::sync_channel(1);
    let mut next = track(1, 1);
    Arc::get_mut(&mut next).unwrap().song = song;
    let job = DecodeJob {
        chain: 1,
        track: next,
        client,
        samples,
    };
    assert!(
        decode_job(
            &job,
            &ProxyConfig::Off,
            &Arc::new(AtomicU64::new(1)),
            &resolver,
            &|_| {}
        )
        .unwrap_err()
        .contains("not downloaded")
    );
    std::fs::remove_dir_all(root).unwrap();
}
