//! Deterministic Navidrome catalog data for Rune previews.
use crate::navidrome::api::{Album, ArtistCredits, Song};
const ARTISTS: &[&str] = &[
    "Bonobo",
    "Khruangbin",
    "Nils Frahm",
    "Little Simz",
    "Floating Points",
    "Jon Hopkins",
    "Sault",
    "Four Tet",
];

pub(crate) const ALBUMS: &[(&str, usize, &str)] = &[
    ("Fragments", 0, "2022"),
    ("Mordechai", 1, "2020"),
    ("All Melody", 2, "2018"),
    ("Sometimes I Might Be Introvert", 3, "2021"),
    ("Promises", 4, "2021"),
    ("Immunity", 5, "2013"),
    ("Untitled (Black Is)", 6, "2020"),
    ("There Is Love in You", 7, "2010"),
];

const TRACKS: &[&str] = &[
    "Rosewood",
    "Otomo",
    "Shadows",
    "Tides",
    "Elysian",
    "Closer",
    "Counterpart",
    "Sapien",
    "From You",
    "Day by Day",
    "Age of Phase",
    "Polyghost",
    "Time Moves Slow",
    "August 10",
    "So Rare",
    "Fugue",
    "Encores",
    "Sunlight",
    "My Friend the Forest",
    "Kaleidoscope",
];

pub(crate) fn album(index: usize) -> Album {
    let (name, artist_index, year) = ALBUMS[index % ALBUMS.len()];
    let id = format!("alb{index}");
    Album {
        id: id.clone(),
        user_rating: None,
        name: name.into(),
        artist: ARTISTS[artist_index].into(),
        credits: ArtistCredits {
            artist_id: Some(format!("art{artist_index}")),
            ..Default::default()
        },
        cover_art: Some(id.clone()),
        song_count: 12,
        release_types: vec!["Album".into()],
        year: year.parse().unwrap(),
        song: (0..12)
            .map(|track| {
                let album_index = track % ALBUMS.len();
                Song {
                    id: format!("trk{track}"),
                    play_count: Some((12 - track) as u64 * 7),
                    user_rating: None,
                    title: TRACKS[track % TRACKS.len()].into(),
                    credits: ArtistCredits {
                        artist_id: Some(format!("art{album_index}")),
                        ..Default::default()
                    },
                    artist: ARTISTS[album_index % ARTISTS.len()].into(),
                    album: ALBUMS[album_index].0.into(),
                    album_id: Some(format!("alb{album_index}")),
                    cover_art: Some(id.clone()),
                    duration: f64::from(180_000 + (track as u32 * 37_000) % 240_000) / 1000.,
                    suffix: "flac".into(),
                    track: track as u32 + 1,
                    disc_number: 1,
                }
            })
            .collect(),
    }
}
