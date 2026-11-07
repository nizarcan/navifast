//! Daily update check against GitHub releases.

use std::time::Duration;

use anyhow::Result;
use serde::Deserialize;

#[path = "install.rs"]
pub mod install;
#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod macos;
#[path = "transfer.rs"]
mod transfer;
pub use install::{Installation, Kind, Prepared};
pub use transfer::Source;

#[derive(Default)]
pub enum DownloadState {
    #[default]
    Idle,
    Downloading {
        received: u64,
        total: u64,
    },
    Ready(Box<install::Prepared>),
    Installing,
    Failed(String),
}

const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/nizarcan/navifast/releases/latest";

/// Update-check interval.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The version number, without a leading `v`.
    pub version: String,
    /// The release page, with every download.
    pub url: String,
}

#[derive(Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: String,
}

/// `major.minor.patch`, and whether a `-rc1` or similar suffix marks it
/// as a pre-release of that version; anything else is `None`.
fn parse(version: &str) -> Option<([u64; 3], bool)> {
    let version = version.trim();
    let (numbers, pre_release) = match version.split_once('-') {
        Some((numbers, _)) => (numbers, true),
        None => (version, false),
    };
    let mut parts = numbers.split('.').map(|part| part.parse::<u64>().ok());
    let numbers = [parts.next()??, parts.next()??, parts.next()??];
    parts.next().is_none().then_some((numbers, pre_release))
}

/// Whether `candidate` is a newer stable version than `current`.
/// Stable releases supersede their release candidates. Other prereleases and
/// invalid versions are ignored.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse(candidate), parse(current)) {
        (Some((candidate, false)), Some((current, current_pre))) => {
            candidate > current || (candidate == current && current_pre)
        }
        _ => false,
    }
}

#[derive(Clone, Copy)]
pub struct Config {
    pub slug: &'static str,
    pub legacy_names: &'static [&'static str],
}
pub const CONFIG: Config = Config {
    slug: "navifast",
    legacy_names: &["fastpotify"],
};

pub struct Updater {
    proxy: crate::settings::ProxyConfig,
    http: reqwest::blocking::Client,
    source: Source,
}

pub fn updater(proxy: &crate::settings::ProxyConfig) -> Result<Updater> {
    let http = crate::http::blocking_builder(proxy)
        .map_err(anyhow::Error::msg)?
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(30))
        .build()?;
    Ok(Updater {
        proxy: proxy.clone(),
        http,
        source: Source::default(),
    })
}

impl Updater {
    pub fn with_source(mut self, source: Source) -> Self {
        self.source = source;
        self
    }
    pub fn installation(&self) -> Result<Installation> {
        install::detect()
    }
    pub fn installation_at(&self, path: &std::path::Path) -> Result<Installation> {
        install::detect_at(path)
    }
    pub fn check(&self) -> Result<Option<Release>> {
        let response: LatestRelease = self
            .http
            .get(self.source.latest())
            .header("Accept", "application/vnd.github+json")
            .send()?
            .error_for_status()?
            .json()?;
        let version = response.tag_name.trim_start_matches('v').to_owned();
        Ok(
            is_newer(&version, env!("CARGO_PKG_VERSION")).then_some(Release {
                version,
                url: response.html_url,
            }),
        )
    }
    pub fn download(&self, release: &Release, progress: impl Fn(u64, u64)) -> Result<Prepared> {
        transfer::download(release, &self.source, &self.proxy, progress)
    }
    pub fn handoff(&self, prepared: Prepared, arguments: Vec<String>) -> Result<()> {
        install::handoff(&prepared, arguments)
    }
}

pub struct Receipt(std::path::PathBuf);
impl Receipt {
    pub fn acknowledge(&self) -> Result<()> {
        install::acknowledge(&self.0)
    }
}

pub struct Launch {
    pub arguments: Vec<std::ffi::OsString>,
    pub receipt: Option<Receipt>,
    pub error: Option<String>,
}

pub fn intercept() -> Launch {
    let mut args = std::env::args_os();
    let program = args.next().unwrap_or_default();
    let mut launch = Launch {
        arguments: vec![program],
        receipt: None,
        error: None,
    };
    let mut args = args.peekable();
    if args.peek().is_some_and(|arg| arg == "--apply-update") {
        args.next();
        if let Some(path) = args.next() {
            match install::run_helper(std::path::Path::new(&path)) {
                Ok(()) => std::process::exit(0),
                Err(error) => {
                    eprintln!("{error:#}");
                    std::process::exit(1);
                }
            }
        }
        launch.arguments.push("--apply-update".into());
    }
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        let (flag, inline) = text
            .split_once('=')
            .map_or((text.as_ref(), None), |(f, v)| (f, Some(v)));
        if !matches!(flag, "--update-receipt" | "--update-error") {
            launch.arguments.push(arg);
            continue;
        }
        let Some(value) = inline.map(std::ffi::OsString::from).or_else(|| args.next()) else {
            launch.arguments.push(arg);
            continue;
        };
        if flag == "--update-receipt" {
            launch.receipt = Some(Receipt(value.into()));
        } else {
            launch.error = Some(value.to_string_lossy().into_owned());
        }
    }
    launch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("0.1.4", "0.1.3"));
        assert!(is_newer("0.2.0", "0.1.9"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.1.3", "0.1.3"));
        assert!(!is_newer("0.1.2", "0.1.3"));
        assert!(
            !is_newer("0.2.0-rc1", "0.1.3"),
            "pre-releases are not announced"
        );
        assert!(!is_newer("nightly", "0.1.3"));
        // A release candidate hears about its release, and nothing older.
        assert!(is_newer("0.4.0", "0.4.0-rc1"));
        assert!(is_newer("0.4.1", "0.4.0-rc1"));
        assert!(!is_newer("0.4.0-rc1", "0.4.0"));
        assert!(!is_newer("0.4.0-rc2", "0.4.0-rc1"));
        assert!(!is_newer("0.3.0", "0.4.0-rc1"));
    }
}
