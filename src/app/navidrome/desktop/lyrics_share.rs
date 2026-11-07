//! An explicit, bounded local export. No server request is made by this workflow.
use super::State;
use crate::{i18n::mark, model::Loadable, navidrome::lyrics::Outcome, theme::Palette};
use rune::{ClipboardContent, ClipboardImage, Pixels, Tx};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, mpsc},
};

pub(crate) const MAX_LINES: usize = 32;
const MAX_TEXT: usize = 16 * 1024;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Background {
    #[default]
    Theme,
    Dark,
    Light,
}

pub(crate) struct Draft {
    pub title: String,
    pub artist: String,
    // Freeze the document while selecting, so playback cannot change a user's passage.
    pub lines: Vec<String>,
    pub selected: BTreeSet<usize>,
    pub cover: Option<Pixels>,
    pub show_cover: bool,
    pub background: Background,
    pub palette: Palette,
    pub preview_open: bool,
    pub ready: Option<Rendered>,
    pub error: Option<String>,
    pub notice: Option<&'static str>,
}

#[derive(Clone)]
pub(crate) struct Card {
    pub title: String,
    pub artist: String,
    pub lines: Vec<String>,
    pub cover: Option<Pixels>,
    pub palette: Palette,
}

pub(crate) struct Rendered {
    pub png: Arc<[u8]>,
    pub pixels: Pixels,
    pub digest: [u8; 32],
    pub description: String,
}

type Job<T> = (u64, mpsc::Receiver<Result<T, String>>);

#[derive(Default)]
pub(crate) struct Share {
    pub draft: Option<Draft>,
    revision: u64,
    render: Option<Job<Rendered>>,
    operation: Option<Job<()>>,
    dirty: bool,
    pub saving: bool,
    operation_notice: Option<&'static str>,
}

impl Draft {
    fn card(&self) -> Card {
        Card {
            title: self.title.clone(),
            artist: self.artist.clone(),
            lines: self
                .selected
                .iter()
                .map(|&i| self.lines[i].clone())
                .collect(),
            cover: self.show_cover.then(|| self.cover.clone()).flatten(),
            palette: match self.background {
                Background::Theme => self.palette,
                Background::Dark => Palette::dark(),
                Background::Light => Palette::light(),
            },
        }
    }

    fn toggle(&mut self, index: usize) -> bool {
        let Some(line) = self.lines.get(index) else {
            return false;
        };
        if self.selected.remove(&index) {
            return true;
        }
        let bytes: usize = self.selected.iter().map(|&i| self.lines[i].len()).sum();
        if self.selected.len() >= MAX_LINES || bytes + line.len() > MAX_TEXT {
            return false;
        }
        self.selected.insert(index);
        true
    }
}

impl State {
    #[cfg(feature = "navidrome-demo")]
    pub(super) fn populate_lyric_share_demo(&mut self, flags: &[&str]) {
        if !flags
            .iter()
            .any(|f| f.starts_with("lyrics-select") || f.starts_with("lyrics-card"))
        {
            return;
        }
        self.select_lyrics(self.palette(!flags.contains(&"light")));
        let Some(draft) = &mut self.lyrics_share.draft else {
            return;
        };
        if flags.contains(&"lyrics-card-no-cover") {
            draft.cover = None;
        }
        if flags.contains(&"lyrics-card-light") {
            draft.background = Background::Light;
        }
        if flags.contains(&"lyrics-card-long") {
            draft.lines[8] =
                "We can take the long way home, past the trees and city lights. ".repeat(4);
            draft.lines[9] = "مرحبا hello עולם, we can take the long way home together\nKeep a little room for wonder".into();
        }
        draft.selected.extend([8, 9, 10]);
        if !flags.contains(&"lyrics-select") {
            draft.preview_open = true;
            // Demo-only startup fixture, before any native window opens.
            match crate::ui::desktop::lyrics_card::render(draft.card()) {
                Ok(ready) => draft.ready = Some(ready),
                Err(error) => draft.error = Some(error),
            }
            if flags.contains(&"lyrics-card-error") {
                draft.error = Some("Could not save the image. Choose another location.".into());
            }
        }
    }
    pub(crate) fn select_lyrics(&mut self, palette: Palette) {
        let Loadable::Loaded(Outcome::Found(lyrics)) = &self.app.lyrics.data else {
            return;
        };
        let Some(song) = self.app.transport.song() else {
            return;
        };
        if song.title.len() + song.artist.len() > 4096 {
            self.app
                .presentation
                .toast_error("The song title or artist is too long for a lyric image.");
            return;
        }
        let cover = self.app.playing_cover().and_then(|id| {
            self.cover_image(id, crate::navidrome::api::CoverSize::Sleeve)
                .1
        });
        self.lyrics_share.revision = self.lyrics_share.revision.wrapping_add(1);
        self.lyrics_share.draft = Some(Draft {
            title: song.title.clone(),
            artist: song.artist.clone(),
            lines: lyrics.lines.iter().map(|l| l.text.clone()).collect(),
            selected: BTreeSet::new(),
            cover,
            show_cover: true,
            background: Background::Theme,
            palette,
            preview_open: false,
            ready: None,
            error: None,
            notice: None,
        });
        self.pause_lyrics_follow();
    }

    pub(crate) fn cancel_lyric_selection(&mut self) {
        self.lyrics_share.draft = None;
        self.lyrics_share.dirty = false;
        self.lyrics_share.revision = self.lyrics_share.revision.wrapping_add(1);
    }

    pub(crate) fn select_lyric_line(&mut self, index: usize) {
        let Some(draft) = &mut self.lyrics_share.draft else {
            return;
        };
        draft.error = if draft.toggle(index) {
            None
        } else {
            Some(
                crate::i18n::gettext(
                    self.app.presentation.locale,
                    mark("Select fewer or shorter lines for this image."),
                )
                .into_owned(),
            )
        };
        draft.ready = None;
        draft.notice = None;
    }

    pub(crate) fn preview_lyrics(&mut self) {
        let Some(draft) = &mut self.lyrics_share.draft else {
            return;
        };
        if draft.selected.is_empty() {
            return;
        }
        draft.preview_open = true;
        self.refresh_lyric_card();
    }

    pub(crate) fn edit_lyric_selection(&mut self) {
        if let Some(draft) = &mut self.lyrics_share.draft {
            draft.preview_open = false;
        }
    }

    pub(crate) fn lyric_background(&mut self, background: Background) {
        if let Some(draft) = &mut self.lyrics_share.draft {
            draft.background = background;
        }
        self.refresh_lyric_card();
    }

    pub(crate) fn lyric_cover(&mut self) {
        if let Some(draft) = &mut self.lyrics_share.draft {
            draft.show_cover = !draft.show_cover;
        }
        self.refresh_lyric_card();
    }

    fn refresh_lyric_card(&mut self) {
        if let Some(draft) = &mut self.lyrics_share.draft {
            draft.ready = None;
            draft.error = None;
            draft.notice = None;
            self.lyrics_share.revision = self.lyrics_share.revision.wrapping_add(1);
            self.lyrics_share.dirty = true;
        }
        self.start_lyric_render();
    }

    fn start_lyric_render(&mut self) {
        let share = &mut self.lyrics_share;
        if !share.dirty || share.render.is_some() {
            return;
        }
        let Some(draft) = &share.draft else { return };
        let card = draft.card();
        let (send, receive) = mpsc::channel();
        let wake = self.window.wake.clone();
        share.render = Some((share.revision, receive));
        share.dirty = false;
        std::thread::spawn(move || {
            let result = crate::ui::desktop::lyrics_card::render(card);
            let _ = send.send(result);
            wake();
        });
    }

    pub(crate) fn poll_lyric_share(&mut self) -> bool {
        let share = &mut self.lyrics_share;
        let mut changed = false;
        if let Some((revision, result)) = take_result(&mut share.render) {
            if revision == share.revision
                && let Some(draft) = &mut share.draft
            {
                match result {
                    Ok(image) => draft.ready = Some(image),
                    Err(error) => draft.error = Some(error),
                }
            }
            changed = true;
        }
        if let Some((revision, result)) = take_result(&mut share.operation) {
            share.saving = false;
            if revision == share.revision
                && let Some(draft) = &mut share.draft
            {
                draft.notice = result.is_ok().then_some(share.operation_notice).flatten();
                draft.error = result.err();
            } else if let Err(error) = result {
                self.app.presentation.toast_error(error);
            }
            changed = true;
        }
        self.start_lyric_render();
        changed
    }

    pub(crate) fn copy_lyric_image(&mut self, tx: &mut Tx) {
        if self.lyrics_share.saving {
            return;
        }
        let Some(ready) = self
            .lyrics_share
            .draft
            .as_ref()
            .and_then(|d| d.ready.as_ref())
        else {
            return;
        };
        let Some(image) = ClipboardImage::from_png(ready.png.clone()) else {
            return;
        };
        let digest = ready.digest;
        let size = ready.pixels.size();
        let revision = self.lyrics_share.revision;
        self.lyrics_share.saving = true;
        self.lyrics_share.operation_notice = Some(mark("Image copied"));
        if let Some(draft) = &mut self.lyrics_share.draft {
            draft.error = None;
            draft.notice = None;
        }
        tx.copy_rich(ClipboardContent::default().with_image(image));
        // Rune writes, then reads on its native clipboard worker. Verify the pixels off-thread:
        // a clipboard may re-encode PNG. This also reports an unavailable clipboard.
        tx.read_clipboard(move |s: &mut State, content, _| {
            let image = content.image().filter(|i| i.size() == size).cloned();
            s.lyric_image_operation(revision, move || {
                if image.is_some_and(|image| {
                    image::load_from_memory_with_format(image.png(), image::ImageFormat::Png)
                        .is_ok_and(|image| pixel_digest(image.to_rgba8().as_raw()) == digest)
                }) {
                    Ok(())
                } else {
                    Err("Could not copy the image. Try Save image instead.".into())
                }
            });
        });
    }

    pub(crate) fn save_lyric_image(&mut self, tx: &mut Tx) {
        if self.lyrics_share.saving {
            return;
        }
        let Some(ready) = self
            .lyrics_share
            .draft
            .as_ref()
            .and_then(|d| d.ready.as_ref())
        else {
            return;
        };
        let png = ready.png.clone();
        let revision = self.lyrics_share.revision;
        self.lyrics_share.saving = true;
        self.lyrics_share.operation_notice = Some(mark("Image saved"));
        if let Some(draft) = &mut self.lyrics_share.draft {
            draft.error = None;
            draft.notice = None;
        }
        tx.save_file(
            rune::FileDialogOptions {
                title: crate::i18n::gettext(self.app.presentation.locale, mark("Save image"))
                    .into_owned(),
                file_name: Some("lyrics.png".into()),
                ..Default::default()
            }
            .filter("PNG image", &["png"]),
            move |s: &mut State, result, _| match result {
                Ok(Some(path)) => s.lyric_image_operation(revision, move || {
                    save_png(&path, &png).map_err(|e| {
                        format!("Could not save the image. Choose another location. {e}")
                    })
                }),
                Ok(None) => s.lyrics_share.saving = false,
                Err(error) => {
                    s.lyrics_share.saving = false;
                    if let Some(draft) = &mut s.lyrics_share.draft {
                        draft.error = Some(format!("Could not open the Save dialog. {error}"));
                    }
                }
            },
        );
    }

    fn lyric_image_operation(
        &mut self,
        revision: u64,
        operation: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) {
        let (send, receive) = mpsc::channel();
        let wake = self.window.wake.clone();
        self.lyrics_share.operation = Some((revision, receive));
        std::thread::spawn(move || {
            let _ = send.send(operation());
            wake();
        });
    }
}

fn take_result<T>(job: &mut Option<Job<T>>) -> Option<(u64, Result<T, String>)> {
    let (revision, receive) = job.as_ref()?;
    let result = match receive.try_recv() {
        Ok(result) => result,
        Err(mpsc::TryRecvError::Empty) => return None,
        Err(mpsc::TryRecvError::Disconnected) => {
            Err("Could not prepare the image. Try again.".into())
        }
    };
    let revision = *revision;
    *job = None;
    Some((revision, result))
}

pub(crate) fn pixel_digest(rgba: &[u8]) -> [u8; 32] {
    use sha2::Digest;
    sha2::Sha256::digest(rgba).into()
}

fn save_png(path: &Path, png: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    // Use an exclusive sibling temporary file; a failed write preserves any existing image.
    let temporary = path.with_extension(format!("png.{:016x}.tmp", rand::random::<u64>()));
    // A collision is not our file to remove. Only clean up after exclusive creation succeeds.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(png)?;
        file.sync_all()?;
        drop(file);
        crate::util::replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Draft {
        Draft {
            title: "Track".into(),
            artist: "Artist".into(),
            lines: vec![
                "First".into(),
                "مرحبا hello עולם".into(),
                "Last\nline".into(),
            ],
            selected: BTreeSet::new(),
            cover: None,
            show_cover: true,
            background: Background::Theme,
            palette: Palette::dark(),
            preview_open: false,
            ready: None,
            error: None,
            notice: None,
        }
    }

    #[test]
    fn selection_exports_in_song_order_without_changing_unicode_or_breaks() {
        let mut d = draft();
        assert!(d.toggle(2));
        assert!(d.toggle(0));
        assert!(d.toggle(1));
        assert_eq!(d.card().lines, d.lines);
        assert!(d.toggle(0));
        assert_eq!(d.card().lines, d.lines[1..]);
        d.background = Background::Light;
        assert_eq!(d.card().palette, Palette::light());
        assert!(d.card().cover.is_none());
    }

    #[test]
    fn selection_limits_reject_excess_without_truncating_the_passage() {
        let mut d = draft();
        d.lines = vec!["line".into(); MAX_LINES + 1];
        for i in 0..MAX_LINES {
            assert!(d.toggle(i));
        }
        assert!(!d.toggle(MAX_LINES));
        assert_eq!(d.card().lines.len(), MAX_LINES);
        d.selected.clear();
        d.lines[0] = "a".repeat(MAX_TEXT + 1);
        assert!(!d.toggle(0));
        assert!(d.selected.is_empty());
        assert!(!d.toggle(usize::MAX));
    }

    #[test]
    fn failed_save_keeps_existing_destination_and_cleans_its_temporary_file() {
        let root =
            std::env::temp_dir().join(format!("navifast-image-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("lyrics.png");
        std::fs::write(&path, b"original").unwrap();
        save_png(&path, b"replacement").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"replacement");
        let directory = root.join("directory.png");
        std::fs::create_dir(&directory).unwrap();
        assert!(save_png(&directory, b"image").is_err());
        assert!(directory.is_dir());
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        assert!(save_png(&root.join("missing/lyrics.png"), b"image").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(feature = "navidrome-demo")]
    fn ready_fixture() -> Rendered {
        use image::ImageEncoder;
        let rgba = [120, 80, 30, 255].repeat(4);
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&rgba, 2, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        let pixels = Pixels::new();
        let digest = pixel_digest(&rgba);
        pixels.set(2, 2, rgba);
        Rendered {
            png: png.into(),
            pixels,
            digest,
            description: "Fixture".into(),
        }
    }

    #[cfg(feature = "navidrome-demo")]
    fn wait_operation(rt: &mut rune::Runtime<State>) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while rt.state.lyrics_share.saving {
            rt.update(|s, _| {
                s.poll_lyric_share();
            });
            rt.frame(0.);
            assert!(
                std::time::Instant::now() < deadline,
                "image operation timed out"
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    #[cfg(feature = "navidrome-demo")]
    #[test]
    fn native_selection_keeps_its_song_and_canceled_save_keeps_its_lines() {
        let mut rt = super::super::tests::runtime("album:alb0", "lyrics");
        assert!(rt.accessibility_action("lyrics-image", rune::accesskit::Action::Click, None));
        rt.frame(0.);
        rt.settle_all();
        let position = rt.state.app.transport.position;
        for index in [10, 8, 9] {
            assert!(rt.accessibility_action(
                &format!("share-line-{index}"),
                rune::accesskit::Action::Click,
                None
            ));
            rt.frame(0.);
        }
        assert_eq!(
            rt.state.app.transport.position, position,
            "selection must not seek"
        );
        rt.update(|s, tx| {
            s.app.preferences.volume = 0.5;
            assert!(crate::ui::desktop::keys(
                s,
                &rune::KeyEvent {
                    key: rune::Key::Up,
                    mods: rune::Mods {
                        cmd: true,
                        ..Default::default()
                    },
                },
                tx
            ));
        });
        assert!(
            (rt.state.app.preferences.volume - 0.55).abs() < 0.001,
            "selection keeps playback shortcuts available"
        );
        let original = rt.state.lyrics_share.draft.as_ref().unwrap().card();
        rt.update(|s, _| {
            s.app.apply(crate::app::navidrome::Action::Next);
            s.lyrics_share.draft.as_mut().unwrap().ready = Some(ready_fixture());
            s.lyrics_share.draft.as_mut().unwrap().preview_open = true;
        });
        rt.frame(0.);
        assert_eq!(
            rt.state.lyrics_share.draft.as_ref().unwrap().card().title,
            original.title
        );
        rt.update(|s, tx| s.save_lyric_image(tx));
        assert_eq!(
            rt.file_dialog().map(|(kind, _)| kind),
            Some(rune::FileDialogKind::Save)
        );
        rt.resolve_file_dialog(None).unwrap();
        rt.frame(0.);
        assert!(!rt.state.lyrics_share.saving);
        assert_eq!(
            rt.state.lyrics_share.draft.as_ref().unwrap().card().lines,
            original.lines
        );
        assert!(
            rt.state
                .lyrics_share
                .draft
                .as_ref()
                .unwrap()
                .ready
                .is_some()
        );
        rt.update(|s, _| s.edit_lyric_selection());
        rt.frame(0.);
        assert!(!rt.state.lyrics_share.draft.as_ref().unwrap().preview_open);
        rt.update(|s, _| s.select_lyric_line(9));
        assert_eq!(
            rt.state.lyrics_share.draft.as_ref().unwrap().selected.len(),
            2
        );
    }

    #[cfg(feature = "navidrome-demo")]
    #[test]
    fn copy_verifies_pixels_and_failed_clipboard_or_save_remains_recoverable() {
        let mut rt = super::super::tests::runtime("album:alb0", "lyrics-select");
        let board = rune::clipboard::memory();
        rt.use_clipboard(&board);
        let ready = ready_fixture();
        let png = ready.png.clone();
        rt.update(|s, _| s.lyrics_share.draft.as_mut().unwrap().ready = Some(ready));
        board.fail_writes(Some("unavailable"));
        rt.update(|s, tx| s.copy_lyric_image(tx));
        rt.frame(0.);
        wait_operation(&mut rt);
        assert!(
            rt.state
                .lyrics_share
                .draft
                .as_ref()
                .unwrap()
                .error
                .as_ref()
                .unwrap()
                .contains("Try Save image")
        );
        assert_eq!(
            rt.state.lyrics_share.draft.as_ref().unwrap().selected.len(),
            3
        );
        board.fail_writes(None);
        rt.update(|s, tx| s.copy_lyric_image(tx));
        rt.frame(0.);
        wait_operation(&mut rt);
        assert!(
            rt.state
                .lyrics_share
                .draft
                .as_ref()
                .unwrap()
                .error
                .is_none()
        );
        assert_eq!(
            board.content().unwrap().image().unwrap().png(),
            png.as_ref()
        );
        assert_eq!(
            rt.state.lyrics_share.draft.as_ref().unwrap().notice,
            Some("Image copied")
        );
        let root =
            std::env::temp_dir().join(format!("navifast-save-{:016x}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        for (path, success) in [
            (root.join("missing/lyrics.png"), false),
            (root.join("lyrics.png"), true),
        ] {
            rt.update(|s, tx| s.save_lyric_image(tx));
            rt.resolve_file_dialog(Some(path.clone())).unwrap();
            rt.frame(0.);
            wait_operation(&mut rt);
            assert_eq!(
                rt.state
                    .lyrics_share
                    .draft
                    .as_ref()
                    .unwrap()
                    .error
                    .is_none(),
                success
            );
            if success {
                assert_eq!(std::fs::read(path).unwrap(), png.as_ref());
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
