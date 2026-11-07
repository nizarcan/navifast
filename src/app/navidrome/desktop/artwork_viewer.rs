//! The viewer identifies cached artwork by account and cover, never by the last reply.
use super::{State, presentation};
use crate::{i18n::mark, navidrome::api::CoverSize};
use std::sync::{Arc, mpsc};

pub(crate) struct Artwork {
    pub title: String,
    pub id: Option<String>,
    pub error: Option<&'static str>,
    pub saved: bool,
    session: u64,
}
#[derive(Default)]
pub(crate) struct Viewer {
    pub open: Option<Artwork>,
    pub saving: bool,
    revision: u64,
    job: Option<(u64, mpsc::Receiver<bool>)>,
}
impl State {
    pub(crate) fn open_album_artwork(&mut self) {
        let Some(album) = &self.app.album else { return };
        self.artwork_viewer.revision = self.artwork_viewer.revision.wrapping_add(1);
        self.artwork_viewer.open = Some(Artwork {
            title: album.name.clone(),
            id: album.cover_art.clone(),
            error: None,
            saved: false,
            session: self.app.session,
        });
        self.retry_artwork();
    }
    pub(crate) fn close_artwork(&mut self) {
        self.artwork_viewer.open = None;
        self.artwork_viewer.revision = self.artwork_viewer.revision.wrapping_add(1);
        // Keep the single export slot until its picker/write completes, even if closed.
    }
    fn artwork_key(&self) -> Option<String> {
        let art = self.artwork_viewer.open.as_ref()?;
        (art.session == self.app.session).then_some(())?;
        Some(presentation::artwork_uri(
            art.session,
            art.id.as_deref()?,
            CoverSize::Original,
        ))
    }
    pub(crate) fn artwork_failed(&self) -> bool {
        self.artwork_key()
            .is_some_and(|key| self.window.covers.borrow().has_failed(&key))
    }
    pub(crate) fn artwork_bytes(&self) -> Option<(Arc<[u8]>, &'static str)> {
        let bytes = self.window.covers.borrow().bytes(&self.artwork_key()?)?;
        // Validation/decoding already happened off-thread. Only inspect the magic bytes here.
        let extension = match image::guess_format(&bytes).ok()? {
            image::ImageFormat::Png => "png",
            image::ImageFormat::Jpeg => "jpg",
            _ => return None,
        };
        Some((bytes, extension))
    }
    pub(crate) fn retry_artwork(&mut self) {
        let Some(key) = self.artwork_key() else {
            return;
        };
        self.window.covers.borrow_mut().retry(&key);
        let id = self
            .artwork_viewer
            .open
            .as_ref()
            .unwrap()
            .id
            .clone()
            .unwrap();
        self.request_cover(&id, CoverSize::Original);
    }
    pub(crate) fn save_artwork(&mut self, tx: &mut rune::Tx) {
        if self.artwork_viewer.saving {
            return;
        }
        let Some((bytes, extension)) = self.artwork_bytes() else {
            return;
        };
        let viewer = &mut self.artwork_viewer;
        let Some(art) = &mut viewer.open else { return };
        let filename = suggested_name(&art.title, extension);
        art.error = None;
        art.saved = false;
        viewer.saving = true;
        let revision = viewer.revision;
        tx.save_file(
            rune::FileDialogOptions {
                title: crate::i18n::gettext(self.app.presentation.locale, mark("Save image"))
                    .into_owned(),
                file_name: Some(filename),
                ..Default::default()
            }
            .filter(
                if extension == "png" {
                    "PNG image"
                } else {
                    "JPEG image"
                },
                &[extension],
            ),
            move |s: &mut State, result, _| {
                match result {
                    Ok(Some(path)) => {
                        // Never rewrite the selected path after the picker confirmed overwrite.
                        let selected = path.extension().and_then(|s| s.to_str());
                        let matches = selected.is_some_and(|e| {
                            e.eq_ignore_ascii_case(extension)
                                || (extension == "jpg" && e.eq_ignore_ascii_case("jpeg"))
                        });
                        if !matches {
                            s.artwork_viewer.saving = false;
                            if revision == s.artwork_viewer.revision
                                && let Some(art) = &mut s.artwork_viewer.open
                            {
                                art.error = Some(mark("Keep the suggested image file extension."));
                            }
                            return;
                        }
                        let (send, receive) = mpsc::channel();
                        let wake = s.window.wake.clone();
                        s.artwork_viewer.job = Some((revision, receive));
                        std::thread::spawn(move || {
                            let _ = send.send(crate::util::save_image(&path, &bytes).is_ok());
                            wake();
                        });
                    }
                    result => {
                        s.artwork_viewer.saving = false;
                        if result.is_err()
                            && revision == s.artwork_viewer.revision
                            && let Some(art) = &mut s.artwork_viewer.open
                        {
                            art.error = Some(mark("Could not open the Save dialog. Try again."));
                        }
                    }
                }
            },
        );
    }
    pub(crate) fn poll_artwork_save(&mut self) -> bool {
        let viewer = &mut self.artwork_viewer;
        let Some((revision, receive)) = &viewer.job else {
            return false;
        };
        let success = match receive.try_recv() {
            Ok(success) => success,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => false,
        };
        if *revision == viewer.revision
            && let Some(art) = &mut viewer.open
        {
            art.saved = success;
            art.error =
                (!success).then_some(mark("Could not save the image. Choose another location."));
        }
        viewer.saving = false;
        viewer.job = None;
        true
    }
}
fn suggested_name(title: &str, extension: &str) -> String {
    // A fixed prefix also avoids reserved Windows device names (CON, AUX, etc.).
    let mut name = String::from("cover-");
    for c in title.chars() {
        if name.len() + c.len_utf8() > 160 {
            break;
        }
        name.push(if c.is_control() || r#"<>:"/\|?*"#.contains(c) {
            '_'
        } else {
            c
        });
    }
    format!("{}.{}", name.trim_end_matches([' ', '.']), extension)
}

#[cfg(feature = "navidrome-demo")]
impl State {
    pub(super) fn demo_artwork(&mut self, flags: &[&str]) {
        use crate::navidrome::backend::Cover;
        if flags.contains(&"artwork-missing") {
            self.app.album.as_mut().unwrap().cover_art = None;
        }
        self.open_album_artwork();
        let Some(key) = self.artwork_key() else {
            return;
        };
        if flags.contains(&"artwork-loading") {
            return;
        }
        if flags.contains(&"artwork-error") {
            self.window
                .covers
                .borrow_mut()
                .failed(&key, std::time::Instant::now());
            return;
        }
        let (w, h) = if flags.contains(&"artwork-portrait") {
            (900, 1500)
        } else if flags.contains(&"artwork-landscape") {
            (1800, 900)
        } else {
            (1200, 1200)
        };
        let image = image::RgbaImage::from_fn(w, h, |x, y| {
            let light = ((x as f32 / w as f32 * 8. + y as f32 / h as f32 * 6.).sin() * 0.25 + 0.75)
                .clamp(0., 1.);
            image::Rgba([
                (35. * light) as u8,
                (85. * light) as u8,
                (112. * light) as u8,
                255,
            ])
        });
        let mut encoded = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        let thumb = image::imageops::thumbnail(&image, 300, 300);
        let preview = image::imageops::thumbnail(&image, 64, 64);
        self.window.covers.borrow_mut().insert(
            key,
            Cover {
                image,
                preview: preview.clone(),
                bytes: encoded.into_inner().into(),
                color: None,
            },
        );
        let id = self
            .artwork_viewer
            .open
            .as_ref()
            .unwrap()
            .id
            .as_ref()
            .unwrap();
        self.window.covers.borrow_mut().insert(
            presentation::artwork_uri(self.app.session, id, CoverSize::Sleeve),
            Cover {
                image: thumb,
                preview,
                bytes: Arc::from([]),
                color: None,
            },
        );
        if flags.contains(&"artwork-save-error") {
            self.artwork_viewer.open.as_mut().unwrap().error =
                Some(mark("Could not save the image. Choose another location."));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn suggested_filename_is_bounded_and_safe_on_all_platforms() {
        assert_eq!(suggested_name("CON", "jpg"), "cover-CON.jpg");
        assert_eq!(
            suggested_name("../A\\B:*?\n. ", "png"),
            "cover-.._A_B____.png"
        );
        let long = suggested_name(&"🎵".repeat(300), "png");
        assert!(long.len() <= 164);
        assert!(!suggested_name("/\\", "png").contains(['/', '\\']));
    }
    #[cfg(feature = "navidrome-demo")]
    mod native {
        use super::*;
        use crate::app::navidrome::desktop::tests::runtime;
        #[test]
        fn opens_from_album_and_dismisses_without_changing_playback() {
            for flags in ["restored", "restored,light"] {
                for width in [760., 1280.] {
                    let mut rt = runtime("album:alb0", flags);
                    rt.set_viewport((width, 800.), 1.);
                    rt.frame(0.);
                    rt.settle_all();
                    let generation = rt.state.app.transport.generation;
                    assert!(rt.accessibility_action(
                        "collection-art",
                        rune::accesskit::Action::Click,
                        None
                    ));
                    rt.frame(0.);
                    rt.settle_all();
                    assert!(rt.state.artwork_viewer.open.is_some());
                    assert!(!rt.state.artwork_failed());
                    assert!(rt.state.artwork_bytes().is_none());
                    let image = rt
                        .elements()
                        .into_iter()
                        .find(|e| e.key.as_deref() == Some("artwork-image"))
                        .unwrap();
                    assert!(image.rect[0] >= 0. && image.rect[1] >= 0.);
                    assert!(image.rect[0] + image.rect[2] <= width);
                    assert!(image.rect[1] + image.rect[3] <= 800.);
                    rt.key(rune::KeyEvent {
                        key: rune::Key::Escape,
                        mods: Default::default(),
                    });
                    rt.frame(0.);
                    assert!(rt.state.artwork_viewer.open.is_none());
                    assert_eq!(generation, rt.state.app.transport.generation);
                    assert_eq!(
                        rt.elements()
                            .into_iter()
                            .find(|e| e.focused)
                            .and_then(|e| e.key)
                            .as_deref(),
                        Some("collection-art")
                    );
                    rt.key(rune::KeyEvent {
                        key: rune::Key::Text(" ".into()),
                        mods: Default::default(),
                    });
                    rt.frame(0.);
                    assert!(
                        rt.state.artwork_viewer.open.is_some(),
                        "focus returns to the album cover"
                    );
                }
            }
        }
        #[test]
        fn cached_image_survives_reopen_but_never_leaks_into_another_album_or_account() {
            let mut rt = runtime("album:alb0", "restored,artwork,artwork-portrait");
            let original = rt.state.artwork_bytes().unwrap().0;
            rt.update(|s, _| {
                s.close_artwork();
                s.open_album_artwork();
            });
            assert!(Arc::ptr_eq(&original, &rt.state.artwork_bytes().unwrap().0));
            assert!(rt.state.app.requested_covers.is_empty());
            rt.update(|s, _| {
                s.app.album = Some(crate::demo::album(1));
                s.open_album_artwork();
            });
            assert!(rt.state.artwork_bytes().is_none());
            // A late old response only updates its old cache key.
            let key = presentation::artwork_uri(rt.state.app.session, "alb0", CoverSize::Original);
            rt.state
                .window
                .covers
                .borrow_mut()
                .failed(&key, std::time::Instant::now());
            assert!(!rt.state.artwork_failed());
            rt.update(|s, tx| s.dispatch(crate::model::Action::SignOut, tx));
            assert!(rt.state.artwork_viewer.open.is_none());
            assert!(rt.state.artwork_bytes().is_none());
        }
        #[test]
        fn native_save_cancels_retries_failure_and_preserves_matching_encoded_bytes() {
            let mut rt = runtime("album:alb0", "restored,artwork");
            let bytes = rt.state.artwork_bytes().unwrap().0;
            rt.update(|s, tx| s.save_artwork(tx));
            let (_, options) = rt.file_dialog().unwrap();
            assert!(options.file_name.as_ref().unwrap().ends_with(".png"));
            rt.resolve_file_dialog(None).unwrap();
            rt.frame(0.);
            assert!(!rt.state.artwork_viewer.saving);
            let root =
                std::env::temp_dir().join(format!("navifast-art-{:016x}", rand::random::<u64>()));
            std::fs::create_dir(&root).unwrap();
            for (path, success) in [
                (root.join("missing/image.png"), false),
                (root.join("wrong.jpg"), false),
                (root.join("cover.png"), true),
            ] {
                rt.update(|s, tx| s.save_artwork(tx));
                rt.resolve_file_dialog(Some(path.clone())).unwrap();
                rt.frame(0.);
                let start = std::time::Instant::now();
                while rt.state.artwork_viewer.saving {
                    rt.update(|s, _| {
                        s.poll_artwork_save();
                    });
                    assert!(start.elapsed() < std::time::Duration::from_secs(5));
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                let art = rt.state.artwork_viewer.open.as_ref().unwrap();
                assert_eq!(art.saved, success);
                assert_eq!(art.error.is_none(), success);
                if success {
                    assert_eq!(
                        std::fs::read(path.with_extension("png")).unwrap(),
                        bytes.as_ref()
                    );
                }
            }
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn closing_during_save_holds_one_worker_and_discards_old_notifications() {
            let mut rt = runtime("album:alb0", "restored,artwork");
            rt.update(|s, tx| s.save_artwork(tx));
            rt.update(|s, _| {
                s.close_artwork();
                s.open_album_artwork();
            });
            assert!(rt.state.artwork_viewer.saving);
            rt.resolve_file_dialog(None).unwrap();
            rt.frame(0.);
            assert!(!rt.state.artwork_viewer.saving);
            let (tx, rx) = mpsc::channel();
            rt.state.artwork_viewer.job =
                Some((rt.state.artwork_viewer.revision.wrapping_sub(1), rx));
            tx.send(false).unwrap();
            assert!(rt.state.poll_artwork_save());
            assert!(
                rt.state
                    .artwork_viewer
                    .open
                    .as_ref()
                    .unwrap()
                    .error
                    .is_none()
            );
        }
    }
}
