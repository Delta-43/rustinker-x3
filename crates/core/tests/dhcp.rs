use x3_core::dhcp::*;

fn request(kind: u8, xid: [u8; 4]) -> Vec<u8> {
    let mut p = vec![0u8; 240];
    p[0] = 1;
    p[1] = 1;
    p[2] = 6;
    p[4..8].copy_from_slice(&xid);
    p[28..34].copy_from_slice(&[1, 2, 3, 4, 5, 6]);
    p[236..240].copy_from_slice(&[99, 130, 83, 99]);
    p.extend([55, 3, 1, 3, 6]); // parameter list first, then the type
    p.extend([53, 1, kind, 255]);
    p
}

fn option(reply: &[u8], code: u8) -> Option<Vec<u8>> {
    let mut o = &reply[240..];
    while let [c, rest @ ..] = o {
        if *c == 255 {
            return None;
        }
        let len = rest[0] as usize;
        if *c == code {
            return Some(rest[1..1 + len].to_vec());
        }
        o = &rest[1 + len..];
    }
    None
}

#[test]
fn discover_gets_an_offer_and_request_gets_an_ack() {
    let mut out = [0u8; REPLY_CAP];
    let n = reply(&request(1, [9, 8, 7, 6]), &mut out).unwrap();
    let r = &out[..n];
    assert_eq!(r[0], 2);
    assert_eq!(&r[4..8], &[9, 8, 7, 6]);
    assert_eq!(&r[16..20], &CLIENT);
    assert_eq!(&r[28..34], &[1, 2, 3, 4, 5, 6]);
    assert_eq!(option(r, 53).unwrap(), [2]);
    assert_eq!(option(r, 54).unwrap(), SERVER);
    assert_eq!(option(r, 1).unwrap(), [255, 255, 255, 0]);
    assert_eq!(option(r, 3).unwrap(), SERVER);
    assert_eq!(option(r, 6).unwrap(), SERVER, "DNS server option");
    assert_eq!(option(r, 51).unwrap(), [0, 0, 14, 16]);
    let n = reply(&request(3, [1, 1, 1, 1]), &mut out).unwrap();
    assert_eq!(option(&out[..n], 53).unwrap(), [5]);
}

#[test]
fn other_messages_and_junk_get_no_reply() {
    let mut out = [0u8; REPLY_CAP];
    for kind in [0u8, 2, 4, 5, 6, 7, 8, 255] {
        assert!(reply(&request(kind, [0; 4]), &mut out).is_none(), "{kind}");
    }
    let good = request(1, [0; 4]);
    assert!(reply(&good[..239], &mut out).is_none());
    assert!(reply(&[], &mut out).is_none());
    for (i, v) in [(0, 2u8), (1, 2), (2, 5), (236, 0)] {
        let mut b = good.clone();
        b[i] = v;
        assert!(reply(&b, &mut out).is_none(), "byte {i}");
    }
    // An option whose length runs past the end, and one with no type.
    let mut b = good[..240].to_vec();
    b.extend([53, 9, 1]);
    assert!(reply(&b, &mut out).is_none());
    let mut b = good[..240].to_vec();
    b.extend([12, 3, b'a', b'b', b'c', 255]);
    assert!(reply(&b, &mut out).is_none());
}

#[test]
fn random_packets_never_panic() {
    let mut out = [0u8; REPLY_CAP];
    let good = request(1, [5; 4]);
    let mut x = 77u32;
    for _ in 0..3000 {
        let mut b = good.clone();
        for _ in 0..4 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            let i = (x >> 8) as usize % b.len();
            b[i] = (x >> 24) as u8;
        }
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        b.truncate((x >> 8) as usize % (b.len() + 1));
        let _ = reply(&b, &mut out);
    }
}
