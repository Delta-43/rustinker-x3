//! Writes a side-by-side sheet of token encodings for visual comparison.
//! Usage: cargo run -p x3-tools --example dither_compare -- OUT.png IMAGE...
use image::{GrayImage, Luma, imageops::FilterType};
use x3_tools::{Dither, Fit, Orientation, Quality, token_preview, token_with};
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (out, inputs) = args.split_first().expect("OUT.png IMAGE...");
    let lanczos = |dither, levels, sharpen, gamma| Quality {
        filter: FilterType::Lanczos3,
        levels,
        sharpen,
        gamma,
        dither,
    };
    let variants = [
        Quality::ORIGINAL,
        lanczos(Dither::FloydSteinberg, false, 0.0, 1.0),
        lanczos(Dither::Atkinson, false, 0.0, 1.0),
        lanczos(Dither::Atkinson, true, 0.8, 1.0),
        lanczos(Dither::Atkinson, true, 0.8, 0.8),
    ];
    let (cw, ch) = (528u32, 792u32);
    let mut sheet = GrayImage::from_pixel(
        cw * variants.len() as u32 + 8 * (variants.len() as u32 - 1),
        (ch + 8) * inputs.len() as u32,
        Luma([128]),
    );
    for (row, path) in inputs.iter().enumerate() {
        let bytes = std::fs::read(path).expect("read");
        for (col, q) in variants.iter().enumerate() {
            let payload =
                token_with(&bytes, Fit::Contain, Orientation::Portrait, *q).expect("encode");
            let img = token_preview(&payload).expect("preview");
            image::imageops::replace(
                &mut sheet,
                &img,
                i64::from(col as u32 * (cw + 8)),
                i64::from(row as u32 * (ch + 8)),
            );
        }
    }
    sheet.save(out).expect("save");
}
