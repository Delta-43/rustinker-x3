use x3_core::{
    Error, bitmap,
    reader::{FRAME_LEN, Frame, HEIGHT, WIDTH},
    token::{BASE, Decoder, HEADER_LEN, HIGH, LOW, MONO},
};
use x3_proto::payload::{TokenHeader, crc32};

/// A token whose level at (x, y) is `f(x, y)`.
fn token(width: u16, height: u16, bpp: u8, f: impl Fn(usize, usize) -> u8) -> Vec<u8> {
    let (w, h) = (usize::from(width), usize::from(height));
    let samples: Vec<u8> = (0..w * h).map(|i| f(i % w, i / w)).collect();
    let mut out = vec![0; HEADER_LEN + w * h * usize::from(bpp) / 8];
    bitmap::pack(&samples, bpp, &mut out[HEADER_LEN..]).unwrap();
    let crc = crc32(&out[HEADER_LEN..]);
    TokenHeader {
        bpp,
        width,
        height,
        crc,
    }
    .encode(&mut out)
    .unwrap();
    out
}

fn decode(payload: &[u8], mask: u8, chunk: usize) -> Box<[u8; FRAME_LEN]> {
    let mut buf = Box::new([0u8; FRAME_LEN]);
    let mut frame = Frame(&mut buf);
    frame.clear();
    let mut d = Decoder::new(&payload[..HEADER_LEN], mask).unwrap();
    for c in payload[HEADER_LEN..].chunks(chunk) {
        d.feed(&mut frame, c).unwrap();
    }
    assert!(d.done());
    buf
}

fn ink(buf: &[u8; FRAME_LEN], x: usize, y: usize) -> bool {
    buf[y * (WIDTH / 8) + x / 8] & (0x80 >> (x % 8)) == 0
}

#[test]
fn portrait_two_bpp_planes() {
    // Four vertical bands: black, dark, light, white.
    let p = token(528, 792, 2, |x, _| (x * 4 / 528) as u8);
    for (mask, inked) in [
        (MONO, [true, true, false, false]),
        (BASE, [true, true, true, false]),
        (LOW, [false, true, false, false]),
        (HIGH, [false, true, true, false]),
    ] {
        let f = decode(&p, mask, 97);
        for (band, &want) in inked.iter().enumerate() {
            let x = band * 132 + 60;
            assert_eq!(ink(&f, x, 400), want, "mask {mask:04b} band {band}");
        }
    }
}

#[test]
fn landscape_turns_a_quarter_clockwise() {
    // A black square at the landscape image's top-left corner.
    let p = token(792, 528, 1, |x, y| u8::from(!(x < 40 && y < 40)));
    let f = decode(&p, MONO, 1000);
    // Top edge → right edge: the corner lands top-right in portrait.
    assert!(ink(&f, WIDTH - 1, 0) && ink(&f, WIDTH - 40, 39));
    assert!(!ink(&f, 0, 0) && !ink(&f, WIDTH - 41, 0) && !ink(&f, WIDTH - 1, 40));
    let inked = (0..HEIGHT)
        .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
        .filter(|&(x, y)| ink(&f, x, y))
        .count();
    assert_eq!(inked, 40 * 40);
}

#[test]
fn chunking_does_not_matter() {
    let p = token(792, 528, 2, |x, y| ((x * 7 + y * 3) % 4) as u8);
    let whole = decode(&p, HIGH, usize::MAX);
    for chunk in [1, 3, 512, 4099] {
        assert_eq!(decode(&p, HIGH, chunk), whole, "chunk {chunk}");
    }
}

#[test]
fn hostile_input() {
    let p = token(528, 792, 1, |_, _| 1);
    assert_eq!(Decoder::new(&p[..10], MONO).err(), Some(Error::Corrupt));
    let mut bad = p.clone();
    bad[0] = b'Y';
    assert_eq!(
        Decoder::new(&bad[..HEADER_LEN], MONO).err(),
        Some(Error::Corrupt)
    );
    bad = p.clone();
    bad[6..8].copy_from_slice(&600u16.to_le_bytes());
    assert_eq!(
        Decoder::new(&bad[..HEADER_LEN], MONO).err(),
        Some(Error::Corrupt)
    );
    assert_eq!(
        Decoder::new(&p[..HEADER_LEN], 0b1_0000).err(),
        Some(Error::Invalid)
    );
    // Feeding past the end is refused.
    let mut buf = Box::new([0u8; FRAME_LEN]);
    let mut frame = Frame(&mut buf);
    let mut d = Decoder::new(&p[..HEADER_LEN], MONO).unwrap();
    d.feed(&mut frame, &p[HEADER_LEN..]).unwrap();
    assert_eq!(d.feed(&mut frame, &[0]), Err(Error::TooLarge));
    let mut d = Decoder::new(&p[..HEADER_LEN], MONO).unwrap();
    assert_eq!(d.feed(&mut frame, &vec![0; p.len()]), Err(Error::TooLarge));
}
