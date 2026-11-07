//! Local playback: a responsive output thread and a cancellable decoder worker.

use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};

use rodio::Source;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::{FormatOptions, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};

use super::api::{Client, Song};
use super::stream::HttpSource;

pub const RATE: u32 = 44_100;
// Use time, not codec packets: four high-rate packets can be less than 100 ms.
const PCM_BLOCK_SAMPLES: usize = RATE as usize / 50 * 2; // 20 ms stereo
const PCM_BUFFER_BLOCKS: usize = 25; // 500 ms on the decoder/output channel
const PCM_PREBUFFER_SAMPLES: usize = PCM_BLOCK_SAMPLES * 10; // 200 ms

pub enum Command {
    Load {
        generation: u64,
        listening: super::scrobble::Context,
        client: Client,
        song: Box<Song>,
        position: f64,
        paused: bool,
    },
    Scrobbling {
        epoch: u64,
        enabled: bool,
    },
    Pause(bool),
    Volume(f32),
    Stop,
    Shutdown,
    Failed {
        generation: u64,
        message: String,
    },
}

#[derive(Clone, Debug)]
pub enum Event {
    Scrobble {
        context: super::scrobble::Context,
        report: super::scrobble::Report,
    },
    Position {
        generation: u64,
        seconds: f64,
        paused: bool,
        started: bool,
    },
    Finished(u64),
    Failed {
        generation: u64,
        message: String,
    },
}

pub struct Player {
    commands: mpsc::Sender<Command>,
    current: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Player {
    pub fn spawn(
        proxy: crate::settings::ProxyConfig,
        eq: crate::eq::SharedEq,
        tap: Arc<crate::vis::AudioTap>,
        notify: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> Self {
        let (commands, receive) = mpsc::channel();
        let (jobs, work) = mpsc::channel::<DecodeJob>();
        let current = Arc::new(AtomicU64::new(0));
        let decoder_current = current.clone();
        let errors = commands.clone();
        std::thread::Builder::new()
            .name("navifast-decoder".into())
            .spawn(move || {
                while let Ok(mut job) = work.recv() {
                    // A burst of skips only decodes the latest request.
                    while let Ok(newest) = work.try_recv() {
                        job = newest;
                    }
                    if decoder_current.load(Ordering::Acquire) != job.generation {
                        continue;
                    }
                    let generation = job.generation;
                    let result = decode_job(job, &proxy, &decoder_current, &eq);
                    if let Err(message) = result
                        && decoder_current.load(Ordering::Acquire) == generation
                    {
                        let _ = errors.send(Command::Failed {
                            generation,
                            message,
                        });
                    }
                }
            })
            .expect("start decoder worker");
        let output_current = current.clone();
        let thread = std::thread::Builder::new()
            .name("navifast-output".into())
            .spawn(move || {
                output_loop(receive, jobs, output_current, tap, notify);
            })
            .expect("start output worker");
        Self {
            commands,
            current,
            thread: Some(thread),
        }
    }

    pub fn send(&self, command: Command) {
        match &command {
            Command::Load { generation, .. } => {
                self.current.store(*generation, Ordering::Release);
            }
            Command::Stop | Command::Shutdown => {
                self.current.fetch_add(1, Ordering::AcqRel);
            }
            _ => {}
        }
        let _ = self.commands.send(command);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // A blocked HTTP read has a deadline; it holds no output and cannot
        // keep window shutdown waiting for the network.
    }
}

struct DecodeJob {
    generation: u64,
    client: Client,
    song: Song,
    position: f64,
    samples: mpsc::SyncSender<Vec<f32>>,
    completed: Arc<std::sync::atomic::AtomicBool>,
}

struct Playing {
    generation: u64,
    offset: f64,
    samples: Arc<AtomicU64>,
    completed: Arc<std::sync::atomic::AtomicBool>,
    sink: rodio::Sink,
}

impl Playing {
    fn report_listening(&self, listening: &mut super::scrobble::Listening, notify: &dyn Fn(Event)) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        for report in listening
            .advance(self.samples.load(Ordering::Relaxed), now)
            .into_iter()
            .flatten()
        {
            notify(Event::Scrobble {
                context: listening.context,
                report,
            });
        }
    }
}

fn output_loop(
    receive: mpsc::Receiver<Command>,
    jobs: mpsc::Sender<DecodeJob>,
    current: Arc<AtomicU64>,
    tap: Arc<crate::vis::AudioTap>,
    notify: Arc<dyn Fn(Event) + Send + Sync>,
) {
    let mut output = None;
    let mut playing: Option<Playing> = None;
    let mut volume = 0.7;
    let mut reported = Instant::now();
    let mut listening = super::scrobble::Listening::default();
    loop {
        match receive.recv_timeout(Duration::from_millis(20)) {
            Ok(Command::Load {
                generation,
                listening: context,
                client,
                song,
                position,
                paused,
            }) => {
                if let Some(old) = playing.take() {
                    old.sink.stop();
                    old.report_listening(&mut listening, &*notify);
                }
                listening.begin(context, &song);
                if output.is_none() {
                    match rodio::OutputStreamBuilder::open_default_stream() {
                        Ok(mut stream) => {
                            stream.log_on_drop(false);
                            output = Some(stream);
                        }
                        Err(_) => {
                            notify(Event::Failed {
                                generation,
                            message: "No audio output device was found. Connect or enable one, then press play again.".into(),
                            });
                            continue;
                        }
                    }
                }
                let Some(stream) = &output else {
                    continue;
                };
                tap.clear();
                let sink = rodio::Sink::connect_new(stream.mixer());
                sink.set_volume(volume);
                if paused {
                    sink.pause();
                }
                let (send, audio) = mpsc::sync_channel(PCM_BUFFER_BLOCKS);
                let samples = Arc::new(AtomicU64::new(0));
                let completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
                sink.append(PcmSource::new(audio, samples.clone(), tap.clone()));
                let _ = jobs.send(DecodeJob {
                    generation,
                    client,
                    song: *song,
                    position,
                    samples: send,
                    completed: completed.clone(),
                });
                playing = Some(Playing {
                    generation,
                    offset: position,
                    samples,
                    completed,
                    sink,
                });
            }
            Ok(Command::Scrobbling { epoch, enabled }) => {
                if let Some(play) = &playing {
                    play.report_listening(&mut listening, &*notify);
                }
                listening.configure(epoch, enabled);
            }
            Ok(Command::Pause(paused)) => {
                if let Some(play) = &playing {
                    if paused {
                        play.sink.pause();
                    } else {
                        play.sink.play();
                    }
                }
            }
            Ok(Command::Volume(value)) => {
                volume = value.clamp(0.0, 1.0);
                if let Some(play) = &playing {
                    play.sink.set_volume(volume);
                }
            }
            Ok(Command::Stop) => {
                if let Some(play) = playing.take() {
                    play.sink.stop();
                    play.report_listening(&mut listening, &*notify);
                }
            }
            Ok(Command::Failed {
                generation,
                message,
            }) => {
                if current.load(Ordering::Acquire) == generation {
                    if let Some(play) = playing.take() {
                        play.sink.stop();
                        play.report_listening(&mut listening, &*notify);
                    }
                    notify(Event::Failed {
                        generation,
                        message,
                    });
                }
            }
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        if let Some(play) = &playing {
            if play.sink.empty() && play.completed.load(Ordering::Acquire) {
                play.report_listening(&mut listening, &*notify);
                let generation = play.generation;
                playing = None;
                notify(Event::Finished(generation));
            } else if reported.elapsed() >= Duration::from_millis(100) {
                play.report_listening(&mut listening, &*notify);
                let samples = play.samples.load(Ordering::Relaxed);
                notify(Event::Position {
                    generation: play.generation,
                    seconds: play.offset + samples as f64 / (2.0 * f64::from(RATE)),
                    paused: play.sink.is_paused(),
                    started: samples > 0,
                });
                reported = Instant::now();
            }
        }
    }
    if let Some(play) = playing {
        play.sink.stop();
        play.report_listening(&mut listening, &*notify);
    }
}

struct PcmSource {
    receive: mpsc::Receiver<Vec<f32>>,
    block: std::vec::IntoIter<f32>,
    samples: Arc<AtomicU64>,
    tap: Arc<crate::vis::AudioTap>,
    silence_remaining: bool,
    pending: VecDeque<Vec<f32>>,
    buffered_samples: usize,
    buffering: bool,
    ended: bool,
}

impl PcmSource {
    fn new(
        receive: mpsc::Receiver<Vec<f32>>,
        samples: Arc<AtomicU64>,
        tap: Arc<crate::vis::AudioTap>,
    ) -> Self {
        Self {
            receive,
            block: Vec::new().into_iter(),
            samples,
            tap,
            silence_remaining: false,
            pending: VecDeque::with_capacity(PCM_PREBUFFER_SAMPLES / PCM_BLOCK_SAMPLES),
            buffered_samples: 0,
            buffering: true,
            ended: false,
        }
    }

    fn next_block(&mut self) -> Option<Vec<f32>> {
        // Prime at startup and after an underrun. Never alternate a tiny new
        // packet with silence while the network is still catching up. EOF must
        // drain even a short track or seek tail below the normal watermark.
        while !self.ended
            && (self.pending.is_empty()
                || (self.buffering && self.buffered_samples < PCM_PREBUFFER_SAMPLES))
        {
            match self.receive.try_recv() {
                Ok(block) => {
                    self.buffered_samples += block.len();
                    self.pending.push_back(block);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => self.ended = true,
            }
        }
        if self.buffering && self.buffered_samples < PCM_PREBUFFER_SAMPLES && !self.ended {
            return None;
        }
        let block = self.pending.pop_front()?;
        self.buffered_samples -= block.len();
        self.buffering = false;
        Some(block)
    }
}

impl Iterator for PcmSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.silence_remaining {
            self.silence_remaining = false;
            return Some(0.0);
        }
        if self.block.len() == 0 {
            match self.next_block() {
                Some(block) => {
                    // Tap only as the device consumes a block, never during prefetch.
                    // Sink applies volume afterwards, so mute still visualizes.
                    self.tap.push(
                        &block.iter().map(|s| f64::from(*s)).collect::<Vec<_>>(),
                        1.0,
                    );
                    self.block = block.into_iter();
                }
                // Audio callbacks must never wait for HTTP or decoding.
                None if !self.ended => {
                    // Buffering emits whole stereo frames, even if a block
                    // arrives between callbacks for the left/right channels.
                    self.silence_remaining = true;
                    self.buffering = true;
                    return Some(0.0);
                }
                None => return None,
            }
        }
        let sample = self.block.next()?;
        self.samples.fetch_add(1, Ordering::Relaxed);
        Some(sample)
    }
}

impl Source for PcmSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

fn decode_job(
    job: DecodeJob,
    proxy: &crate::settings::ProxyConfig,
    current: &Arc<AtomicU64>,
    eq: &crate::eq::SharedEq,
) -> Result<(), String> {
    let http = Client::stream_client(proxy)?;
    let source = HttpSource::open(
        job.client.clone(),
        http,
        job.song.id.clone(),
        current.clone(),
        job.generation,
    )
    .map_err(|e| e.to_string())?;
    decode(
        Box::new(source),
        &job.song.suffix,
        job.position,
        eq,
        |mut samples| {
            if current.load(Ordering::Acquire) != job.generation {
                return false;
            }
            loop {
                match job.samples.try_send(samples) {
                    Ok(()) => return true,
                    Err(mpsc::TrySendError::Disconnected(_)) => return false,
                    Err(mpsc::TrySendError::Full(held)) => {
                        if current.load(Ordering::Acquire) != job.generation {
                            return false;
                        }
                        samples = held;
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            }
        },
    )?;
    job.completed.store(true, Ordering::Release);
    Ok(())
}

fn decode(
    source: Box<dyn MediaSource>,
    suffix: &str,
    position: f64,
    shared: &crate::eq::SharedEq,
    mut consume: impl FnMut(Vec<f32>) -> bool,
) -> Result<(), String> {
    let mut hint = Hint::new();
    if !suffix.is_empty() {
        hint.with_extension(suffix);
    }
    let stream = MediaSourceStream::new(source, Default::default());
    let mut format = symphonia::default::get_probe().format(&hint, stream, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map_err(|_| "This audio format could not be opened. Stage 1 supports MP3, FLAC, WAV, AAC/ALAC, AIFF and Vorbis originals.".to_owned())?.format;
    let track = format.default_track().ok_or("No audio track was found.")?;
    let track_id = track.id;
    let time_base = track.codec_params.time_base;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| "This audio codec is not supported for original-file playback.".to_owned())?;
    if position > 0.0 {
        if time_base.is_none() {
            return Err("This file does not provide timestamps needed for seeking.".into());
        }
        format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(position),
                    track_id: Some(track_id),
                },
            )
            .map_err(|_| {
                "The decoder could not seek in this file. Press Play to retry.".to_owned()
            })?;
        decoder.reset();
    }
    let mut eq = crate::eq::Processor::new(shared.clone());
    let mut resampler = None;
    let mut source_rate = 0;
    let mut limiter = crate::limiter::Limiter::new(f64::from(RATE));
    let mut priming = limiter.delayed_frames() * 2;
    let mut pending = VecDeque::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(Error::IoError(error)) => return Err(error.to_string()),
            Err(_) => return Err("The audio file could not be read.".into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|_| "The audio file contains unsupported or damaged data.".to_owned())?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        if channels == 0 || channels > 2 {
            return Err("Stage 1 supports mono and stereo audio. Multichannel conversion is not available yet.".into());
        }
        if source_rate != spec.rate {
            source_rate = spec.rate;
            resampler = crate::resample::Resampler::new(spec.rate, RATE, 2);
        }
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        let start = time_base
            .map(|base| base.calc_time(packet.ts()))
            .map(|t| t.seconds as f64 + t.frac)
            .unwrap_or(0.0);
        let skip = if position > start {
            ((position - start) * f64::from(spec.rate)).round() as usize
        } else {
            0
        };
        let mut stereo: Vec<f32> = buffer
            .samples()
            .chunks_exact(channels)
            .skip(skip)
            .flat_map(|frame| [frame[0], frame[channels - 1]])
            .collect();
        if let Some(resampler) = &mut resampler {
            stereo = resampler.process(&stereo);
        }
        if stereo.is_empty() {
            continue;
        }
        let mut samples: Vec<f64> = stereo.into_iter().map(f64::from).collect();
        eq.process(&mut samples);
        // Limiting protects the device; volume is applied by rodio after this.
        limiter.process(&mut samples, 1.0);
        let skip = priming.min(samples.len());
        samples.drain(..skip);
        priming -= skip;
        if samples.is_empty() {
            continue;
        }
        pending.extend(samples.into_iter().map(|s| s as f32));
        while pending.len() >= PCM_BLOCK_SAMPLES {
            if !consume(pending.drain(..PCM_BLOCK_SAMPLES).collect()) {
                return Ok(());
            }
        }
    }
    // Remove the limiter's initial silence and preserve the last real frames.
    let mut tail = vec![0.0; limiter.delayed_frames() * 2];
    limiter.process(&mut tail, 1.0);
    if priming < tail.len() {
        pending.extend(tail[priming..].iter().map(|s| *s as f32));
    }
    while !pending.is_empty() {
        let count = pending.len().min(PCM_BLOCK_SAMPLES);
        if !consume(pending.drain(..count).collect()) {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffering_silence_does_not_advance_the_playback_clock() {
        let (send, receive) = mpsc::sync_channel(1);
        let samples = Arc::new(AtomicU64::new(0));
        let mut source = PcmSource::new(receive, samples.clone(), crate::vis::AudioTap::new());
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(samples.load(Ordering::Relaxed), 0);
        send.send(vec![0.25, 0.5]).unwrap();
        drop(send);
        assert_eq!(source.next(), Some(0.0), "complete the silent stereo frame");
        assert_eq!(source.next(), Some(0.25));
        assert_eq!(source.next(), Some(0.5));
        assert_eq!(samples.load(Ordering::Relaxed), 2);
        assert_eq!(source.next(), None);
    }

    #[test]
    fn playback_primes_a_time_reserve_and_rebuffers_after_an_underrun() {
        let (send, receive) = mpsc::sync_channel(PCM_BUFFER_BLOCKS);
        let samples = Arc::new(AtomicU64::new(0));
        let tap = crate::vis::AudioTap::new();
        let mut source = PcmSource::new(receive, samples.clone(), tap.clone());
        let block = || vec![0.25; PCM_BLOCK_SAMPLES];
        for _ in 0..9 {
            send.send(block()).unwrap();
            assert_eq!(source.next(), Some(0.0));
            assert_eq!(source.next(), Some(0.0));
        }
        assert_eq!(samples.load(Ordering::Relaxed), 0);
        assert!(tap.window(512, 0).iter().all(|s| *s == 0.0));
        send.send(block()).unwrap();
        // No producer activity for the next 200 ms of output. This reserve is
        // independent of the original sample rate and the decoder's packet size.
        for _ in 0..PCM_PREBUFFER_SAMPLES {
            assert_eq!(source.next(), Some(0.25));
        }
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(0.0));
        let consumed = samples.load(Ordering::Relaxed);
        send.send(block()).unwrap();
        assert_eq!(
            source.next(),
            Some(0.0),
            "one packet must not restart stuttering playback"
        );
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(samples.load(Ordering::Relaxed), consumed);
        for _ in 0..9 {
            send.send(block()).unwrap();
        }
        for _ in 0..PCM_PREBUFFER_SAMPLES {
            assert_eq!(source.next(), Some(0.25));
        }
        assert_eq!(
            samples.load(Ordering::Relaxed),
            (PCM_PREBUFFER_SAMPLES * 2) as u64
        );
        // A short final tail still drains when EOF arrives below the watermark.
        assert_eq!(source.next(), Some(0.0));
        send.send(vec![0.5, 0.75]).unwrap();
        drop(send);
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(source.next(), Some(0.5));
        assert_eq!(source.next(), Some(0.75));
        assert_eq!(source.next(), None);
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
            let sample = ((f64::from(index) * 440.0 * std::f64::consts::TAU / f64::from(rate))
                .sin()
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
        let (send, receive) = mpsc::sync_channel(1);
        let tap = crate::vis::AudioTap::new();
        send.send(quiet.clone()).unwrap();
        drop(send);
        let source = PcmSource::new(receive, Arc::new(AtomicU64::new(0)), tap.clone());
        assert!(source.amplify(0.0).all(|s| s == 0.0));
        assert!(tap.window(512, 0).iter().any(|s| s.abs() > 0.02));
    }

    #[test]
    fn decoder_stops_when_consumer_cancels() {
        let eq = Arc::new(std::sync::Mutex::new(Default::default()));
        let mut blocks = 0;
        decode(
            Box::new(std::io::Cursor::new(wav(RATE, 2))),
            "wav",
            0.0,
            &eq,
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
            assert!(
                (count as f64 / (2.0 * f64::from(RATE)) - (song.duration - position)).abs() < 0.2
            );
        }
    }
}
