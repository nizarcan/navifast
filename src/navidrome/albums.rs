//! Server album orders and inclusive year ranges.
use super::api::{Album, Client, PAGE_SIZE, decode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Order {
    Newest,
    Name,
    Frequent,
    Recent,
    Random,
    Year { from: u16, to: u16 },
}

impl Order {
    pub fn years(from: &str, to: &str) -> Option<Self> {
        let (from, to) = (from.trim().parse().ok()?, to.trim().parse().ok()?);
        (from > 0 && from <= to && to <= 9999).then_some(Self::Year { from, to })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Newest => "newest",
            Self::Name => "alphabeticalByName",
            Self::Frequent => "frequent",
            Self::Recent => "recent",
            Self::Random => "random",
            Self::Year { .. } => "byYear",
        }
    }
    pub fn encode(self) -> String {
        match self {
            Self::Year { from, to } => format!("year:{from}:{to}"),
            _ => self.name().into(),
        }
    }
    pub fn decode(value: &str) -> Option<Self> {
        Some(match value {
            "newest" => Self::Newest,
            "alphabeticalByName" => Self::Name,
            "frequent" => Self::Frequent,
            "recent" => Self::Recent,
            "random" => Self::Random,
            other => {
                let (from, to) = other.strip_prefix("year:")?.split_once(':')?;
                return Self::years(from, to);
            }
        })
    }
}

impl Client {
    pub async fn album_list(&self, offset: usize, order: Order) -> Result<Vec<Album>, String> {
        let mut params = vec![
            ("type", order.name().into()),
            ("size", PAGE_SIZE.to_string()),
            ("offset", offset.to_string()),
        ];
        if let Order::Year { from, to } = order {
            if from == 0 || from > to || to > 9999 {
                return Err("Enter a valid inclusive year range.".into());
            }
            params.extend([("fromYear", from.to_string()), ("toYear", to.to_string())]);
        }
        let body = self
            .checked_request("getAlbumList2", &params)
            .await
            .map_err(|e| e.to_string())?;
        let group = body
            .get("albumList2")
            .filter(|v| v.is_object())
            .ok_or("The server returned no album list.")?;
        let albums: Vec<Album> = decode(
            group
                .get("album")
                .filter(|v| !v.is_null())
                .cloned()
                .unwrap_or_else(|| serde_json::json!([])),
        )?;
        if albums.len() > PAGE_SIZE || albums.iter().any(|a| a.id.is_empty()) {
            return Err("The server returned an invalid album page.".into());
        }
        Ok(albums)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        navidrome::test_server::{Server, response},
        settings::ProxyConfig,
    };
    #[test]
    fn year_ranges_validate_and_round_trip() {
        for (from, to) in [("1990", "1999"), ("2000", "2000"), ("1", "9999")] {
            let order = Order::years(from, to).unwrap();
            assert_eq!(Order::decode(&order.encode()), Some(order));
        }
        for (from, to) in [
            ("", "2000"),
            ("199x", "2000"),
            ("2001", "2000"),
            ("0", "1"),
            ("1", "10000"),
            ("-1", "2000"),
        ] {
            assert!(Order::years(from, to).is_none());
        }
    }
    #[tokio::test]
    async fn inclusive_year_bounds_and_server_orders_use_the_album_endpoint() {
        for order in [
            Order::Year {
                from: 1990,
                to: 1999,
            },
            Order::Year {
                from: 2000,
                to: 2000,
            },
            Order::Frequent,
            Order::Recent,
            Order::Random,
        ] {
            let server = Server::spawn(move |request| {
                let url = reqwest::Url::parse(&format!(
                    "http://localhost{}",
                    request.split_whitespace().nth(1).unwrap()
                ))
                .unwrap();
                let pairs: std::collections::HashMap<_, _> = url.query_pairs().collect();
                assert!(url.path().ends_with("getAlbumList2.view"));
                assert_eq!(pairs.get("type").unwrap(), order.name());
                assert_eq!(pairs.get("offset").unwrap(), "0");
                assert_eq!(pairs.get("size").unwrap(), "60");
                if let Order::Year { from, to } = order {
                    assert_eq!(pairs.get("fromYear").unwrap(), &from.to_string());
                    assert_eq!(pairs.get("toYear").unwrap(), &to.to_string());
                }
                // A fresh account has no reported listening history.
                response(
                    "200 OK",
                    "",
                    br#"{"subsonic-response":{"status":"ok","albumList2":{}}}"#,
                )
            });
            let client =
                Client::new(&server.url, "listener", "dummy".into(), &ProxyConfig::Off).unwrap();
            assert!(client.album_list(0, order).await.unwrap().is_empty());
        }
    }
}
