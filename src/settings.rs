//! User preferences, stored as one readable JSON file.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LibraryShelf {
    #[default]
    Playlists,
    Albums,
    Artists,
    Podcasts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySort {
    Library,
    RecentlyPlayed,
    Name,
    RecentlyAdded,
    Local,
    Spotify,
}

impl LibrarySort {
    pub fn supports(self, shelf: LibraryShelf) -> bool {
        match self {
            Self::RecentlyPlayed | Self::Name | Self::Library => true,
            Self::RecentlyAdded => matches!(shelf, LibraryShelf::Albums | LibraryShelf::Podcasts),
            Self::Local | Self::Spotify => shelf == LibraryShelf::Playlists,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    Dark,
    Light,
    #[default]
    System,
}

/// What moves behind the player bar's controls.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerBarVis {
    #[default]
    Off,
    Spectrum,
    Waveform,
}

impl PlayerBarVis {
    /// The mode a click on the player bar moves to: spectrum, waveform,
    /// then off, as Winamp's visualizer cycles.
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Spectrum,
            Self::Spectrum => Self::Waveform,
            Self::Waveform => Self::Off,
        }
    }
}

/// The interface language: the operating system's, or one chosen in Settings.
///
/// Stored as `"system"` or a locale tag such as `"es"` or `"pt-BR"`. A file
/// without the field follows the system; a tag this version does not carry
/// also follows the system, rather than making the whole file unreadable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanguageChoice {
    #[default]
    System,
    Locale(crate::i18n::Locale),
}

impl LanguageChoice {
    /// The locale the interface is drawn in.
    pub fn resolve(self) -> crate::i18n::Locale {
        match self {
            Self::System => crate::i18n::Locale::from_system(),
            Self::Locale(locale) => locale,
        }
    }
}

impl Serialize for LanguageChoice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::System => "system",
            Self::Locale(locale) => locale.tag(),
        })
    }
}

impl<'de> Deserialize<'de> for LanguageChoice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(value
            .as_str()
            .and_then(crate::i18n::Locale::from_tag)
            .map_or(Self::System, Self::Locale))
    }
}

impl ThemeChoice {
    pub const ALL: [ThemeChoice; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::{gettext, pgettext};
        match self {
            Self::Dark => pgettext(locale, "theme", "Dark"),
            Self::Light => pgettext(locale, "theme", "Light"),
            Self::System => gettext(locale, "Follow system"),
        }
    }
}

/// How outbound HTTP traffic reaches the network.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    /// No proxy, ignoring environment and OS proxy settings.
    Off,
    /// Environment variables and, on macOS and Windows, the OS proxy.
    #[default]
    System,
    /// A configured HTTP proxy.
    Http,
    /// A configured SOCKS5 proxy.
    Socks,
}

impl ProxyMode {
    pub const ALL: [ProxyMode; 4] = [Self::Off, Self::System, Self::Http, Self::Socks];

    pub fn label(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::pgettext;
        match self {
            Self::Off => pgettext(locale, "proxy", "Off"),
            Self::System => pgettext(locale, "proxy", "System"),
            Self::Http => "HTTP".into(),
            Self::Socks => "SOCKS5".into(),
        }
    }

    pub fn is_manual(self) -> bool {
        matches!(self, Self::Http | Self::Socks)
    }
}

/// Only confirmed proxy preferences are written with other settings. The
/// password stays in memory and the protected store, never in this snapshot.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyPreferences {
    mode: ProxyMode,
    host: String,
    port: String,
    username: String,
}

impl ProxyPreferences {
    pub fn apply_to(&self, settings: &mut Settings) {
        settings.proxy_mode = self.mode;
        settings.proxy_host.clone_from(&self.host);
        settings.proxy_port.clone_from(&self.port);
        settings.proxy_username.clone_from(&self.username);
    }
}

fn proxy_mode_is_system(mode: &ProxyMode) -> bool {
    *mode == ProxyMode::System
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: ThemeChoice,
    /// The interface language; older files without it follow the system.
    pub language: LanguageChoice,
    /// Filename selected from the local themes directory.
    pub custom_theme: Option<String>,
    /// Last accepted appearance, retained if its source file becomes unavailable.
    #[serde(
        default,
        deserialize_with = "crate::theme::read_cached_theme",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_theme_cache: Option<crate::theme::CustomTheme>,
    /// Last detected system palette, so following Omarchy survives a restart.
    #[serde(
        default,
        deserialize_with = "crate::theme::read_cached_theme",
        skip_serializing_if = "Option::is_none"
    )]
    pub system_theme_cache: Option<crate::theme::CustomTheme>,
    /// Tint the interface with the colour of the playing album's art.
    pub accent_from_art: bool,
    /// A spectrum or waveform of the playing song behind the player bar.
    pub player_bar_vis: PlayerBarVis,
    /// Last local volume, 0..=65535.
    pub volume: u16,
    /// Whether the library sidebar is visible.
    pub sidebar_visible: bool,
    /// The playing album's art docked large at the sidebar's bottom.
    pub art_expanded: bool,
    /// Use compact single-line rows without cover art in the sidebar.
    pub sidebar_compact: bool,
    /// Show the Library as responsive cover cards instead of rows.
    pub sidebar_grid: bool,
    pub sidebar_width: f32,
    /// Use compact single-line rows without cover art in track lists.
    pub tracklist_compact: bool,
    /// Linux: middle-click a list to autoscroll it. Off by default, because
    /// Linux desktops usually paste the primary selection on middle click.
    /// Windows always autoscrolls and macOS never does.
    pub middle_click_autoscroll: bool,
    pub search_history: Vec<String>,
    /// Ask GitHub once a day whether a newer release exists.
    pub check_for_updates: bool,
    /// Explicit order per Library shelf. Missing shelves keep their previous
    /// behaviour; selecting another order never deletes the local arrangement.
    pub library_sort: std::collections::BTreeMap<LibraryShelf, LibrarySort>,
    /// Interface zoom; Ctrl+plus/minus changes it.
    pub zoom: f32,
    /// The equalizer shapes local playback.
    pub eq_on: bool,
    /// The preamp, in decibels, never above zero.
    pub eq_preamp_db: f32,
    /// The ten bands, in decibels, 60 Hz to 16 kHz.
    pub eq_bands_db: [f32; 10],
    /// The balance, -1 all left to 1 all right.
    pub balance: f32,
    /// Play both channels the same.
    pub mono: bool,
    /// Which proxy to use. Older files without this field stay on `system`.
    #[serde(default, skip_serializing_if = "proxy_mode_is_system")]
    pub proxy_mode: ProxyMode,
    /// Combined address from older settings files. Split into host and port
    /// on load; not written again.
    #[serde(default, skip_serializing)]
    pub proxy: String,
    /// Host of a manual HTTP or SOCKS5 proxy. Ignored when the mode is Off
    /// or System.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_host: String,
    /// Port of a manual HTTP or SOCKS5 proxy.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_port: String,
    /// Optional proxy login for Web requests.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub proxy_username: String,
    /// Optional proxy password, in memory and the platform credential store.
    /// Legacy JSON copies remain until protected migration succeeds.
    #[serde(default, skip_serializing)]
    pub proxy_password: String,
    /// Legacy settings and their password must survive until protected migration
    /// succeeds. This flag is only in memory and is never persisted.
    #[serde(skip)]
    pub proxy_password_legacy: bool,
    /// Retain historical preferences without compiling their retired features.
    #[serde(flatten)]
    pub retained: std::collections::BTreeMap<String, serde_json::Value>,
}

impl std::fmt::Debug for Settings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Serialization already omits the password and old combined URL.
        // Keep the proxy username out of diagnostics as well.
        let mut preferences = serde_json::to_value(self).map_err(|_| std::fmt::Error)?;
        if let Some(fields) = preferences.as_object_mut() {
            fields.remove("proxy_username");
            for key in self.retained.keys() {
                fields.remove(key);
            }
        }
        f.debug_struct("Settings")
            .field("preferences", &preferences)
            .finish()
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            retained: Default::default(),
            theme: ThemeChoice::System,
            language: LanguageChoice::System,
            custom_theme: None,
            custom_theme_cache: None,
            system_theme_cache: None,
            accent_from_art: true,
            player_bar_vis: PlayerBarVis::Off,
            volume: (u16::MAX as u32 * 70 / 100) as u16,
            sidebar_visible: true,
            art_expanded: false,
            sidebar_compact: false,
            sidebar_grid: false,
            sidebar_width: 250.0,
            tracklist_compact: false,
            middle_click_autoscroll: false,
            search_history: Vec::new(),
            check_for_updates: true,
            library_sort: std::collections::BTreeMap::new(),
            zoom: 1.0,
            eq_on: false,
            eq_preamp_db: 0.0,
            eq_bands_db: [0.0; 10],
            balance: 0.0,
            mono: false,
            proxy_mode: ProxyMode::System,
            proxy: String::new(),
            proxy_host: String::new(),
            proxy_port: String::new(),
            proxy_username: String::new(),
            proxy_password: String::new(),
            proxy_password_legacy: false,
        }
    }
}

impl Settings {
    pub(crate) fn cached_palette(&self) -> Option<crate::theme::Palette> {
        let theme = if self.custom_theme.is_some() {
            self.custom_theme_cache.as_ref()
        } else if self.theme == ThemeChoice::System {
            self.system_theme_cache.as_ref()
        } else {
            None
        };
        theme.map(|theme| theme.palette)
    }

    pub(crate) fn proxy_password_record(
        &self,
    ) -> Result<Option<crate::credentials::ProxyPassword>, String> {
        if self.proxy_password.is_empty() {
            return Ok(None);
        }
        let manual = ManualProxy::parse(
            ManualKind::Http,
            &self.proxy_host,
            &self.proxy_port,
            &self.proxy_username,
            &self.proxy_password,
        )?;
        Ok(manual.password_record())
    }

    pub fn proxy_preferences(&self) -> ProxyPreferences {
        ProxyPreferences {
            mode: self.proxy_mode,
            host: self.proxy_host.clone(),
            port: self.proxy_port.clone(),
            username: self.proxy_username.clone(),
        }
    }

    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => {
                // A file edited in Notepad or Windows PowerShell can start with
                // a byte order mark, which JSON does not allow.
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                let mut settings = serde_json::from_str(text).unwrap_or_else(|error| {
                    log::warn!("settings at {} are unreadable: {error}", path.display());
                    Self::default()
                });
                settings.migrate_proxy(text);
                settings.proxy_password_legacy = !settings.proxy_password.is_empty();
                settings
            }
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = match self.encode() {
            Ok(text) => text,
            Err(error) => {
                log::warn!("unable to encode settings: {error}");
                return;
            }
        };
        let temporary = path.with_extension("json.tmp");
        let written = std::fs::write(&temporary, text)
            .and_then(|()| crate::util::replace_file(&temporary, path));
        if let Err(error) = written {
            log::warn!("unable to save settings to {}: {error}", path.display());
        }
    }

    fn encode(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn remember_search(&mut self, query: &str) {
        let query = query.trim();
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|entry| entry != query);
        self.search_history.insert(0, query.to_string());
        self.search_history.truncate(12);
    }

    pub(crate) fn migrate_proxy(&mut self, text: &str) {
        if !text.contains("\"proxy_mode\"") {
            self.proxy_mode = infer_legacy_proxy_mode(&self.proxy);
        }
        if self.proxy_host.is_empty() && !self.proxy.trim().is_empty() {
            let (host, port) = split_legacy_address(&self.proxy);
            self.proxy_host = host;
            if self.proxy_port.is_empty() {
                self.proxy_port = port;
            }
        }
    }

    /// The proxy the HTTP client should use. Off and System always succeed.
    /// HTTP and SOCKS5 need a host and port.
    pub fn proxy_config(&self) -> Result<ProxyConfig, String> {
        match self.proxy_mode {
            ProxyMode::Off => Ok(ProxyConfig::Off),
            ProxyMode::System => Ok(ProxyConfig::System),
            ProxyMode::Http => Ok(ProxyConfig::Http(ManualProxy::parse(
                ManualKind::Http,
                &self.proxy_host,
                &self.proxy_port,
                &self.proxy_username,
                &self.proxy_password,
            )?)),
            ProxyMode::Socks => Ok(ProxyConfig::Socks(ManualProxy::parse(
                ManualKind::Socks,
                &self.proxy_host,
                &self.proxy_port,
                &self.proxy_username,
                &self.proxy_password,
            )?)),
        }
    }
}

fn infer_legacy_proxy_mode(proxy: &str) -> ProxyMode {
    let raw = proxy.trim();
    if raw.is_empty() {
        return ProxyMode::System;
    }
    let scheme = raw.split("://").next().unwrap_or("").to_ascii_lowercase();
    match scheme.as_str() {
        "socks" | "socks5" | "socks5h" => ProxyMode::Socks,
        _ => ProxyMode::Http,
    }
}

fn validate_host(host: &str) -> Result<String, String> {
    let host = host.trim();
    if host.is_empty() {
        return Err("enter a host".into());
    }
    let host = host
        .split_once("://")
        .map_or(host, |(_, rest)| rest)
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host.is_empty() {
        return Err("enter a host".into());
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return Ok(host.to_string());
    }
    if is_hostname(host) {
        return Ok(host.to_string());
    }
    Err("the host must be a hostname or IP address".into())
}

fn is_hostname(host: &str) -> bool {
    if host.len() > 253 || host.starts_with('.') || host.ends_with('.') {
        return false;
    }
    host.split('.').all(|label| {
        (1..=63).contains(&label.len())
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    })
}

fn validate_port(port: &str) -> Result<u16, String> {
    let port = port.trim();
    if port.is_empty() {
        return Err("enter a port".into());
    }
    if !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("the port must be a number between 1 and 65535".into());
    }
    match port.parse::<u16>() {
        Ok(port) if port > 0 => Ok(port),
        _ => Err("the port must be a number between 1 and 65535".into()),
    }
}

fn split_legacy_address(raw: &str) -> (String, String) {
    let raw = raw.trim();
    let rest = raw.split_once("://").map_or(raw, |(_, rest)| rest);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
    if let Some(inner) = rest.strip_prefix('[')
        && let Some((host, port)) = inner.split_once("]:")
    {
        return (host.to_string(), port.trim().to_string());
    }
    if let Some((host, port)) = rest.rsplit_once(':')
        && !host.contains(':')
    {
        return (host.to_string(), port.trim().to_string());
    }
    (rest.to_string(), String::new())
}

/// The resolved proxy policy used by the HTTP client and local playback.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ProxyConfig {
    /// Invalid saved settings block requests until corrected in the interface.
    Invalid(String),
    Off,
    #[default]
    System,
    Http(ManualProxy),
    Socks(ManualProxy),
}

#[derive(Clone, Copy)]
enum ManualKind {
    Http,
    Socks,
}

/// A configured HTTP or SOCKS5 endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct ManualProxy {
    endpoint: reqwest::Url,
    username: String,
    password: String,
}

impl std::fmt::Debug for ManualProxy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ManualProxy")
            .field("url", &self.redacted())
            .finish()
    }
}

impl ManualProxy {
    fn password_record(&self) -> Option<crate::credentials::ProxyPassword> {
        if self.password.is_empty() {
            return None;
        }
        Some(crate::credentials::ProxyPassword {
            host: self.endpoint.host_str()?.to_string(),
            port: self.endpoint.port()?,
            username: self.username.clone(),
            password: self.password.clone(),
        })
    }

    fn parse(
        kind: ManualKind,
        host: &str,
        port: &str,
        username: &str,
        password: &str,
    ) -> Result<Self, String> {
        let host = validate_host(host)?;
        let port = validate_port(port)?;
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let scheme = match kind {
            ManualKind::Http => "http",
            // Resolve server hostnames at the proxy, so SOCKS mode does not
            // leak DNS queries or fail behind a proxy-only network.
            ManualKind::Socks => "socks5h",
        };
        let endpoint = reqwest::Url::parse(&format!("{scheme}://{host}:{port}"))
            .map_err(|error| format!("not a proxy address: {error}"))?;
        Ok(Self {
            endpoint,
            username: username.to_string(),
            password: password.to_string(),
        })
    }

    /// A reqwest proxy with credentials applied separately so build errors
    /// cannot echo a password.
    pub fn reqwest_proxy(&self) -> Result<reqwest::Proxy, reqwest::Error> {
        let mut proxy = reqwest::Proxy::all(self.endpoint.clone())?;
        if !self.username.is_empty() || !self.password.is_empty() {
            proxy = proxy.basic_auth(&self.username, &self.password);
        }
        Ok(proxy)
    }

    /// The credential-free endpoint, for logs and the debugger.
    pub fn redacted(&self) -> reqwest::Url {
        self.endpoint.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn historical_preferences_round_trip_without_recreating_retired_defaults() {
        let original = serde_json::json!({"volume": 12345, "home": {"made_for_you": {"visible": false}}, "skin": "custom.wsz", "audio_cache_mb": 777, "pinned_contexts": ["spotify:playlist:one"]});
        let settings: super::Settings = serde_json::from_value(original.clone()).unwrap();
        let saved = serde_json::to_value(&settings).unwrap();
        for (key, value) in original.as_object().unwrap() {
            assert_eq!(&saved[key], value);
        }
        let defaults = serde_json::to_value(super::Settings::default()).unwrap();
        for key in ["home", "skin", "audio_cache_mb", "pinned_contexts"] {
            assert!(defaults.get(key).is_none());
        }
        assert!(!format!("{settings:?}").contains("spotify:playlist"));
    }

    #[test]
    fn new_profiles_follow_the_system_and_saved_choices_are_preserved() {
        use super::ThemeChoice;
        assert_eq!(Settings::default().theme, ThemeChoice::System);
        assert_eq!(ThemeChoice::default(), ThemeChoice::System);
        let empty: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.theme, ThemeChoice::System);
        for (json, choice) in [("dark", ThemeChoice::Dark), ("light", ThemeChoice::Light)] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"theme": json, "volume": 37})).unwrap();
            assert_eq!(settings.theme, choice);
            assert_eq!(settings.volume, 37);
        }
        let settings: Settings = serde_json::from_value(serde_json::json!({
            "theme": "dark", "system_theme_cache": {"broken": true}, "volume": 37
        }))
        .unwrap();
        assert!(settings.system_theme_cache.is_none());
        assert_eq!(settings.theme, ThemeChoice::Dark);
        assert_eq!(settings.volume, 37);
    }

    #[test]
    fn language_follows_the_system_unless_a_known_locale_was_chosen() {
        use super::LanguageChoice;
        use crate::i18n::Locale;
        assert_eq!(Settings::default().language, LanguageChoice::System);
        let older: Settings = serde_json::from_value(serde_json::json!({"volume": 37})).unwrap();
        assert_eq!(older.language, LanguageChoice::System);
        assert_eq!(older.volume, 37);
        for (json, choice) in [
            (serde_json::json!("system"), LanguageChoice::System),
            (
                serde_json::json!("es"),
                LanguageChoice::Locale(Locale::Spanish),
            ),
            (
                serde_json::json!("pt-BR"),
                LanguageChoice::Locale(Locale::PortugueseBrazil),
            ),
            (
                serde_json::json!("zh-Hant"),
                LanguageChoice::Locale(Locale::ChineseTraditional),
            ),
            (
                serde_json::json!("de"),
                LanguageChoice::Locale(Locale::German),
            ),
            // A language a later version added, or a hand-edited typo, keeps
            // the rest of the file and follows the system.
            (serde_json::json!("tlh"), LanguageChoice::System),
            (serde_json::json!(7), LanguageChoice::System),
            (serde_json::Value::Null, LanguageChoice::System),
        ] {
            let settings: Settings =
                serde_json::from_value(serde_json::json!({"language": json, "volume": 37}))
                    .unwrap();
            assert_eq!(settings.language, choice, "{json}");
            assert_eq!(settings.volume, 37);
        }
        for &locale in crate::i18n::LOCALES {
            let settings = Settings {
                language: LanguageChoice::Locale(locale),
                ..Default::default()
            };
            let text = serde_json::to_string(&settings).unwrap();
            assert!(text.contains(&format!("\"language\":\"{}\"", locale.tag())));
            let saved: Settings = serde_json::from_str(&text).unwrap();
            assert_eq!(saved.language, settings.language);
        }
        let text = serde_json::to_string(&Settings::default()).unwrap();
        assert!(text.contains("\"language\":\"system\""));
        assert_eq!(
            LanguageChoice::System.resolve(),
            Locale::English,
            "tests pin English"
        );
    }

    #[test]
    fn custom_theme_cache_round_trips_and_a_bad_cache_keeps_other_settings() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.custom_theme.is_none() && settings.custom_theme_cache.is_none());
        settings.custom_theme = Some("gruvbox.json".into());
        let mut palette = crate::theme::Palette::light();
        palette.shadow = crate::color::Srgba::from_rgba_unmultiplied(37, 128, 249, 117);
        settings.custom_theme_cache = Some(crate::theme::CustomTheme {
            filename: "gruvbox.json".into(),
            palette,
        });
        let encoded = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<Settings>(&encoded).unwrap(),
            settings
        );
        let mut damaged = serde_json::to_value(&settings).unwrap();
        damaged["custom_theme_cache"] = serde_json::json!({"palette": "broken"});
        damaged["audio_cache_mb"] = 777.into();
        let recovered: Settings = serde_json::from_value(damaged).unwrap();
        assert_eq!(recovered.custom_theme.as_deref(), Some("gruvbox.json"));
        assert_eq!(recovered.retained["audio_cache_mb"], 777);
        assert!(recovered.custom_theme_cache.is_none());
    }

    /// Notepad and Windows PowerShell can save UTF-8 with a byte order mark.
    /// A file edited that way, as the Home shelves guide asks, is still read
    /// rather than dropped as unreadable and replaced with the defaults.
    #[test]
    fn a_settings_file_saved_with_a_byte_order_mark_keeps_its_preferences() {
        let dir = std::env::temp_dir().join(format!("navifast-bom-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            "\u{feff}{\"volume\":12345,\"home\":{\"made_for_you\":{\"visible\":false}}}",
        )
        .unwrap();
        let settings = Settings::load(&path);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(settings.volume, 12345);
        assert_eq!(settings.retained["home"]["made_for_you"]["visible"], false);
    }

    #[test]
    fn older_settings_keep_the_sidebar_visible() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.sidebar_visible);
    }

    #[test]
    fn hidden_sidebar_round_trips() {
        let settings = Settings {
            sidebar_visible: false,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(!restored.sidebar_visible);
    }

    #[test]
    fn older_settings_default_to_standard_sidebar() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.sidebar_compact);
    }

    #[test]
    fn compact_sidebar_round_trips() {
        let settings = Settings {
            sidebar_compact: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.sidebar_compact);
    }

    #[test]
    fn library_grid_round_trips_and_older_settings_keep_the_list() {
        let old: Settings = serde_json::from_str("{}").unwrap();
        assert!(!old.sidebar_grid);

        let settings = Settings {
            sidebar_grid: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.sidebar_grid);
    }

    #[test]
    fn older_settings_default_to_standard_tracklist() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.tracklist_compact);
    }

    #[test]
    fn compact_tracklist_round_trips() {
        let settings = Settings {
            tracklist_compact: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.tracklist_compact);
    }

    #[test]
    fn the_player_bar_visualizer_is_opt_in_and_round_trips() {
        use super::PlayerBarVis;
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.player_bar_vis, PlayerBarVis::Off);
        for mode in [PlayerBarVis::Spectrum, PlayerBarVis::Waveform] {
            let settings = Settings {
                player_bar_vis: mode,
                ..Settings::default()
            };
            let json = serde_json::to_string(&settings).unwrap();
            let restored: Settings = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.player_bar_vis, mode);
        }
        let spectrum: Settings = serde_json::from_str(r#"{"player_bar_vis":"spectrum"}"#).unwrap();
        assert_eq!(spectrum.player_bar_vis, PlayerBarVis::Spectrum);
        assert_eq!(PlayerBarVis::Off.next(), PlayerBarVis::Spectrum);
        assert_eq!(PlayerBarVis::Spectrum.next(), PlayerBarVis::Waveform);
        assert_eq!(PlayerBarVis::Waveform.next(), PlayerBarVis::Off);
    }

    #[test]
    fn middle_click_autoscroll_is_opt_in_and_round_trips() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.middle_click_autoscroll);
        let settings = Settings {
            middle_click_autoscroll: true,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert!(restored.middle_click_autoscroll);
    }

    #[test]
    fn older_settings_use_the_system_proxy() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.proxy_mode, super::ProxyMode::System);
        assert!(settings.proxy_host.is_empty());
        assert!(settings.proxy_port.is_empty());
        assert!(settings.proxy_username.is_empty());
        assert!(settings.proxy_password.is_empty());
        assert_eq!(settings.proxy_config().unwrap(), super::ProxyConfig::System);
    }

    #[test]
    fn a_proxy_round_trips_through_settings() {
        let settings = Settings {
            proxy_mode: super::ProxyMode::Socks,
            proxy_host: "127.0.0.1".into(),
            proxy_port: "1080".into(),
            proxy_username: "alice".into(),
            proxy_password: "secret".into(),
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.proxy_mode, settings.proxy_mode);
        assert_eq!(restored.proxy_host, settings.proxy_host);
        assert_eq!(restored.proxy_port, settings.proxy_port);
        assert_eq!(restored.proxy_username, settings.proxy_username);
        assert!(restored.proxy_password.is_empty());
        assert!(json.contains("socks"));
        assert!(json.contains("127.0.0.1"));
        assert!(json.contains("1080"));
        assert!(json.contains("alice"));
        assert!(!json.contains("secret"));
        assert!(!json.contains("proxy_password"));
        assert!(!json.contains("\"proxy\":"));
    }

    #[test]
    fn settings_debug_keeps_proxy_credentials_private() {
        let settings = Settings {
            proxy_username: "private-proxy-user".into(),
            proxy_password: "private-proxy-password".into(),
            ..Default::default()
        };
        let debug = format!("{settings:?}");
        assert!(!debug.contains("private-proxy-user"));
        assert!(!debug.contains("private-proxy-password"));
    }

    #[test]
    fn a_saved_proxy_password_is_bound_to_its_endpoint_and_username() {
        let original = Settings {
            proxy_mode: super::ProxyMode::Http,
            proxy_host: "127.0.0.1".into(),
            proxy_port: "8080".into(),
            proxy_username: "dummy-user".into(),
            proxy_password: " dummy-private-password\n".into(),
            ..Default::default()
        };
        let password = original.proxy_password_record().unwrap().unwrap();
        assert_eq!(password.host, "127.0.0.1");
        assert_eq!(password.port, 8080);
        assert_eq!(password.username, "dummy-user");
        assert_eq!(password.password, original.proxy_password);
        assert!(
            !serde_json::to_string(&original)
                .unwrap()
                .contains("dummy-private-password")
        );
    }

    #[test]
    fn an_empty_proxy_is_omitted_from_the_file() {
        let json = serde_json::to_string(&Settings::default()).unwrap();
        assert!(!json.contains("proxy"));
    }

    #[test]
    fn off_is_written_and_round_trips() {
        let settings = Settings {
            proxy_mode: super::ProxyMode::Off,
            ..Settings::default()
        };
        let json = serde_json::to_string(&settings).unwrap();
        assert!(json.contains("\"off\""));
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.proxy_mode, super::ProxyMode::Off);
        assert_eq!(restored.proxy_config().unwrap(), super::ProxyConfig::Off);
    }

    #[test]
    fn a_legacy_socks_url_becomes_socks_mode() {
        let dir = std::env::temp_dir().join(format!("navifast-proxy-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"proxy":"socks5://127.0.0.1:1080","proxy_username":"alice"}"#,
        )
        .unwrap();
        let settings = Settings::load(&path);
        assert_eq!(settings.proxy_mode, super::ProxyMode::Socks);
        assert_eq!(settings.proxy_host, "127.0.0.1");
        assert_eq!(settings.proxy_port, "1080");
        let super::ProxyConfig::Socks(manual) = settings.proxy_config().unwrap() else {
            panic!("expected a SOCKS proxy");
        };
        assert_eq!(manual.redacted().scheme(), "socks5h");
        assert_eq!(manual.redacted().host_str(), Some("127.0.0.1"));
        assert_eq!(manual.redacted().port(), Some(1080));
        assert!(manual.redacted().username().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn http_and_socks_parse_a_host_and_port() {
        let http = super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "7890", "", "")
            .unwrap();
        assert_eq!(http.redacted().as_str(), "http://127.0.0.1:7890/");

        let socks =
            super::ManualProxy::parse(super::ManualKind::Socks, "127.0.0.1", "1080", "", "")
                .unwrap();
        assert_eq!(socks.redacted().scheme(), "socks5h");
        assert_eq!(socks.redacted().port(), Some(1080));
    }

    #[test]
    fn proxy_credentials_stay_opaque_and_out_of_urls() {
        let proxy = super::ManualProxy::parse(
            super::ManualKind::Http,
            "127.0.0.1",
            "7890",
            "alice/name",
            " secret/with spaces ",
        )
        .unwrap();
        let redacted = proxy.redacted();
        assert!(redacted.username().is_empty());
        assert_eq!(redacted.password(), None);
        assert_eq!(proxy.username, "alice/name");
        assert_eq!(proxy.password, " secret/with spaces ");
        let debug = format!("{proxy:?}");
        assert!(!debug.contains("alice"));
        assert!(!debug.contains("secret"));
    }

    #[test]
    fn proxy_parse_rejects_a_missing_host_or_port() {
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "", "8080", "", "")
                .unwrap_err()
                .contains("host")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "", "", "")
                .unwrap_err()
                .contains("port")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "abc", "", "")
                .unwrap_err()
                .contains("number")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "not a host", "1080", "", "")
                .unwrap_err()
                .contains("hostname")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "0", "", "")
                .unwrap_err()
                .contains("65535")
        );
        assert!(
            super::ManualProxy::parse(super::ManualKind::Http, "127.0.0.1", "65536", "", "")
                .unwrap_err()
                .contains("65535")
        );
        super::ManualProxy::parse(super::ManualKind::Http, "localhost", "8080", "", "").unwrap();
        super::ManualProxy::parse(super::ManualKind::Http, "::1", "8080", "", "").unwrap();
    }
}

/// Table preferences retained in the existing session file.
/// Unknown historical fields round-trip without retaining obsolete service models.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub sorts: Vec<(String, crate::model::TableSort)>,
    #[serde(flatten)]
    pub retained: std::collections::BTreeMap<String, serde_json::Value>,
}

impl SessionState {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let text = match serde_json::to_string(self) {
            Ok(text) => text,
            Err(error) => {
                log::warn!("unable to encode session: {error}");
                return;
            }
        };
        let temporary = path.with_extension("json.tmp");
        let written = std::fs::write(&temporary, text)
            .and_then(|()| crate::util::replace_file(&temporary, path));
        if let Err(error) = written {
            log::warn!("unable to save session to {}: {error}", path.display());
        }
    }
}

#[cfg(test)]
mod session_tests {
    use super::SessionState;

    #[test]
    fn historical_session_fields_survive_without_service_specific_models() {
        let original = serde_json::json!({
            "last_page": "home", "sorts": [],
            "rootlist": {"account_id": "listener", "entries": [{"Playlist": "old:playlist:one"}]},
            "last_queue_rows": [{"track": {"uri": "old:track:one", "name": "Remembered"}}],
            "window_pos": [20.0, 30.0], "winamp_pos": [40.0, 50.0]
        });
        let state: SessionState = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(serde_json::to_value(state).unwrap(), original);
    }

    #[test]
    fn a_new_session_atomically_replaces_the_previous_one() {
        let root = std::env::temp_dir().join(format!(
            "navifast-session-test-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let path = root.join("session.json");
        let state = |page: &str| SessionState {
            retained: [("last_page".into(), serde_json::json!(page))].into(),
            ..Default::default()
        };
        state("home").save(&path);
        state("albums").save(&path);
        assert_eq!(SessionState::load(&path).retained["last_page"], "albums");
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
