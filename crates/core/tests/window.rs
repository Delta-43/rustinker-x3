use x3_core::{
    Error,
    font::Font,
    reader::{FRAME_LEN, Frame, Style, next_page, render_page},
    window::{MIN_WINDOW, Source, Window},
};

static BODY: &[u8] = include_bytes!("../../../assets/fonts/literata-regular-24.x3f");
static BOLD: &[u8] = include_bytes!("../../../assets/fonts/literata-bold-28.x3f");

fn style() -> Style<'static> {
    Style {
        body: Font::parse(BODY).unwrap(),
        heading: Font::parse(BOLD).unwrap(),
        margin_x: 20,
        margin_top: 24,
        margin_bottom: 48,
    }
}

/// Text in RAM, read in small, uneven pieces like a card.
struct Mem<'a> {
    text: &'a [u8],
    reads: usize,
}

impl Source for Mem<'_> {
    fn len(&self) -> usize {
        self.text.len()
    }
    fn read(&mut self, at: usize, out: &mut [u8]) -> Result<usize, Error> {
        self.reads += 1;
        if at >= self.text.len() {
            return Ok(0);
        }
        let n = out.len().min(self.text.len() - at).min(1000);
        out[..n].copy_from_slice(&self.text[at..at + n]);
        Ok(n)
    }
}

fn long(paragraphs: usize) -> String {
    let mut s = String::from("# A very long article\n\n");
    for i in 0..paragraphs {
        if i % 25 == 0 {
            s.push_str(&format!("# Część {i}\n\n"));
        }
        s.push_str(&format!(
            "Akapit {i}: zażółć gęślą jaźń. The e-ink screen keeps its image while the \
             device sleeps, and the reader turns pa\u{ad}ges with the but\u{ad}tons on the \
             right bar. „Cytat” – myślnik… i\u{a0}spacja.\n\n"
        ));
    }
    s
}

/// All page starts, paging forward through a window.
fn forward(text: &str, buf: &mut [u8]) -> Vec<usize> {
    let st = style();
    let mut src = Mem {
        text: text.as_bytes(),
        reads: 0,
    };
    let mut w = Window::new(buf, text.len()).unwrap();
    let mut f = [0u8; FRAME_LEN];
    let mut at = 0;
    let mut pages = vec![];
    loop {
        pages.push(at);
        let next = w.render(&mut src, &mut Frame(&mut f), &st, at).unwrap();
        assert!(next > at || next == text.len());
        if next >= text.len() {
            return pages;
        }
        at = next;
    }
}

#[test]
fn a_big_window_pages_exactly_like_the_whole_text() {
    let text = long(80);
    let st = style();
    let mut whole = vec![];
    let mut at = 0;
    while at < text.len() {
        whole.push(at);
        at = next_page(&text, &st, at).unwrap();
    }
    let mut buf = vec![0u8; text.len() + 10];
    assert_eq!(forward(&text, &mut buf), whole);
}

#[test]
fn a_small_window_reaches_the_end_of_a_long_article() {
    let text = long(1200); // ~250 KiB
    assert!(text.len() > 200 * 1024, "{}", text.len());
    let mut buf = vec![0u8; MIN_WINDOW * 2];
    let pages = forward(&text, &mut buf);
    assert!(pages.len() > 100);
    // Every page draws the same text as the whole-text layout from the
    // same start, so nothing is cut at a window edge.
    let st = style();
    let mut a = [0u8; FRAME_LEN];
    let mut b = [0u8; FRAME_LEN];
    let mut src = Mem {
        text: text.as_bytes(),
        reads: 0,
    };
    let mut w = Window::new(&mut buf, text.len()).unwrap();
    for &p in pages.iter().step_by(7) {
        let n1 = w.render(&mut src, &mut Frame(&mut a), &st, p).unwrap();
        let n2 = render_page(&mut Frame(&mut b), &text, &st, p).unwrap();
        assert_eq!(n1, n2, "page at {p}");
        assert!(a == b, "page at {p} differs");
    }
}

#[test]
fn paging_back_steps_back_and_reaches_the_start() {
    let text = long(400);
    let st = style();
    let mut buf = vec![0u8; MIN_WINDOW * 2];
    let mut src = Mem {
        text: text.as_bytes(),
        reads: 0,
    };
    let mut w = Window::new(&mut buf, text.len()).unwrap();
    let mut at = w.settle(&mut src, text.len() - 1).unwrap();
    let mut steps = 0;
    while at > 0 {
        let p = w.previous(&mut src, &st, at).unwrap();
        assert!(p < at, "stuck at {at}");
        // A page back is never more than one page long.
        let mut f = [0u8; FRAME_LEN];
        let next = w.render(&mut src, &mut Frame(&mut f), &st, p).unwrap();
        assert!(next >= at, "page back from {at} ends at {next}: a gap");
        assert!(at - p < 4 * 1024, "jumped {} bytes", at - p);
        at = p;
        steps += 1;
        assert!(steps < 10_000);
    }
    // Forward then back returns to the same page inside one window.
    let n = w
        .render(&mut src, &mut Frame(&mut [0u8; FRAME_LEN]), &st, 0)
        .unwrap();
    assert_eq!(w.previous(&mut src, &st, n).unwrap(), 0);
}

#[test]
fn settle_fixes_bad_saved_positions() {
    let text = long(50);
    let mut buf = vec![0u8; MIN_WINDOW];
    let mut src = Mem {
        text: text.as_bytes(),
        reads: 0,
    };
    let mut w = Window::new(&mut buf, text.len()).unwrap();
    let mid = text.find('ż').unwrap() + 1; // inside a two-byte character
    assert_eq!(w.settle(&mut src, mid).unwrap(), mid - 1);
    assert_eq!(w.settle(&mut src, text.len()).unwrap(), 0);
    assert_eq!(w.settle(&mut src, usize::MAX).unwrap(), 0);
}

#[test]
fn text_without_newlines_and_multibyte_edges_still_pages() {
    let text = "żółw ".repeat(10_000); // 2-byte characters, no paragraph breaks
    let mut buf = vec![0u8; MIN_WINDOW + 1]; // odd size cuts characters
    let pages = forward(&text, &mut buf);
    assert!(pages.len() > 10);
    for p in pages {
        assert!(text.is_char_boundary(p));
    }
}

#[test]
fn hostile_sources_are_rejected() {
    let st = style();
    let mut f = [0u8; FRAME_LEN];
    let mut buf = vec![0u8; MIN_WINDOW];
    assert_eq!(
        Window::new(&mut buf[..100], 10).err(),
        Some(Error::BufferTooSmall)
    );
    assert_eq!(
        Window::new(&mut buf, 300 * 1024).err(),
        Some(Error::TooLarge)
    );

    // Invalid UTF-8 in the middle.
    let mut bad = b"abc def\n".repeat(100);
    bad[50] = 0xFF;
    let mut src = Mem {
        text: &bad,
        reads: 0,
    };
    let mut w = Window::new(&mut buf, bad.len()).unwrap();
    assert_eq!(
        w.render(&mut src, &mut Frame(&mut f), &st, 0).err(),
        Some(Error::Corrupt)
    );

    // Truncated character at the very end of the text.
    let cut = "ab ż".as_bytes();
    let cut = &cut[..cut.len() - 1];
    let mut src = Mem {
        text: cut,
        reads: 0,
    };
    let mut w = Window::new(&mut buf, cut.len()).unwrap();
    assert_eq!(
        w.render(&mut src, &mut Frame(&mut f), &st, 0).err(),
        Some(Error::Corrupt)
    );

    // Control characters.
    let ctl = b"hello\x07world";
    let mut src = Mem {
        text: ctl,
        reads: 0,
    };
    let mut w = Window::new(&mut buf, ctl.len()).unwrap();
    assert_eq!(
        w.render(&mut src, &mut Frame(&mut f), &st, 0).err(),
        Some(Error::Invalid)
    );

    // A source shorter than it claims.
    struct Short;
    impl Source for Short {
        fn len(&self) -> usize {
            5000
        }
        fn read(&mut self, at: usize, out: &mut [u8]) -> Result<usize, Error> {
            if at >= 100 {
                return Ok(0);
            }
            let n = out.len().min(100 - at);
            out[..n].fill(b'a');
            Ok(n)
        }
    }
    let mut w = Window::new(&mut buf, 5000).unwrap();
    assert_eq!(
        w.render(&mut Short, &mut Frame(&mut f), &st, 0).err(),
        Some(Error::Corrupt)
    );

    // Empty text renders a blank page and ends.
    let mut src = Mem {
        text: b"",
        reads: 0,
    };
    let mut w = Window::new(&mut buf, 0).unwrap();
    assert_eq!(w.render(&mut src, &mut Frame(&mut f), &st, 0).unwrap(), 0);
}
