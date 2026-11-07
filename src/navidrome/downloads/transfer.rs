//! One transfer at a time, bounded buffers, explicit cancellation and no playable partials.
use super::*;
use crate::navidrome::api::Client;
use std::time::{Duration, Instant};
use tokio::{
    io::AsyncWriteExt,
    sync::{mpsc, watch},
};
const MAX_AUDIO_BYTES: u64 = 8 * 1024 * 1024 * 1024;

pub(super) struct Completed {
    pub bytes: u64,
    pub codec: String,
}
pub(super) type Progress = (u64, String, u64, Option<u64>);

pub(super) async fn download(
    client: Client,
    http: reqwest::Client,
    song: Song,
    root: PathBuf,
    mut cancel: watch::Receiver<bool>,
    progress: mpsc::Sender<Progress>,
    serial: u64,
) -> Result<Completed, String> {
    let partial = storage::partial_path(&root, &song.id);
    let operation = async {
        if !cancellable(&mut cancel, client.download_allowed()).await? {
            return Err("Your Navidrome account does not have download permission.".into());
        }
        let mut response =
            cancellable(&mut cancel, client.download_response(&http, &song.id)).await?;
        let advertised = response.content_length();
        let original_size = song.size.filter(|s| *s > 0);
        if let (Some(actual), Some(expected)) = (advertised, original_size)
            && actual != expected
        {
            return Err(
                "The source file changed. Refresh the collection, remove this download and add it again."
                    .into(),
            );
        }
        let total = advertised
            .or(original_size)
            .ok_or("The server did not provide a file length needed to verify this download.")?;
        if total == 0 || total > MAX_AUDIO_BYTES {
            return Err("The audio file is empty or larger than 8 GB.".into());
        }
        let mut file = tokio::fs::File::create(&partial).await.map_err(
            |_| "Could not create the download. Check free space and folder permissions.",
        )?;
        let mut bytes = 0u64;
        let mut last = Instant::now() - Duration::from_secs(1);
        while let Some(chunk) = cancellable(&mut cancel, async {
            response.chunk().await.map_err(|_| {
                "The download was interrupted. Retry when the server is available.".to_owned()
            })
        })
        .await?
        {
            bytes = bytes
                .checked_add(chunk.len() as u64)
                .ok_or("The audio file is too large.")?;
            if bytes > total {
                return Err("The server sent more data than the declared file length.".into());
            }
            // reqwest chunks are streamed directly; no whole-file buffer or copy is retained.
            file.write_all(&chunk)
                .await
                .map_err(|_| "Could not write the download. Check free space and try again.")?;
            if last.elapsed() >= Duration::from_millis(250) {
                let _ = progress.try_send((serial, song.id.clone(), bytes, Some(total)));
                last = Instant::now();
            }
        }
        if bytes != total {
            return Err("The download is incomplete. Try again.".into());
        }
        file.flush()
            .await
            .map_err(|_| "Could not finish writing the download.")?;
        file.sync_all()
            .await
            .map_err(|_| "Could not finish writing the download.")?;
        drop(file);
        let path = partial.clone();
        let suffix = song.suffix.clone();
        let decoded = tokio::task::spawn_blocking(move || {
            crate::navidrome::player::inspect_download(&path, &suffix)
        })
        .await
        .map_err(|_| "The downloaded audio could not be checked.")??;
        if *cancel.borrow() {
            return Err("Download cancelled.".into());
        }
        Ok(Completed {
            bytes,
            codec: decoded.codec,
        })
    };
    let result = operation.await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&partial).await;
    }
    result
}

async fn cancellable<T>(
    cancel: &mut watch::Receiver<bool>,
    future: impl std::future::Future<Output = Result<T, String>>,
) -> Result<T, String> {
    if *cancel.borrow() {
        return Err("Download cancelled.".into());
    }
    tokio::select! {
        biased;
        _ = cancel.changed() => Err("Download cancelled.".into()),
        result = future => result,
    }
}
