//! Host preview with fixed 10×20 Latin-2 glyphs, including Polish letters.
pub mod disk;
pub mod receive;
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    mono_font::{MonoTextStyle, iso_8859_2::FONT_10X20},
    pixelcolor::BinaryColor,
    prelude::*,
    text::{Baseline, Text},
};
use image::{GrayImage, Luma};
use x3_core::{
    Error,
    bitmap::{HEIGHT, WIDTH},
    text,
};
pub const ARTICLE: &str = "# Czytanie bez telefonu\n\nZażółć gęślą jaźń. Pchnąć w tę łódź jeża lub ośm skrzyń fig.\n\nTo jest przykładowy artykuł zapisany na karcie pamięci. Strony zachowują polskie litery i można je czytać bez połączenia z siecią.\n\n# A quiet afternoon\n\nThe e-ink screen keeps its image while the device sleeps. This sample contains original text and no card art.\n\nA long word: abcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyzabcdefghijklmnopqrstuvwxyz.\n\nSoft hyphen: prze\u{ad}czytany. Unsupported symbols: 😀.";
struct Canvas(GrayImage);
impl OriginDimensions for Canvas {
    fn size(&self) -> Size {
        Size::new(WIDTH as u32, HEIGHT as u32)
    }
}
impl DrawTarget for Canvas {
    type Color = BinaryColor;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(p, c) in pixels {
            if p.x >= 0 && p.y >= 0 && (p.x as usize) < WIDTH && (p.y as usize) < HEIGHT {
                self.0.put_pixel(
                    p.x as u32,
                    p.y as u32,
                    Luma([if c == BinaryColor::On { 0 } else { 255 }]),
                );
            }
        }
        Ok(())
    }
}
/// Characters per line and lines per screen for the 10×20 simulator font.
const COLUMNS: usize = 74;
const ROWS: usize = 21;
/// Start offsets of every screen, in reading order. At most `limit` pages.
pub fn article_pages(text: &str, limit: usize) -> Result<Vec<usize>, Error> {
    let mut pages = vec![0];
    let mut offset = 0;
    while pages.len() < limit {
        let next = text::page(text, COLUMNS, ROWS, offset, |_, _| {})?;
        if next <= offset || text[next..].trim().is_empty() {
            break;
        }
        pages.push(next);
        offset = next;
    }
    Ok(pages)
}
pub fn article(text: &str, offset: usize) -> Result<(GrayImage, usize), Error> {
    let mut canvas = Canvas(GrayImage::from_pixel(
        WIDTH as u32,
        HEIGHT as u32,
        Luma([255]),
    ));
    let next = text::page(text, COLUMNS, ROWS, offset, |row, line| {
        let mut rendered = String::new();
        for c in line.text.chars().filter(|&c| c != '\u{ad}') {
            // The Latin-2 simulator font lacks typographic punctuation; the
            // stored text keeps it. The device font must cover it (Phase 3).
            match c {
                '\t' => rendered.push(' '),
                '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{2032}' => rendered.push('\''),
                '\u{201c}' | '\u{201d}' | '\u{201e}' | '\u{2033}' | '\u{ab}' | '\u{bb}' => {
                    rendered.push('"')
                }
                '\u{2013}' | '\u{2014}' | '\u{2212}' => rendered.push('-'),
                '\u{2026}' => rendered.push_str("..."),
                '\u{a0}' | '\u{2009}' | '\u{202f}' => rendered.push(' '),
                '\u{2022}' => rendered.push('*'),
                _ => rendered.push(c),
            }
        }
        if line.hyphen {
            rendered.push('-');
        }
        let _ = Text::with_baseline(
            &rendered,
            Point::new(24, 24 + row as i32 * 23),
            MonoTextStyle::new(&FONT_10X20, BinaryColor::On),
            Baseline::Top,
        )
        .draw(&mut canvas);
        if line.heading {
            let _ = Text::with_baseline(
                &rendered,
                Point::new(25, 24 + row as i32 * 23),
                MonoTextStyle::new(&FONT_10X20, BinaryColor::On),
                Baseline::Top,
            )
            .draw(&mut canvas);
        }
    })?;
    Ok((canvas.0, next))
}
/// Original geometric test card; no third-party art.
pub fn token(bpp: u8) -> Result<GrayImage, Error> {
    let mut grey = vec![0; WIDTH * HEIGHT];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let inside = (x as i32 - 396).pow(2) + (y as i32 - 264).pow(2) < 180 * 180;
            grey[y * WIDTH + x] = if inside {
                ((x * 255) / (WIDTH - 1)) as u8
            } else if (x / 32 + y / 32) % 2 == 0 {
                235
            } else {
                30
            };
        }
    }
    let mut samples = vec![0; grey.len()];
    x3_core::bitmap::dither(&grey, WIDTH, bpp, &mut samples)?;
    let max = (1u8 << bpp) - 1;
    let image = GrayImage::from_fn(WIDTH as u32, HEIGHT as u32, |x, y| {
        Luma([samples[y as usize * WIDTH + x as usize] * (255 / max)])
    });
    Ok(image)
}

/// Device fonts and style for the Phase 3 reader (ADR 020).
pub static BODY_FONT: &[u8] = include_bytes!("../../../assets/fonts/literata-regular-24.x3f");
pub static HEADING_FONT: &[u8] = include_bytes!("../../../assets/fonts/literata-bold-28.x3f");

pub fn reader_style() -> Result<x3_core::reader::Style<'static>, Error> {
    Ok(x3_core::reader::Style {
        body: x3_core::font::Font::parse(BODY_FONT)?,
        heading: x3_core::font::Font::parse(HEADING_FONT)?,
        margin_x: 20,
        margin_top: 24,
        margin_bottom: 48,
    })
}

/// A sample with the companion's soft hyphens (U+00AD) already inserted.
pub const READER_SAMPLE: &str = "# Czytanie bez telefonu\n\nZażółć gęślą jaźń. Pchnąć w\u{a0}tę łódź jeża lub ośm skrzyń fig. „Cytat w\u{a0}cudzysłowie” – oraz myślnik… i\u{a0}wielokropek.\n\nTo jest przy\u{ad}kła\u{ad}do\u{ad}wy ar\u{ad}ty\u{ad}kuł za\u{ad}pi\u{ad}sa\u{ad}ny na kar\u{ad}cie pa\u{ad}mię\u{ad}ci. Stro\u{ad}ny za\u{ad}cho\u{ad}wu\u{ad}ją pol\u{ad}skie li\u{ad}te\u{ad}ry i\u{a0}moż\u{ad}na je czy\u{ad}tać bez po\u{ad}łą\u{ad}cze\u{ad}nia z\u{a0}sie\u{ad}cią, na\u{ad}wet gdy te\u{ad}le\u{ad}fon le\u{ad}ży w\u{a0}dru\u{ad}gim po\u{ad}ko\u{ad}ju.\n\n# A quiet afternoon\n\nThe e-ink screen keeps its im\u{ad}age while the de\u{ad}vice sleeps. Jus\u{ad}ti\u{ad}fied text fills each line to the right mar\u{ad}gin, and the last line of a para\u{ad}graph stays ragged. This sam\u{ad}ple con\u{ad}tains orig\u{ad}i\u{ad}nal text and no card art.\n\nUn\u{ad}sup\u{ad}port\u{ad}ed sym\u{ad}bols fall back: 😀.\n";

/// One portrait reader page as a 528×792 image, and the next page's offset.
pub fn reader_page(text: &str, offset: usize) -> Result<(GrayImage, usize), Error> {
    use x3_core::reader::{FRAME_LEN, Frame, HEIGHT as H, WIDTH as W, render_page};
    let style = reader_style()?;
    let mut buf = vec![0u8; FRAME_LEN];
    let buf: &mut [u8; FRAME_LEN] = buf.as_mut_slice().try_into().map_err(|_| Error::Invalid)?;
    let mut frame = Frame(buf);
    let next = render_page(&mut frame, text, &style, offset)?;
    let mut img = GrayImage::new(W as u32, H as u32);
    for y in 0..H {
        for x in 0..W {
            img.put_pixel(
                x as u32,
                y as u32,
                Luma([if frame.is_ink(x, y) { 0 } else { 255 }]),
            );
        }
    }
    Ok((img, next))
}

/// Every device GUI screen (Phase 6B) as a 528×792 image, with sample data.
pub fn gui_screens() -> Result<Vec<(&'static str, GrayImage)>, Error> {
    use x3_core::gui::{self, Keys, Status, Step};
    use x3_core::reader::{FRAME_LEN, Frame, HEIGHT as H, WIDTH as W};
    let style = reader_style()?;
    let st = Status {
        clock: "14:07",
        battery: 82,
        wifi: true,
    };
    let mut out = Vec::new();
    let mut shoot = |name: &'static str, draw: &dyn Fn(&mut Frame<'_>)| {
        let mut buf = vec![0u8; FRAME_LEN];
        let buf: &mut [u8; FRAME_LEN] = buf.as_mut_slice().try_into().expect("frame length");
        let mut frame = Frame(buf);
        draw(&mut frame);
        let mut img = GrayImage::new(W as u32, H as u32);
        for y in 0..H {
            for x in 0..W {
                let v = if frame.is_ink(x, y) { 0 } else { 255 };
                img.put_pixel(x as u32, y as u32, Luma([v]));
            }
        }
        out.push((name, img));
    };
    // Sample data; the wording and layout come from x3_core::screens, the
    // same code the firmware draws with (audit F1).
    use x3_core::library::Progress;
    use x3_core::rtc::DateTime;
    use x3_core::screens::{
        self, AboutInfo, DeviceInfo, HomeHero, HomeInfo, ReceiveDone, WifiSummary,
    };
    let home = HomeInfo {
        card: true,
        articles: 5,
        pictures: 6,
        last_receive: Some(DateTime {
            year: 2026,
            month: 10,
            day: 31,
            hour: 14,
            minute: 2,
            second: 0,
        }),
        today: Some((2026, 10, 31)),
        battery: 82,
        hero: Some(HomeHero {
            title: "AGENTS.md: how agents work on this repo",
            percent: 57,
        }),
    };
    shoot("gui-home", &|f| screens::home(f, &style, &st, &home, 0));
    shoot("gui-home-tile", &|f| {
        screens::home(f, &style, &st, &home, 2)
    });
    let empty = HomeInfo {
        card: false,
        hero: None,
        last_receive: None,
        ..home
    };
    let low = Status {
        wifi: false,
        battery: 12,
        ..st
    };
    shoot("gui-home-empty", &|f| {
        screens::home(f, &style, &low, &empty, 3)
    });
    let library: [(&str, Progress); 10] = [
        ("Czytanie bez telefonu", Progress::New),
        (
            "AGENTS.md: how agents work on this repo",
            Progress::Percent(57),
        ),
        (
            "A very long article title that must be cut with an ellipsis",
            Progress::New,
        ),
        ("Zażółć gęślą jaźń", Progress::Read),
        ("Short", Progress::New),
        ("Another article", Progress::Percent(12)),
        ("One more", Progress::New),
        ("And one more", Progress::Read),
        ("Ninth", Progress::New),
        ("Tenth", Progress::New),
    ];
    let titles: Vec<&str> = library.iter().map(|(t, _)| *t).collect();
    let progress: Vec<Progress> = library.iter().map(|(_, p)| *p).collect();
    shoot("gui-library", &|f| {
        screens::library(f, &style, &st, &titles, &progress, 1);
    });
    shoot("gui-library-page2", &|f| {
        screens::library(f, &style, &st, &titles, &progress, 9);
    });
    shoot("gui-library-empty", &|f| {
        screens::library(f, &style, &st, &[], &[], 0);
    });
    let ids = [
        "t-treasure-0123456789ab",
        "t-goblin-warrior-0123456789ab",
        "t-elspeths-soldier-0123456789ab",
        "i-harbour-at-dusk-0123456789ab",
        "t-0123456789abcdef0123456789abcdef",
    ];
    shoot("gui-tokens", &|f| {
        screens::pictures(f, &style, &st, &ids, 1)
    });
    let steps = [
        Step {
            label: "Wi-Fi: Home network",
            state: "joined",
        },
        Step {
            label: "Treasure",
            state: "received",
        },
        Step {
            label: "Goblin",
            state: "received",
        },
        Step {
            label: "Czytanie bez telefonu",
            state: "received",
        },
        Step {
            label: "Old article",
            state: "deleted",
        },
        Step {
            label: "Soldier",
            state: "downloading",
        },
    ];
    shoot("gui-receive", &|f| {
        gui::receive(f, &style, &st, "Receive", 3, 5, &steps, Keys::NONE);
    });
    let device = DeviceInfo {
        wifi: WifiSummary::Chosen("My iPhone"),
        articles: 3,
        pictures: 20,
        firmware: "0.1.0",
        battery: 82,
    };
    shoot("gui-device", &|f| {
        screens::device(f, &style, &st, &device, 0)
    });
    let nets = ["Home network", "My iPhone"];
    shoot("gui-wifi", &|f| {
        screens::wifi(f, &style, &st, &nets, Some(1), 2);
    });
    let about = AboutInfo {
        firmware: "0.1.0",
        battery: Some(82),
        millivolts: Some(4120),
        clock: Some(DateTime {
            year: 2026,
            month: 10,
            day: 31,
            hour: 14,
            minute: 7,
            second: 0,
        }),
        articles: 3,
        pictures: 20,
        stack_free: 59 * 1024,
    };
    shoot("gui-about", &|f| screens::about(f, &style, &st, &about));
    shoot("gui-storage", &|f| screens::storage(f, &style, &st, 3, 20));
    shoot("gui-delete-everything", &|f| {
        screens::delete_everything(f, &style, &st, 3, 20);
    });
    let done = ReceiveDone {
        new: 1,
        deleted: 0,
        rejected: 1,
        made_room: 1,
        network: "My iPhone",
        route: "Internet (signed)",
        took: "3.9 s",
        charge_used: Some("0.071 mAh"),
        battery: "100% (4307 mV)",
    };
    shoot("gui-receive-done", &|f| {
        screens::receive_done(f, &style, &st, &done);
    });
    shoot("gui-receive-failed", &|f| {
        screens::receive_failed(
            f,
            &style,
            &st,
            "Cannot reach the companion.",
            "Home network",
        );
    });
    shoot("gui-crosspoint", &|f| {
        screens::confirm_crosspoint(f, &style, &st)
    });
    shoot("gui-no-card", &|f| screens::no_card(f, &style, &st));
    shoot("gui-sleep-set", &|f| {
        screens::sleep_picture_set(f, &style, &st, true)
    });
    shoot("gui-wifi-setup", &|f| {
        gui::wifi_setup(
            f,
            &style,
            &st,
            "X3-setup-4k7q",
            "m3x9tz4hpc",
            "WIFI:T:WPA;S:X3-setup-4k7q;P:m3x9tz4hpc;;",
            Keys::BACK,
        );
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn golden(name: &str, actual: GrayImage) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(name);
        let expected = image::open(path).unwrap().to_luma8();
        assert_eq!(actual, expected);
    }
    /// The review PNGs in samples/gui are what the code draws now. If this
    /// fails, run `cargo run -p sim -- gui` and look at the changes.
    #[test]
    fn gui_samples_up_to_date() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples/gui");
        let screens = gui_screens().unwrap();
        for (name, actual) in &screens {
            let path = dir.join(format!("{name}.png"));
            let expected = image::open(&path)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
                .to_luma8();
            assert!(actual == &expected, "{name}.png is out of date");
        }
        let on_disk = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(on_disk, screens.len(), "stale PNGs in samples/gui");
    }
    #[test]
    fn token_golden() {
        golden("token-1.png", token(1).unwrap());
        golden("token-2.png", token(2).unwrap());
    }
    #[test]
    fn article_golden() {
        let (frame, next) = article(ARTICLE, 0).unwrap();
        assert!(next > 0);
        golden("article.png", frame);
    }
    #[test]
    fn reader_golden() {
        // Approved on the panel by the owner, 2026-10-27 (24/28 px, ADR 020).
        let (frame, next) = reader_page(READER_SAMPLE, 0).unwrap();
        assert_eq!(next, READER_SAMPLE.len(), "the sample fits one page");
        golden("reader-1.png", frame);
    }
    #[test]
    fn polish_glyphs_are_distinct() {
        let (a, _) = article("ą ć ę ł ń ó ś ź ż", 0).unwrap();
        let (b, _) = article("a c e l n o s z z", 0).unwrap();
        assert_ne!(a, b);
    }
    #[test]
    fn second_page_golden() {
        let long = [ARTICLE; 3].join("\n\n");
        let (_, next) = article(&long, 0).unwrap();
        assert!(next > 0 && next < long.len());
        let (frame, end) = article(&long, next).unwrap();
        assert!(end > next);
        golden("article-page2.png", frame);
    }
}
