---
title: Design Language
description: The existing Rune interface and the rules for extending it.
nav_order: 9
---

Navifast extends one native music interface. A new feature should look and
behave like the closest existing feature. The album page is the reference for
playable collections, including playlists. Search results are not the reference
for a collection's detail page.

This document records the shipping Rune design, not a proposed redesign.
Implementation lives in `src/ui/desktop/`; shared colors, icons and dimensions
live in `src/theme.rs`. Update this guide when an approved change establishes a
new pattern. Do not create a second set of components with copied styling.

## Shell and navigation

- Keep the left library sidebar, top navigation/search bar and bottom player
  in their existing positions. Page changes do not replace the player.
- The primary sidebar has exactly **Home**, **Playlists**, **Favorites** and
  **Discover**, in that order. Reuse the existing Lucide icons, navigation row,
  text, hover treatment and keyboard accessibility. Keep the Library shelf below.
- Discover opens Albums and groups **Albums**, **Songs**, **Genres** and
  **Listening stats** in a shared top row of the existing 32-pixel pills, with
  8-pixel gaps and wrapping at narrow widths. The active pill uses the accent
  treatment. Keep 20 pixels before the page heading and existing 24-pixel page
  padding. All four destinations retain the pills; album-order and genre drill
  downs keep their parent pill active. Album/playlist detail pages stay unchanged.
  Navigation preserves playback, back/forward history and saved scroll positions.
  Do not add each discovery feature back to the primary sidebar.
- The top bar is 56 logical pixels high and the player bar is 88. Respect the
  platform titlebar inset and the user's zoom and sidebar width.
- Ordinary page content has 24-pixel horizontal padding. Artist pages have a
  deliberate full-width portrait treatment; do not copy that treatment to
  albums or playlists.

The Library shelf uses the approved **Quiet shelf** layout. Below the existing
divider, use a muted 12-pixel Library heading with Search and Library options
icons. Keep the 48-pixel header and align its label with the navigation's left
inset. Search opens the existing search field above the album controls; closing
search clears its filter. The next 36-pixel row has a plain 13-pixel Albums label
and a quiet, right-aligned sort control. Albums is a label, not a single-option
filter. Keep the selected sort's full accessible name when its text truncates.

Library options uses the shared anchored menu with checked **List view** and
**Grid view** choices, then **Hide sidebar**. The existing top-bar control
restores the sidebar. Keep the four navigation rows, album rows and artwork,
compact layout preference, resizing, and sorting behavior intact.

## Collection browsers

**The Albums and Playlists screens use the same artwork card grid.** A playlist
browser must not become a text list or reuse search-result rows. Share the card
component and responsive column calculation with Albums: 172-pixel cards,
12-pixel padding, 148-pixel square artwork with 6-pixel corners, 7-pixel column
gaps and 240-pixel rows. Keep the same title typography, secondary metadata,
hover surface, focus behavior and circular playback overlay. Use server artwork
when available and the existing cover fallback otherwise. Virtualize grid rows
so a large playlist library does not build every card each frame.

A card opens that collection's detail page. Its playback overlay starts the
collection from the top. Differences in metadata or available actions do not
justify a separate card design. Compact destination lists remain appropriate
inside search results and selection dialogs, not on the playlist browser.

## Home

Home uses **Quick return**, option A approved for NAV-33. It is the startup page;
Albums is reached through Discover. Preserve the existing sidebar, top bar,
player and paused session restoration.

Use a 28-pixel bold welcome heading with a 14-pixel secondary subtitle. Show up
to six recently played collections as 72-pixel compact tiles, in three columns
when content is at least 720 pixels wide and two otherwise. Use the existing
surface/hover colors, 6-pixel corners, artwork and truncated collection labels.
Clicking a tile opens its album or playlist; it does not start playback.

Below the shortcuts, use 20-pixel section headings for **Your playlists** and
**Recently played artists**. Playlists reuse the exact album/playlist card grid
described above. Artists use the existing 148-pixel circular portrait treatment.
Show one row initially; **More** and **Less** expand or collapse the section in
place. Virtualize the entire Home list, including expanded sections.

A first visit shows a brief welcome and **From your library** album cards instead
of invented listening history. Loading and failed refreshes keep their own inline
feedback, with usable cached cards retained. The recent list is local to the
current server/account, never a claim about listening in other clients.

## Listening stats

Use the Listening stats pill in Discover, a 28-pixel heading and the existing refresh
icon. Put source and last completed refresh in muted text, then quiet total-play
and estimated-time metrics. Top songs, albums and artists use compact 58-pixel
ranked rows with ordinary hover and keyboard focus. Show three columns when the
content is at least 780 pixels wide, otherwise stack the sections. Preserve the
shell, player and collection grids. Selecting a song opens its album and selects
that track; ranking rows do not start playback. Show partial, unavailable, empty,
loading and failed-refresh states inline and retain a usable previous summary.

## Album discovery

Keep the Albums heading and shared sleeve grid. Beside the heading, use the existing
pill menu and refresh icon for the active order. Offer recently added, name,
frequently played, recently played on server, random selection and year range.
Keep the selected order visible, with the applied year bounds when relevant.
Year inputs reuse the existing 34-pixel text fields, muted labels and Apply button;
the row wraps at narrow widths. Invalid input shows inline feedback while retaining
the last applied range and its albums. Server-history modes explain their source
in muted text. Their results do not replace the sidebar's local listening history.

## Songs and genres

Songs and Genres use Discover pills and 28-pixel bold headings.
Genres is a virtual list of the server's exact names with muted song counts and
the existing hover/chevron treatment. Do not invent artwork or colors for genres.
Inside a genre, Songs and Albums use the existing pill controls to switch views.
Songs reuses the collection track table and its selection, sorting, playback,
queue and playlist actions. Albums reuses the exact album card grid. Keep both
tabs independent: a compilation track can match a genre without its whole album
matching. Keep loading, retry, empty and end-of-list feedback inline at the end
of each list. Failed requests preserve the loaded rows and back navigation keeps
the cached page's scroll position.

## Playable collection pages

Albums and playlists use the same header, action row and track table. Differences
come from their metadata and supported actions, not separate layouts.

| Element | Existing album pattern to reuse |
| --- | --- |
| Artwork | Square sleeve, 6-pixel corner radius; existing fallback when absent |
| Header layout | Artwork beside a flexible text column, separated by 24 pixels |
| Responsive size | 212-pixel art and 56-pixel title when content exceeds 720 pixels; otherwise 160 and 40 |
| Collection type | 12.5-pixel, medium-weight label above the title |
| Title | Weight 700, wrapping in logical text order |
| Metadata | Secondary color, approximately 14 pixels; creator, applicable year, song count and duration |
| Action row | 56-pixel circular accent Play/Pause button, then supported secondary icon controls; 18-pixel gaps |
| Track heading | Number, title and duration columns with the existing divider and sorting behavior |
| Track rows | 56-pixel normal rows or 36-pixel compact rows; shared selection, hover, active-song and context-menu treatments |

Use a playlist's artwork and owner only when the server supplies them. Song
counts and duration must describe the loaded contents. Do not invent an artist,
release year, or album-favorite action for a playlist. Unsupported operations
must not appear functional. Existing disabled controls stay consistent until
their corresponding feature is implemented.

Single-click selects a track, double-click plays it, and the row's number/play
control provides direct playback. Enter plays the selected occurrence. The
same keyboard, selection, sorting and context-menu code serves both collection
types. Repeated song IDs remain separate occurrences. Playing a sorted view
uses its displayed order; the active-row marker follows the occurrence.

## Artwork viewer

The album header sleeve opens a centered native modal without changing the
collection layout. Reuse overlay, outline, shadow, 12-pixel corners, 24-pixel
padding and existing 32-pixel pill buttons. The panel is at most 900 pixels wide
and leaves 8 pixels at each window edge. Use a single-line 20-pixel bold album
title, complete aspect-fitted artwork, then quiet inline status and Close/Save
buttons. The thumbnail remains visible while larger artwork loads. Retry appears
inline on failure; Save is disabled until validated image bytes are ready.
Escape and Close return focus to the album sleeve. Portrait, landscape and square
images retain their complete bounds in both narrow and normal windows.

## Queue

Use the player bar's Queue control to show or hide it. At widths of at least
1060 logical pixels, show a 320-pixel right-side panel when at least 340 pixels
remain for the main content. At narrower widths or with a wider library sidebar,
the queue uses the main content area below the existing top bar. Keep navigation,
the player bar, browsing history and scroll position intact. Navigating from the
compact queue returns to browsing. This is layout A approved for NAV-6.

Reuse Palette colors, Inter, artwork sleeves and the existing menu controls.
Show Now playing separately, followed by Playing next with Clear for manual
additions, and Next up for the collection. Upcoming rows are virtualized, with
42-pixel artwork, a 14-pixel title and secondary artist text. Repeated songs are
separate selectable occurrences. Pending albums use the same row with a spinner.

Place the optional Autoplay switch below the Queue heading, using the shared
settings switch, a 13-pixel label and muted 12-pixel explanation. Its loading or
failure text stays inline. Automatic additions use a final Autoplay section with
the same queue rows; Clear also cancels a waiting refill. Keep the established
responsive panel and player unchanged.

The existing Shuffle and Repeat icons keep their positions and hit areas.
Enabled modes use the accent color and expose their checked state to assistive
technology. Repeat One adds a small centered 1 inside the existing repeat glyph;
its accessible name distinguishes Off, All and One. Collection Shuffle controls
share the player control's state and behavior.

## Track details

Use **Track details** in the existing song and queue menus. Right-clicking the
playing track opens the same song menu without adding another player-bar button.
The inspector shares the playlist dialog's rounded overlay, padding, shadow,
20-pixel heading, scrolling body and Close pill. Keep the selected song's title
and artist above quiet 13-pixel label/value rows. Group **Source file**,
**Decoded audio**, **Processing** and **Device output** under 15-pixel headings.
Use explicit units and Unknown for missing facts. Preserve keyboard dismissal,
return focus to the originating control, and fit narrow windows without changing
the shell or collection layout.

## Streaming quality

Use the existing Settings section, row and 200 × 30 dropdown for **Playback →
Streaming quality**. Original is the default; MP3 choices state the bitrate.
Keep capability loading, Retry, unavailable seeking and deferred changes as muted,
wrapping text below the row. The existing responsive Settings layout stacks the
control when needed. Selected menu choices expose their checked state. Disable the
player seek slider and timed lyric seek actions together when seeking is unavailable;
do not change the player shell or lyric typography.

## Type, color and controls

- Use Inter and its real font weights through Rune's text layout. Keep logical
  Unicode text intact, including mixed right-to-left and left-to-right text.
- Read colors from `Palette`: `window` and `panel` for the shell, `surface`
  variants for controls and interaction, `text`/`secondary`/`dim` for hierarchy,
  and `accent` for playback and selected states. Do not hard-code a dark or
  light palette inside a new component.
- Use the shared Lucide `Icon` set. Match neighboring icon sizes and hit areas.
- Reuse `Paint` helpers for labels, icon buttons, menus, artwork, loading and
  empty states. Text actions use the existing rounded button; a collection's
  main playback control uses the circular play button.
- Let artwork tint use the existing palette-aware backdrop. Preserve text
  contrast and the same fallback behavior in both themes and custom themes.
- Keep actions accessible with names, roles, focus and keyboard behavior.
  Hover-only controls still need a keyboard or accessibility path.

## Ratings

Keep ratings inside the existing item menus, under a small secondary **Your rating**
label. Use five labelled, keyboard-focusable numeric buttons, with the selected
value in the accent color, and a separate **Clear rating** action. An unrated item
says **Not rated**. The existing star remains the Favorites control. Song menus
in collections, search and the queue share the same controls and account value;
an album's rating is independent of its songs. Save failures use the existing
inline notice and Retry pattern, naming the affected item.

## Listening reports

Use the existing Settings section and switch for **Scrobble to Navidrome**, under
**Listening history**. Explain that the server may forward reports to configured
services. Reporting errors reuse the bounded inline notice area with a named song,
wrapped copy and Dismiss. Only definitely rejected completed submissions offer
Retry; uncertain results explain why they are not automatically sent again.
Playback remains usable while a notice is visible.

## Lists and feedback

Use the shared artwork card grid for album and playlist browsers, and the track
table for songs. Use compact rows for search results and destination pickers.
Large collections use Rune's virtual rows. Do not build or clone every row on every frame.

Loading, empty, missing and failed content are different states. Reuse the
spinner, empty-state and inline retry patterns. Preserve usable content on a
failed refresh. A user action appears immediately; a lagging server response
must not undo it. Requests, image decoding and persistence stay off the UI
thread. Idle views should not schedule unnecessary work.

## Review before extending

Find the nearest existing screen and share its components first. Capture the
baseline before editing. Use `navidrome-demo` for matching before/after images
at 760 × 800 and 1280 × 800 in light and dark themes, plus the relevant empty,
loading, error, menu and selection states. Include the HTML comparison required
by `CONTRIBUTING.md`.

A requested adjustment authorizes that scope. Broader changes to navigation,
spacing, hierarchy or interaction still need the maintainer's approval. When
there is no clear existing pattern, illustrate alternatives for a decision
before establishing a new one.

## Restored playback

A restored listening session uses the existing paused player and queue. Preserve
the saved occurrence, position and visible order. Do not add a separate resume
screen or start audio automatically. Normal playback actions take precedence over
a restoration still in flight.

## Lyrics

The player bar's existing microphone control opens Lyrics. It swaps with Queue in
the same 320-pixel side panel and uses the same content-area fallback at narrow
widths. Reuse the panel padding, heading, Close control, Palette and Inter. Closing
returns to browsing without replacing the player. This carries forward the previous
lyrics panel's ownership and the approved queue pattern.

The Lyrics heading is 28-pixel bold primary text at both window sizes. A subtle
one-pixel outline divider and breathing room separate it from the lyric content.
Keep the header fixed while the lyrics scroll underneath it.

Lyrics use wrapping 19-pixel text. The active timed line uses bold accent text;
other lines use the secondary text color. Automatic follow brings the active line
into view. Manual scrolling suspends following, with the existing pill-shaped
**Follow** control to return. Plain lines are text, timed lines are accessible seek
buttons. Do not reorder mixed-direction strings. Virtualize long documents.

When genuine word timestamps exist, keep the active line bold and apply the accent
only to currently timed UTF-8 ranges within the same shaped text element. Keep
wrapping and bidi layout intact; do not split a line into separately laid-out words.
Plain and line-only lyrics never simulate word timing. Put the source credit after
the final lyric line, inside the scrolling content, in 11-pixel muted text. It is
visible at the end of the lyrics rather than beside the heading. Keep provider
controls in Settings; the lyrics panel has no provider-disable button.
Use the existing Settings section and switch components for provider opt-in, with
the provider's destination and metadata disclosure visible before enabling it.

### Lyric images

The image icon beside the Lyrics heading opens line selection in the same panel.
Use the existing checkbox, hover and selected surfaces. Freeze the selected song's
words and metadata while editing. Selection order never changes the song's order.
Keep Cancel and Preview below the scrolling list.

Preview uses the existing rounded modal, border, shadow and pill buttons. Background
choices use the current Palette, dark or light. Artwork is optional; a missing cover
leaves a clean text credit. The card separates a compact artwork/title/artist header
from the larger lyric passage with space and an outline divider. Use Inter and Rune's
wrapping and bidi layout without reordering strings. Show the rendered PNG itself in
the preview so Copy and Save cannot differ from it. Keep editing and export actions
visible below the scrolling preview.

## Downloads

Use approved NAV-28 layout A: an **Available offline** shelf on Home, with the
same collection cards as Albums and Playlists. **Manage** opens Downloads within
Home; keep exactly four sidebar destinations. The manager uses virtual compact
rows with a sleeve, title, completed-song count, bytes and the existing Cancel,
Retry and Remove pills. Selection details reuse the collection header, playback
controls and virtual track table. Local availability uses a small accent check;
queued, transferring and failed tracks have muted or error status icons with
accessible labels and per-track Cancel/Retry controls. Preserve the collection's
ordering and duplicate occurrences.

Offline Home retains the shelf and Manage, with a quiet Working offline label
and Reconnect pill. Empty, loading, transfer and storage failures remain distinct.
Put Open downloads offline below the existing sign-in controls. Reuse Palette,
Inter, spacing, artwork fallbacks, focus behavior and responsive collection sizes;
there is no fifth navigation destination or new application shell.

## Sync feedback

Keep account sync inside Settings, using the existing 18-pixel section heading,
outlined panel, 15-pixel pending count and 13-pixel muted details. Show last sync
in UTC and ordinary Retry, Reconnect and Dismiss buttons. Failure rows wrap at
narrow widths and list the item name above the explanation. Uncertain listens
explain why they will not be resent and offer only Dismiss. Render at most eight
failures at once; resolving them reveals later entries. A compact inline banner
below the top bar links to this section when attention is needed. Do not add a
sidebar destination or change the collection layout.

## Audio cache

Keep Audio cache in Settings after Playback, with the existing outlined section,
switch, size dropdown and ordinary Clear/Retry pills. Show usage in muted 13-pixel
text. Explain retained active files and failures inline with wrapping text. Use the
same controls and spacing at narrow widths; do not add a navigation destination.

## Autoplay exclusions

Use **Exclude from autoplay** and **Include in autoplay** in the shared item menu,
album More menu and queue song menu. The artist header supports the same right-click
menu as artist cards, without another permanent header control. Keep exclusions in
Settings with the existing section, muted explanatory copy, compact title/kind rows
and Include pills. Render eight entries per page with ordinary Previous/Next controls.
Explain that account-local rules are additive and leave direct playback available.
Keep the four sidebar destinations, collection grids and existing typography.
