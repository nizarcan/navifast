use crate::i18n::{Locale, gettext, pgettext};
use std::borrow::Cow;
pub(super) const fn platform_shortcut<'a>(ctrl: &'a str, cmd: &'a str) -> &'a str {
    if cfg!(target_os = "macos") { cmd } else { ctrl }
}
pub(super) const SIDEBAR_SHORTCUT: &str = platform_shortcut("Ctrl+B", "Cmd+B");
pub(super) const QUIT_SHORTCUT: &str = platform_shortcut("Ctrl+Q", "Cmd+Q");
pub fn navidrome_shortcuts(locale: Locale) -> Vec<(Cow<'static, str>, Cow<'static, str>)> {
    let keys = |text: &'static str| Cow::Borrowed(text);
    vec![
        (
            pgettext(locale, "key", "Space"),
            gettext(locale, "Play or pause"),
        ),
        (
            keys(platform_shortcut("Ctrl+←  /  Ctrl+→", "Cmd+←  /  Cmd+→")),
            gettext(locale, "Previous or next"),
        ),
        (
            keys("Shift+←  /  Shift+→"),
            gettext(locale, "Seek 10 seconds"),
        ),
        (
            keys(platform_shortcut("Ctrl+↑  /  Ctrl+↓", "Cmd+↑  /  Cmd+↓")),
            gettext(locale, "Volume up or down"),
        ),
        (keys("M"), gettext(locale, "Mute or unmute")),
        (
            keys("Shift+↑  /  Shift+↓"),
            gettext(locale, "Song list: extend or shrink the selection"),
        ),
        (
            keys(platform_shortcut("Ctrl+A", "Cmd+A")),
            gettext(locale, "Song list: select all"),
        ),
        (
            keys(SIDEBAR_SHORTCUT),
            gettext(locale, "Show or hide the sidebar"),
        ),
        (keys("Alt+←  /  Alt+→"), gettext(locale, "Back or forward")),
        (
            keys(platform_shortcut("Ctrl+H", "Cmd+Shift+H")),
            gettext(locale, "Albums"),
        ),
        (
            keys(platform_shortcut("Ctrl+L", "Cmd+L")),
            gettext(locale, "Favorites"),
        ),
        (
            keys(platform_shortcut("Ctrl+Shift+B", "Cmd+Shift+B")),
            gettext(locale, "Go to the playing album"),
        ),
        (
            keys(platform_shortcut("Ctrl+,", "Cmd+,")),
            gettext(locale, "Settings"),
        ),
        (
            keys(platform_shortcut("Ctrl+W", "Cmd+W")),
            gettext(locale, "Close the window"),
        ),
        (keys(QUIT_SHORTCUT), gettext(locale, "Quit")),
    ]
}
