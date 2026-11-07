//! A converted HTTP body is a forward stream, regardless of estimated lengths.
use super::*;

pub(crate) struct ForwardSource {
    response: reqwest::blocking::Response,
    current: Arc<AtomicU64>,
    generation: u64,
    guard: Option<Arc<AtomicU8>>,
}
impl ForwardSource {
    pub fn open(
        http: reqwest::blocking::Client,
        url: reqwest::Url,
        current: Arc<AtomicU64>,
        generation: u64,
        guard: Option<Arc<AtomicU8>>,
    ) -> io::Result<Self> {
        check_generation(&current, generation, guard.as_deref())?;
        let response = http
            .get(url)
            .send()
            .map_err(|e| io::Error::other(super::super::api::network_error(e)))?
            .error_for_status()
            .map_err(|e| io::Error::other(super::super::api::network_error(e)))?;
        if response.status().is_redirection() {
            return Err(io::Error::other(
                "The audio request was redirected. Use your server's final URL.",
            ));
        }
        Ok(Self {
            response,
            current,
            generation,
            guard,
        })
    }
}
impl Read for ForwardSource {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        check_generation(&self.current, self.generation, self.guard.as_deref())?;
        let count = self.response.read(buffer).map_err(|error| {
            io::Error::new(
                error.kind(),
                "The converted audio stream was interrupted. Retry or choose another quality.",
            )
        })?;
        check_generation(&self.current, self.generation, self.guard.as_deref())?;
        Ok(count)
    }
}
impl Seek for ForwardSource {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Converted audio uses server time offsets, not byte seeking.",
        ))
    }
}
impl MediaSource for ForwardSource {
    fn is_seekable(&self) -> bool {
        false
    }
    fn byte_len(&self) -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::test_server::Server;
    #[test]
    fn unknown_length_and_chunked_streams_do_not_require_ranges_or_fake_lengths() {
        for chunked in [false, true] {
            let server = Server::spawn(move |request| {
                assert!(!request.to_lowercase().contains("range:"));
                if chunked {
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n".to_vec()
                } else {
                    b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nabcdef".to_vec()
                }
            });
            let current = Arc::new(AtomicU64::new(1));
            let http =
                crate::navidrome::api::Client::stream_client(&crate::settings::ProxyConfig::Off)
                    .unwrap();
            let mut source = ForwardSource::open(
                http,
                reqwest::Url::parse(&server.url).unwrap(),
                current.clone(),
                1,
                None,
            )
            .unwrap();
            assert_eq!(source.byte_len(), None);
            assert!(!source.is_seekable());
            assert!(source.seek(SeekFrom::Start(1)).is_err());
            let mut bytes = Vec::new();
            source.read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"abcdef");
            current.store(2, Ordering::Release);
            assert_eq!(
                source.read(&mut [0; 1]).unwrap_err().kind(),
                io::ErrorKind::Interrupted
            );
        }
    }
    #[test]
    fn redirects_and_failed_encoders_never_return_a_fallback_source() {
        for status in ["302 Found", "500 Internal Server Error"] {
            let server = Server::spawn(move |_| {
                crate::navidrome::test_server::response(
                    status,
                    "Location: http://example.invalid/\r\n",
                    b"failed",
                )
            });
            let http =
                crate::navidrome::api::Client::stream_client(&crate::settings::ProxyConfig::Off)
                    .unwrap();
            assert!(
                ForwardSource::open(
                    http,
                    reqwest::Url::parse(&server.url).unwrap(),
                    Arc::new(AtomicU64::new(1)),
                    1,
                    None
                )
                .is_err()
            );
        }
    }
}
