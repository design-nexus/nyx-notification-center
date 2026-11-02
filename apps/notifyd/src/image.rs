//! Pictures sent as raw pixels (the `image-data` hint: avatars, album art) are saved as
//! PNG files in `~/.cache/nyx/notification-images`, so the panel can show them later.

use std::path::PathBuf;

pub fn dir() -> PathBuf {
    std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache"))
        .join("nyx/notification-images")
}

/// `(width, height, rowstride, has_alpha, bits_per_sample, channels, data)`.
pub type Raw = (i32, i32, i32, bool, i32, i32, Vec<u8>);

/// Save raw pixels as a PNG named after their content; returns the file.
pub fn save(raw: &Raw) -> Option<PathBuf> {
    let (w, h, stride, alpha, bits, channels, data) = raw;
    let (w, h, stride, channels) = (*w as usize, *h as usize, *stride as usize, *channels as usize);
    if *bits != 8 || !(3..=4).contains(&channels) || w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    if (*alpha && channels != 4) || data.len() < stride * (h - 1) + w * channels {
        return None;
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = &data[y * stride..y * stride + w * channels];
        for px in row.chunks_exact(channels) {
            rgba.extend_from_slice(&[px[0], px[1], px[2], if channels == 4 { px[3] } else { 255 }]);
        }
    }
    // Named by a hash of the pixels: the same avatar is one file.
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in rgba.iter().step_by(7).chain([w as u8, h as u8].iter()) {
        hash = (hash ^ u64::from(*b)).wrapping_mul(0x100000001b3);
    }
    let dir = dir();
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{hash:016x}-{w}x{h}.png"));
    if path.is_file() {
        return Some(path);
    }
    let tmp = dir.join(format!(".{hash:016x}.tmp"));
    let file = std::fs::File::create(&tmp).ok()?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().ok()?.write_image_data(&rgba).ok()?;
    std::fs::rename(&tmp, &path).ok()?;
    Some(path)
}

/// Drop pictures nobody has looked at for `days`.
pub fn prune(days: u32) {
    let Ok(entries) = std::fs::read_dir(dir()) else { return };
    let limit = std::time::Duration::from_secs(u64::from(days) * 86_400);
    for e in entries.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age > limit);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}
