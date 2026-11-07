//! Listening counts consumed interleaved PCM samples, never wall time or seek position.
use super::{
    api::{Client, RequestError, Song},
    player::RATE,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct Context {
    pub session: u64,
    pub epoch: u64,
    pub occurrence: u64,
    pub enabled: bool,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Report {
    pub id: String,
    pub title: String,
    pub occurrence: u64,
    pub time: u64,
    pub submission: bool,
}

impl Client {
    pub async fn scrobble(&self, report: &Report) -> Result<(), RequestError> {
        self.checked_request(
            "scrobble",
            &[
                ("id", report.id.clone()),
                ("time", report.time.to_string()),
                ("submission", report.submission.to_string()),
            ],
        )
        .await
        .map(|_| ())
    }
}

#[derive(Default)]
pub struct Listening {
    pub context: Context,
    id: String,
    title: String,
    threshold: Option<u64>,
    consumed: u64,
    segment_samples: u64,
    started: Option<u64>,
    announced: bool,
    submitted: bool,
}

impl Listening {
    /// Seeks create a fresh PCM segment but keep this play's total and timestamp.
    pub fn begin(&mut self, context: Context, song: &Song) {
        if self.context.session != context.session
            || self.context.occurrence != context.occurrence
            || self.id != song.id
        {
            *self = Self {
                id: song.id.clone(),
                title: song.title.clone(),
                // Half the duration, at RATE frames/s and two samples/frame.
                threshold: (song.duration.is_finite() && song.duration >= 30.)
                    .then(|| (song.duration * f64::from(RATE)).ceil() as u64),
                ..Default::default()
            };
        }
        self.configure(context.epoch, context.enabled);
        self.context = context;
        self.segment_samples = 0;
    }

    pub fn configure(&mut self, epoch: u64, enabled: bool) {
        if enabled && !self.context.enabled {
            self.announced = false;
        }
        self.context.epoch = epoch;
        self.context.enabled = enabled;
    }

    /// Called with the audio device's cumulative consumed count for this segment.
    pub fn advance(&mut self, samples: u64, now_ms: u64) -> [Option<Report>; 2] {
        let delta = samples.saturating_sub(self.segment_samples);
        self.segment_samples = samples;
        if delta == 0 {
            return [None, None];
        }
        let time = *self.started.get_or_insert_with(|| {
            now_ms.saturating_sub(delta.saturating_mul(1000) / (2 * u64::from(RATE)))
        });
        if !self.context.enabled {
            return [None, None];
        }
        self.consumed = self.consumed.saturating_add(delta);
        let report = |submission| Report {
            id: self.id.clone(),
            title: self.title.clone(),
            occurrence: self.context.occurrence,
            time,
            submission,
        };
        let now_playing = if !self.announced {
            self.announced = true;
            Some(report(false))
        } else {
            None
        };
        let completed = if !self.submitted
            && self
                .threshold
                .is_some_and(|threshold| self.consumed >= threshold)
        {
            self.submitted = true;
            Some(report(true))
        } else {
            None
        };
        [now_playing, completed]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SECOND: u64 = 2 * RATE as u64;
    fn song(duration: f64) -> Song {
        Song {
            id: "opaque:/α & 夜".into(),
            title: "The song".into(),
            duration,
            ..Default::default()
        }
    }
    fn context(occurrence: u64) -> Context {
        Context {
            session: 1,
            epoch: 1,
            occurrence,
            enabled: true,
        }
    }

    #[test]
    fn threshold_counts_only_delivered_samples_and_submits_once_with_original_time() {
        let mut listening = Listening::default();
        listening.begin(context(1), &song(30.));
        assert!(listening.advance(0, 100_000).iter().all(Option::is_none));
        let [now, complete] = listening.advance(SECOND, 101_000);
        assert_eq!(now.unwrap().time, 100_000);
        assert!(complete.is_none());
        // A long pause or underrun changes the clock, never the delivered count.
        assert!(
            listening
                .advance(SECOND, 400_000)
                .iter()
                .all(Option::is_none)
        );
        assert!(listening.advance(15 * SECOND - 1, 414_000)[1].is_none());
        let complete = listening.advance(15 * SECOND, 414_001)[1].take().unwrap();
        assert_eq!(complete.time, 100_000);
        assert!(complete.submission);
        assert!(listening.advance(30 * SECOND, 430_000)[1].is_none());
    }

    #[test]
    fn short_tracks_and_immediate_skips_do_not_submit() {
        let mut listening = Listening::default();
        listening.begin(context(1), &song(29.99));
        assert!(listening.advance(30 * SECOND, 100_000)[1].is_none());
        listening.begin(context(2), &song(300.));
        assert!(listening.advance(0, 100_000).iter().all(Option::is_none));
        listening.begin(context(3), &song(300.));
        assert!(listening.advance(SECOND, 101_000)[1].is_none());
    }

    #[test]
    fn seeks_keep_the_play_but_genuine_repeats_start_another_occurrence() {
        let mut listening = Listening::default();
        let song = song(60.);
        listening.begin(context(1), &song);
        listening.advance(10 * SECOND, 110_000);
        // Seeking to the end or backward resets only the segment counter.
        listening.begin(context(1), &song);
        assert!(listening.advance(SECOND, 111_000)[1].is_none());
        let complete = listening.advance(20 * SECOND, 130_000)[1].take().unwrap();
        assert_eq!(complete.time, 100_000);
        listening.begin(context(1), &song);
        assert!(listening.advance(60 * SECOND, 190_000)[1].is_none());
        listening.begin(context(2), &song);
        let [now, complete] = listening.advance(30 * SECOND, 220_000);
        assert!(now.is_some());
        assert_eq!(complete.unwrap().time, 190_000);
    }

    #[test]
    fn disabling_excludes_samples_and_reenabling_cannot_submit_the_same_play_twice() {
        let mut listening = Listening::default();
        listening.begin(context(1), &song(60.));
        listening.advance(SECOND, 101_000);
        listening.configure(2, false);
        assert!(
            listening
                .advance(59 * SECOND, 159_000)
                .iter()
                .all(Option::is_none)
        );
        listening.configure(3, true);
        assert!(listening.advance(60 * SECOND, 160_000)[1].is_none());
        assert!(listening.advance(88 * SECOND, 188_000)[1].is_some());
        listening.configure(4, false);
        listening.configure(5, true);
        assert!(listening.advance(100 * SECOND, 200_000)[1].is_none());
    }

    #[tokio::test]
    async fn scrobble_preserves_opaque_id_timestamp_and_submission_kind() {
        use crate::navidrome::test_server::{Server, response};
        let server = Server::spawn(|request| {
            let url = reqwest::Url::parse(&format!(
                "http://localhost{}",
                request.split_whitespace().nth(1).unwrap()
            ))
            .unwrap();
            assert!(url.path().ends_with("scrobble.view"));
            let p: std::collections::HashMap<_, _> = url.query_pairs().collect();
            assert_eq!(p["id"], "opaque:/α & 夜");
            assert_eq!(p["time"], "1234567890000");
            assert!(matches!(&*p["submission"], "true" | "false"));
            response("200 OK", "", br#"{"subsonic-response":{"status":"ok"}}"#)
        });
        let client = Client::new(
            &server.url,
            "listener",
            "dummy".into(),
            &crate::settings::ProxyConfig::Off,
        )
        .unwrap();
        for submission in [false, true] {
            client
                .scrobble(&Report {
                    id: song(60.).id,
                    title: "Song".into(),
                    occurrence: 1,
                    time: 1234567890000,
                    submission,
                })
                .await
                .unwrap();
        }
    }
}
