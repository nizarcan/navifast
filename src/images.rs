//! Extract a representative album-art color.
use std::collections::HashMap;
pub fn accent_color(bytes: &[u8]) -> Option<[u8; 3]> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let small = decoded.thumbnail(48, 48).to_rgb8();
    let mut buckets: HashMap<(u8, u8, u8), (u64, [u64; 3])> = HashMap::new();
    for pixel in small.pixels() {
        let [r, g, b] = pixel.0;
        let (max, min) = (r.max(g).max(b) as f32, r.min(g).min(b) as f32);
        let saturation = if max == 0.0 { 0.0 } else { (max - min) / max };
        let lightness = (max + min) / 510.0;
        // Weight toward vivid mid-tones so black borders and white text lose.
        let weight = (1.0 + saturation * 6.0) * (1.0 - (lightness - 0.5).abs() * 1.4).max(0.05);
        let weight = (weight * 100.0) as u64;
        let key = (r >> 4, g >> 4, b >> 4);
        let bucket = buckets.entry(key).or_insert((0, [0, 0, 0]));
        bucket.0 += weight;
        bucket.1[0] += r as u64 * weight;
        bucket.1[1] += g as u64 * weight;
        bucket.1[2] += b as u64 * weight;
    }
    let (_, (weight, sum)) = buckets.into_iter().max_by_key(|(_, (weight, _))| *weight)?;
    if weight == 0 {
        return None;
    }
    Some([
        (sum[0] / weight) as u8,
        (sum[1] / weight) as u8,
        (sum[2] / weight) as u8,
    ])
}
