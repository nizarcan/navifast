//! Bounded, seekable HTTP input with independent download-ahead buffering.

use std::collections::VecDeque;
use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicU8, AtomicU64, Ordering},
};
use std::time::Duration;

use symphonia::core::io::MediaSource;

use super::api::Client;

const BLOCK: u64 = 256 * 1024;
// 4 MiB queued ahead, plus the block being read and one in-flight request.
// Downloads continue while decoding is backpressured by the PCM buffer.
const READ_AHEAD_BLOCKS: usize = 16;
const CANCEL_POLL: Duration = Duration::from_millis(20);

pub struct HttpSource {
    position: u64,
    length: u64,
    start: u64,
    bytes: Vec<u8>,
    ahead: Arc<ReadAhead>,
    current: Arc<AtomicU64>,
    generation: u64,
    guard: Option<Arc<AtomicU8>>,
}

#[derive(Default)]
struct ReadAhead {
    state: Mutex<DownloadState>,
    changed: Condvar,
}

#[derive(Default)]
struct DownloadState {
    blocks: VecDeque<io::Result<Vec<u8>>>,
    next: u64,
    revision: u64,
    finished: bool,
    stopped: bool,
}

impl HttpSource {
    #[cfg(test)]
    pub fn open(
        client: Client,
        http: reqwest::blocking::Client,
        id: String,
        current: Arc<AtomicU64>,
        generation: u64,
    ) -> io::Result<Self> {
        Self::open_guarded(client, http, id, current, generation, None)
    }

    /// A prepared occurrence can be invalidated without stopping the current one.
    pub(crate) fn open_guarded(
        client: Client,
        http: reqwest::blocking::Client,
        id: String,
        current: Arc<AtomicU64>,
        generation: u64,
        guard: Option<Arc<AtomicU8>>,
    ) -> io::Result<Self> {
        let mut download = Download {
            client,
            http,
            id,
            length: u64::MAX,
            current: current.clone(),
            generation,
            guard: guard.clone(),
        };
        let bytes = download.fetch(0)?;
        let length = download.length;
        let ahead = Arc::new(ReadAhead {
            state: Mutex::new(DownloadState {
                next: bytes.len() as u64,
                finished: bytes.len() as u64 == length,
                ..Default::default()
            }),
            changed: Condvar::new(),
        });
        let worker = ahead.clone();
        std::thread::Builder::new()
            .name("navifast-download".into())
            .spawn(move || download.run(&worker))?;
        Ok(Self {
            position: 0,
            length,
            start: 0,
            bytes,
            ahead,
            current,
            generation,
            guard,
        })
    }

    fn check(&self) -> io::Result<()> {
        check_generation(&self.current, self.generation, self.guard.as_deref())
    }

    fn fetch(&mut self) -> io::Result<()> {
        let mut state = self.ahead.state.lock().unwrap_or_else(|p| p.into_inner());
        if self.position != self.start + self.bytes.len() as u64 {
            // Decoder probes and seeks can jump anywhere. Retire both queued
            // blocks and the in-flight result before requesting that position.
            state.revision = state.revision.wrapping_add(1);
            state.blocks.clear();
            state.next = self.position;
            state.finished = false;
            self.ahead.changed.notify_one();
        }
        loop {
            self.check()?;
            if let Some(block) = state.blocks.pop_front() {
                self.ahead.changed.notify_one();
                self.bytes = block?;
                self.start = self.position;
                return Ok(());
            }
            if state.finished {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "The server returned incomplete audio.",
                ));
            }
            state = self
                .ahead
                .changed
                .wait_timeout(state, CANCEL_POLL)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
}

impl Drop for HttpSource {
    fn drop(&mut self) {
        let mut state = self.ahead.state.lock().unwrap_or_else(|p| p.into_inner());
        state.stopped = true;
        state.blocks.clear();
        self.ahead.changed.notify_one();
        // Never join a network read. Its existing request deadline bounds its
        // lifetime; a dropped source cannot enqueue data or request another block.
    }
}

fn check_generation(
    current: &AtomicU64,
    generation: u64,
    guard: Option<&AtomicU8>,
) -> io::Result<()> {
    if current.load(Ordering::Acquire) != generation
        || guard.is_some_and(|g| g.load(Ordering::Acquire) == 0)
    {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Playback was replaced.",
        ))
    } else {
        Ok(())
    }
}

struct Download {
    client: Client,
    http: reqwest::blocking::Client,
    id: String,
    length: u64,
    current: Arc<AtomicU64>,
    generation: u64,
    guard: Option<Arc<AtomicU8>>,
}

impl Download {
    fn run(&mut self, ahead: &ReadAhead) {
        loop {
            let mut state = ahead.state.lock().unwrap_or_else(|p| p.into_inner());
            while !state.stopped
                && self.check().is_ok()
                && (state.finished || state.blocks.len() >= READ_AHEAD_BLOCKS)
            {
                state = ahead
                    .changed
                    .wait_timeout(state, CANCEL_POLL)
                    .unwrap_or_else(|p| p.into_inner())
                    .0;
            }
            if state.stopped || self.check().is_err() {
                return;
            }
            let position = state.next;
            let revision = state.revision;
            drop(state);
            let block = self.fetch(position);
            let mut state = ahead.state.lock().unwrap_or_else(|p| p.into_inner());
            if state.stopped || self.check().is_err() {
                return;
            }
            if state.revision != revision {
                continue;
            }
            match &block {
                Ok(bytes) => {
                    state.next += bytes.len() as u64;
                    state.finished = state.next == self.length;
                }
                Err(_) => state.finished = true,
            }
            state.blocks.push_back(block);
            ahead.changed.notify_one();
        }
    }

    fn check(&self) -> io::Result<()> {
        check_generation(&self.current, self.generation, self.guard.as_deref())
    }

    fn fetch(&mut self, position: u64) -> io::Result<Vec<u8>> {
        self.check()?;
        let end = position
            .saturating_add(BLOCK - 1)
            .min(self.length.saturating_sub(1));
        let response = self
            .http
            .get(self.client.stream_url(&self.id))
            .header(reqwest::header::RANGE, format!("bytes={position}-{end}"))
            .send()
            .map_err(|e| io::Error::other(super::api::network_error(e)))?;
        self.check()?;
        let status = response.status();
        let expected = if status == reqwest::StatusCode::PARTIAL_CONTENT {
            let range = response
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_range)
                .ok_or_else(|| {
                    io::Error::other("The server returned an invalid audio byte range.")
                })?;
            if range.0 != position
                || range.1 < range.0
                || range.1 > end
                || range.2 <= range.1
                || (self.length != u64::MAX && self.length != range.2)
            {
                return Err(io::Error::other(
                    "The server returned an unexpected audio byte range.",
                ));
            }
            self.length = range.2;
            range.1 - range.0 + 1
        } else if status == reqwest::StatusCode::OK
            && position == 0
            && response.content_length().is_some_and(|len| len <= BLOCK)
        {
            self.length = response.content_length().unwrap_or(0);
            self.length
        } else if status.is_redirection() {
            return Err(io::Error::other(
                "The audio request was redirected. Use your server's final URL.",
            ));
        } else if !status.is_success() {
            return Err(io::Error::other(format!(
                "The server refused audio playback (HTTP {}).",
                status.as_u16()
            )));
        } else {
            return Err(io::Error::other(
                "Original-file playback needs HTTP byte ranges. Check that your reverse proxy forwards Range headers and that the server allows original streaming.",
            ));
        };
        let mut bytes = Vec::with_capacity(expected as usize);
        response
            .take(BLOCK + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| {
                io::Error::other(
                    "Audio download was interrupted. Check the connection and press Play to retry.",
                )
            })?;
        if bytes.len() as u64 != expected {
            return Err(io::Error::other("The server returned incomplete audio."));
        }
        self.check()?;
        Ok(bytes)
    }
}

fn parse_range(value: &str) -> Option<(u64, u64, u64)> {
    let (range, size) = value.strip_prefix("bytes ")?.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, size.parse().ok()?))
}

impl Read for HttpSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.check()?;
        if buffer.is_empty() || self.position >= self.length {
            return Ok(0);
        }
        if self.position < self.start || self.position >= self.start + self.bytes.len() as u64 {
            self.fetch()?;
        }
        let offset = (self.position - self.start) as usize;
        let count = buffer.len().min(self.bytes.len() - offset);
        buffer[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}

impl Seek for HttpSource {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.check()?;
        let position = match from {
            SeekFrom::Start(n) => i128::from(n),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.length) + i128::from(n),
        };
        if !(0..=i128::from(self.length)).contains(&position) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid audio seek.",
            ));
        }
        self.position = position as u64;
        Ok(self.position)
    }
}

impl MediaSource for HttpSource {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        Some(self.length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_parser_requires_a_known_length() {
        assert_eq!(
            parse_range("bytes 0-262143/900000"),
            Some((0, 262143, 900000))
        );
        assert_eq!(parse_range("bytes 0-99/*"), None);
        assert_eq!(parse_range("garbage"), None);
    }

    #[test]
    fn reads_seek_within_the_buffer_and_cancel_before_network() {
        let current = Arc::new(AtomicU64::new(1));
        let mut source = HttpSource {
            position: 0,
            length: 6,
            start: 0,
            bytes: b"abcdef".to_vec(),
            ahead: Arc::new(ReadAhead::default()),
            current: current.clone(),
            generation: 1,
            guard: None,
        };
        source.seek(SeekFrom::End(-3)).unwrap();
        let mut tail = Vec::new();
        source.read_to_end(&mut tail).unwrap();
        assert_eq!(tail, b"def");
        assert!(source.seek(SeekFrom::Start(7)).is_err());
        current.store(2, Ordering::Release);
        assert_eq!(
            source.read(&mut [0; 1]).unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }
    #[test]
    fn prepared_read_cancellation_is_independent_of_playing_chain() {
        let current = AtomicU64::new(9);
        let prepared = AtomicU8::new(1);
        assert!(check_generation(&current, 9, Some(&prepared)).is_ok());
        prepared.store(0, Ordering::Release);
        assert_eq!(
            check_generation(&current, 9, Some(&prepared))
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(check_generation(&current, 9, None).is_ok());
        prepared.store(2, Ordering::Release);
        assert!(check_generation(&current, 9, Some(&prepared)).is_ok());
        current.store(10, Ordering::Release);
        assert!(check_generation(&current, 9, Some(&prepared)).is_err());
    }
    use crate::navidrome::test_server::{Server, response};
    use std::sync::mpsc;

    fn open(server: &Server) -> HttpSource {
        HttpSource::open(
            Client::new(
                &server.url,
                "a",
                "secret".into(),
                &crate::settings::ProxyConfig::Off,
            )
            .unwrap(),
            Client::stream_client(&crate::settings::ProxyConfig::Off).unwrap(),
            "song".into(),
            Arc::new(AtomicU64::new(1)),
            1,
        )
        .unwrap()
    }

    fn requested_range(request: &str) -> (u64, u64) {
        assert!(
            request
                .split_whitespace()
                .nth(1)
                .unwrap()
                .contains("format=raw")
        );
        let range = request
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("range: bytes=")
                    .map(str::to_owned)
            })
            .unwrap();
        let (start, end) = range.trim().split_once('-').unwrap();
        (start.parse().unwrap(), end.parse().unwrap())
    }

    fn range_response(start: u64, end: u64, length: u64) -> Vec<u8> {
        let end = end.min(length - 1);
        assert!(start <= end && end - start < BLOCK);
        let body = (start..=end)
            .map(|offset| (offset / BLOCK) as u8)
            .collect::<Vec<_>>();
        response(
            "206 Partial Content",
            &format!("Content-Range: bytes {start}-{end}/{length}\r\n"),
            &body,
        )
    }

    fn wait_for(ahead: &ReadAhead, condition: impl Fn(&DownloadState) -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut state = ahead.state.lock().unwrap();
        while !condition(&state) {
            assert!(
                std::time::Instant::now() < deadline,
                "download worker did not reach expected state"
            );
            state = ahead.changed.wait_timeout(state, CANCEL_POLL).unwrap().0;
        }
    }

    #[test]
    fn downloads_a_bounded_reserve_before_the_decoder_needs_it() {
        let requests = Arc::new(AtomicU64::new(0));
        let seen = requests.clone();
        let length = BLOCK * (READ_AHEAD_BLOCKS as u64 + 10);
        let server = Server::spawn(move |request| {
            seen.fetch_add(1, Ordering::Relaxed);
            let (start, end) = requested_range(request);
            range_response(start, end, length)
        });
        let mut source = open(&server);
        wait_for(&source.ahead, |state| {
            state.blocks.len() == READ_AHEAD_BLOCKS
        });
        assert_eq!(
            requests.load(Ordering::Relaxed),
            1 + READ_AHEAD_BLOCKS as u64
        );
        assert!(
            !source.ahead.state.lock().unwrap().finished,
            "do not download the whole song"
        );
        // The already downloaded reserve is usable in exact byte order, across
        // block boundaries, even when the decoder consumes it in one burst.
        let mut bytes = vec![0; BLOCK as usize * 3 + 17];
        source.read_exact(&mut bytes).unwrap();
        assert!(
            bytes
                .iter()
                .enumerate()
                .all(|(i, &b)| b == (i as u64 / BLOCK) as u8)
        );
        source.seek(SeekFrom::Start(10)).unwrap();
        source.read_exact(&mut bytes[..20]).unwrap();
        assert_eq!(&bytes[..20], &[0; 20]);
        source.seek(SeekFrom::End(-20)).unwrap();
        source.read_exact(&mut bytes[..20]).unwrap();
        assert_eq!(&bytes[..20], &[(length / BLOCK - 1) as u8; 20]);
        assert_eq!(source.read(&mut bytes).unwrap(), 0);
    }

    #[test]
    fn seek_discards_an_in_flight_prefetch_and_keeps_cached_reads_responsive() {
        let (started, pending) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let server = Server::spawn(move |request| {
            let (start, end) = requested_range(request);
            if start == BLOCK {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            range_response(start, end, BLOCK * 5)
        });
        let mut source = open(&server);
        pending.recv_timeout(Duration::from_secs(5)).unwrap();
        let mut bytes = [0; 20];
        source.read_exact(&mut bytes).unwrap();
        assert_eq!(
            bytes, [0; 20],
            "cached reads do not wait for the stalled request"
        );
        source.seek(SeekFrom::Start(BLOCK * 3 + 10)).unwrap();
        let ahead = source.ahead.clone();
        let reader = std::thread::spawn(move || {
            source.read_exact(&mut bytes).unwrap();
            bytes
        });
        wait_for(&ahead, |state| state.revision == 1);
        release.send(()).unwrap();
        assert_eq!(reader.join().unwrap(), [3; 20]);
    }

    #[test]
    fn cancel_interrupts_a_decoder_wait_without_waiting_for_http() {
        let (started, pending) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let requests = Arc::new(AtomicU64::new(0));
        let seen = requests.clone();
        let server = Server::spawn(move |request| {
            seen.fetch_add(1, Ordering::Relaxed);
            let (start, end) = requested_range(request);
            if start == BLOCK {
                started.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            range_response(start, end, BLOCK * 5)
        });
        let mut source = open(&server);
        pending.recv_timeout(Duration::from_secs(5)).unwrap();
        let current = source.current.clone();
        let ahead = source.ahead.clone();
        source.seek(SeekFrom::Start(BLOCK)).unwrap();
        let (send, done) = mpsc::channel();
        let reader = std::thread::spawn(move || {
            send.send(source.read(&mut [0; 1]).unwrap_err().kind())
                .unwrap();
        });
        current.store(2, Ordering::Release);
        assert_eq!(
            done.recv_timeout(Duration::from_secs(2)).unwrap(),
            io::ErrorKind::Interrupted
        );
        reader.join().unwrap();
        assert!(ahead.state.lock().unwrap().stopped);
        release.send(()).unwrap();
        drop(server);
        assert_eq!(requests.load(Ordering::Relaxed), 2);
        assert!(ahead.state.lock().unwrap().blocks.is_empty());
    }

    #[test]
    fn dropping_a_source_discards_the_reserve_and_stops_downloads() {
        let requests = Arc::new(AtomicU64::new(0));
        let seen = requests.clone();
        let server = Server::spawn(move |request| {
            seen.fetch_add(1, Ordering::Relaxed);
            let (start, end) = requested_range(request);
            range_response(start, end, BLOCK * 100)
        });
        let source = open(&server);
        let ahead = source.ahead.clone();
        wait_for(&ahead, |state| state.blocks.len() == READ_AHEAD_BLOCKS);
        drop(source);
        let state = ahead.state.lock().unwrap();
        assert!(state.stopped);
        assert!(state.blocks.is_empty());
        assert_eq!(
            requests.load(Ordering::Relaxed),
            1 + READ_AHEAD_BLOCKS as u64
        );
    }

    #[test]
    fn rejects_bad_ranges_redirects_and_incomplete_prefetches() {
        for reply in [
            response(
                "206 Partial Content",
                "Content-Range: bytes 0-100/1000\r\n",
                b"short",
            ),
            response(
                "206 Partial Content",
                "Content-Range: bytes 1-5/1000\r\n",
                b"wrong",
            ),
            response("302 Found", "Location: http://127.0.0.1:1/secret\r\n", b""),
            response("200 OK", "", &vec![0; BLOCK as usize + 1]),
        ] {
            let server = Server::spawn(move |_| reply.clone());
            let result = HttpSource::open(
                Client::new(
                    &server.url,
                    "a",
                    "secret".into(),
                    &crate::settings::ProxyConfig::Off,
                )
                .unwrap(),
                Client::stream_client(&crate::settings::ProxyConfig::Off).unwrap(),
                "song".into(),
                Arc::new(AtomicU64::new(1)),
                1,
            );
            assert!(result.is_err());
        }
        let server = Server::spawn(|request| {
            let (start, end) = requested_range(request);
            if start == 0 {
                range_response(start, end, BLOCK * 3)
            } else {
                response(
                    "206 Partial Content",
                    &format!("Content-Range: bytes {start}-{end}/{}\r\n", BLOCK * 3),
                    b"short",
                )
            }
        });
        let mut source = open(&server);
        source.seek(SeekFrom::Start(BLOCK)).unwrap();
        assert_eq!(
            source.read(&mut [0; 1]).unwrap_err().to_string(),
            "The server returned incomplete audio."
        );
    }

    #[test]
    fn short_file_can_drain_without_filling_the_download_reserve() {
        let server = Server::spawn(|_| response("200 OK", "", b"short"));
        let mut source = open(&server);
        let mut bytes = Vec::new();
        source.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"short");
    }
}
