//! Small loopback server for testing the real HTTP transport without accounts.
use std::io::{Read, Write};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub struct Server {
    pub url: String,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    pub fn spawn(mut respond: impl FnMut(&str) -> Vec<u8> + Send + 'static) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                            .unwrap();
                        let mut request = Vec::new();
                        while !request.ends_with(b"\r\n\r\n") && request.len() < 16384 {
                            let mut byte = [0];
                            if socket.read(&mut byte).unwrap_or(0) == 0 {
                                break;
                            }
                            request.push(byte[0]);
                        }
                        // A client can abandon a speculative connection before
                        // sending headers. It is not an HTTP request to assert on.
                        if !request.ends_with(b"\r\n\r\n") {
                            continue;
                        }
                        let response = respond(std::str::from_utf8(&request).unwrap());
                        let _ = socket.write_all(&response);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(2))
                    }
                    Err(error) => panic!("{error}"),
                }
            }
        });
        Self {
            url,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Err(error) = self.thread.take().unwrap().join()
            && !std::thread::panicking()
        {
            std::panic::resume_unwind(error);
        }
    }
}

#[test]
fn canceled_connection_cannot_crash_the_http_fixture() {
    let server = Server::spawn(|request| {
        assert!(request.starts_with("GET /fixture HTTP/1.1\r\n"));
        response("200 OK", "", b"fixture")
    });
    let address = server.url.trim_start_matches("http://");
    drop(std::net::TcpStream::connect(address).unwrap());
    let mut socket = std::net::TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    socket
        .write_all(b"GET /fixture HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut body = String::new();
    socket.read_to_string(&mut body).unwrap();
    assert!(body.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(body.ends_with("fixture"));
}

pub fn response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}
