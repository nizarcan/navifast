//! Decode to bounded stereo PCM blocks. Output DSP belongs to the continuous source.

use std::collections::VecDeque;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error,
    formats::{FormatOptions, SeekMode, SeekTo},
    io::{MediaSource, MediaSourceStream},
    meta::MetadataOptions,
    probe::Hint,
    units::Time,
};

use super::{PCM_BLOCK_SAMPLES, RATE};

pub(super) fn decode_raw(
    source: Box<dyn MediaSource>,
    suffix: &str,
    position: f64,
    consume: impl FnMut(Vec<f32>) -> bool,
) -> Result<(), String> {
    decode_checked(source, suffix, position, None, consume)
}

pub(super) fn decode_mp3(
    source: Box<dyn MediaSource>,
    position: f64,
    consume: impl FnMut(Vec<f32>) -> bool,
) -> Result<(), String> {
    decode_checked(
        source,
        "mp3",
        position,
        Some(symphonia::core::codecs::CODEC_TYPE_MP3),
        consume,
    )
}

fn decode_checked(
    source: Box<dyn MediaSource>,
    suffix: &str,
    position: f64,
    expected: Option<symphonia::core::codecs::CodecType>,
    mut consume: impl FnMut(Vec<f32>) -> bool,
) -> Result<(), String> {
    let mut hint = Hint::new();
    if !suffix.is_empty() {
        hint.with_extension(suffix);
    }
    let stream = MediaSourceStream::new(source, Default::default());
    let mut format = symphonia::default::get_probe().format(&hint, stream, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map_err(|_| "This audio format could not be opened. Supported originals include MP3, FLAC, WAV, AAC/ALAC, AIFF and Vorbis.".to_owned())?.format;
    let track = format.default_track().ok_or("No audio track was found.")?;
    if expected.is_some_and(|codec| codec != track.codec_params.codec) {
        return Err("The server returned a different codec instead of MP3. Check its MP3 encoder or choose another quality.".into());
    }
    let track_id = track.id;
    let time_base = track.codec_params.time_base;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|_| "This audio codec is not supported for original-file playback.".to_owned())?;
    if position > 0.0 {
        if time_base.is_none() {
            return Err("This file does not provide timestamps needed for seeking.".into());
        }
        format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(position),
                    track_id: Some(track_id),
                },
            )
            .map_err(|_| {
                "The decoder could not seek in this file. Press Play to retry.".to_owned()
            })?;
        decoder.reset();
    }
    let mut resampler = None;
    let mut source_rate = 0;
    let mut pending = VecDeque::new();
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(error)) if error.kind() == std::io::ErrorKind::UnexpectedEof => {
                break;
            }
            Err(Error::IoError(error)) => return Err(error.to_string()),
            Err(_) => return Err("The audio file could not be read.".into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|_| "The audio file contains unsupported or damaged data.".to_owned())?;
        let spec = *decoded.spec();
        let channels = spec.channels.count();
        if channels == 0 || channels > 2 {
            return Err("Only mono and stereo audio are supported. Multichannel conversion is not available yet.".into());
        }
        if source_rate != spec.rate {
            source_rate = spec.rate;
            resampler = crate::resample::Resampler::new(spec.rate, RATE, 2);
        }
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        let start = time_base
            .map(|base| base.calc_time(packet.ts()))
            .map(|t| t.seconds as f64 + t.frac)
            .unwrap_or(0.0);
        let skip = if position > start {
            ((position - start) * f64::from(spec.rate)).round() as usize
        } else {
            0
        };
        let mut stereo: Vec<f32> = buffer
            .samples()
            .chunks_exact(channels)
            .skip(skip)
            .flat_map(|frame| [frame[0], frame[channels - 1]])
            .collect();
        if let Some(resampler) = &mut resampler {
            stereo = resampler.process(&stereo);
        }
        if stereo.is_empty() {
            continue;
        }
        pending.extend(stereo);
        while pending.len() >= PCM_BLOCK_SAMPLES {
            if !consume(pending.drain(..PCM_BLOCK_SAMPLES).collect()) {
                return Ok(());
            }
        }
    }
    while !pending.is_empty() {
        let count = pending.len().min(PCM_BLOCK_SAMPLES);
        if !consume(pending.drain(..count).collect()) {
            break;
        }
    }
    Ok(())
}
