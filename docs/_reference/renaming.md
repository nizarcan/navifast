---
title: Fork Identity
description: Navifast uses a separate profile and application identity.
nav_order: 5
---

Navifast is a Navidrome fork of the original Spotify client. It does not migrate
upstream settings, credentials or caches. Its command is `navifast` and its
package/bundle identity is `io.github.nizarcan.Navifast` where applicable.

Installers do not register the Spotify URL scheme or replace upstream package
names. The inherited compatibility executable remains in source for regression
tests; current native package manifests do not install its command alias.
