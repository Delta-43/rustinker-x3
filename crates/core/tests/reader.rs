use x3_core::{
    Error,
    font::{Font, GLYPH_LEN, HEADER_LEN},
    reader::*,
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

const POLISH: &str = "# Czytanie bez telefonu\n\nZażółć gęślą jaźń. Pchnąć w tę łódź jeża lub ośm skrzyń fig. „Cytat” – myślnik… i\u{a0}spacja nierozdzielająca.\n\nTo jest przykładowy artykuł zapisany na karcie pamięci. Strony za\u{ad}cho\u{ad}wu\u{ad}ją polskie litery i można je czytać bez połączenia z siecią, nawet gdy telefon leży w drugim pokoju.\n";

fn long_text() -> std::string::String {
    let mut s = std::string::String::from("# A long article\n\n");
    for i in 0..60 {
        s.push_str(&std::format!(
            "Paragraph {i}: the e-ink screen keeps its image while the device sleeps, \
             and the reader turns pages with the buttons on the right bar.\n\n"
        ));
    }
    s
}

#[test]
fn fonts_parse_and_cover_polish_and_punctuation() {
    for f in [BODY, BOLD] {
        let font = Font::parse(f).unwrap();
        assert!(font.len() > 300, "{}", font.len());
        for c in "ąćęłńóśźżĄĆĘŁŃÓŚŹŻ’‘“”„–—…€\u{a0}".chars() {
            assert!(font.glyph(c).is_some(), "{c:?} missing");
        }
        assert!(font.glyph('😀').is_none());
        assert!(font.advance('😀') > 0, "falls back to a visible glyph");
    }
}

#[test]
fn hostile_font_files_are_rejected() {
    assert_eq!(Font::parse(b"").err(), Some(Error::Invalid));
    assert_eq!(Font::parse(&BODY[..10]).err(), Some(Error::Invalid));
    assert_eq!(
        Font::parse(&BODY[..HEADER_LEN + 5]).err(),
        Some(Error::Corrupt)
    );
    let mut big = std::vec![0u8; 300 * 1024];
    big[..4].copy_from_slice(b"X3F1");
    assert_eq!(Font::parse(&big).err(), Some(Error::TooLarge));
    let mut bad = BODY.to_vec();
    bad[0] = b'Y';
    assert!(Font::parse(&bad).is_err());
    // Truncated bitmaps: the last glyph's data runs off the end.
    assert_eq!(
        Font::parse(&BODY[..BODY.len() - 1]).err(),
        Some(Error::Corrupt)
    );
    // Glyph table out of order.
    let mut swapped = BODY.to_vec();
    let (a, b) = (HEADER_LEN, HEADER_LEN + GLYPH_LEN);
    let first: std::vec::Vec<u8> = swapped[a..b].to_vec();
    swapped.copy_within(b..b + GLYPH_LEN, a);
    swapped[b..b + GLYPH_LEN].copy_from_slice(&first);
    assert_eq!(Font::parse(&swapped).err(), Some(Error::Corrupt));
    // Bitmap offset far past the end.
    let mut off = BODY.to_vec();
    off[HEADER_LEN + 12..HEADER_LEN + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(Font::parse(&off).err(), Some(Error::Corrupt));
    // Non-zero reserved header field and impossible metrics.
    let mut r = BODY.to_vec();
    r[14] = 1;
    assert!(Font::parse(&r).is_err());
    let mut d = BODY.to_vec();
    d[8..10].copy_from_slice(&5i16.to_le_bytes());
    assert!(Font::parse(&d).is_err());
}

fn lines(text: &str) -> std::vec::Vec<Line> {
    let s = style();
    let mut out = std::vec::Vec::new();
    let mut at = 0;
    while at < text.len() {
        let l = line(text, &s, at);
        assert!(l.next > at, "no progress at {at}");
        out.push(l);
        at = l.next;
    }
    out
}

#[test]
fn lines_fit_and_justified_lines_fill_the_width() {
    let s = style();
    let max = (WIDTH - 2 * s.margin_x as usize) as i32;
    let text = long_text();
    for l in lines(&text).iter().chain(lines(POLISH).iter()) {
        assert!(l.width <= max, "{l:?}");
    }
    // Justification only on non-final body lines: render and check that the
    // rightmost ink of such a line reaches close to the right margin.
    let mut buf = std::boxed::Box::new([0u8; FRAME_LEN]);
    let mut frame = Frame(&mut buf);
    render_page(&mut frame, &text, &s, 0).unwrap();
    let right = WIDTH - s.margin_x as usize;
    let mut full = 0;
    for y in 0..HEIGHT {
        if (right - 8..right).any(|x| frame.is_ink(x, y)) {
            full += 1;
        }
        assert!(
            (right + 4..WIDTH).all(|x| !frame.is_ink(x, y)),
            "ink past margin at y {y}"
        );
    }
    assert!(
        full > 50,
        "justified lines should reach the margin ({full} rows)"
    );
}

#[test]
fn soft_hyphens_break_and_show_a_hyphen() {
    // Many long hyphenated words: some line edge must fall inside one.
    let text = "nie\u{ad}za\u{ad}po\u{ad}mi\u{ad}na\u{ad}ją\u{ad}cy ".repeat(30);
    let ls = lines(&text);
    assert!(ls.iter().any(|l| l.hyphen), "{ls:?}");
    for l in ls.iter().filter(|l| l.hyphen) {
        assert!(text[..l.next].ends_with('\u{ad}'));
        assert!(!text[l.content..l.end].ends_with(' '));
    }
    // Without soft hyphens the same words never end a line in a hyphen.
    let plain = text.replace('\u{ad}', "");
    assert!(lines(&plain).iter().all(|l| !l.hyphen));
}

#[test]
fn nbsp_never_breaks() {
    for l in lines(POLISH) {
        assert!(!POLISH[l.content..l.end].ends_with('\u{a0}'));
        assert!(!POLISH[l.next..].starts_with('\u{a0}') || l.next == l.end);
    }
}

#[test]
fn headings_and_paragraph_ends() {
    let ls = lines(POLISH);
    assert!(ls[0].heading && ls[0].last);
    assert_eq!(&POLISH[ls[0].content..ls[0].end], "Czytanie bez telefonu");
    // The blank line between paragraphs is an empty final line.
    assert_eq!(ls[1].content, ls[1].end);
    assert!(
        ls.iter()
            .filter(|l| !l.heading)
            .all(|l| !l.last || l.end == l.next - 1 || l.next == POLISH.len())
    );
}

#[test]
fn a_long_heading_keeps_its_font_on_every_line() {
    let text = "# What crying means across species and why the answer depends on who asks\n\nBody.";
    let ls = lines(text);
    let n = ls
        .iter()
        .take_while(|l| l.end <= text.find('\n').unwrap())
        .count();
    assert!(n >= 2, "heading should wrap: {n}");
    assert!(ls[..n].iter().all(|l| l.heading));
    assert!(ls[..n - 1].iter().all(|l| !l.last) && ls[n - 1].last);
    assert_eq!(
        ls[1].content, ls[0].next,
        "no \"# \" skip after the first line"
    );
    assert!(!ls.last().unwrap().heading);
}

#[test]
fn a_word_wider_than_the_line_is_split() {
    let word = "x".repeat(200);
    let ls = lines(&word);
    assert!(ls.len() > 1);
    assert_eq!(ls.iter().map(|l| l.end - l.content).sum::<usize>(), 200);
}

#[test]
fn pages_tile_the_text_and_previous_page_inverts_next() {
    let s = style();
    let text = long_text();
    let mut starts = std::vec![0];
    loop {
        let next = next_page(&text, &s, *starts.last().unwrap()).unwrap();
        if next >= text.len() {
            break;
        }
        assert!(next > *starts.last().unwrap());
        starts.push(next);
    }
    assert!(starts.len() > 5, "{} pages", starts.len());
    for w in starts.windows(2) {
        assert_eq!(previous_page(&text, &s, w[1]).unwrap(), w[0]);
    }
    assert_eq!(previous_page(&text, &s, 0).unwrap(), 0);
    // Rendering returns the same positions as layout alone.
    let mut buf = std::boxed::Box::new([0u8; FRAME_LEN]);
    let mut frame = Frame(&mut buf);
    for &p in &starts {
        assert_eq!(
            render_page(&mut frame, &text, &s, p).unwrap(),
            next_page(&text, &s, p).unwrap()
        );
    }
}

#[test]
fn bad_offsets_and_text_are_rejected() {
    let s = style();
    assert_eq!(next_page(POLISH, &s, POLISH.len() + 1), Err(Error::Invalid));
    let inside = POLISH
        .char_indices()
        .find(|(_, c)| c.len_utf8() > 1)
        .unwrap()
        .0
        + 1;
    assert_eq!(next_page(POLISH, &s, inside), Err(Error::Invalid));
    assert_eq!(next_page("", &s, 0), Ok(0));
    assert_eq!(validate("ok\u{7}"), Err(Error::Invalid));
    assert_eq!(validate(POLISH), Ok(()));
}

#[test]
fn raw_rows_transpose_the_portrait_frame() {
    let mut buf = std::boxed::Box::new([0xFFu8; FRAME_LEN]);
    let mut frame = Frame(&mut buf);
    frame.ink(0, 0); // portrait top-left
    frame.ink(3, 100); // column 3, row 100
    let mut row = [0u8; x3_core::panel::ROW_BYTES];
    frame.raw_row(0, &mut row);
    assert_eq!(row[0], 0x7F, "raw row 0, raw x 0");
    frame.raw_row(3, &mut row);
    assert_eq!(row[100 / 8], !(0x80 >> (100 % 8)));
    frame.raw_row(600, &mut row);
    assert!(row.iter().all(|&b| b == 0xFF));
}
