// Shared by the Navifast command and its Fastpotify compatibility command.

use navifast::{app, paths, single_instance};

use clap::{CommandFactory, FromArgMatches, Parser};

/// A native Navidrome client.
#[derive(Debug, Parser)]
#[command(name = env!("CARGO_BIN_NAME"), version, about)]
struct Cli {
    /// A command for the running instance; without one, the app starts.
    #[command(subcommand)]
    control: Option<Control>,

    /// Log additional application diagnostics.
    #[arg(short, long)]
    verbose: bool,

    /// Start deterministic Navidrome sample screens.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long)]
    demo: bool,

    /// Compatibility spelling used by existing preview scripts.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, requires = "demo", hide = true)]
    demo_navidrome: bool,

    #[cfg(feature = "navidrome-demo")]
    #[arg(long, requires = "demo")]
    demo_data: Option<std::path::PathBuf>,

    /// Page to open in demo mode, e.g. `albums`, `playlist:pl1`, `artist:art0`.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long)]
    demo_page: Option<String>,

    /// Sample states such as `light`, `signed-out`, `details`, or `equalizer`.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long)]
    demo_show: Option<String>,

    /// Write a PNG of the demo window to this path and exit. Implies
    /// `--demo`. Without `--demo-size`, the shot is the window's own frame
    /// buffer: full screen where that request is honoured, and the size of
    /// the tile under a tiling window manager, which decides for itself.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, value_name = "PATH")]
    demo_shot: Option<std::path::PathBuf>,

    /// Inner window size for `--demo-shot`, as `WIDTHxHEIGHT`.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, value_name = "WIDTHxHEIGHT", value_parser = parse_demo_size)]
    demo_size: Option<[f32; 2]>,
}

impl Cli {
    #[cfg(feature = "navidrome-demo")]
    fn requests_demo(&self) -> bool {
        self.demo || self.demo_shot.is_some()
    }
}

/// Remote control of the running instance, for Raycast scripts, launchers,
/// and hands on keyboards.
#[derive(Debug, clap::Subcommand)]
enum Control {
    /// Toggle play/pause
    PlayPause,
    /// Start playback if paused
    Play,
    /// Pause playback if playing
    Pause,
    /// Skip to the next track
    Next,
    /// Return to the previous track
    Previous,
    /// Seek by this many seconds; negative seeks backwards
    Seek {
        #[arg(allow_negative_numbers = true)]
        seconds: i64,
    },
    /// Seek to a position, in seconds from the start
    SeekTo { seconds: u32 },
    /// Set the volume to a percentage
    Volume {
        #[arg(value_parser = clap::value_parser!(u8).range(0..=100))]
        percent: u8,
    },
    /// Raise the volume
    VolumeUp {
        #[arg(default_value_t = 10, value_parser = clap::value_parser!(u8).range(1..=100))]
        percent: u8,
    },
    /// Lower the volume
    VolumeDown {
        #[arg(default_value_t = 10, value_parser = clap::value_parser!(u8).range(1..=100))]
        percent: u8,
    },
    /// Toggle mute
    Mute,
    /// Toggle shuffle, or set it outright
    #[command(hide = true)]
    Shuffle { state: Option<OnOff> },
    /// Cycle the repeat mode, or set it outright
    #[command(hide = true)]
    Repeat { mode: Option<Repeat> },
    /// Save the playing track to your library, or take it back out
    #[command(hide = true)]
    Like,
    /// Print the playing track
    NowPlaying {
        /// Print the fields tab-separated instead: state, title, artists,
        /// album, position_ms, duration_ms, volume, shuffle, repeat,
        /// art_url, saved, device.
        #[arg(long)]
        raw: bool,
    },
    /// Bring the window of the running instance forward
    Show,
    /// Reload local palette files without starting the app or interrupting playback
    #[command(hide = true)]
    ReloadThemes,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum Repeat {
    /// Play through and stop
    Off,
    /// Repeat the album, playlist, or queue
    Context,
    /// Repeat this track
    Track,
}

/// Sends one control verb to the running instance. Speaks over the
/// single-instance loopback socket, which Linux does not have.
#[cfg(not(target_os = "linux"))]
fn run_control(control: Control) -> i32 {
    let raw = matches!(control, Control::NowPlaying { raw: true });
    let verb = match control {
        Control::PlayPause => "playpause".to_owned(),
        Control::Play => "play".to_owned(),
        Control::Pause => "pause".to_owned(),
        Control::Next => "next".to_owned(),
        Control::Previous => "previous".to_owned(),
        Control::Seek { seconds } => format!("seek-by {}", seconds.saturating_mul(1000)),
        Control::SeekTo { seconds } => format!("seek-to {}", u64::from(seconds) * 1000),
        Control::Volume { percent } => format!("volume-set {percent}"),
        Control::VolumeUp { percent } => format!("volume-by {percent}"),
        Control::VolumeDown { percent } => format!("volume-by -{percent}"),
        Control::Mute => "mute".to_owned(),
        Control::Shuffle { state: None } => "shuffle".to_owned(),
        Control::Shuffle { state: Some(state) } => {
            let state = match state {
                OnOff::On => "on",
                OnOff::Off => "off",
            };
            format!("shuffle-set {state}")
        }
        Control::Repeat { mode: None } => "repeat".to_owned(),
        Control::Repeat { mode: Some(mode) } => {
            let mode = match mode {
                Repeat::Off => "off",
                Repeat::Context => "context",
                Repeat::Track => "track",
            };
            format!("repeat-set {mode}")
        }
        Control::Like => "save-toggle".to_owned(),
        Control::NowPlaying { .. } => "nowplaying".to_owned(),
        Control::Show => "show".to_owned(),
        Control::ReloadThemes => "reload-themes".to_owned(),
    };
    match single_instance::send(&verb) {
        Ok(single_instance::Reply::Ok) => 0,
        Ok(single_instance::Reply::NowPlaying(snapshot)) => {
            if raw {
                println!("{snapshot}");
            } else {
                println!("{}", format_now_playing(&snapshot));
            }
            0
        }
        Err(error) => {
            eprintln!("Navifast is not running or does not support remote control: {error}");
            1
        }
    }
}

#[cfg(target_os = "linux")]
fn run_control(control: Control) -> i32 {
    let result = match control {
        Control::Like => single_instance::toggle_saved(),
        Control::ReloadThemes => single_instance::reload_themes(),
        _ => {
            eprintln!(
                "On Linux the running instance speaks MPRIS instead; use e.g. \
                 `playerctl --player=navifast play-pause`."
            );
            return 2;
        }
    };
    match result {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("Navifast is not running or does not support remote control: {error}");
            1
        }
    }
}

#[cfg(target_os = "linux")]
const PULSEAUDIO_PROPERTIES: [(&str, &str); 2] = [
    ("PULSE_PROP_application.name", "Navifast"),
    ("PULSE_PROP_stream.description", "Navidrome playback"),
];

#[cfg(target_os = "linux")]
fn missing_pulseaudio_properties(
    mut is_set: impl FnMut(&str) -> bool,
) -> impl Iterator<Item = (&'static str, &'static str)> {
    PULSEAUDIO_PROPERTIES
        .into_iter()
        .filter(move |(key, _)| !is_set(key))
}

/// Sets application metadata for native PulseAudio output.
///
/// That backend leaves both names empty unless its caller provides them. An
/// unnamed stream cannot be identified reliably by mixers and audio processors.
#[cfg(target_os = "linux")]
fn configure_pulseaudio_properties() {
    for (key, value) in missing_pulseaudio_properties(|key| std::env::var_os(key).is_some()) {
        // SAFETY: run calls this at process startup, before it creates any
        // threads. Keep explicit values supplied by the launcher or user.
        unsafe { std::env::set_var(key, value) };
    }
}

/// The `nowplaying` snapshot as one human-readable line.
#[cfg(not(target_os = "linux"))]
fn format_now_playing(snapshot: &str) -> String {
    let mut fields = snapshot.split('\t');
    let state = fields.next().unwrap_or_default();
    let title = fields.next().unwrap_or_default();
    let artists = fields.next().unwrap_or_default();
    let _album = fields.next();
    let position_ms: u32 = fields.next().and_then(|ms| ms.parse().ok()).unwrap_or(0);
    let duration_ms: u32 = fields.next().and_then(|ms| ms.parse().ok()).unwrap_or(0);
    let clock = |ms: u32| format!("{}:{:02}", ms / 60_000, ms % 60_000 / 1000);
    match state {
        "playing" | "paused" => {
            let mark = if state == "playing" { "▶" } else { "⏸" };
            format!(
                "{mark} {title} — {artists}  [{} / {}]",
                clock(position_ms),
                clock(duration_ms)
            )
        }
        _ => "Nothing playing".to_owned(),
    }
}

/// The `devices` snapshot as one line per device, the active one marked.
/// The id comes first because `navifast transfer` is what it is for.
pub(crate) fn run() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    configure_pulseaudio_properties();
    // First of all: `--apply-update <job>` makes this process the update
    // helper, which installs and exits; otherwise the receipt and error an
    // update relaunch carries are taken out of the arguments.
    let launch = navifast::updates::intercept();
    // Follow the invoked command, including the Linux package's navifast
    // symlink. Old updaters execute a file named fastpotify and require its
    // original --version output; both commands otherwise start the same app.
    let name = match launch
        .arguments
        .first()
        .and_then(|arg| std::path::Path::new(arg).file_stem())
        .and_then(|arg| arg.to_str())
    {
        Some(name) if name.eq_ignore_ascii_case("fastpotify") => "fastpotify",
        _ => "navifast",
    };
    let cli = Cli::from_arg_matches(
        &Cli::command()
            .name(name)
            .get_matches_from(&launch.arguments),
    )
    .unwrap_or_else(|error| error.exit());
    // Demo mode invents plays, settings, and a signed-in account. Without a
    // folder of its own it would write them into the real profile, where
    // they would pass for the user's history and stop the next real launch
    // from moving an older profile across.
    #[cfg(feature = "navidrome-demo")]
    let cli = if cli.requests_demo() && cli.demo_data.is_none() {
        Cli {
            demo_data: Some(
                std::env::temp_dir().join(format!("navifast-demo-{}", std::process::id())),
            ),
            ..cli
        }
    } else {
        cli
    };
    // A control launch is a client, not a second app: talk to the running
    // instance and exit before touching the log file it is writing to.
    if let Some(control) = cli.control {
        std::process::exit(run_control(control));
    }
    #[cfg(feature = "navidrome-demo")]
    let demo = cli.requests_demo();
    {
        let dirs = paths::AppDirs::discover();
        #[cfg(feature = "navidrome-demo")]
        let dirs = cli
            .demo_data
            .as_ref()
            .map(|base| paths::AppDirs {
                config: base.join("config"),
                state: base.join("state"),
                cache: base.join("cache"),
            })
            .unwrap_or(dirs);
        #[cfg(feature = "navidrome-demo")]
        let (page, flags, snapshot, size) =
            (cli.demo_page, cli.demo_show, cli.demo_shot, cli.demo_size);
        #[cfg(not(feature = "navidrome-demo"))]
        let (demo, page, flags, snapshot, size) = (false, None, None, None, None);
        app::navidrome::desktop::run(
            app::navidrome::desktop::Launch {
                dirs,
                receipt: launch.receipt,
                update_error: launch.error,
                demo,
                page,
                flags,
                snapshot,
                size,
            },
            |dirs| {
                let _ = dirs.ensure();
                navifast::logging::init(&dirs.log_file(), cli.verbose);
            },
        )
    }
}

#[cfg(feature = "navidrome-demo")]
fn parse_demo_size(spec: &str) -> Result<[f32; 2], String> {
    let (width, height) = spec
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {spec}"))?;
    let width: f32 = width
        .trim()
        .parse()
        .map_err(|_| format!("invalid width in {spec}"))?;
    let height: f32 = height
        .trim()
        .parse()
        .map_err(|_| format!("invalid height in {spec}"))?;
    if width < 1.0 || height < 1.0 {
        return Err(format!("size must be at least 1x1, got {spec}"));
    }
    Ok([width, height])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_commands_accept_native_launches_and_controls() {
        for name in ["navifast", "fastpotify"] {
            assert!(Cli::try_parse_from([name]).unwrap().control.is_none());
            assert!(matches!(
                Cli::try_parse_from([name, "next"]).unwrap().control,
                Some(Control::Next)
            ));
            for retired in ["spotify:track:abc", "devices", "transfer", "play-uri"] {
                assert!(Cli::try_parse_from([name, retired]).is_err());
            }
        }
    }

    #[cfg(feature = "navidrome-demo")]
    #[test]
    fn previews_request_the_native_demo() {
        for args in [
            vec!["navifast", "--demo", "--demo-navidrome"],
            vec!["navifast", "--demo"],
            vec!["navifast", "--demo-shot", "screen.png"],
        ] {
            assert!(Cli::try_parse_from(args).unwrap().requests_demo());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn pulse_metadata_defaults_preserve_existing_properties() {
        let defaults = missing_pulseaudio_properties(|_| false).collect::<Vec<_>>();
        assert_eq!(defaults, PULSEAUDIO_PROPERTIES);

        let defaults = missing_pulseaudio_properties(|key| key == "PULSE_PROP_application.name")
            .collect::<Vec<_>>();
        assert_eq!(
            defaults,
            [("PULSE_PROP_stream.description", "Navidrome playback")]
        );

        assert_eq!(missing_pulseaudio_properties(|_| true).count(), 0);
    }
}
