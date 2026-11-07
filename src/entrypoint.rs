// Shared by the Navifast command and its Fastpotify compatibility command.

use navifast::{app, paths, single_instance};

#[cfg(feature = "demo")]
mod legacy;

use clap::{CommandFactory, FromArgMatches, Parser};

/// A native Navidrome client.
#[derive(Debug, Parser)]
#[command(name = env!("CARGO_BIN_NAME"), version, about)]
struct Cli {
    /// A command for the running instance; without one, the app starts.
    #[command(subcommand)]
    control: Option<Control>,

    /// A Spotify link to open: spotify:track:…, or an open.spotify.com
    /// address. The running Navifast opens it when there is one, which
    /// is how the desktop hands links over.
    #[arg(value_name = "LINK", hide = true)]
    link: Option<String>,

    /// Spotify Connect device name for this session.
    #[arg(long, hide = true)]
    device_name: Option<String>,

    /// Log additional application diagnostics.
    #[arg(short, long)]
    verbose: bool,

    /// Start the inherited sample interface for migration comparisons.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long)]
    demo: bool,

    /// Use the Navidrome screens with deterministic sample albums.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, requires = "demo")]
    demo_navidrome: bool,

    #[cfg(feature = "navidrome-demo")]
    #[arg(long, requires = "demo")]
    demo_update_feed: Option<String>,

    #[cfg(feature = "navidrome-demo")]
    #[arg(long, requires = "demo")]
    demo_data: Option<std::path::PathBuf>,

    /// Page to open in demo mode, e.g. `home`, `playlist:pl1`, `artist:art0`.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long)]
    demo_page: Option<String>,

    /// Extra demo surfaces: a comma-separated list of `queue`, `playing-next`,
    /// `devices`, `shortcuts`, `create`, `light`, `focus`, `update`, `personal-app`,
    /// `windows-taskbar`, `german`, `lyrics`, `lyrics-fullscreen`, `collection-loading`,
    /// `shuffle-selected`, `shuffle-started`, `undated-mix`, `signed-out`, `connecting`, `library-list`,
    /// `library-list-narrow`, `library-list-wide`, `library-grid`, `library-grid-narrow`,
    /// or `library-grid-wide`.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long)]
    demo_show: Option<String>,

    /// Interface language, in place of the saved setting and the system's.
    /// Requires demo mode.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, value_enum)]
    demo_language: Option<navifast::i18n::Locale>,

    /// Write a PNG of the demo window to this path and exit. Implies
    /// `--demo`. Without `--demo-size`, the shot is the window's own frame
    /// buffer: full screen where that request is honoured, and the size of
    /// the tile under a tiling window manager, which decides for itself.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, value_name = "PATH")]
    demo_shot: Option<std::path::PathBuf>,

    /// How long to let cover art download before the shot is taken.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, value_name = "MS", default_value_t = 6000)]
    demo_shot_delay: u64,

    /// Inner window size for `--demo-shot`, as `WIDTHxHEIGHT`.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, value_name = "WIDTHxHEIGHT", value_parser = parse_demo_size)]
    demo_size: Option<[f32; 2]>,

    /// Hold a drag for `--demo-shot`: press the primary button at the first
    /// point, move to the second and keep it held, as `X,Y:X,Y` in points.
    #[cfg(feature = "navidrome-demo")]
    #[arg(long, value_name = "X,Y:X,Y", value_parser = parse_demo_drag)]
    demo_drag: Option<[egui::Pos2; 2]>,
}

impl Cli {
    #[cfg(feature = "navidrome-demo")]
    fn requests_demo(&self) -> bool {
        self.demo || self.demo_shot.is_some()
    }

    fn uses_navidrome_desktop(&self) -> bool {
        #[cfg(feature = "navidrome-demo")]
        {
            !self.requests_demo() || self.demo_navidrome
        }
        #[cfg(not(feature = "navidrome-demo"))]
        {
            true
        }
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
    /// Play a Spotify URI: a track, album, playlist, artist, or show
    #[command(hide = true)]
    PlayUri { uri: String },
    /// List the Spotify Connect devices
    #[command(hide = true)]
    Devices {
        /// Print the JSON the running instance sent instead.
        #[arg(long)]
        raw: bool,
    },
    /// Move playback to a device, by the id `devices` prints
    #[command(hide = true)]
    Transfer { device_id: String },
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
    let raw = matches!(
        control,
        Control::NowPlaying { raw: true } | Control::Devices { raw: true }
    );
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
        Control::PlayUri { uri } => format!("play-uri {uri}"),
        Control::Devices { .. } => "devices".to_owned(),
        Control::Transfer { device_id } => format!("transfer {device_id}"),
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
        Ok(single_instance::Reply::Devices(snapshot)) => {
            if raw {
                println!("{snapshot}");
            } else {
                print!("{}", format_devices(&snapshot));
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

/// Gives the librespot PulseAudio backend useful stream metadata.
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
#[cfg(not(target_os = "linux"))]
fn format_devices(snapshot: &str) -> String {
    let Ok(devices) = serde_json::from_str::<Vec<serde_json::Value>>(snapshot) else {
        return String::new();
    };
    let field =
        |device: &serde_json::Value, key: &str| device[key].as_str().unwrap_or_default().to_owned();
    devices
        .iter()
        .map(|device| {
            format!(
                "{}{}\t{}\t{}\n",
                if device["active"].as_bool().unwrap_or(false) {
                    "* "
                } else {
                    "  "
                },
                field(device, "id"),
                field(device, "name"),
                field(device, "kind"),
            )
        })
        .collect()
}

pub(crate) fn run() -> eframe::Result<()> {
    #[cfg(target_os = "linux")]
    configure_pulseaudio_properties();
    // First of all: `--apply-update <job>` makes this process the update
    // helper, which installs and exits; otherwise the receipt and error an
    // update relaunch carries are taken out of the arguments.
    let launch = navifast::updates::intercept();
    // A MilkDrop child launch is a bare visualiser window, not the app: it has
    // its own event loop and OpenGL context, reads the sound from a shared
    // buffer, and never touches the app's state. Handle it before anything
    // else, including the argument parser, which does not know its flags.
    #[cfg(feature = "milkdrop")]
    if let Some(args) = navifast::milkdrop::child::Args::parse() {
        std::process::exit(navifast::milkdrop::child::run(args));
    }

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
    // A link is read before anything starts: one that is not a Spotify
    // link ends the launch here rather than reaching the running instance.
    let link = cli
        .link
        .as_deref()
        .map(|text| match navifast::link::parse(text) {
            Some(uri) => uri,
            None => {
                eprintln!("not a Spotify link: {text}");
                std::process::exit(2);
            }
        });
    #[cfg(feature = "navidrome-demo")]
    let demo = cli.requests_demo();
    if cli.uses_navidrome_desktop() {
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
        return app::navidrome::desktop::run(
            app::navidrome::desktop::Launch {
                dirs,
                receipt: launch.receipt,
                update_error: launch.error,
                link,
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
        .map_err(|error| eframe::Error::AppCreation(error.into()));
    }
    #[cfg(feature = "demo")]
    return legacy::run(cli, launch, link);
    #[cfg(not(feature = "demo"))]
    Err(eframe::Error::AppCreation("The inherited demo requires --features demo. Use --demo --demo-navidrome for the Rune preview.".into()))
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

#[cfg(feature = "navidrome-demo")]
fn parse_demo_drag(spec: &str) -> Result<[egui::Pos2; 2], String> {
    let point = |point: &str| -> Result<egui::Pos2, String> {
        let (x, y) = point
            .split_once(',')
            .ok_or_else(|| format!("expected X,Y, got {point}"))?;
        let x: f32 = x
            .trim()
            .parse()
            .map_err(|_| format!("invalid x in {point}"))?;
        let y: f32 = y
            .trim()
            .parse()
            .map_err(|_| format!("invalid y in {point}"))?;
        Ok(egui::pos2(x, y))
    };
    let (from, to) = spec
        .split_once(':')
        .ok_or_else(|| format!("expected X,Y:X,Y, got {spec}"))?;
    Ok([point(from)?, point(to)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_commands_launch_the_navidrome_desktop_by_default() {
        for name in ["navifast", "fastpotify"] {
            assert!(
                Cli::try_parse_from([name])
                    .unwrap()
                    .uses_navidrome_desktop()
            );
        }
    }

    #[cfg(feature = "navidrome-demo")]
    #[test]
    fn navidrome_previews_use_the_same_desktop_as_a_normal_launch() {
        for (args, navidrome) in [
            (vec!["navifast", "--demo", "--demo-navidrome"], true),
            (vec!["navifast", "--demo"], false),
            (vec!["navifast", "--demo-shot", "screen.png"], false),
        ] {
            assert_eq!(
                Cli::try_parse_from(args).unwrap().uses_navidrome_desktop(),
                navidrome
            );
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

    /// A link on the command line is a link, and a control verb is still a
    /// verb: the two do not get in each other's way.
    #[test]
    fn a_link_and_a_verb_are_told_apart() {
        // #given / #when / #then
        let launch = Cli::try_parse_from(["fastpotify", "spotify:track:4uLU6hMCjMI75M1A2tKUQC"])
            .expect("a link parses");
        assert_eq!(
            launch.link.as_deref(),
            Some("spotify:track:4uLU6hMCjMI75M1A2tKUQC")
        );
        assert!(launch.control.is_none());

        let launch = Cli::try_parse_from([
            "fastpotify",
            "https://open.spotify.com/album/1DFixLWuPkv3KT3TnV35m3?si=x",
            "--verbose",
        ])
        .expect("a web address parses");
        assert!(launch.link.is_some());
        assert!(launch.verbose);

        let verb = Cli::try_parse_from(["fastpotify", "next"]).expect("a verb parses");
        assert!(matches!(verb.control, Some(Control::Next)));
        assert!(verb.link.is_none());

        let bare = Cli::try_parse_from(["fastpotify"]).expect("a plain launch parses");
        assert!(bare.link.is_none() && bare.control.is_none());
    }
}
