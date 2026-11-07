//! Small helpers shared across the application.

use crate::i18n::{Locale, gettext};

/// `3:45` for track lengths, `1:02:03` past an hour.
pub fn format_duration_ms(ms: u32) -> String {
    let total = ms / 1000;
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// `2 hr 13 min` for playlist totals, `45 min 12 sec` under an hour.
pub fn format_total_ms(locale: Locale, ms: u64) -> String {
    let total = ms / 1000;
    let hours = total / 3600;
    let minutes = (total / 60) % 60;
    let seconds = total % 60;
    if hours > 0 {
        // Translators: A length of time, abbreviated. Keep {hours} and {minutes}.
        gettext(locale, "{hours} hr {minutes} min")
            .replace("{hours}", &hours.to_string())
            .replace("{minutes}", &minutes.to_string())
    } else if minutes > 0 {
        // Translators: A length of time, abbreviated. Keep {minutes} and {seconds}.
        gettext(locale, "{minutes} min {seconds} sec")
            .replace("{minutes}", &minutes.to_string())
            .replace("{seconds}", &seconds.to_string())
    } else {
        // Translators: A length of time, abbreviated. Keep {seconds}.
        gettext(locale, "{seconds} sec").replace("{seconds}", &seconds.to_string())
    }
}

pub fn format_count(count: u64) -> String {
    let digits = count.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(character);
    }
    out
}

/// Render the application mark with the established alpha rounding.
pub fn app_icon_rgba(size: usize) -> Vec<u8> {
    if size == 0 {
        return Vec::new();
    }
    let tree = resvg::usvg::Tree::from_data(
        include_bytes!("../packaging/icons/navifast.svg"),
        &Default::default(),
    )
    .expect("bundled SVG");
    let mut bitmap =
        resvg::tiny_skia::Pixmap::new(size as u32, size as u32).expect("icon dimensions");
    let scale = size as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut bitmap.as_mut(),
    );
    bitmap
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            crate::color::Srgba::from_rgba_premultiplied(p[0], p[1], p[2], p[3])
                .to_srgba_unmultiplied()
        })
        .collect()
}

pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for character in text.chars() {
        match character {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(character),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&#x2F;", "/")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Atomically replaces `path` with `temporary` on the current platform.
#[cfg(not(windows))]
pub(crate) fn replace_file(
    temporary: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::rename(temporary, path)
}

/// Atomically replaces `path` with `temporary` on Windows.
#[cfg(windows)]
pub(crate) fn replace_file(
    temporary: &std::path::Path,
    path: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let path: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let moved = unsafe {
        MoveFileExW(
            temporary.as_ptr(),
            path.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Atomically publish a user-selected image without damaging an existing file.
pub(crate) fn save_image(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    // Use an exclusive sibling temporary file; a failed write preserves any existing image.
    let temporary = path.with_extension(format!("image.{:016x}.tmp", rand::random::<u64>()));
    // A collision is not our file to remove. Only clean up after exclusive creation succeeds.
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        crate::util::replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

pub(crate) fn write_json_atomic(
    path: &std::path::Path,
    value: &impl serde::Serialize,
    limit: usize,
) -> std::io::Result<()> {
    use std::io::Write;
    let temporary = path.with_extension(format!("{:016x}.tmp", rand::random::<u64>()));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(
            LimitedWriter {
                writer: &mut writer,
                remaining: limit,
            },
            value,
        )?;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        drop(writer);
        crate::util::replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}
struct LimitedWriter<'a, W> {
    writer: &'a mut W,
    remaining: usize,
}
impl<W: std::io::Write> std::io::Write for LimitedWriter<'_, W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.remaining {
            return Err(std::io::Error::other("The state file is too large."));
        }
        let count = self.writer.write(bytes)?;
        self.remaining -= count;
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(rgba: &[u8], size: usize, x: usize, y: usize) -> [u8; 4] {
        let index = (y * size + x) * 4;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    }

    #[test]
    fn vinyl_icons_keep_transparency_and_a_distinct_label_at_small_sizes() {
        for size in [16, 24, 32, 64, 128, 256] {
            let icon = app_icon_rgba(size);
            assert_eq!(icon.len(), size * size * 4);
            assert_eq!(pixel(&icon, size, 0, 0)[3], 0);
            let label = pixel(&icon, size, size / 2, size * 19 / 32);
            let record = pixel(&icon, size, size / 4, size / 2);
            assert!(
                label[2] > 150 && label[0] > record[0] + 30,
                "vinyl label at {size}: {label:?}"
            );
            assert!(record[2] > record[0], "blue vinyl at {size}: {record:?}");
        }
        assert_eq!(
            pixel(&app_icon_rgba(128), 128, 64, 64)[3],
            0,
            "spindle hole"
        );
        assert!(app_icon_rgba(0).is_empty());
    }

    #[test]
    fn durations_and_counts_follow_the_interface_language() {
        assert_eq!(format_duration_ms(225_000), "3:45");
        assert_eq!(format_duration_ms(3_723_000), "1:02:03");
        assert_eq!(format_total_ms(Locale::English, 7_980_000), "2 hr 13 min");
        assert_eq!(format_total_ms(Locale::English, 2_712_000), "45 min 12 sec");
        assert_eq!(format_total_ms(Locale::Spanish, 7_980_000), "2 h 13 min");
        assert_eq!(format_count(1_234_567), "1,234,567");
        assert_eq!(format_count(12), "12");
    }

    #[test]
    fn html_is_stripped() {
        assert_eq!(
            strip_html("Hi <a href=\"x\">there</a> &amp; you"),
            "Hi there & you"
        );
        assert_eq!(strip_html("ONE&#x2F;TWO&#x2F;THREE"), "ONE/TWO/THREE");
    }
}
