//! One device source across track boundaries, including continuous EQ and limiting.
use super::*;
use std::sync::{Mutex, atomic::AtomicU8};

const CANCELLED: u8 = 0;
const PREPARED: u8 = 1;
const ACTIVE: u8 = 2;

pub(super) struct Track {
    pub generation: u64,
    pub from: u64,
    pub occurrence: u64,
    pub context: super::super::scrobble::Context,
    pub song: Song,
    pub offset: f64,
    pub samples: AtomicU64,
    pub gate: Arc<AtomicU8>,
    pub worker: usize,
}

impl Track {
    pub fn cancel_prepared(&self) {
        let _ =
            self.gate
                .compare_exchange(PREPARED, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
    }
    fn activate(&self) -> bool {
        self.gate
            .compare_exchange(PREPARED, ACTIVE, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            || self.gate.load(Ordering::Acquire) == ACTIVE
    }
}

#[derive(Default)]
pub(super) struct Preparation {
    pub revision: AtomicU64,
    gate: Mutex<Option<Arc<Track>>>,
    input: Mutex<Option<Input>>,
}

impl Preparation {
    pub fn cancel(&self) -> u64 {
        let revision = self.revision.fetch_add(1, Ordering::AcqRel) + 1;
        if let Some(track) = self.gate.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            track.cancel_prepared();
        }
        revision
    }
    pub fn put(&self, revision: u64, input: Input) -> bool {
        let mut gate = self.gate.lock().unwrap_or_else(|p| p.into_inner());
        if self.revision.load(Ordering::Acquire) != revision {
            input.track.cancel_prepared();
            return false;
        }
        *gate = Some(input.track.clone());
        *self.input.lock().unwrap_or_else(|p| p.into_inner()) = Some(input);
        true
    }
}

pub(super) struct Input {
    pub track: Arc<Track>,
    pub receive: mpsc::Receiver<Result<Vec<f32>, String>>,
    pending: VecDeque<Vec<f32>>,
    buffered: usize,
    buffering: bool,
    ended: bool,
    error: Option<String>,
}

impl Input {
    pub fn new(track: Arc<Track>, receive: mpsc::Receiver<Result<Vec<f32>, String>>) -> Self {
        Self {
            track,
            receive,
            pending: VecDeque::new(),
            buffered: 0,
            buffering: true,
            ended: false,
            error: None,
        }
    }
    fn block(&mut self) -> Option<Vec<f32>> {
        while !self.ended
            && (self.pending.is_empty() || self.buffering && self.buffered < PCM_PREBUFFER_SAMPLES)
        {
            match self.receive.try_recv() {
                Ok(Ok(block)) => {
                    self.buffered += block.len();
                    self.pending.push_back(block);
                }
                Ok(Err(message)) => {
                    self.error = Some(message);
                    self.ended = true;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => self.ended = true,
            }
        }
        if self.buffering && self.buffered < PCM_PREBUFFER_SAMPLES && !self.ended {
            return None;
        }
        let block = self.pending.pop_front()?;
        self.buffered -= block.len();
        self.buffering = false;
        Some(block)
    }
}

pub(super) enum Boundary {
    Started(Arc<Track>),
    Failed(Arc<Track>, String),
}

struct Run {
    track: Arc<Track>,
    count: usize,
}

pub(super) struct ContinuousSource {
    input: Input,
    preparation: Arc<Preparation>,
    current: Arc<AtomicU64>,
    chain: u64,
    boundary: mpsc::Sender<Boundary>,
    tap: Arc<crate::vis::AudioTap>,
    eq: crate::eq::Processor,
    limiter: crate::limiter::Limiter,
    priming: usize,
    runs: VecDeque<Run>,
    output: std::vec::IntoIter<f64>,
    audible: u64,
    tapped: Vec<f64>,
    silence_remaining: bool,
    drained: bool,
    failed: bool,
}

impl ContinuousSource {
    pub fn new(
        input: Input,
        preparation: Arc<Preparation>,
        current: Arc<AtomicU64>,
        boundary: mpsc::Sender<Boundary>,
        eq: crate::eq::SharedEq,
        tap: Arc<crate::vis::AudioTap>,
    ) -> Self {
        let limiter = crate::limiter::Limiter::new(f64::from(RATE));
        Self {
            tapped: Vec::with_capacity(PCM_BLOCK_SAMPLES),
            chain: input.track.generation,
            audible: input.track.generation,
            input,
            preparation,
            current,
            boundary,
            tap,
            eq: crate::eq::Processor::new(eq),
            priming: limiter.delayed_frames() * 2,
            limiter,
            runs: VecDeque::new(),
            output: Vec::new().into_iter(),
            silence_remaining: false,
            drained: false,
            failed: false,
        }
    }
    fn process(&mut self, block: Vec<f32>) {
        if let Some(run) = self
            .runs
            .back_mut()
            .filter(|r| r.track.generation == self.input.track.generation)
        {
            run.count += block.len();
        } else {
            self.runs.push_back(Run {
                track: self.input.track.clone(),
                count: block.len(),
            });
        }
        let mut samples: Vec<f64> = block.into_iter().map(f64::from).collect();
        self.eq.process(&mut samples);
        self.limit(samples);
    }
    fn limit(&mut self, mut samples: Vec<f64>) {
        self.limiter.process(&mut samples, 1.0);
        let skip = self.priming.min(samples.len());
        self.priming -= skip;
        self.output = samples.into_iter();
        if skip > 0 {
            self.output.nth(skip - 1);
        }
    }
    fn fill(&mut self) -> bool {
        loop {
            if let Some(block) = self.input.block() {
                self.process(block);
                if self.output.len() != 0 {
                    return true;
                }
                continue;
            }
            if !self.input.ended {
                self.input.buffering = true;
                if self
                    .runs
                    .back()
                    .is_some_and(|run| run.track.generation != self.input.track.generation)
                {
                    // A late successor must not hold back the current track's
                    // final lookahead frames. Drain them, then remove only the
                    // new priming silence when the successor eventually arrives.
                    let latency = self.limiter.delayed_frames() * 2;
                    self.limit(vec![0.0; latency]);
                    self.priming = latency;
                    return self.output.len() != 0;
                }
                return false;
            }
            if self.input.error.is_none() {
                // Never wait for the UI thread in the device callback. Try again
                // next frame if a new candidate is being installed.
                let Ok(mut next) = self.preparation.input.try_lock() else {
                    return false;
                };
                if let Some(candidate) = next.take()
                    && candidate.track.from == self.input.track.generation
                    && candidate.track.gate.load(Ordering::Acquire) == PREPARED
                {
                    self.input = candidate;
                    continue;
                }
            }
            self.drained = true;
            let tail = vec![0.0; self.limiter.delayed_frames() * 2];
            self.limit(tail);
            return self.output.len() != 0;
        }
    }
}

impl Iterator for ContinuousSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.current.load(Ordering::Acquire) != self.chain {
            return None;
        }
        if self.silence_remaining {
            self.silence_remaining = false;
            return Some(0.0);
        }
        if self.output.len() == 0 && !self.drained && !self.fill() && !self.drained {
            self.silence_remaining = true;
            return Some(0.0);
        }
        let Some(run) = self.runs.front_mut() else {
            if !self.tapped.is_empty() {
                self.tap.push(&self.tapped, 1.0);
                self.tapped.clear();
            }
            if !self.failed
                && self.input.track.activate()
                && let Some(message) = self.input.error.take()
            {
                self.failed = true;
                let _ = self
                    .boundary
                    .send(Boundary::Failed(self.input.track.clone(), message));
            }
            return None;
        };
        if run.track.generation != self.audible {
            // Activation and cancellation race atomically. Prepared audio can
            // never become audible after a successful queue-edit cancellation.
            if !run.track.activate() {
                return None;
            }
            if !self.tapped.is_empty() {
                self.tap.push(&self.tapped, 1.0);
                self.tapped.clear();
            }
            self.tap.clear();
            self.audible = run.track.generation;
            let _ = self.boundary.send(Boundary::Started(run.track.clone()));
        }
        let sample = self.output.next()?;
        run.track.samples.fetch_add(1, Ordering::Relaxed);
        run.count -= 1;
        self.tapped.push(sample);
        if self.tapped.len() == PCM_BLOCK_SAMPLES {
            self.tap.push(&self.tapped, 1.0);
            self.tapped.clear();
        }
        if run.count == 0 {
            self.runs.pop_front();
        }
        Some(sample as f32)
    }
}
impl Source for ContinuousSource {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prime_and_rebuffer_use_time_reserves_but_eof_drains_short_tails() {
        let (send, receive) = mpsc::sync_channel(PCM_BUFFER_BLOCKS);
        let mut input = Input::new(super::super::tests::track(1, 1), receive);
        for _ in 0..9 {
            send.send(Ok(vec![0.25; PCM_BLOCK_SAMPLES])).unwrap();
            assert!(input.block().is_none());
        }
        send.send(Ok(vec![0.25; PCM_BLOCK_SAMPLES])).unwrap();
        for _ in 0..10 {
            assert_eq!(input.block().unwrap().len(), PCM_BLOCK_SAMPLES);
        }
        assert!(input.block().is_none());
        input.buffering = true;
        send.send(Ok(vec![0.25; PCM_BLOCK_SAMPLES])).unwrap();
        assert!(input.block().is_none());
        drop(send);
        assert_eq!(input.block().unwrap().len(), PCM_BLOCK_SAMPLES);
        assert!(input.block().is_none());
        assert!(input.ended);
    }
    #[test]
    fn buffering_silence_never_advances_sample_clock() {
        let (send, receive) = mpsc::sync_channel(1);
        let track = super::super::tests::track(1, 1);
        let (boundary, _) = mpsc::channel();
        let mut source = ContinuousSource::new(
            Input::new(track.clone(), receive),
            Arc::new(Preparation::default()),
            Arc::new(AtomicU64::new(1)),
            boundary,
            crate::eq::shared(),
            crate::vis::AudioTap::new(),
        );
        assert_eq!(source.next(), Some(0.0));
        send.send(Ok(vec![0.25, 0.5])).unwrap();
        drop(send);
        assert_eq!(source.next(), Some(0.0));
        assert_eq!(track.samples.load(Ordering::Relaxed), 0);
        assert_eq!(source.next(), Some(0.25));
        assert_eq!(source.next(), Some(0.5));
        assert_eq!(source.next(), None);
        assert_eq!(track.samples.load(Ordering::Relaxed), 2);
    }
}
