---
title: The Queue's Rules
description: What the queue shows, what Add to queue does, and what the app promises about both.
nav_order: 2
---

## Playback order

The queue belongs to the current local session. Each occurrence has its own ID,
including repeated copies of one song. The current occurrence is separate from
upcoming entries. **Next** and natural completion consume the visible head.
Player generations reject position and completion reports from older selections.
Paused skips, moves and seeks preserve the paused intention.

**Play next** inserts a song or ordered selection at the head of **Playing next**.
**Add to queue** appends after earlier manual additions, before **Next up**, the
remaining collection. An album is added in disc/track order, including duplicates.
Starting a different collection keeps manual additions and replaces the collection
remainder. Playlist playback keeps its displayed sort order.

## Editing

Open **Queue** in the player. It appears beside browsing at normal widths and in
the content area at narrow widths. Closing it returns to the same browsing page.
Single-click selects an occurrence; double-click, its Play control or Enter plays
it and skips the entries above it. The menu offers **Move up**, **Move down** and
**Remove from queue**. Arrow keys select, Alt+Up/Down move, and Delete removes.
Moves stay within the manual or collection section, preserving the promise that
explicit additions come before the remaining collection. Selection follows the
occurrence ID after a move or album load.

**Clear** removes only upcoming manual additions, including pending albums. It
keeps the current song, its position and the remaining collection. Previous
restarts the current song after three seconds; otherwise it returns to the prior
occurrence. Recent playback history retains at most 128 occurrences, then falls
back to the current collection's preceding track. This bounds memory for a queue
that is continuously fed manual additions.

## Pending albums

An album request immediately reserves one visible loading row. Later additions
keep their order even if requests complete out of order. A successful read replaces
that placeholder with the entire album in one update. Its first occurrence inherits
the placeholder ID. A failed or empty result removes only its reservation and shows
an error. Add it again to retry.

Next waits if its head is still loading. Pause/resume changes the intention while
waiting. The resolved first song becomes current at position zero; failure advances
to the following entry. Clear cancels that wait and restores the existing current
song at its position. Removing, skipping, clearing or signing out prevents late
replies from restoring removed entries. Album reads run on the backend worker and
survive ordinary browsing navigation. Clear and sign-out cancel their requests.

## Shuffle and repeat

Shuffle only reorders the remaining collection. Manual additions stay first,
including albums that are still loading. The shuffled order remains stable as
playback advances and the view redraws. Turning shuffle off sorts the remaining
collection occurrences back into their original play order, without restoring
consumed or removed occurrences. Starting a chosen song while shuffle is enabled
puts it first, followed by every other occurrence in that collection once.
Changing shuffle from any collection page affects the playing collection without
starting the viewed one. Mode changes preserve the current occurrence, position,
pause intention and player generation.

Repeat cycles through **Off**, **All**, and **One**. Off stops after the last
occurrence. One restarts the current occurrence on natural completion; explicit
Next still consumes the visible head when one exists. All repeats the underlying
collection, with explicit manual additions played once. Without an underlying
collection, All repeats the current and upcoming manual songs captured when the
mode is enabled, including later additions. Song data is shared across cycles.

A future cycle appears in Next up when the current collection has no more
upcoming tracks. Its entries have new occurrence IDs. Starting that cycle consumes
its visible head. Turning All off discards only a cycle that has not started yet;
a cycle already playing continues normally. Removal excludes that occurrence
from subsequent cycles until a new collection starts. In a standalone manual
loop, Clear retains only the current song in the loop. New explicit additions
postpone the standalone loop preview until those additions play, so pending
albums can resolve before the next cycle is prepared.

The existing command and media interfaces report and set the same modes as the
native controls.

## Sleep timer

Open the clock in the Queue heading. Choose 15, 30, 60 or 120 minutes, a positive
whole number of songs including the current one, or **Pause at end of queue**.
The queue and dialog show the deadline in local time or the remaining song count.
A new choice replaces the previous timer. **Cancel timer** leaves playback and
position unchanged. Timers belong to this session and clear on sign-out or exit.

The duration uses wall-clock time, including time paused or with the window hidden.
Expiry pauses the current source and retains its buffered audio and resume position.
The output worker enforces the deadline independently of window redraws.

Natural completion and manual Next each consume one song. Repeated copies of a
song and Repeat One plays count separately. Seeking, Previous and editing the
queue do not consume a song. Selecting another song or collection keeps the
remaining count, with that selection becoming the current song. Next waits for a
pending album without counting again when the album resolves. A one-song timer
pauses before advancing, including when Next is pressed early.

The queue timer follows additions, removals and the current collection. It ignores
Repeat One so this pass can reach its end, then pauses before a future Repeat All
cycle or automatic refill. Repeat settings remain selected. A song boundary is
checked before the controller selects or submits the next source, so a timer stop
cannot play the beginning of the following song. Playback stays paused until the
listener resumes; stale completion reports cannot restart it.

## Listening history

With scrobbling enabled, audio consumed within one play accumulates across seeks
and pause/resume. Seeking backward does not create another completed submission.
A natural repeat, a different queue occurrence, or playing again after completion
starts a new play. Restored paused audio sends no report until it actually starts.
Reporting failures do not change the playing song, queue, position or pause state.

## Restarting

`navidrome-session.json` in the state directory stores the playing occurrence,
position, ordered upcoming entries, collection order, manual additions, bounded
history, shuffle order and repeat state. Each manual occurrence keeps its own
identity even when another occurrence has the same song ID. Pending album
reservations are resolved during restoration. The snapshot contains opaque IDs
and a hash of the normalized server URL and username, never stream URLs or secrets.

After successful sign-in, a worker validates each unique song through `getSong`,
sharing the library's four-request concurrency limit. Only the protocol's missing
item response removes a song. Other failures keep the checkpoint for another
launch. Valid entries keep their order; a removed playing song advances to the
next available occurrence at zero, and a shorter song clamps the saved position.
Restoration always pauses. New playback or queue edits cancel restoration, and
session/token checks discard late results. Signing out deletes the saved session;
a different account never restores it.

Writes use a same-directory temporary file, flush it, then atomically replace the
checkpoint on the backend worker. Edits are coalesced over 500 ms, position-only
updates are limited to one every ten seconds, and pause/exit checkpoint immediately.
Interrupted temporary files are ignored. Invalid, oversized or unsupported-version
snapshots are ignored without blocking login. Existing preferences and historical
`session.json` files are unchanged. This is local restoration only; no server play
queue is read or written.

The queue model and regression tests live in `src/navidrome/queue.rs`; controller
and transport coverage lives in `src/app/navidrome/queue.rs` and
`src/app/navidrome.rs`. Rune interaction tests live in
`src/app/navidrome/desktop/regression.rs`.
