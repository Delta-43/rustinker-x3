//! Writes 1-bit and four-level encodings of each image as PNG (level × 85),
//! for judging a possible 2-bit token on the panel.
//! Usage: cargo run -p x3-tools --example grey_compare -- OUT_DIR NAME=IMAGE...
use image::{GrayImage, Luma};
use x3_tools::{Fit, Orientation, Quality, grey4, token_preview, token_with};
/// Even steps: what the four levels are called (0, 85, 170, 255).
const EVEN: [f32; 4] = [0.0, 1.0 / 3.0, 2.0 / 3.0, 1.0];
/// CrossPoint's panel model for the X3 (levels look like 15, 35, 90 and 210),
/// stretched so black is 0.0 and white is 1.0.
const PANEL: [f32; 4] = [0.0, 20.0 / 195.0, 75.0 / 195.0, 1.0];
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (dir, inputs) = args.split_first().expect("OUT_DIR NAME=IMAGE...");
    for input in inputs {
        let (name, path) = input.split_once('=').expect("NAME=IMAGE");
        let bytes = std::fs::read(path).expect("read");
        let (fit, orientation, q) = (Fit::Contain, Orientation::Portrait, Quality::TOKEN);
        let one = token_preview(&token_with(&bytes, fit, orientation, q).expect("encode"))
            .expect("preview");
        one.save(format!("{dir}/{name}-1bit.png")).expect("save");
        for (tag, tones) in [("even", EVEN), ("panel", PANEL)] {
            let levels = grey4(&bytes, fit, orientation, q, tones).expect("encode");
            let shown = GrayImage::from_fn(levels.width(), levels.height(), |x, y| {
                Luma([levels.get_pixel(x, y)[0] * 85])
            });
            shown
                .save(format!("{dir}/{name}-grey-{tag}.png"))
                .expect("save");
        }
    }
}
