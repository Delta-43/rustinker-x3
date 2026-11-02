//! The Wi-Fi setup page logic (ADR 025): hostile requests, forms and output.
use x3_core::setup::*;

fn post(body: &str) -> Vec<u8> {
    format!(
        "POST /save HTTP/1.1\r\nHost: 192.168.4.1\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

#[test]
fn a_get_and_a_post_parse() {
    let get = b"GET / HTTP/1.1\r\nHost: x\r\nAccept: */*\r\n\r\n";
    assert_eq!(
        parse_request(get),
        Parse::Request {
            method: Method::Get,
            path: "/",
            body: b""
        }
    );
    let p = post("ssid=a&psk=bbbbbbbb");
    match parse_request(&p) {
        Parse::Request {
            method: Method::Post,
            path: "/save",
            body,
        } => {
            assert_eq!(body, b"ssid=a&psk=bbbbbbbb")
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_request_is_incomplete_until_all_of_it_has_arrived() {
    let p = post("ssid=a&psk=bbbbbbbb");
    for cut in 0..p.len() {
        assert_eq!(parse_request(&p[..cut]), Parse::Incomplete, "cut {cut}");
    }
    assert!(matches!(parse_request(&p), Parse::Request { .. }));
}

#[test]
fn hostile_requests_are_refused() {
    let mut bad: Vec<Vec<u8>> = vec![
        b"GET / HTTP/1.0\r\n\r\n".to_vec(),
        b"DELETE / HTTP/1.1\r\n\r\n".to_vec(),
        b"GET  / HTTP/1.1\r\n\r\n".to_vec(),
        b"GET / HTTP/1.1 extra\r\n\r\n".to_vec(),
        b"GET x HTTP/1.1\r\n\r\n".to_vec(),
        b"GET /a b HTTP/1.1\r\n\r\n".to_vec(),
        b"GET / HTTP/1.1\r\nNoColon\r\n\r\n".to_vec(),
        b"GET / HTTP/1.1\r\n\xff\r\n\r\n".to_vec(),
        // Body on a GET.
        b"GET / HTTP/1.1\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
        // POST without a length, chunked, double length, signed, huge.
        b"POST /save HTTP/1.1\r\n\r\n".to_vec(),
        b"POST /save HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
        b"POST /save HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\na".to_vec(),
        b"POST /save HTTP/1.1\r\nContent-Length: +1\r\n\r\na".to_vec(),
        b"POST /save HTTP/1.1\r\nContent-Length: 257\r\n\r\n".to_vec(),
        b"POST /save HTTP/1.1\r\nContent-Length: 99999999999999999999\r\n\r\n".to_vec(),
        // More bytes than the length said (a pipelined second request).
        b"POST /save HTTP/1.1\r\nContent-Length: 1\r\n\r\nabGET".to_vec(),
    ];
    // Head with no end that has already passed the cap.
    bad.push(vec![b'a'; HEAD_CAP]);
    // Head that ends after the cap.
    let mut long = b"GET / HTTP/1.1\r\nX: ".to_vec();
    long.extend(vec![b'a'; HEAD_CAP]);
    long.extend(b"\r\n\r\n");
    bad.push(long);
    // Path over 64 bytes.
    bad.push(format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(64)).into_bytes());
    for b in &bad {
        assert_eq!(
            parse_request(b),
            Parse::Invalid,
            "{:?}",
            String::from_utf8_lossy(b)
        );
    }
}

#[test]
fn arbitrary_bytes_never_panic() {
    let mut x = 0x1234_5678u32;
    for len in 0..300 {
        let mut b = Vec::new();
        for _ in 0..len {
            x = x.wrapping_mul(1664525).wrapping_add(1013904223);
            b.push((x >> 24) as u8);
        }
        let _ = parse_request(&b);
        let _ = form_credentials(&b);
    }
    // Mutations of a good request.
    let good = post("ssid=a&psk=bbbbbbbb");
    for i in 0..good.len() {
        for v in [0u8, b'\r', b'\n', b'&', b'%', 0xff] {
            let mut m = good.clone();
            m[i] = v;
            let _ = parse_request(&m);
        }
    }
}

#[test]
fn the_form_decodes_names_and_passwords() {
    let c = form_credentials(b"ssid=My+Caf%C3%A9&psk=p%40ss%26word%3D1").unwrap();
    assert_eq!(c.ssid(), "My Café");
    assert_eq!(c.psk(), "p@ss&word=1");
    // Either order.
    let c = form_credentials(b"psk=12345678&ssid=x").unwrap();
    assert_eq!((c.ssid(), c.psk()), ("x", "12345678"));
}

#[test]
fn a_bad_form_says_why() {
    use FormError::*;
    let cases: [(&[u8], FormError); 14] = [
        (b"", Malformed),
        (b"ssid=a", Malformed),
        (b"psk=12345678", Malformed),
        (b"ssid=a&psk=12345678&x=1", Malformed),
        (b"ssid=a&ssid=b&psk=12345678", Malformed),
        (b"ssid=a&psk=12345678&psk=12345678", Malformed),
        (b"ssid&psk=12345678", Malformed),
        (b"ssid=a&psk=1234567", Password),
        (b"ssid=&psk=12345678", Name),
        (b"ssid=a%0Ab&psk=12345678", Malformed),
        (b"ssid=a&psk=1234%00678", Malformed),
        (b"ssid=%ff&psk=12345678", Malformed),
        (b"ssid=%zz&psk=12345678", Name),
        (b"ssid=a&psk=%4", Password),
    ];
    for (body, want) in cases {
        assert_eq!(
            form_credentials(body).err(),
            Some(want),
            "{:?}",
            String::from_utf8_lossy(body)
        );
    }
    // 33-byte name, 64-byte password, and very long values.
    let long_name = format!("ssid={}&psk=12345678", "n".repeat(33));
    assert_eq!(form_credentials(long_name.as_bytes()).err(), Some(Name));
    let long_psk = format!("ssid=a&psk={}", "p".repeat(64));
    assert_eq!(form_credentials(long_psk.as_bytes()).err(), Some(Password));
    let huge = format!("ssid=a&psk={}", "p".repeat(200));
    assert_eq!(form_credentials(huge.as_bytes()).err(), Some(Password));
    // The longest legal pair is accepted.
    let max = format!("ssid={}&psk={}", "n".repeat(32), "p".repeat(63));
    assert!(form_credentials(max.as_bytes()).is_ok());
}

#[test]
fn a_multibyte_name_counts_bytes() {
    // 11 x 3-byte characters = 33 bytes: too long for an SSID.
    let body = format!("ssid={}&psk=12345678", "%E2%82%AC".repeat(11));
    assert_eq!(
        form_credentials(body.as_bytes()).err(),
        Some(FormError::Name)
    );
    let body = format!("ssid={}&psk=12345678", "%E2%82%AC".repeat(10));
    assert!(form_credentials(body.as_bytes()).is_ok());
}

#[test]
fn the_page_escapes_every_name_and_stays_small() {
    let mut html = String::new();
    page(
        &mut html,
        ["<script>alert(1)</script>", "a\"b'c&d", "ctl\nname"],
        Notice::None,
    )
    .unwrap();
    assert!(!html.contains("<script>alert"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("a&quot;b&#39;c&amp;d"));
    assert!(!html.contains("ctl\nname"));
    let mut many = String::new();
    let names: Vec<String> = (0..40).map(|n| format!("{n:0>32}")).collect();
    page(
        &mut many,
        names.iter().map(String::as_str),
        Notice::Password,
    )
    .unwrap();
    assert_eq!(many.matches("<option").count(), LISTED);
    assert!(many.contains("8 to 63"));
    assert!(many.len() < 2048, "{} bytes", many.len());
    assert!(DONE.len() < 600);
}

#[test]
fn passwords_use_the_whole_alphabet_and_are_readable() {
    let mut seen = std::collections::HashSet::new();
    let mut n = 0u32;
    for _ in 0..200 {
        let mut p = [0u8; PASSWORD_LEN];
        password(
            || {
                n = n.wrapping_add(7);
                n
            },
            &mut p,
        );
        for b in p {
            assert!(b.is_ascii_lowercase() || (b'2'..=b'9').contains(&b));
            assert!(!b"ilo01".contains(&b));
            seen.insert(b);
        }
    }
    assert!(seen.len() >= 28, "{}", seen.len());
}

#[test]
fn the_join_code_escapes_special_characters() {
    let mut s = String::new();
    join_qr_text(&mut s, "plain", "abcdefghij").unwrap();
    assert_eq!(s, "WIFI:T:WPA;S:plain;P:abcdefghij;;");
    let mut s = String::new();
    join_qr_text(&mut s, "a;b,c:d\\e\"f", "p;q").unwrap();
    assert_eq!(s, r#"WIFI:T:WPA;S:a\;b\,c\:d\\e\"f;P:p\;q;;"#);
}

#[test]
fn saved_networks_are_tried_strongest_first_then_unseen_in_file_order() {
    let saved = ["home", "cafe", "hidden", "phone"];
    let seen = [("phone", -40), ("home", -70), ("home", -55), ("other", -30)];
    let (o, n) = join_order(&saved, &seen, None);
    // phone -40, home -55 (best of its two), then cafe and hidden unseen.
    assert_eq!(&o[..n], &[3, 0, 1, 2]);
    // Nothing seen: file order.
    assert_eq!(join_order(&saved, &[], None), ([0, 1, 2, 3], 4));
    // One network, and none.
    assert_eq!(join_order(&["a"], &[("a", -90)], None).1, 1);
    assert_eq!(join_order(&[], &[], None).1, 0);
    // More than four names never overruns.
    assert_eq!(join_order(&["a", "b", "c", "d", "e"], &[], None).1, 4);
}

#[test]
fn the_chosen_network_goes_first_and_the_rest_stay_as_fallback() {
    let saved = ["home", "cafe", "hidden", "phone"];
    let seen = [("phone", -40), ("home", -55)];
    // Chosen and seen, but weaker: still first.
    let (o, n) = join_order(&saved, &seen, Some("home"));
    assert_eq!(&o[..n], &[0, 3, 1, 2]);
    // Chosen but not seen in the scan (hidden): still first.
    let (o, n) = join_order(&saved, &seen, Some("hidden"));
    assert_eq!(&o[..n], &[2, 3, 0, 1]);
    // A chosen name that is no longer saved changes nothing.
    assert_eq!(
        join_order(&saved, &seen, Some("gone")),
        join_order(&saved, &seen, None)
    );
}
