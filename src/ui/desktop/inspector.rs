//! Library metadata and observed playback facts share the existing detail modal.
use super::*;

fn number(value: Option<u32>, unit: &str, unknown: &str) -> String {
    value
        .filter(|v| *v > 0)
        .map_or_else(|| unknown.into(), |n| format!("{n}{unit}"))
}
fn gain(value: Option<f64>, unknown: &str) -> String {
    value
        .filter(|v| v.is_finite())
        .map_or_else(|| unknown.into(), |n| format!("{n:+.2} dB"))
}
fn peak(value: Option<f64>, unknown: &str) -> String {
    value
        .filter(|v| v.is_finite() && *v > 0.)
        .map_or_else(|| unknown.into(), |n| format!("{n:.4}×"))
}
fn field(v: &Paint<'_>, name: &'static str, value: String) -> El {
    row()
        .w_full()
        .gap(16.)
        .items_start()
        .child(label(v.tr(name), 13., 400., v.p.secondary).w(156.).wrap())
        .child(label(value, 13., 500., v.p.text).grow(1.).min_w(0.).wrap())
}
fn heading(v: &Paint<'_>, name: &'static str) -> El {
    label(v.tr(name), 15., 700., v.p.text).mt(10.).mb(2.)
}

pub(super) fn dialog(v: &Paint<'_>) -> El {
    let Some(inspector) = &v.s.inspector else {
        return el();
    };
    let song = &inspector.song;
    let unknown = v.tr(mark("Unknown"));
    let mut contents = col()
        .w_full()
        .gap(8.)
        .child(label(&song.title, 18., 650., v.p.text).wrap())
        .child(label(&song.artist, 13., 400., v.p.secondary).wrap())
        .child(heading(v, mark("Source file")))
        .child(
            label(
                v.tr(mark("Metadata reported by your server.")),
                12.,
                400.,
                v.p.dim,
            )
            .wrap(),
        );
    for (name, value) in [
        (
            mark("Disc number"),
            number(Some(song.disc_number), "", &unknown),
        ),
        (mark("Track number"), number(Some(song.track), "", &unknown)),
        (
            mark("Duration"),
            if song.duration.is_finite() && song.duration > 0. {
                format!("{:.1} s", song.duration)
            } else {
                unknown.clone()
            },
        ),
        (
            mark("Format"),
            if song.suffix.is_empty() {
                unknown.clone()
            } else {
                song.suffix.to_uppercase()
            },
        ),
        (mark("Bitrate"), number(song.bit_rate, " kbps", &unknown)),
        (
            mark("Sample rate"),
            number(song.sampling_rate, " Hz", &unknown),
        ),
        (mark("Bit depth"), number(song.bit_depth, " bit", &unknown)),
        (mark("Channels"), number(song.channel_count, "", &unknown)),
        (
            mark("Track ReplayGain"),
            gain(
                song.replay_gain.as_ref().and_then(|g| g.track_gain),
                &unknown,
            ),
        ),
        (
            mark("Album ReplayGain"),
            gain(
                song.replay_gain.as_ref().and_then(|g| g.album_gain),
                &unknown,
            ),
        ),
        (
            mark("Track peak"),
            peak(
                song.replay_gain.as_ref().and_then(|g| g.track_peak),
                &unknown,
            ),
        ),
        (
            mark("Album peak"),
            peak(
                song.replay_gain.as_ref().and_then(|g| g.album_peak),
                &unknown,
            ),
        ),
    ] {
        contents = contents.child(field(v, name, value));
    }
    contents = contents.child(heading(v, mark("Decoded audio")));
    if let Some(info) = inspector.playback(&v.s.app) {
        let quality = info
            .quality
            .kbps()
            .map_or_else(|| v.tr(mark("Original")), |n| format!("MP3 ≤ {n} kbps"));
        for (name, value) in [
            (mark("Requested quality"), quality),
            (
                mark("Codec"),
                if info.decoded.codec.is_empty() {
                    unknown.clone()
                } else {
                    info.decoded.codec.to_uppercase()
                },
            ),
            (
                mark("Sample rate"),
                number(Some(info.decoded.sample_rate), " Hz", &unknown),
            ),
            (
                mark("Bit depth"),
                number(info.decoded.bit_depth, " bit", &unknown),
            ),
            (
                mark("Channels"),
                number(Some(info.decoded.channels), "", &unknown),
            ),
        ] {
            contents = contents.child(field(v, name, value));
        }
        contents = contents.child(heading(v, mark("Processing")))
            .child(label(format!("{} Hz · 2 ch · f32", crate::navidrome::player::RATE), 13., 500., v.p.text))
            .child(heading(v, mark("Device output")))
            .child(label(v.tr(mark("Configured audio stream. Your operating system or device may convert it further.")), 12., 400., v.p.dim).wrap());
        for (name, value) in [
            (
                mark("Sample rate"),
                number(Some(info.output.sample_rate), " Hz", &unknown),
            ),
            (
                mark("Channels"),
                number(Some(u32::from(info.output.channels)), "", &unknown),
            ),
            (mark("Sample format"), info.output.sample_format.clone()),
        ] {
            contents = contents.child(field(v, name, value));
        }
    } else {
        let current = v.s.app.transport.song().is_some_and(|s| s.id == song.id);
        let message = if current {
            v.s.app
                .transport
                .error
                .clone()
                .unwrap_or_else(|| v.tr(mark("Waiting for decoded audio…")))
        } else {
            v.tr(mark("Playback details appear when this track is playing."))
        };
        contents = contents.child(label(message, 13., 400., v.p.secondary).wrap());
    }
    detail_dialog(
        v,
        &v.tr(mark("Track details")),
        contents.key("track-details-scroll"),
        v.button(&v.tr(mark("Close")), false, |s, _| s.close_inspector())
            .key("track-details-close"),
        State::close_inspector,
        inspector.origin.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_metadata_is_distinct_from_zero_gain() {
        assert_eq!(number(None, " Hz", "Unknown"), "Unknown");
        assert_eq!(number(Some(0), " Hz", "Unknown"), "Unknown");
        assert_eq!(number(Some(96_000), " Hz", "Unknown"), "96000 Hz");
        assert_eq!(gain(Some(0.0), "Unknown"), "+0.00 dB");
        assert_eq!(gain(Some(f64::NAN), "Unknown"), "Unknown");
        assert_eq!(peak(Some(0.0), "Unknown"), "Unknown");
    }
}
