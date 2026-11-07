//! Single-instance guard and native remote-control channel.
//! Linux uses a D-Bus name and MPRIS Raise. macOS and Windows use an
//! exclusive loopback socket. The OS releases either guard on process exit.
//! Commands are queued for the UI; read requests use an immutable snapshot.

/// The name held for the lifetime of the running instance.
#[cfg(target_os = "linux")]
const INSTANCE_NAME: &str = "io.github.nizarcan.Navifast.Instance";

/// The MPRIS player to ask when another instance already holds the name.
#[cfg(target_os = "linux")]
const MPRIS_NAME: &str = "org.mpris.MediaPlayer2.navifast";

/// Where the running instance answers local controls, on [`INSTANCE_NAME`].
#[cfg(target_os = "linux")]
const INSTANCE_PATH: &str = "/io/github/nizarcan/Navifast/Instance";

pub enum Outcome {
    /// This process is the only instance. Hold the guard until it exits.
    Only(Guard),
    /// Another instance is running and has been asked to show its window.
    Surfaced,
}

/// What a control client asked the running instance to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ControlCommand {
    /// Bring the window forward, creating it if needed.
    Show,
    /// Re-read local palette files without showing the window or restarting audio.
    ReloadThemes,
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// Milliseconds; negative seeks backwards.
    SeekBy(i64),
    /// Percentage points; negative lowers the volume.
    VolumeBy(i8),
    /// Absolute percentage.
    SetVolume(u8),
    ToggleMute,
    ToggleShuffle,
    CycleRepeat,
    /// Set shuffle explicitly, avoiding missed toggle updates.
    SetShuffle(bool),
    /// Repeat set outright, for the same reason.
    SetRepeat(crate::player::RepeatMode),
    /// Absolute position, in milliseconds.
    SeekTo(u32),
    /// Save the playing track to the library, or take it back out.
    ToggleSaved,
}

/// Marks this process as the running instance until dropped.
pub struct Guard {
    #[cfg(target_os = "linux")]
    _connection: Option<mpris_server::zbus::blocking::Connection>,
    /// Filled by control clients, drained by the app every frame.
    commands: std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>>,
    /// Current-track snapshot for `nowplaying` requests.
    now_playing: std::sync::Arc<std::sync::Mutex<String>>,
}

impl Guard {
    /// The queue a control client's commands land in. The app drains it.
    pub fn commands(&self) -> std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>> {
        std::sync::Arc::clone(&self.commands)
    }

    /// The slot the app writes the now-playing snapshot into.
    pub fn now_playing_slot(&self) -> std::sync::Arc<std::sync::Mutex<String>> {
        std::sync::Arc::clone(&self.now_playing)
    }
}

/// Snapshot value reported when nothing is playing.
pub const NOTHING_PLAYING: &str = "stopped";

/// Loopback port that marks a running instance on platforms without a bus.
/// Registered to nothing; chosen high and out of the ephemeral range.
#[cfg(not(target_os = "linux"))]
const INSTANCE_PORT: u16 = 47_163;

/// Every request and reply starts with this, so a foreign program that
/// happens to hold the port is never mistaken for Navifast.
#[cfg(not(target_os = "linux"))]
const PREFIX: &str = "navifast:";
#[cfg(not(target_os = "linux"))]
const OK_REPLY: &str = "navifast:ok";
#[cfg(not(target_os = "linux"))]
const NOW_REPLY: &str = "navifast:now ";

/// What the running instance said back.
#[cfg(not(target_os = "linux"))]
pub enum Reply {
    /// The command was accepted.
    Ok,
    /// The `nowplaying` snapshot: [`NOTHING_PLAYING`], or tab-separated
    /// `state, title, artists, album, position_ms, duration_ms, volume,
    /// shuffle, repeat, art_url, saved, device`.
    NowPlaying(String),
}

/// Sends one verb to the running instance and reads its reply.
#[cfg(not(target_os = "linux"))]
pub fn send(verb: &str) -> std::io::Result<Reply> {
    send_to(INSTANCE_PORT, verb)
}

#[cfg(not(target_os = "linux"))]
fn send_to(port: u16, verb: &str) -> std::io::Result<Reply> {
    use std::io::{Read, Write};
    use std::net::{Ipv4Addr, TcpStream};
    use std::time::Duration;

    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.write_all(format!("{PREFIX}{verb}\n").as_bytes())?;
    // The listener writes one line and closes. Older instances do not reply to
    // unknown verbs, so the read times out.
    let mut reply = String::new();
    stream.read_to_string(&mut reply)?;
    let line = reply.lines().next().unwrap_or("");
    if line == OK_REPLY {
        Ok(Reply::Ok)
    } else if let Some(snapshot) = line.strip_prefix(NOW_REPLY) {
        Ok(Reply::NowPlaying(snapshot.to_owned()))
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the port is held by something other than Navifast",
        ))
    }
}

/// Claim the running-instance role, or raise the existing instance.
#[cfg(not(target_os = "linux"))]
pub fn acquire_with_wake(waker: rune::Waker) -> Outcome {
    use std::net::{Ipv4Addr, TcpListener};
    use std::sync::{Arc, Mutex};

    let unguarded = || Guard {
        commands: Default::default(),
        now_playing: Arc::new(Mutex::new(NOTHING_PLAYING.to_owned())),
    };

    let listener = match TcpListener::bind((Ipv4Addr::LOCALHOST, INSTANCE_PORT)) {
        Ok(listener) => listener,
        Err(_) => {
            let answered = send("show").is_ok_and(|reply| matches!(reply, Reply::Ok));
            if answered {
                return Outcome::Surfaced;
            }
            log::warn!("port {INSTANCE_PORT} is busy but not with Navifast; running unguarded");
            return Outcome::Only(unguarded());
        }
    };

    let guard = unguarded();
    let commands = Arc::clone(&guard.commands);
    let now_playing = Arc::clone(&guard.now_playing);
    let waker = waker.clone();
    let spawned = std::thread::Builder::new()
        .name("navifast-instance".to_owned())
        .spawn(move || serve(listener, &commands, &now_playing, &waker));
    if let Err(error) = spawned {
        log::warn!("cannot listen for other launches: {error}");
    }
    Outcome::Only(guard)
}

/// Answers control clients until the listener closes. One request line and
/// one reply line per connection.
#[cfg(not(target_os = "linux"))]
fn serve(
    listener: std::net::TcpListener,
    commands: &std::sync::Mutex<Vec<ControlCommand>>,
    now_playing: &std::sync::Mutex<String>,
    waker: &rune::Waker,
) {
    use std::io::Write;
    use std::time::Duration;

    let queue = |command| {
        commands
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(command);
        waker();
    };

    for mut stream in listener.incoming().flatten() {
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let Some(line) = read_line(&mut stream) else {
            continue;
        };
        match parse(&line) {
            Some(Request::Command(command)) => {
                let _ = stream.write_all(format!("{OK_REPLY}\n").as_bytes());
                queue(command);
            }
            Some(Request::NowPlaying) => {
                let snapshot = now_playing
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                let _ = stream.write_all(format!("{NOW_REPLY}{snapshot}\n").as_bytes());
            }
            // Not our client; say nothing and hang up.
            None => {}
        }
    }
}

/// A parsed request line: a command for the app, or a read the listener
/// answers itself.
#[cfg(not(target_os = "linux"))]
enum Request {
    Command(ControlCommand),
    NowPlaying,
}

#[cfg(not(target_os = "linux"))]
fn parse(line: &str) -> Option<Request> {
    let verb = line.trim_end().strip_prefix(PREFIX)?;
    let (verb, argument) = match verb.split_once(' ') {
        Some((verb, argument)) => (verb, Some(argument.trim())),
        None => (verb, None),
    };
    let command = match (verb, argument) {
        ("show", None) => ControlCommand::Show,
        ("reload-themes", None) => ControlCommand::ReloadThemes,
        ("playpause", None) => ControlCommand::PlayPause,
        ("play", None) => ControlCommand::Play,
        ("pause", None) => ControlCommand::Pause,
        ("next", None) => ControlCommand::Next,
        ("previous", None) => ControlCommand::Previous,
        ("seek-by", Some(ms)) => ControlCommand::SeekBy(ms.parse().ok()?),
        ("seek-to", Some(ms)) => ControlCommand::SeekTo(ms.parse().ok()?),
        ("volume-by", Some(delta)) => ControlCommand::VolumeBy(delta.parse().ok()?),
        ("volume-set", Some(volume)) => ControlCommand::SetVolume(volume.parse().ok()?),
        ("mute", None) => ControlCommand::ToggleMute,
        ("shuffle", None) => ControlCommand::ToggleShuffle,
        ("shuffle-set", Some("on")) => ControlCommand::SetShuffle(true),
        ("shuffle-set", Some("off")) => ControlCommand::SetShuffle(false),
        ("repeat", None) => ControlCommand::CycleRepeat,
        // Match explicitly because `RepeatMode::from_api` maps unknown values
        // to `off`; control clients should reject them.
        ("repeat-set", Some("off")) => ControlCommand::SetRepeat(crate::player::RepeatMode::Off),
        ("repeat-set", Some("context")) => {
            ControlCommand::SetRepeat(crate::player::RepeatMode::Context)
        }
        ("repeat-set", Some("track")) => {
            ControlCommand::SetRepeat(crate::player::RepeatMode::Track)
        }
        ("save-toggle", None) => ControlCommand::ToggleSaved,
        ("nowplaying", None) => return Some(Request::NowPlaying),
        _ => return None,
    };
    Some(Request::Command(command))
}

/// Reads up to the first newline. A line too long to be one of ours, or any
/// read error, disqualifies the client.
#[cfg(not(target_os = "linux"))]
fn read_line(stream: &mut std::net::TcpStream) -> Option<String> {
    use std::io::Read;
    // Search links contain percent-encoded text, which can be much longer
    // than a track id, especially for non-ASCII queries. Keep a finite bound.
    let mut buffer = [0u8; 16 * 1024];
    let mut filled = 0;
    loop {
        if filled == buffer.len() {
            return None;
        }
        match stream.read(&mut buffer[filled..]) {
            Ok(0) => break,
            Ok(read) => {
                filled += read;
                if buffer[..filled].contains(&b'\n') {
                    break;
                }
            }
            Err(_) => return None,
        }
    }
    let line = buffer[..filled].split(|&byte| byte == b'\n').next()?;
    String::from_utf8(line.to_vec()).ok()
}

/// Commands supported on Linux that are not provided by MPRIS.
#[cfg(target_os = "linux")]
struct Instance {
    commands: std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>>,
    waker: rune::Waker,
}

#[cfg(target_os = "linux")]
#[zbus::interface(name = "io.github.nizarcan.Navifast.Instance")]
impl Instance {
    fn toggle_saved(&self) {
        self.commands
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(ControlCommand::ToggleSaved);
        (self.waker)();
    }

    fn reload_themes(&self) {
        self.commands
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(ControlCommand::ReloadThemes);
        (self.waker)();
    }
}

/// Reread local themes without starting or raising the app.
#[cfg(target_os = "linux")]
pub fn reload_themes() -> zbus::Result<()> {
    call_instance("ReloadThemes")
}

/// Toggle the playing item's saved state without starting or raising the app.
#[cfg(target_os = "linux")]
pub fn toggle_saved() -> zbus::Result<()> {
    call_instance("ToggleSaved")
}

#[cfg(target_os = "linux")]
fn call_instance(method: &str) -> zbus::Result<()> {
    let connection = zbus::blocking::connection::Builder::session()?
        .method_timeout(std::time::Duration::from_secs(2))
        .build()?;
    let proxy =
        zbus::blocking::Proxy::new(&connection, INSTANCE_NAME, INSTANCE_PATH, INSTANCE_NAME)?;
    let _: Option<()> =
        proxy.call_with_flags(method, zbus::proxy::MethodFlags::NoAutoStart.into(), &())?;
    Ok(())
}

/// Claim the running-instance role, or raise the existing instance.
#[cfg(target_os = "linux")]
pub fn acquire_with_wake(waker: rune::Waker) -> Outcome {
    use mpris_server::zbus::blocking::Connection;
    use mpris_server::zbus::fdo::{RequestNameFlags, RequestNameReply};

    let guard = |connection: Option<Connection>| Guard {
        _connection: connection,
        commands: Default::default(),
        now_playing: std::sync::Arc::new(std::sync::Mutex::new(NOTHING_PLAYING.to_owned())),
    };

    let connection = match Connection::session() {
        Ok(connection) => connection,
        Err(error) => {
            // No session bus at all: nothing to coordinate through, so run.
            log::debug!("no session bus, running unguarded: {error}");
            return Outcome::Only(guard(None));
        }
    };

    // Holding the D-Bus name marks this process as the running instance.
    // `NameTaken` is the normal second-launch result.
    match connection.request_name_with_flags(INSTANCE_NAME, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            let guard = guard(Some(connection.clone()));
            // Control requests arrive on the name just taken.
            // Registered before anything else, so a launch that follows
            // this one closely finds it.
            let instance = Instance {
                commands: std::sync::Arc::clone(&guard.commands),
                waker: waker.clone(),
            };
            if let Err(error) = serve_controls(connection, instance) {
                log::warn!("cannot accept local control requests: {error}");
            }
            Outcome::Only(guard)
        }
        Ok(_) | Err(mpris_server::zbus::Error::NameTaken) => {
            if !raise_running_instance(&connection) {
                log::warn!(
                    "Navifast is already running but did not answer; not starting a second copy"
                );
            }
            Outcome::Surfaced
        }
        Err(error) => {
            log::warn!("cannot check for a running instance, starting anyway: {error}");
            Outcome::Only(guard(None))
        }
    }
}

/// Serves `instance` on `connection` for the rest of the process, from a
/// thread of its own: with tokio behind zbus, an interface's dispatch task
/// runs on whichever runtime registers it, and outside one there is no
/// registering it at all. This runtime is never dropped. Returns once the
/// interface answers, or with what went wrong.
#[cfg(target_os = "linux")]
fn serve_controls(
    connection: zbus::blocking::Connection,
    instance: Instance,
) -> Result<(), String> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("navifast-controls".to_owned())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            runtime.block_on(async move {
                let served = connection
                    .inner()
                    .object_server()
                    .at(INSTANCE_PATH, instance)
                    .await;
                match served {
                    Ok(_) => {
                        let _ = ready_tx.send(Ok(()));
                        // The interface's dispatch task lives on this
                        // runtime; so does the thread.
                        std::future::pending::<()>().await;
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error.to_string()));
                    }
                }
            });
        });
    if let Err(error) = spawned {
        return Err(error.to_string());
    }
    match ready_rx.recv_timeout(std::time::Duration::from_secs(5)) {
        Ok(result) => result,
        Err(_) => Err("the control interface did not come up in time".to_owned()),
    }
}

/// Raise the current player, allowing its MPRIS interface time to register.
#[cfg(target_os = "linux")]
fn raise_running_instance(connection: &mpris_server::zbus::blocking::Connection) -> bool {
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        let raised = connection.call_method(
            Some(MPRIS_NAME),
            "/org/mpris/MediaPlayer2",
            Some("org.mpris.MediaPlayer2"),
            "Raise",
            &(),
        );
        if raised.is_ok() {
            return true;
        }
    }
    false
}

#[cfg(all(test, target_os = "linux"))]
mod bus_tests {
    use super::*;

    #[test]
    fn controls_reach_the_running_instance_queue() {
        let commands = Default::default();
        let instance = Instance {
            commands: std::sync::Arc::clone(&commands),
            waker: std::sync::Arc::new(|| {}),
        };
        instance.reload_themes();
        assert_eq!(
            *commands.lock().unwrap(),
            vec![ControlCommand::ReloadThemes]
        );
    }

    /// A usable session bus carries `ReloadThemes` to the running instance. The
    /// runner's bus is external test infrastructure, so bound the call and
    /// leave the interface behaviour to the deterministic test above if it
    /// accepts a connection but does not answer.
    #[test]
    fn a_reload_reaches_the_running_instance_over_the_bus() {
        // #given an instance answering on its own connection, not the
        // shared name, so a Navifast already running is left alone
        let Ok(server) = zbus::blocking::Connection::session() else {
            eprintln!("no session bus here; nothing to test");
            return;
        };
        let commands: std::sync::Arc<std::sync::Mutex<Vec<ControlCommand>>> = Default::default();
        let instance = Instance {
            commands: std::sync::Arc::clone(&commands),
            waker: std::sync::Arc::new(|| {}),
        };
        serve_controls(server.clone(), instance).expect("the interface is served");
        let name = server.unique_name().expect("a unique name").to_string();
        let client = zbus::blocking::connection::Builder::session()
            .expect("a session bus address")
            .method_timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("a second connection");

        // #when
        let opened = client.call_method(
            Some(name.as_str()),
            INSTANCE_PATH,
            Some("io.github.nizarcan.Navifast.Instance"),
            "ReloadThemes",
            &(),
        );
        if matches!(
            &opened,
            Err(zbus::Error::InputOutput(error))
                if error.kind() == std::io::ErrorKind::TimedOut
        ) {
            eprintln!("the session bus did not answer in time; nothing to test");
            return;
        };

        // #then
        assert!(opened.is_ok(), "{opened:?}");
        assert_eq!(
            *commands.lock().expect("the queue"),
            vec![ControlCommand::ReloadThemes]
        );
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod tests {
    use super::*;
    use crate::player::RepeatMode;

    fn command(line: &str) -> Option<ControlCommand> {
        match parse(line) {
            Some(Request::Command(command)) => Some(command),
            _ => None,
        }
    }

    #[test]
    fn parses_every_control_verb() {
        // #given / #when / #then
        assert_eq!(command("navifast:show\n"), Some(ControlCommand::Show));
        assert_eq!(
            command("navifast:reload-themes"),
            Some(ControlCommand::ReloadThemes)
        );
        assert_eq!(command("navifast:reload-themes extra"), None);
        assert_eq!(
            command("navifast:playpause"),
            Some(ControlCommand::PlayPause)
        );
        assert_eq!(command("navifast:play"), Some(ControlCommand::Play));
        assert_eq!(command("navifast:pause"), Some(ControlCommand::Pause));
        assert_eq!(command("navifast:next"), Some(ControlCommand::Next));
        assert_eq!(command("navifast:previous"), Some(ControlCommand::Previous));
        assert_eq!(
            command("navifast:seek-by -10000"),
            Some(ControlCommand::SeekBy(-10_000))
        );
        assert_eq!(
            command("navifast:volume-by +5"),
            Some(ControlCommand::VolumeBy(5))
        );
        assert_eq!(
            command("navifast:volume-set 40"),
            Some(ControlCommand::SetVolume(40))
        );
        assert_eq!(command("navifast:mute"), Some(ControlCommand::ToggleMute));
        assert_eq!(
            command("navifast:shuffle"),
            Some(ControlCommand::ToggleShuffle)
        );
        assert_eq!(
            command("navifast:repeat"),
            Some(ControlCommand::CycleRepeat)
        );
        assert_eq!(
            command("navifast:shuffle-set on"),
            Some(ControlCommand::SetShuffle(true))
        );
        assert_eq!(
            command("navifast:shuffle-set off"),
            Some(ControlCommand::SetShuffle(false))
        );
        assert_eq!(
            command("navifast:repeat-set track"),
            Some(ControlCommand::SetRepeat(RepeatMode::Track))
        );
        assert_eq!(
            command("navifast:repeat-set context"),
            Some(ControlCommand::SetRepeat(RepeatMode::Context))
        );
        assert_eq!(
            command("navifast:repeat-set off"),
            Some(ControlCommand::SetRepeat(RepeatMode::Off))
        );
        assert_eq!(
            command("navifast:seek-to 90000"),
            Some(ControlCommand::SeekTo(90_000))
        );
        assert_eq!(
            command("navifast:save-toggle"),
            Some(ControlCommand::ToggleSaved)
        );
        assert!(matches!(
            parse("navifast:nowplaying"),
            Some(Request::NowPlaying)
        ));
    }

    #[test]
    fn rejects_lines_that_are_not_ours() {
        assert!(parse("GET / HTTP/1.1").is_none());
        assert!(parse("navifast:frobnicate").is_none());
        assert!(parse("navifast:seek-by soon").is_none());
        assert!(parse("navifast:volume-set 999").is_none());
        assert!(parse("navifast:next please").is_none());
        assert!(parse("").is_none());
    }

    /// Free-text control arguments are validated before reaching the app.
    #[test]
    fn rejects_retired_commands_and_invalid_arguments() {
        // #given / #when / #then
        assert!(command("navifast:play-uri http://example.com/pwn").is_none());
        assert!(command("navifast:play-uri spotify:track:a b").is_none());
        assert!(command("navifast:play-uri ../../etc/passwd").is_none());
        assert!(command("navifast:play-uri").is_none());
        assert!(command(&format!("navifast:play-uri spotify:{}", "x".repeat(200))).is_none());
        assert!(command("navifast:transfer ../secrets").is_none());
        assert!(command("navifast:transfer").is_none());
        assert!(command("navifast:open-link https://example.com/track/x").is_none());
        assert!(command("navifast:open-link spotify:user:someone").is_none());
        assert!(command("navifast:open-link").is_none());
        // A word that is not one of the three is refused rather than read
        // as `off`, which is what `RepeatMode::from_api` would have done.
        assert!(command("navifast:repeat-set sometimes").is_none());
        assert!(command("navifast:shuffle-set maybe").is_none());
        assert!(command("navifast:seek-to -1").is_none());
    }

    /// Socket commands reach the queue and reads return published snapshots.
    #[test]
    fn a_client_reaches_the_command_queue_and_the_snapshot() {
        use std::net::{Ipv4Addr, TcpListener};
        use std::sync::{Arc, Mutex};

        // #given
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("a loopback port");
        let port = listener.local_addr().expect("a bound address").port();
        let commands: Arc<Mutex<Vec<ControlCommand>>> = Default::default();
        let now_playing = Arc::new(Mutex::new("playing\tGo\tThe Band".to_owned()));
        let served = {
            let commands = Arc::clone(&commands);
            let now_playing = Arc::clone(&now_playing);
            let waker: rune::Waker = std::sync::Arc::new(|| {});
            std::thread::spawn(move || serve(listener, &commands, &now_playing, &waker))
        };

        // #when
        let accepted = send_to(port, "next").expect("a reply");
        let volume = send_to(port, "volume-by -5").expect("a reply");
        let liked = send_to(port, "save-toggle").expect("a reply");
        let snapshot = send_to(port, "nowplaying").expect("a reply");
        let oversized = send_to(
            port,
            &format!("open-link spotify:search:{}", "x".repeat(16 * 1024)),
        );
        let refused = send_to(port, "frobnicate");

        // #then
        assert!(matches!(accepted, Reply::Ok));
        assert!(matches!(volume, Reply::Ok));
        assert!(matches!(liked, Reply::Ok));
        assert!(
            oversized.is_err(),
            "requests beyond the frame bound are rejected"
        );
        match snapshot {
            Reply::NowPlaying(line) => assert_eq!(line, "playing\tGo\tThe Band"),
            _ => panic!("nowplaying answered with something else"),
        }
        // An unknown verb gets no reply at all, so the client sees a closed
        // connection rather than a command it never sent being obeyed.
        assert!(refused.is_err());
        assert_eq!(
            *commands.lock().expect("the queue"),
            vec![
                ControlCommand::Next,
                ControlCommand::VolumeBy(-5),
                ControlCommand::ToggleSaved,
            ]
        );

        drop(served);
    }
}
