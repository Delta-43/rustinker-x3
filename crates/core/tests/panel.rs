use x3_core::panel::*;

fn frame(p: Pattern) -> std::vec::Vec<[u8; ROW_BYTES]> {
    (0..HEIGHT)
        .map(|y| {
            let mut r = [0; ROW_BYTES];
            p.row(y, &mut r);
            r
        })
        .collect()
}

#[test]
fn geometry() {
    assert_eq!(ROW_BYTES, 99);
    assert_eq!(ROW_BYTES * HEIGHT, 52_272);
}

#[test]
fn solid_patterns() {
    assert!(
        frame(Pattern::White)
            .iter()
            .all(|r| r.iter().all(|&b| b == 0xFF))
    );
    assert!(
        frame(Pattern::Black)
            .iter()
            .all(|r| r.iter().all(|&b| b == 0))
    );
    let mut r = [0; ROW_BYTES];
    Pattern::Black.row(HEIGHT, &mut r);
    assert!(
        r.iter().all(|&b| b == 0xFF),
        "rows past the panel are white"
    );
}

#[test]
fn checker_is_half_black() {
    let black: u32 = frame(Pattern::Checker)
        .iter()
        .flat_map(|r| r.iter())
        .map(|b| b.count_zeros())
        .sum();
    let total = (WIDTH * HEIGHT) as u32;
    assert!(black.abs_diff(total / 2) < total / 50, "{black}");
    assert!(!Pattern::Checker.black(0, 0) && Pattern::Checker.black(33, 0));
}

#[test]
fn corners_marks_raw_origin_msb_first() {
    let mut r = [0; ROW_BYTES];
    Pattern::Corners.row(0, &mut r);
    assert_eq!(&r[..12], &[0; 12]); // 96 px
    assert_eq!(r[12], 0xFF);
    assert_eq!(&r[15..21], &[0; 6]); // 48 px at x 120..168
    Pattern::Corners.row(130, &mut r);
    assert_eq!(&r[..3], &[0; 3]); // 24 px
    assert_eq!(r[3], 0xFF);
    let mut edge = [0; ROW_BYTES];
    Pattern::Checker.row(0, &mut edge);
    assert_eq!(
        edge[4], 0x80,
        "x 32 white, 33..39 black: bit 7 is the first pixel"
    );
}

#[test]
fn blocks_move_and_wrap() {
    assert!(Pattern::Block(0).black(0, 216));
    assert!(!Pattern::Block(0).black(96, 216));
    assert!(Pattern::Block(1).black(96, 311));
    assert!(!Pattern::Block(1).black(96, 312));
    assert_eq!(
        Pattern::Block(8).black(0, 250),
        Pattern::Block(0).black(0, 250)
    );
    assert_eq!(Pattern::parse(b"checker"), Some(Pattern::Checker));
    assert_eq!(Pattern::parse(b"nope"), None);
}
