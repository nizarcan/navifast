use super::super::api::{Album, FavoriteKind, NamedItem, Song};
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Item {
    Song(Song),
    Album(Album),
    Artist(NamedItem),
}

impl Item {
    pub fn key(&self) -> (FavoriteKind, &str) {
        match self {
            Self::Song(s) => (FavoriteKind::Song, &s.id),
            Self::Album(a) => (FavoriteKind::Album, &a.id),
            Self::Artist(a) => (FavoriteKind::Artist, &a.id),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Song(s) => &s.title,
            Self::Album(a) => &a.name,
            Self::Artist(a) => &a.name,
        }
    }
}
