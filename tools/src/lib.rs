//! Host-only conversion shared by the CLI and companion.
pub mod flash;
pub mod font;
pub mod store;
use image::{
    DynamicImage, GrayImage, ImageReader, Luma,
    imageops::{self, FilterType},
};
use x3_core::bitmap;
use x3_proto::payload::{TokenHeader, crc32, encode_article};
pub type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    Contain,
    Cover,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Auto,
    Landscape,
    Portrait,
}
/// How greyscale becomes the panel's black and white pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dither {
    /// 4×4 ordered pattern (the original encoder).
    Bayer,
    /// Error diffusion spreading all error; smooth gradients, some worming.
    FloydSteinberg,
    /// Error diffusion spreading 6/8 of the error; crisp text and clean whites.
    Atkinson,
}
/// Image preparation before dithering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quality {
    pub filter: FilterType,
    /// Stretch the darkest and lightest 0.5 % of pixels to full black and white.
    pub levels: bool,
    /// Unsharp-mask radius in pixels; 0 turns it off.
    pub sharpen: f32,
    /// Exponent applied to 0–1 grey before dithering; 1 leaves tones as they are.
    pub gamma: f32,
    pub dither: Dither,
}
impl Quality {
    /// The original encoder, kept for comparisons and golden tests.
    pub const ORIGINAL: Self = Self {
        filter: FilterType::Triangle,
        levels: false,
        sharpen: 0.0,
        gamma: 1.0,
        dither: Dither::Bayer,
    };
    /// Chosen by side-by-side comparison on three Scryfall tokens (ADR 016).
    pub const TOKEN: Self = Self {
        filter: FilterType::Lanczos3,
        levels: true,
        sharpen: 0.8,
        gamma: 0.8,
        dither: Dither::Atkinson,
    };
}
/// Four even grey steps for 2-bit tokens: black, dark grey, light grey, white.
/// Beat CrossPoint's darker panel model in the owner's panel test (ADR 018).
pub const EVEN_TONES: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
/// Token encoding: ADR 016 preparation (Lanczos3, levels, light sharpening, a
/// lighter tone curve), then Atkinson diffusion to four greys (ADR 018).
pub fn token(bytes: &[u8], fit: Fit, orientation: Orientation) -> Result<Vec<u8>> {
    let levels = grey4(bytes, fit, orientation, Quality::TOKEN, EVEN_TONES)?;
    let (w, h) = levels.dimensions();
    encode(levels.as_raw(), w, h, 2)
}
/// One-bit token with the given quality, kept for comparisons and tests.
pub fn token_with(
    bytes: &[u8],
    fit: Fit,
    orientation: Orientation,
    quality: Quality,
) -> Result<Vec<u8>> {
    let grey = prepare(bytes, fit, orientation, quality)?;
    let (w, h) = grey.dimensions();
    let mut samples = vec![0; w as usize * h as usize];
    match quality.dither {
        Dither::Bayer => bitmap::dither(grey.as_raw(), w as usize, 1, &mut samples)
            .map_err(|_| "dither failed")?,
        method => diffuse(&grey, quality.gamma, method, &[0.0, 1.0], &mut samples),
    }
    encode(&samples, w, h, 1)
}
fn encode(samples: &[u8], w: u32, h: u32, bpp: u8) -> Result<Vec<u8>> {
    let mut header = TokenHeader {
        bpp,
        width: w as u16,
        height: h as u16,
        crc: 0,
    };
    let mut out = vec![0; header.payload_len()];
    bitmap::pack(samples, bpp, &mut out[16..]).map_err(|_| "pack failed")?;
    header.crc = crc32(&out[16..]);
    header.encode(&mut out).map_err(|_| "header failed")?;
    Ok(out)
}
/// Four-level encoding. `tones` are the panel's apparent brightness for levels
/// 0–3, from 0.0 (black) to 1.0 (white). Returns level indices 0–3 per pixel.
pub fn grey4(
    bytes: &[u8],
    fit: Fit,
    orientation: Orientation,
    quality: Quality,
    tones: [f32; 4],
) -> Result<GrayImage> {
    let mut grey = prepare(bytes, fit, orientation, quality)?;
    let mut levels = vec![0; grey.as_raw().len()];
    diffuse(&grey, quality.gamma, quality.dither, &tones, &mut levels);
    grey.copy_from_slice(&levels);
    Ok(grey)
}
/// Decode, flatten transparency, resize to the screen and apply the tone steps.
fn prepare(
    bytes: &[u8],
    fit: Fit,
    orientation: Orientation,
    quality: Quality,
) -> Result<GrayImage> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("image exceeds 8 MiB".into());
    }
    let mut reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "unknown image format")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|_| "invalid or oversized image")?;
    let portrait = orientation == Orientation::Portrait
        || (orientation == Orientation::Auto && decoded.height() > decoded.width());
    let (w, h) = if portrait { (528, 792) } else { (792, 528) };
    // Composite transparency onto white before resampling and luminance conversion.
    let mut rgba = decoded.to_rgba8();
    for p in rgba.pixels_mut() {
        for i in 0..3 {
            p[i] = ((u32::from(p[i]) * u32::from(p[3]) + 255 * (255 - u32::from(p[3])) + 127) / 255)
                as u8;
        }
        p[3] = 255;
    }
    let mut image = DynamicImage::ImageRgba8(rgba);
    // An explicit orientation against the image's shape turns the image a
    // quarter clockwise instead of shrinking it between margins. Scryfall
    // stores battles upright with the design sideways, text reading up.
    let source_portrait = image.height() > image.width();
    if image.height() != image.width() && source_portrait != portrait {
        image = image.rotate90();
    }
    let grey = match fit {
        Fit::Cover => tone(
            image.resize_to_fill(w, h, quality.filter).to_luma8(),
            quality,
        ),
        Fit::Contain => {
            let resized = tone(image.resize(w, h, quality.filter).to_luma8(), quality);
            let mut canvas = GrayImage::from_pixel(w, h, Luma([255]));
            imageops::replace(
                &mut canvas,
                &resized,
                i64::from((w - resized.width()) / 2),
                i64::from((h - resized.height()) / 2),
            );
            canvas
        }
    };
    Ok(grey)
}
/// Levels and sharpening on the resized image, before any white padding.
fn tone(mut grey: GrayImage, q: Quality) -> GrayImage {
    if q.levels {
        let mut histogram = [0usize; 256];
        for p in grey.as_raw() {
            histogram[usize::from(*p)] += 1;
        }
        let total = grey.as_raw().len();
        let cut = total / 200;
        let mut seen = 0;
        let lo = histogram.iter().position(|&n| {
            seen += n;
            seen > cut
        });
        seen = 0;
        let hi = histogram.iter().rposition(|&n| {
            seen += n;
            seen > cut
        });
        // Leave nearly flat images alone rather than amplifying noise.
        if let (Some(lo), Some(hi)) = (lo, hi)
            && hi > lo + 32
        {
            let (lo, span) = (lo as f32, (hi - lo) as f32);
            for p in grey.pixels_mut() {
                p[0] = ((f32::from(p[0]) - lo) * 255.0 / span)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
    }
    if q.sharpen > 0.0 {
        grey = imageops::unsharpen(&grey, q.sharpen, 2);
    }
    grey
}
/// Serpentine error diffusion. Each output sample is the index of the nearest
/// entry in `tones` (ascending, 0.0–1.0); `[0.0, 1.0]` gives 0/1 with 1 = white.
fn diffuse(grey: &GrayImage, gamma: f32, method: Dither, tones: &[f32], out: &mut [u8]) {
    let (w, h) = (grey.width() as usize, grey.height() as usize);
    let mut buf: Vec<f32> = grey
        .as_raw()
        .iter()
        .map(|&v| (f32::from(v) / 255.0).powf(gamma))
        .collect();
    // (dx, dy, weight) for left-to-right rows; mirrored on right-to-left rows.
    const FS: [(isize, usize, f32); 4] = [
        (1, 0, 7.0 / 16.0),
        (-1, 1, 3.0 / 16.0),
        (0, 1, 5.0 / 16.0),
        (1, 1, 1.0 / 16.0),
    ];
    const ATKINSON: [(isize, usize, f32); 6] = [
        (1, 0, 0.125),
        (2, 0, 0.125),
        (-1, 1, 0.125),
        (0, 1, 0.125),
        (1, 1, 0.125),
        (0, 2, 0.125),
    ];
    let kernel: &[(isize, usize, f32)] = if method == Dither::Atkinson {
        &ATKINSON
    } else {
        &FS
    };
    for y in 0..h {
        let reverse = y % 2 == 1;
        for i in 0..w {
            let x = if reverse { w - 1 - i } else { i };
            let old = buf[y * w + x];
            let mut level = 0;
            for (i, pair) in tones.windows(2).enumerate() {
                if old >= (pair[0] + pair[1]) / 2.0 {
                    level = i + 1;
                }
            }
            let new = tones[level];
            out[y * w + x] = level as u8;
            let error = old - new;
            for &(dx, dy, weight) in kernel {
                let dx = if reverse { -dx } else { dx };
                let (nx, ny) = (x as isize + dx, y + dy);
                if nx >= 0 && (nx as usize) < w && ny < h {
                    buf[ny * w + nx as usize] += error * weight;
                }
            }
        }
    }
}
pub fn token_preview(payload: &[u8]) -> Result<GrayImage> {
    let h = TokenHeader::payload(payload).map_err(|_| "invalid token payload")?;
    let mut samples = vec![0; usize::from(h.width) * usize::from(h.height)];
    bitmap::unpack(&payload[16..], h.bpp, &mut samples).map_err(|_| "unpack failed")?;
    let step = 255 / ((1 << h.bpp) - 1);
    for p in &mut samples {
        *p *= step;
    }
    GrayImage::from_raw(h.width.into(), h.height.into(), samples)
        .ok_or_else(|| "preview dimensions".into())
}
pub fn article(title: &str, url: &str, text: &str, summary: bool, time: u64) -> Result<Vec<u8>> {
    if text.len() > 1024 * 1024 {
        return Err("input exceeds 1 MiB".into());
    }
    let normal = text.replace("\r\n", "\n").replace('\r', "\n");
    let normal: String = normal
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    let mut out = vec![0; x3_proto::ARTICLE_CAP];
    let n = encode_article(title, url, &normal, summary, time, &mut out)
        .map_err(|_| "invalid article fields or size")?;
    out.truncate(n);
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn png(w: u32, h: u32) -> Vec<u8> {
        let i = GrayImage::from_pixel(w, h, Luma([0]));
        let mut out = std::io::Cursor::new(Vec::new());
        i.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }
    fn white_share(grey: u8, quality: Quality) -> f32 {
        let image = GrayImage::from_pixel(200, 300, Luma([grey]));
        let mut png_bytes = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut png_bytes, image::ImageFormat::Png)
            .unwrap();
        let b = token_with(
            png_bytes.get_ref(),
            Fit::Cover,
            Orientation::Portrait,
            quality,
        )
        .unwrap();
        let p = token_preview(&b).unwrap();
        p.pixels().filter(|p| p[0] == 255).count() as f32 / p.pixels().count() as f32
    }
    #[test]
    fn token_dithering_is_clean_deterministic_and_tone_true() {
        let flat = Quality {
            levels: false,
            gamma: 1.0,
            ..Quality::TOKEN
        };
        assert_eq!(white_share(255, Quality::TOKEN), 1.0);
        assert_eq!(white_share(0, Quality::TOKEN), 0.0);
        let mid = white_share(128, flat);
        assert!((0.45..0.55).contains(&mid), "{mid}");
        assert!(white_share(192, flat) > mid + 0.15);
        // The lighter tone curve opens shadows: more white for the same grey.
        assert!(white_share(64, Quality::TOKEN) > white_share(64, flat));
        let card = png(300, 420);
        let a = token(&card, Fit::Contain, Orientation::Auto).unwrap();
        assert_eq!(a, token(&card, Fit::Contain, Orientation::Auto).unwrap());
    }
    #[test]
    fn tokens_are_four_grey_with_clean_ends() {
        let b = token(&png(300, 420), Fit::Contain, Orientation::Auto).unwrap();
        let h = TokenHeader::payload(&b).unwrap();
        assert_eq!((h.bpp, b.len()), (2, x3_proto::TOKEN_CAP));
        let grey = |v: u8| {
            let image = GrayImage::from_pixel(200, 300, Luma([v]));
            let mut bytes = std::io::Cursor::new(Vec::new());
            image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
            let flat = Quality {
                levels: false,
                gamma: 1.0,
                ..Quality::TOKEN
            };
            let levels = grey4(
                bytes.get_ref(),
                Fit::Cover,
                Orientation::Portrait,
                flat,
                EVEN_TONES,
            )
            .unwrap();
            let mut seen = [0usize; 4];
            for p in levels.pixels() {
                seen[usize::from(p[0])] += 1;
            }
            seen
        };
        // Exact grey steps need no dots at all; black and white stay solid.
        for (v, level) in [(0, 0), (85, 1), (170, 2), (255, 3)] {
            assert_eq!(grey(v)[level], 528 * 792, "grey {v}");
        }
        // Between two steps only those two levels appear.
        let mid = grey(128);
        assert_eq!(mid[0] + mid[3], 0);
        assert!(mid[1] > 0 && mid[2] > 0);
    }
    #[test]
    fn contain_cover_and_portrait() {
        let input = png(100, 200);
        let b = token(&input, Fit::Contain, Orientation::Auto).unwrap();
        let p = token_preview(&b).unwrap();
        assert_eq!(p.dimensions(), (528, 792));
        assert_eq!(p.get_pixel(0, 0)[0], 255);
        assert_eq!(p.get_pixel(264, 396)[0], 0);
        let b = token(&input, Fit::Cover, Orientation::Landscape).unwrap();
        let p = token_preview(&b).unwrap();
        assert_eq!(p.dimensions(), (792, 528));
        assert!(p.pixels().all(|p| p[0] == 0));
        // A portrait image asked for landscape is turned, not letterboxed:
        // a mark at its top-left lands top-right.
        let mut tall = GrayImage::from_pixel(300, 420, Luma([255]));
        for y in 0..30 {
            for x in 0..30 {
                tall.put_pixel(x, y, Luma([0]));
            }
        }
        let mut bytes = std::io::Cursor::new(Vec::new());
        tall.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let turned =
            token_preview(&token(bytes.get_ref(), Fit::Contain, Orientation::Landscape).unwrap())
                .unwrap();
        assert_eq!((turned.width(), turned.height()), (792, 528));
        assert!(turned.get_pixel(745, 15)[0] < 128, "mark top-right");
        assert!(turned.get_pixel(45, 15)[0] > 128, "no mark top-left");
        let b = token(&png(100, 100), Fit::Contain, Orientation::Landscape).unwrap();
        assert_eq!(token_preview(&b).unwrap().get_pixel(0, 0)[0], 255);
    }
    #[test]
    fn hostile_and_article() {
        assert!(token(b"not png", Fit::Cover, Orientation::Auto).is_err());
        assert!(token(&vec![0; 8 * 1024 * 1024 + 1], Fit::Cover, Orientation::Auto).is_err());
        assert!(token_preview(b"no").is_err());
        let b = article("Zażółć", "https://example.org", "a\r\nb\rc\0\t", true, 0).unwrap();
        let a = x3_proto::payload::Article::decode(&b).unwrap();
        assert_eq!(a.text, "a\nb\nc\t");
        assert!(a.header.summary);
        assert!(article("a", "", "a", false, u64::MAX).is_err());
        assert!(article("a", "", &"a".repeat(262145), false, 0).is_err());
    }
}
