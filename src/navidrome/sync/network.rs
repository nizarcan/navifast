//! Read desired state before writing, including after a lost acknowledgement.
use super::{Item, Work};
use crate::navidrome::api::{Catalogue, Client, RequestError};

async fn favorites(client: &Client) -> Result<Catalogue, RequestError> {
    let body = client.checked_request("getStarred2", &[]).await?;
    serde_json::from_value(body.get("starred2").cloned().ok_or_else(|| {
        RequestError::Unconfirmed("The server returned no favorites list.".into())
    })?)
    .map_err(|_| RequestError::Unconfirmed("The favorites response is invalid.".into()))
}
async fn matches(client: &Client, work: &Work) -> Result<bool, RequestError> {
    match work {
        Work::Favorite { item, starred } => Ok(favorites(client).await?.contains(item) == *starred),
        Work::Rating {
            kind, id, value, ..
        } => Ok(client.checked_rating(*kind, id).await? == *value),
        Work::Listen { .. } => unreachable!("listens have no readback identity"),
    }
}
async fn exists(client: &Client, item: &Item) -> Result<(), RequestError> {
    let (_, id) = item.key();
    let endpoint = match item {
        Item::Song(_) => "getSong",
        Item::Album(_) => "getAlbum",
        Item::Artist(_) => "getArtist",
    };
    client
        .checked_request(endpoint, &[("id", id.into())])
        .await
        .map(|_| ())
}
pub(super) async fn execute(client: &Client, work: &Work) -> Result<(), RequestError> {
    if let Work::Listen { report, .. } = work {
        return client.scrobble(report).await;
    }
    if matches(client, work).await? {
        return Ok(());
    }
    let written = match work {
        Work::Favorite { item, starred } => {
            // Some servers acknowledge star for an unknown ID. Reject deleted items explicitly.
            if *starred {
                exists(client, item).await?;
            }
            let (kind, id) = item.key();
            client.set_favorite(kind, id, *starred).await
        }
        Work::Rating { id, value, .. } => client.set_rating(id, *value).await,
        Work::Listen { .. } => unreachable!(),
    };
    match written {
        Err(
            error @ (RequestError::Missing | RequestError::Rejected(_) | RequestError::Unsent(_)),
        ) => Err(error),
        Ok(()) | Err(RequestError::Unconfirmed(_)) => {
            if matches(client, work).await? {
                Ok(())
            } else {
                Err(RequestError::Unconfirmed(
                    "The server has not confirmed this change. Your intended value is kept.".into(),
                ))
            }
        }
    }
}
