---
title: Translating Navifast
description: Help translate Navifast and preview the work so far.
nav_order: 6
---

Navifast follows your computer's language when it has a translation for it,
and uses English otherwise. **Settings → Appearance → Language** picks another
language, listed under its own name, and applies it at once; **System** follows
the computer again. Corrections from
fluent speakers are welcome.

Translations are stored in `.po` files, a common format supported by editors
such as Poedit and Weblate. They are included with Navifast, so the app does
not contact an online translation service.

## Languages and coverage

| Language | Tag | Coverage |
| --- | --- | --- |
| English | `en` | Source language |
| Spanish | `es` | Complete |
| German | `de-DE` | Partial |
| Dutch | `nl` | Partial |
| Portuguese (Brazil) | `pt-BR` | Partial |
| Portuguese (Portugal) | `pt-PT` | Partial |
| French | `fr` | Partial |
| Swedish | `sv` | Partial |
| Polish | `pl` | Partial |
| Russian | `ru` | Partial |
| Italian | `it` | Partial |
| Japanese | `ja` | Partial |
| Chinese (Simplified) | `zh-Hans` | Partial |
| Chinese (Traditional) | `zh-Hant` | Partial |

The interface is marked for translation throughout: navigation, Home, Search,
Library and collection pages, menus, the player bar, Queue and Lyrics, dialogs,
keyboard shortcuts, sign-in, Settings and notifications, including tooltips
and screen-reader names. The partial catalogues translate the earlier pilot:
navigation, Library controls, the player bar, Queue and Lyrics. Everything they
do not translate yet appears in English. The tray menu, the macOS menu bar and
Dock menu, and the Windows taskbar buttons are still English in every language,
as are error details reported by the server, the network or the system.

A regional system language uses the closest catalogue: `es-MX` and `es-419`
use Spanish, `de-AT` uses German, `pt-BR` and a plain `pt` use Portuguese
(Brazil), other Portuguese regions use Portuguese (Portugal), `zh-CN` and
`zh-SG` use Simplified Chinese, and `zh-TW`, `zh-HK` and `zh-MO` use
Traditional Chinese. When the system lists several preferred languages, the
first one with a catalogue wins.

Song, album, artist and playlist names come from the server or their creators and
are kept as provided, as are lyric lines and failure details. Generated queue
playlist names translate the surrounding words while retaining the song title
or the date in `YYYY-MM-DD` form.

## Edit and preview

The repository's `assets/i18n/navifast.pot` is the English source template.
Open the PO for your language, such as `assets/i18n/es.po`, in your translation editor. Edit `msgstr` values;
keep `msgid`, `msgid_plural`, `msgctxt`, and placeholders such as `{count}`, `{date}`,
`{track}` and `{error}` unchanged.
Translator comments explain the placeholders. Clear a fuzzy flag only after
reviewing the translation against its current English source.

Build and preview your changes with the native sample catalog:

```sh
cargo run --locked --features navidrome-demo -- --demo --demo-page settings
cargo run --locked --features navidrome-demo -- --demo --demo-show light --demo-size 760x620
```

Choose the language in **Settings → Appearance → Language**. Check narrow and
normal windows, light and dark themes, keyboard navigation, and accessible names.
For automated captures, pass `--demo-data PATH` with an isolated profile, set
`language` in its `config/settings.json`, then use `--demo-shot PATH`.
Useful sample states include `signed-out`, `restoring`, `details`, `equalizer`
and `many-favorites`.

## Update the template and check catalogs

Maintainers mark source phrases with `gettext(locale, "English text")` and whole
counted phrases with `ngettext(locale, "Singular", "Plural", count)`. Use
`pgettext(locale, "context", "English text")` when the same English word has
different meanings. For example, `Follow` in the `lyrics` context follows the
current lyric line, so its translation can differ from following an artist.
Interpolate with named placeholders, such as `gettext(locale, "Added {name}")`
followed by `.replace("{name}", &name)`, and explain each one in a
`// Translators:` comment above the phrase. Add any
new source file to `assets/i18n/POTFILES`. Rune labels chosen from tables use
`i18n::mark("English text")` so extraction sees the phrase before the view
translates it. Preserve `pgettext` contexts for labels with several meanings. With GNU gettext tools that support
Rust installed, run:

```sh
.github/scripts/update-translations.sh
.github/scripts/update-translations.sh --check
cargo test --locked --test localization
```

The update command extracts the template with `xgettext` and merges it into
existing PO files with `msgmerge`. The check command verifies the template and
uses `msgfmt` to check catalog syntax and marked format placeholders. The
localization tests also check that every translated phrase preserves its named
placeholders, including phrases filled by string replacement. Submit changed
PO files and the template, together with any required source changes. Generated
Rust catalogs stay in Cargo's build directory and are not committed.

Normal application builds need no external gettext tools. The build validates
the PO files and compiles their translations and plural expressions to Rust.
Missing, empty, fuzzy, or incomplete plural entries fall back to the full English
phrase, so a catalogue can be published before it is complete. The localization
test requires completeness only of catalogues that claim it (currently
Spanish); every catalogue must keep the placeholders of what it translates. Each locale's `Plural-Forms` header determines its plural choices;
languages are not restricted to two forms.

## Another language or a correction

Create another PO from the template using your editor's new-translation command,
or `msginit`. Set its language and plural rules and translate what you can.
A maintainer must also register the locale in the app, including how system
language tags map to it, and preview it before it becomes available. Adding a
PO alone does not add a language to the Settings list.

Use the [translation problem form](https://github.com/nizarcan/navifast/issues/new?template=translation.yml)
for incorrect wording, missing translations or text that does not fit. Each
report gets its own issue. Include the language, version, affected control, and
the text you see; a suggested correction is welcome. The catalog headers link
to this form too.
