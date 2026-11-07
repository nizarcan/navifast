//! Account song counts, aggregated on the library worker with bounded storage.
use super::{
    api::{Client, PAGE_SIZE, Song},
    browse::{Items, Query},
};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub byline: String,
    pub album_id: Option<String>,
    pub plays: u128,
}

#[derive(Default, Debug)]
pub struct Summary {
    pub total_plays: u128,
    pub songs_seen: usize,
    pub counts_known: usize,
    pub estimated_seconds: f64,
    pub partial_time: bool,
    pub partial_destinations: bool,
    pub songs: Vec<Entry>,
    pub albums: Vec<Entry>,
    pub artists: Vec<Entry>,
    pub refreshed: String,
}

impl Summary {
    pub fn unavailable(&self) -> bool {
        self.songs_seen > 0 && self.counts_known == 0
    }
    pub fn partial(&self) -> bool {
        self.counts_known < self.songs_seen || self.partial_time || self.partial_destinations
    }
}

#[derive(Default)]
pub struct Aggregate {
    summary: Summary,
    seen: HashSet<String>,
    albums: HashMap<String, Entry>,
    artists: HashMap<String, Entry>,
    metadata_bytes: usize,
}

fn rank(top: &mut Vec<Entry>, entry: Entry) {
    let index = top.partition_point(|old| {
        old.plays > entry.plays || old.plays == entry.plays && old.id < entry.id
    });
    if index < 10 {
        top.insert(index, entry);
        top.truncate(10);
    }
}

impl Aggregate {
    /// Returns newly seen identities, so a repeated full page cannot loop forever.
    pub fn page(&mut self, songs: Vec<Song>) -> Result<usize, String> {
        let before = self.seen.len();
        for song in songs {
            if self.seen.contains(&song.id) {
                continue;
            }
            self.metadata_bytes = self.metadata_bytes.saturating_add(
                song.id.len()
                    + song.title.len()
                    + song.album.len()
                    + song.artist.len()
                    + song.album_id.as_ref().map_or(0, String::len)
                    + song.credits.artist_id.as_ref().map_or(0, String::len)
                    + song
                        .credits
                        .artists
                        .iter()
                        .map(|a| a.name.len() + a.id.as_ref().map_or(0, String::len))
                        .sum::<usize>(),
            );
            if self.seen.len() >= 1_000_000 || self.metadata_bytes > 64 * 1024 * 1024 {
                return Err("The library exceeds the listening summary's scan limit.".into());
            }
            self.seen.insert(song.id.clone());
            self.summary.songs_seen += 1;
            let Some(plays) = song.play_count else {
                continue;
            };
            self.summary.counts_known += 1;
            let plays = u128::from(plays);
            self.summary.total_plays += plays;
            if plays == 0 {
                continue;
            }
            if song.duration.is_finite() && song.duration > 0. {
                let seconds = self.summary.estimated_seconds + song.duration * plays as f64;
                if seconds.is_finite() {
                    self.summary.estimated_seconds = seconds;
                } else {
                    self.summary.partial_time = true;
                }
            } else {
                self.summary.partial_time = true;
            }
            let album_id = song.album_id.filter(|id| !id.is_empty());
            if let Some(id) = &album_id {
                let album = self.albums.entry(id.clone()).or_insert_with(|| Entry {
                    id: id.clone(),
                    name: song.album.clone(),
                    byline: String::new(),
                    album_id: None,
                    plays: 0,
                });
                album.plays += plays;
            } else {
                self.summary.partial_destinations = true;
            }
            let mut credits: Vec<_> = song
                .credits
                .artists
                .iter()
                .filter_map(|a| {
                    a.id.as_deref()
                        .filter(|id| !id.is_empty())
                        .map(|id| (id, a.name.as_str()))
                })
                .collect();
            if !credits.is_empty() && credits.len() < song.credits.artists.len() {
                self.summary.partial_destinations = true;
            }
            if credits.is_empty()
                && let Some(id) = song
                    .credits
                    .artist_id
                    .as_deref()
                    .filter(|id| !id.is_empty())
            {
                credits.push((id, &song.artist));
            }
            if credits.is_empty() {
                self.summary.partial_destinations = true;
            }
            credits.sort_unstable_by_key(|(id, _)| *id);
            credits.dedup_by_key(|(id, _)| *id);
            for (id, name) in credits {
                self.artists
                    .entry(id.into())
                    .or_insert_with(|| Entry {
                        id: id.into(),
                        name: name.into(),
                        byline: String::new(),
                        album_id: None,
                        plays: 0,
                    })
                    .plays += plays;
            }
            rank(
                &mut self.summary.songs,
                Entry {
                    id: song.id,
                    name: song.title,
                    byline: song.artist,
                    album_id,
                    plays,
                },
            );
        }
        Ok(self.seen.len() - before)
    }

    pub fn finish(mut self, refreshed: String) -> Summary {
        for entry in self.albums.into_values() {
            rank(&mut self.summary.albums, entry);
        }
        for entry in self.artists.into_values() {
            rank(&mut self.summary.artists, entry);
        }
        self.summary.refreshed = refreshed;
        self.summary
    }
}

impl Client {
    pub async fn listening_stats(&self) -> Result<Summary, String> {
        let mut aggregate = Aggregate::default();
        let mut offset = 0;
        loop {
            let Items::Songs(songs) = self.browse(&Query::Songs, offset).await? else {
                unreachable!()
            };
            let count = songs.len();
            let added = aggregate.page(songs)?;
            if count < PAGE_SIZE {
                break;
            }
            if added == 0 {
                return Err(
                    "The server repeated a page. The listening summary could not be completed."
                        .into(),
                );
            }
            offset += count;
        }
        Ok(aggregate.finish(
            jiff::Timestamp::now()
                .strftime("%Y-%m-%d %H:%M UTC")
                .to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navidrome::{
        api::ArtistCredit,
        test_server::{Server, response},
    };
    use crate::settings::ProxyConfig;

    fn song(id: &str, plays: Option<u64>, duration: f64, album: &str, artists: &[&str]) -> Song {
        Song {
            id: id.into(),
            title: "Namesake".into(),
            album: "Namesake".into(),
            album_id: Some(album.into()),
            play_count: plays,
            duration,
            credits: super::super::api::ArtistCredits {
                artists: artists
                    .iter()
                    .map(|id| ArtistCredit {
                        id: Some((*id).into()),
                        name: "Namesake".into(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn manually_calculated_totals_dedupe_songs_and_credits_without_merging_namesakes() {
        let a = song("a", Some(3), 120., "compilation", &["one", "two", "one"]);
        let b = song("b", Some(2), 240., "compilation", &["two"]);
        let c = song("c", Some(3), 0., "other", &["three"]);
        let mut aggregate = Aggregate::default();
        assert_eq!(aggregate.page(vec![b.clone(), a.clone()]).unwrap(), 2);
        assert_eq!(
            aggregate
                .page(vec![a, b, c, song("d", None, 100., "other", &["one"])])
                .unwrap(),
            2
        );
        let result = aggregate.finish("done".into());
        assert_eq!(result.total_plays, 8);
        assert_eq!(result.estimated_seconds, 840.);
        assert_eq!((result.songs_seen, result.counts_known), (4, 3));
        assert!(result.partial());
        let ids = |entries: &[Entry]| {
            entries
                .iter()
                .map(|e| (e.id.clone(), e.plays))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ids(&result.songs),
            [("a".into(), 3), ("c".into(), 3), ("b".into(), 2)]
        );
        assert_eq!(
            ids(&result.albums),
            [("compilation".into(), 5), ("other".into(), 3)]
        );
        assert_eq!(
            ids(&result.artists),
            [("two".into(), 5), ("one".into(), 3), ("three".into(), 3)]
        );
    }

    #[test]
    fn zero_missing_counts_and_top_ten_have_distinct_deterministic_results() {
        for plays in [Some(0), None] {
            let mut aggregate = Aggregate::default();
            aggregate
                .page(vec![song("zero", plays, 0., "a", &[])])
                .unwrap();
            let result = aggregate.finish(String::new());
            assert_eq!(result.total_plays, 0);
            assert!(result.songs.is_empty());
            assert_eq!(result.unavailable(), plays.is_none());
            assert_eq!(result.partial(), plays.is_none());
        }
        let empty = Aggregate::default().finish(String::new());
        assert!(!empty.unavailable());
        assert!(!empty.partial());
        let mut aggregate = Aggregate::default();
        aggregate
            .page(
                (0..100)
                    .rev()
                    .map(|i| song(&format!("{i:03}"), Some(1), 1., "a", &["b"]))
                    .collect(),
            )
            .unwrap();
        let result = aggregate.finish(String::new());
        assert_eq!(result.songs.len(), 10);
        assert_eq!(result.songs[0].id, "000");
        assert_eq!(result.songs[9].id, "009");
        assert_eq!(result.total_plays, 100);
    }

    #[test]
    fn legacy_artist_ids_and_missing_destinations_are_honest() {
        let mut track = song("a", Some(u64::MAX), f64::NAN, "", &[]);
        track.credits.artist_id = Some("legacy".into());
        track.artist = "Artist".into();
        let mut aggregate = Aggregate::default();
        aggregate.page(vec![track]).unwrap();
        let result = aggregate.finish(String::new());
        assert_eq!(result.total_plays, u128::from(u64::MAX));
        assert!(result.partial_time && result.partial_destinations);
        assert!(result.songs[0].album_id.is_none());
        assert!(result.albums.is_empty());
        assert_eq!(result.artists[0].id, "legacy");
        assert_eq!(result.estimated_seconds, 0.);
    }

    #[tokio::test]
    async fn scans_multiple_pages_and_refuses_a_repeating_full_page() {
        for repeated in [false, true] {
            let server = Server::spawn(move |request| {
                let url = reqwest::Url::parse(&format!(
                    "http://localhost{}",
                    request.split_whitespace().nth(1).unwrap()
                ))
                .unwrap();
                let offset = url
                    .query_pairs()
                    .find(|(k, _)| k == "songOffset")
                    .unwrap()
                    .1
                    .parse::<usize>()
                    .unwrap();
                let count = if offset == 0 || repeated {
                    PAGE_SIZE
                } else {
                    2
                };
                let songs: Vec<_> = (0..count)
                    .map(|i| {
                        song(
                            &format!("id-{}", if repeated { i } else { offset + i }),
                            Some(2),
                            60.,
                            "album",
                            &["artist"],
                        )
                    })
                    .collect();
                response("200 OK", "", serde_json::json!({"subsonic-response":{"status":"ok","openSubsonic":true,"searchResult3":{"song":songs}}}).to_string().as_bytes())
            });
            let client =
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
            let result = client.listening_stats().await;
            if repeated {
                assert!(result.unwrap_err().contains("repeated a page"));
            } else {
                let summary = result.unwrap();
                assert_eq!(summary.total_plays, 124);
                assert_eq!(summary.estimated_seconds, 7440.);
                assert!(summary.refreshed.ends_with(" UTC"));
            }
        }
    }
}
