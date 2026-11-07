---
title: The Queue's Rules
description: What the queue shows, what Add to queue does, and what the app promises about both.
nav_order: 2
---

## Current Navidrome transport

Navifast plays an album's ordered song list locally. Queue position is an index,
so duplicate song IDs remain distinct occurrences. A new selection or seek
increments the generation, updates the view immediately and rejects old player
events. Next advances to the next occurrence, previous restarts after three
seconds, and natural completion stops after the final song. Paused skips and
seeks preserve the paused intention. Queue editing, shuffle/repeat and saved
server queues are not yet exposed. Tests live in `src/navidrome/state.rs and src/app/navidrome.rs`.
