//! Bounded decoding workers and one continuous device source for queued tracks.
use super::api::{Client, Song};
use super::quality::Quality;
use super::stream::HttpSource;
use rodio::Source;
use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
    mpsc,
};
use std::time::{Duration, Instant};
mod decode;
mod pipeline;
use pipeline::{Boundary, ContinuousSource, Input, Preparation, Track};

/// Unique across explicit loads and reserved natural handoffs. A seek cannot
/// accidentally reuse the token of a prepared occurrence.
pub(crate) fn next_generation() -> u64 {
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    SERIAL.fetch_add(1, Ordering::Relaxed)
}

pub const RATE: u32 = 44_100;
const PCM_BLOCK_SAMPLES: usize = RATE as usize / 50 * 2;
const PCM_BUFFER_BLOCKS: usize = 25;
const PCM_PREBUFFER_SAMPLES: usize = PCM_BLOCK_SAMPLES * 10;

pub enum Command {
    Load {
        quality: Quality,
        generation: u64,
        listening: super::scrobble::Context,
        client: Client,
        song: Box<Song>,
        position: f64,
        paused: bool,
    },
    Prepare {
        quality: Quality,
        from: u64,
        generation: u64,
        occurrence: u64,
        listening: super::scrobble::Context,
        client: Client,
        song: Box<Song>,
    },
    DecodeFailed {
        generation: u64,
        message: String,
    },
    CancelPreparation,
    Scrobbling {
        epoch: u64,
        enabled: bool,
    },
    Pause(bool),
    Volume(f32),
    Stop,
    Shutdown,
}
#[derive(Clone, Debug)]
pub enum Event {
    Information {
        generation: u64,
        info: super::audio::Playback,
    },
    Scrobble {
        context: super::scrobble::Context,
        report: super::scrobble::Report,
    },
    Ready {
        generation: u64,
        seekable: bool,
    },
    Transition {
        information: Option<super::audio::Playback>,
        quality: Quality,
        seekable: bool,
        from: u64,
        generation: u64,
        occurrence: u64,
        listening: super::scrobble::Context,
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
    commands: mpsc::Sender<(u64, Command)>,
    current: Arc<AtomicU64>,
    preparation: Arc<Preparation>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Player {
    pub(crate) fn spawn(
        proxy: crate::settings::ProxyConfig,
        resolver: super::backend::StreamResolver,
        eq: crate::eq::SharedEq,
        tap: Arc<crate::vis::AudioTap>,
        notify: Arc<dyn Fn(Event) + Send + Sync>,
    ) -> Self {
        let (commands, receive) = mpsc::channel();
        let current = Arc::new(AtomicU64::new(0));
        let preparation = Arc::new(Preparation::default());
        // Two fixed workers: one playing occurrence and at most one candidate.
        // A slow cancelled HTTP request cannot create an unbounded worker pool.
        let jobs = std::array::from_fn(|worker| {
            let (send, work) = mpsc::channel::<DecodeJob>();
            let failures = commands.clone();
            let proxy = proxy.clone();
            let resolver = resolver.clone();
            let notify = notify.clone();
            let current = current.clone();
            std::thread::Builder::new()
                .name(format!("navifast-decoder-{worker}"))
                .spawn(move || {
                    while let Ok(mut job) = work.recv() {
                        while let Ok(newest) = work.try_recv() {
                            job = newest;
                        }
                        if !job.valid(&current) {
                            continue;
                        }
                        if let Err(message) =
                            decode_job(&job, &proxy, &current, &resolver, &*notify)
                            && job.valid(&current)
                        {
                            // A paused current occurrence is not pulling PCM.
                            // Surface its failure without waiting for Play. A
                            // future occurrence keeps its error at the boundary.
                            let _ = failures.send((
                                0,
                                Command::DecodeFailed {
                                    generation: job.track.generation,
                                    message: message.clone(),
                                },
                            ));
                            job.send(Err(message), &current);
                        }
                    }
                })
                .expect("start decoder worker");
            send
        });
        let output_current = current.clone();
        let output_preparation = preparation.clone();
        let thread = std::thread::Builder::new()
            .name("navifast-output".into())
            .spawn(move || {
                output_loop(
                    receive,
                    jobs,
                    output_current,
                    output_preparation,
                    eq,
                    tap,
                    notify,
                );
            })
            .expect("start output worker");
        Self {
            commands,
            current,
            preparation,
            thread: Some(thread),
        }
    }
    pub fn send(&self, command: Command) {
        let invalidate = matches!(
            command,
            Command::Prepare { .. }
                | Command::CancelPreparation
                | Command::Load { .. }
                | Command::Pause(true)
                | Command::Stop
                | Command::Shutdown
        );
        let revision = if invalidate {
            self.preparation.cancel()
        } else {
            self.preparation.revision.load(Ordering::Acquire)
        };
        match &command {
            Command::Load { generation, .. } => {
                self.current.store(*generation, Ordering::Release);
            }
            Command::Stop | Command::Shutdown => {
                self.current.store(0, Ordering::Release);
            }
            _ => {}
        }
        let _ = self.commands.send((revision, command));
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
struct DecodeJob {
    chain: u64,
    track: Arc<Track>,
    client: Client,
    samples: mpsc::SyncSender<Result<Vec<f32>, String>>,
}
impl DecodeJob {
    fn valid(&self, current: &AtomicU64) -> bool {
        current.load(Ordering::Acquire) == self.chain
            && self.track.gate.load(Ordering::Acquire) != 0
    }
    fn send(&self, mut block: Result<Vec<f32>, String>, current: &AtomicU64) -> bool {
        while self.valid(current) {
            match self.samples.try_send(block) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Disconnected(_)) => break,
                Err(mpsc::TrySendError::Full(held)) => {
                    block = held;
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
        false
    }
}
fn decode_job(
    job: &DecodeJob,
    proxy: &crate::settings::ProxyConfig,
    current: &Arc<AtomicU64>,
    resolver: &super::backend::StreamResolver,
    notify: &dyn Fn(Event),
) -> Result<(), String> {
    let plan = if job.track.quality == Quality::Original {
        super::quality::Plan {
            url: job.client.stream_url(&job.track.song.id),
            ranged: true,
            seekable: true,
            decode_position: job.track.offset,
        }
    } else {
        let mut result = resolver.request(
            job.client.clone(),
            job.track.song.id.clone(),
            job.track.quality,
            job.track.offset,
        );
        loop {
            if !job.valid(current) {
                return Ok(());
            }
            match result.try_recv() {
                Ok(plan) => break plan?,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    return Err(
                        "Streaming preparation was interrupted. Press Play to retry.".into(),
                    );
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(5))
                }
            }
        }
    };
    let http = Client::stream_client(proxy)?;
    let source: Box<dyn symphonia::core::io::MediaSource> = if plan.ranged {
        Box::new(
            HttpSource::open_url(
                http,
                plan.url,
                current.clone(),
                job.chain,
                Some(job.track.gate.clone()),
            )
            .map_err(|e| e.to_string())?,
        )
    } else {
        Box::new(
            super::stream::forward::ForwardSource::open(
                http,
                plan.url,
                current.clone(),
                job.chain,
                Some(job.track.gate.clone()),
            )
            .map_err(|e| e.to_string())?,
        )
    };
    let mut ready = false;
    let consume = |block| {
        if !ready {
            job.track.seekable.store(plan.seekable, Ordering::Release);
            notify(Event::Ready {
                generation: job.track.generation,
                seekable: plan.seekable,
            });
            ready = true;
        }
        job.send(Ok(block), current)
    };
    let expected =
        (job.track.quality != Quality::Original).then_some(symphonia::core::codecs::CODEC_TYPE_MP3);
    let observe = |decoded| {
        *job.track.decoded.lock().unwrap_or_else(|p| p.into_inner()) = Some(decoded);
        if let Some(info) = job.track.information() {
            notify(Event::Information {
                generation: job.track.generation,
                info,
            });
        }
    };
    decode::decode_observed(
        source,
        if expected.is_some() {
            "mp3"
        } else {
            &job.track.song.suffix
        },
        plan.decode_position,
        expected,
        observe,
        consume,
    )
    .map_err(|message| {
        if let Some(bitrate) = job.track.quality.kbps() {
            format!("MP3 at up to {bitrate} kbps could not play: {message}")
        } else {
            message
        }
    })
}

struct Playing {
    track: Arc<Track>,
    chain: u64,
    sink: rodio::Sink,
    boundaries: mpsc::Receiver<Boundary>,
    failed: bool,
}
fn report(track: &Track, listening: &mut super::scrobble::Listening, notify: &dyn Fn(Event)) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64;
    for report in listening
        .advance(track.samples.load(Ordering::Relaxed), now)
        .into_iter()
        .flatten()
    {
        notify(Event::Scrobble {
            context: listening.context,
            report,
        });
    }
}
fn transition(
    play: &mut Playing,
    track: Arc<Track>,
    listening: &mut super::scrobble::Listening,
    notify: &dyn Fn(Event),
) {
    report(&play.track, listening, notify);
    let mut context = track.context;
    if listening.context.epoch > context.epoch {
        context.epoch = listening.context.epoch;
        context.enabled = listening.context.enabled;
    }
    listening.begin(context, &track.song);
    notify(Event::Transition {
        information: track.information(),
        quality: track.quality,
        seekable: track.seekable.load(Ordering::Acquire),
        from: track.from,
        generation: track.generation,
        occurrence: track.occurrence,
        listening: track.context,
    });
    play.track = track;
}
fn drain_boundaries(
    play: &mut Playing,
    listening: &mut super::scrobble::Listening,
    notify: &dyn Fn(Event),
) {
    while let Ok(boundary) = play.boundaries.try_recv() {
        match boundary {
            Boundary::Started(track) => transition(play, track, listening, notify),
            Boundary::Failed(track, message) => {
                if track.generation != play.track.generation {
                    transition(play, track.clone(), listening, notify);
                }
                play.failed = true;
                notify(Event::Failed {
                    generation: track.generation,
                    message,
                });
            }
        }
    }
}

fn output_loop(
    receive: mpsc::Receiver<(u64, Command)>,
    jobs: [mpsc::Sender<DecodeJob>; 2],
    current: Arc<AtomicU64>,
    preparation: Arc<Preparation>,
    eq: crate::eq::SharedEq,
    tap: Arc<crate::vis::AudioTap>,
    notify: Arc<dyn Fn(Event) + Send + Sync>,
) {
    let mut output = None;
    let mut playing: Option<Playing> = None;
    let mut volume = 0.7;
    let mut reported = Instant::now();
    let mut listening = super::scrobble::Listening::default();
    loop {
        if let Some(play) = &mut playing {
            drain_boundaries(play, &mut listening, &*notify);
            if play.sink.empty() {
                // A callback may finish between the first drain and empty().
                // Its final boundary was sent before Sink published completion.
                drain_boundaries(play, &mut listening, &*notify);
                report(&play.track, &mut listening, &*notify);
                if !play.failed && current.load(Ordering::Acquire) == play.chain {
                    notify(Event::Finished(play.track.generation));
                }
                playing = None;
            } else if reported.elapsed() >= Duration::from_millis(100) {
                report(&play.track, &mut listening, &*notify);
                let samples = play.track.samples.load(Ordering::Relaxed);
                notify(Event::Position {
                    generation: play.track.generation,
                    seconds: play.track.offset + samples as f64 / (2.0 * f64::from(RATE)),
                    paused: play.sink.is_paused(),
                    started: samples > 0,
                });
                reported = Instant::now();
            }
        }
        match receive.recv_timeout(Duration::from_millis(20)) {
            Ok((
                _,
                Command::Load {
                    quality,
                    generation,
                    listening: context,
                    client,
                    song,
                    position,
                    paused,
                },
            )) => {
                if current.load(Ordering::Acquire) != generation {
                    continue;
                }
                if let Some(old) = playing.take() {
                    old.sink.stop();
                    report(&old.track, &mut listening, &*notify);
                }
                listening.begin(context, &song);
                if output.is_none() {
                    match rodio::OutputStreamBuilder::open_default_stream() {
                        Ok(mut stream) => {
                            stream.log_on_drop(false);
                            output = Some(stream);
                        }
                        Err(_) => {
                            notify(Event::Failed { generation, message: "No audio output device was found. Connect or enable one, then press play again.".into() });
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
                let track = Arc::new(Track {
                    decoded: Default::default(),
                    output: super::audio::Output::configured(stream),
                    quality,
                    seekable: std::sync::atomic::AtomicBool::new(false),
                    generation,
                    from: generation,
                    occurrence: 0,
                    context,
                    song: *song,
                    offset: position,
                    samples: AtomicU64::new(0),
                    gate: Arc::new(std::sync::atomic::AtomicU8::new(2)),
                    worker: 0,
                });
                let (send, audio) = mpsc::sync_channel(PCM_BUFFER_BLOCKS);
                let (boundary, boundaries) = mpsc::channel();
                sink.append(ContinuousSource::new(
                    Input::new(track.clone(), audio),
                    preparation.clone(),
                    current.clone(),
                    boundary,
                    eq.clone(),
                    tap.clone(),
                ));
                let _ = jobs[0].send(DecodeJob {
                    chain: generation,
                    track: track.clone(),
                    client,
                    samples: send,
                });
                playing = Some(Playing {
                    track,
                    chain: generation,
                    sink,
                    boundaries,
                    failed: false,
                });
            }
            Ok((
                revision,
                Command::Prepare {
                    quality,
                    from,
                    generation,
                    occurrence,
                    listening: context,
                    client,
                    song,
                },
            )) => {
                let Some(play) = &playing else {
                    continue;
                };
                if play.track.generation != from || play.sink.is_paused() {
                    continue;
                }
                let track = Arc::new(Track {
                    decoded: Default::default(),
                    output: play.track.output.clone(),
                    quality,
                    seekable: std::sync::atomic::AtomicBool::new(false),
                    generation,
                    from,
                    occurrence,
                    context,
                    song: *song,
                    offset: 0.0,
                    samples: AtomicU64::new(0),
                    gate: Arc::new(std::sync::atomic::AtomicU8::new(1)),
                    worker: 1 - play.track.worker,
                });
                let (send, audio) = mpsc::sync_channel(PCM_BUFFER_BLOCKS);
                if preparation.put(revision, Input::new(track.clone(), audio)) {
                    let _ = jobs[track.worker].send(DecodeJob {
                        chain: play.chain,
                        track,
                        client,
                        samples: send,
                    });
                }
            }
            Ok((
                _,
                Command::DecodeFailed {
                    generation,
                    message,
                },
            )) => {
                if playing.as_ref().is_some_and(|play| {
                    play.track.generation == generation
                        && !play.failed
                        && current.load(Ordering::Acquire) == play.chain
                }) {
                    let play = playing.take().expect("matched active playback");
                    preparation.cancel();
                    play.sink.stop();
                    report(&play.track, &mut listening, &*notify);
                    notify(Event::Failed {
                        generation,
                        message,
                    });
                }
            }
            Ok((_, Command::CancelPreparation)) => {}
            Ok((_, Command::Scrobbling { epoch, enabled })) => {
                if let Some(play) = &playing {
                    report(&play.track, &mut listening, &*notify);
                }
                listening.configure(epoch, enabled);
            }
            Ok((_, Command::Pause(paused))) => {
                if let Some(play) = &playing {
                    if paused {
                        play.sink.pause();
                    } else {
                        play.sink.play();
                    }
                }
            }
            Ok((_, Command::Volume(value))) => {
                volume = value.clamp(0.0, 1.0);
                if let Some(play) = &playing {
                    play.sink.set_volume(volume);
                }
            }
            Ok((_, Command::Stop)) => {
                if let Some(play) = playing.take() {
                    play.sink.stop();
                    report(&play.track, &mut listening, &*notify);
                }
            }
            Ok((_, Command::Shutdown)) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
    if let Some(play) = playing {
        play.sink.stop();
        report(&play.track, &mut listening, &*notify);
    }
}

#[cfg(test)]
mod tests;
