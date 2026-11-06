---
title: Settings and Files
description: Preferences used by the Navidrome build.
nav_order: 3
---

Navifast uses its own `navifast` config, state and cache directories via the
platform directory conventions (XDG on Linux; `io.github.nizarcan.navifast`
under Application Support/Caches on macOS, and `nizarcan/navifast` under
the appropriate AppData directories on Windows). The profile is separate from the upstream application.

`navidrome.json` in the config directory stores the server URL, username,
legacy light/dark preference, volume, EQ enablement, preamp and ten band levels. It contains
no password or authenticated stream/artwork URL. It also keeps up to ten recent submitted searches per server
and username. Changes are written atomically
on a background worker. Window geometry is stored by the native shell in its
own Navifast profile. Logs use the state directory's `navifast.log`.

`settings.json` keeps the original interface preferences: system/light/dark or
custom theme, language, zoom, sidebar layout and pins, player-bar visualization,
volume and EQ. Its playback preferences take precedence on startup; older
profiles containing only `navidrome.json` remain readable. Custom JSON themes
live in the config directory's `themes/` folder. Both settings files are written
atomically on the background worker and contain no password. Spotify-specific
settings are inactive, and automatic update checks remain disabled.

A successful Navidrome login is stored separately in Keychain on macOS,
Credential Manager on Windows, or Secret Service on Linux. The protected record
contains the server URL, username and password together. Editing the server or
username in the login form clears its password field. There is no plaintext
fallback if the credential store is locked or unavailable.

**Sign out** removes the saved login. Non-secret revocation markers under
`credential-storage/` in the state directory prevent automatic sign-in even if
the system store cannot delete the password immediately.

Inherited Spotify session caches and credentials are unused by normal Navidrome
startup. In-memory artwork and album metadata
are cleared on sign-out. Preferences are local; queues are not saved yet.

## Custom themes

Select a palette under **Settings → Appearance → Theme**. The existing eight
bundled palettes are installed as ordinary JSON files in `themes/`. The default,
**Follow system**, uses the desktop's appearance. Explicit light, dark and custom
choices are saved. **Open themes folder** opens the folder in your file manager.

For your own palette, add a regular UTF-8 JSON file, for example `gruvbox.json`:

```json
{
  "base": "dark",
  "colors": {
    "window": "#282828",
    "panel": "#1d2021",
    "surface": "#32302f",
    "text": "#ebdbb2",
    "accent": "#b8bb26"
  }
}
```

`base` is `dark` or `light`. Omitted colors inherit that palette. Available keys
are `window`, `panel`, `surface`, `surface_hover`, `surface_active`, `outline`,
`text`, `secondary`, `dim`, `accent`, `accent_hover`, `on_accent`, `danger`,
`warning`, `overlay` and `shadow`. Colors use `#RRGGBB` or `#RRGGBBAA`.

Run `navifast reload-themes` after editing a file while the app is open; Linux
also watches the folder for changes. Scanning runs in the background. Files are
limited to 64 KiB, with at most 128 JSON files and 512 folder entries. Invalid
files are skipped. If the selected file disappears or becomes invalid, the last
accepted palette remains visible and the Theme row explains the problem.
Choosing a built-in theme clears that custom selection and its cache.
