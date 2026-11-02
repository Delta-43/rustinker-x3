use x3_core::{dhcp::SERVER, dns::*};

fn query(name: &str, qtype: u16) -> Vec<u8> {
    let mut p = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
    for label in name.split('.') {
        p.push(label.len() as u8);
        p.extend(label.bytes());
    }
    p.push(0);
    p.extend(qtype.to_be_bytes());
    p.extend([0, 1]);
    p
}

#[test]
fn every_a_question_gets_the_x3() {
    let mut out = [0u8; REPLY_CAP];
    for name in [
        "captive.apple.com",
        "connectivitycheck.gstatic.com",
        "a",
        "x.y.z.example.org",
    ] {
        let q = query(name, 1);
        let n = reply(&q, &mut out).unwrap();
        let r = &out[..n];
        assert_eq!(&r[..2], &[0x12, 0x34], "id echoed");
        assert_eq!(r[2] & 0x80, 0x80, "response bit");
        assert_eq!(r[2] & 0x01, 0x01, "recursion desired echoed");
        assert_eq!(r[3] & 0x0F, 0, "no error");
        assert_eq!(&r[6..8], &[0, 1], "one answer");
        assert_eq!(&r[q.len()..q.len() + 2], &[0xC0, 0x0C]);
        assert_eq!(&r[n - 4..], &SERVER);
        assert_eq!(&r[12..q.len()], &q[12..], "question copied");
    }
}

#[test]
fn other_types_get_an_empty_answer() {
    let mut out = [0u8; REPLY_CAP];
    for t in [28u16, 65, 15, 16] {
        let q = query("example.org", t);
        let n = reply(&q, &mut out).unwrap();
        assert_eq!(n, q.len(), "type {t}");
        assert_eq!(&out[6..8], &[0, 0]);
        assert_eq!(out[3] & 0x0F, 0);
    }
}

#[test]
fn junk_and_odd_packets_get_no_reply() {
    let mut out = [0u8; REPLY_CAP];
    let good = query("example.org", 1);
    assert!(reply(&[], &mut out).is_none());
    assert!(reply(&good[..16], &mut out).is_none());
    assert!(reply(&vec![0u8; REQUEST_CAP + 1], &mut out).is_none());
    for (i, v) in [
        (2, 0x80u8),
        (2, 0x08),
        (5, 2),
        (5, 0),
        (7, 1),
        (9, 1),
        (11, 2),
    ] {
        let mut b = good.clone();
        b[i] = v;
        assert!(reply(&b, &mut out).is_none(), "byte {i}");
    }
    // A compression pointer in the question, a label past the end, trailing bytes.
    let mut b = good.clone();
    b[12] = 0xC0;
    assert!(reply(&b, &mut out).is_none());
    let mut b = good.clone();
    b[12] = 60;
    assert!(reply(&b, &mut out).is_none());
    let mut b = good.clone();
    b.push(0);
    assert!(reply(&b, &mut out).is_none());
}

#[test]
fn random_packets_never_panic() {
    let mut out = [0u8; REPLY_CAP];
    let good = query("captive.apple.com", 1);
    let mut x = 5u32;
    for _ in 0..5000 {
        let mut b = good.clone();
        for _ in 0..3 {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            let i = (x >> 8) as usize % b.len();
            b[i] = (x >> 24) as u8;
        }
        x = x.wrapping_mul(1664525).wrapping_add(1013904223);
        b.truncate((x >> 8) as usize % (b.len() + 1));
        let _ = reply(&b, &mut out);
    }
}

#[test]
fn one_edns_record_is_tolerated_and_ignored() {
    let mut out = [0u8; REPLY_CAP];
    let mut q = query("captive.apple.com", 1);
    q[11] = 1; // one additional record
    q.extend([0, 0, 41, 0x10, 0, 0, 0, 0, 0, 0, 0]); // an OPT record
    let n = reply(&q, &mut out).unwrap();
    assert_eq!(&out[10..12], &[0, 0], "reply carries no additional record");
    assert_eq!(&out[n - 4..n], &x3_core::dhcp::SERVER);
}
