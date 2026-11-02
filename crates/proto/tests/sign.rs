use sha2::{Digest, Sha256};
use x3_proto::sign::*;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
        .collect()
}

/// RFC 4231 test cases 1 and 2 use keys under 64 bytes; our HMAC takes a 32-byte
/// key, so these check it against case 3 (a 20-byte 0xaa key, zero-padded to
/// 32 bytes gives the same HMAC) and a 32-byte key computed independently.
#[test]
fn hmac_matches_known_vectors() {
    // RFC 4231 case 3: key = 20 x 0xaa, data = 50 x 0xdd. A key shorter than the
    // block is zero-padded, so a 32-byte key of the same bytes plus 12 zeros is the same key.
    let mut k = [0u8; 32];
    k[..20].fill(0xaa);
    let mac = hmac(&k, &[&[0xdd; 50]]);
    assert_eq!(
        mac.to_vec(),
        unhex("773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe")
    );
    // RFC 4231 case 2 with its 4-byte key "Jefe", zero-padded.
    let mut k = [0u8; 32];
    k[..4].copy_from_slice(b"Jefe");
    let mac = hmac(&k, &[b"what do ya want ", b"for nothing?"]);
    assert_eq!(
        mac.to_vec(),
        unhex("5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843")
    );
    // Parts are joined, not separated.
    assert_eq!(hmac(&k, &[b"ab", b"cd"]), hmac(&k, &[b"abcd"]));
}

#[test]
fn the_key_is_the_sha256_of_the_token_and_round_trips_through_hex() {
    let token = "A".repeat(43);
    let k = key(&token);
    assert_eq!(k.to_vec(), Sha256::digest(token.as_bytes()).to_vec());
    let mut h = [0u8; 64];
    hex(&k, &mut h);
    assert_eq!(key_from_hex(std::str::from_utf8(&h).unwrap()), Some(k));
    for bad in ["", "zz", &"G".repeat(64), &"A".repeat(64), &"a".repeat(63)] {
        assert_eq!(key_from_hex(bad), None, "{bad}");
    }
}

#[test]
fn a_request_mac_depends_on_every_field() {
    let k = key("x");
    let n = "0123456789abcdef0123456789abcdef";
    let base = request_mac(&k, "GET", "/device/v1/jobs", n, b"");
    assert_eq!(base, request_mac(&k, "GET", "/device/v1/jobs", n, b""));
    let others = [
        request_mac(&k, "POST", "/device/v1/jobs", n, b""),
        request_mac(&k, "GET", "/device/v1/job", n, b""),
        request_mac(
            &k,
            "GET",
            "/device/v1/jobs",
            "1123456789abcdef0123456789abcdef",
            b"",
        ),
        request_mac(&k, "GET", "/device/v1/jobs", n, b"x"),
        request_mac(&key("y"), "GET", "/device/v1/jobs", n, b""),
    ];
    for o in others {
        assert_ne!(base, o);
    }
}

#[test]
fn a_response_mac_depends_on_nonce_status_and_body() {
    let k = key("x");
    let n = "0123456789abcdef0123456789abcdef";
    let h: [u8; 32] = Sha256::digest(b"body").into();
    let base = response_mac(&k, n, 200, &h);
    assert_ne!(
        base,
        response_mac(&k, "1123456789abcdef0123456789abcdef", 200, &h)
    );
    assert_ne!(base, response_mac(&k, n, 404, &h));
    assert_ne!(
        base,
        response_mac(&k, n, 200, &Sha256::digest(b"bodz").into())
    );
    // A request MAC can never be taken for a response MAC.
    let req = request_mac(&k, "GET", "/", n, b"body");
    assert_ne!(req, base);
}

#[test]
fn constant_time_compare_and_hex_decoding() {
    assert!(same(b"abc", b"abc"));
    assert!(!same(b"abc", b"abd"));
    assert!(!same(b"abc", b"ab"));
    let mut out = [0u8; 2];
    assert!(decode_hex("00ff", &mut out).is_some());
    assert_eq!(out, [0, 255]);
    assert!(decode_hex("00FF", &mut out).is_none(), "upper case refused");
    assert!(decode_hex("00f", &mut out).is_none());
}

use x3_proto::http::{ResponseHead, Route, nonce_hex, signed_request};

const JOB: &str = "01JB8ZK3Q4M8D6F2T0V9X7W5YH";

#[test]
fn a_signed_request_carries_no_token_and_a_mac_the_server_can_check() {
    let token = "T".repeat(43);
    let k = key(&token);
    let nonce = [7u8; 16];
    let mut out = [0u8; 1024];
    let n = signed_request(
        x3_proto::config::Remote {
            host: "device.example.org",
            port: 443,
        },
        None,
        &k,
        &nonce,
        Route::Ack(JOB),
        b"committed -\n",
        &mut out,
    )
    .unwrap();
    let head = std::str::from_utf8(&out[..n]).unwrap();
    assert!(!head.contains(&token), "the token is never sent");
    assert!(!head.contains("Bearer"));
    assert!(head.starts_with(&format!(
        "POST /device/v1/jobs/{JOB}/ack HTTP/1.1\r\nHost: device.example.org\r\n"
    )));
    assert!(head.contains("Content-Length: 12\r\n"));
    let nonce_text = String::from_utf8(nonce_hex(&nonce).to_vec()).unwrap();
    assert!(head.contains(&format!("X3-Nonce: {nonce_text}\r\n")));
    let mac = request_mac(
        &k,
        "POST",
        &format!("/device/v1/jobs/{JOB}/ack"),
        &nonce_text,
        b"committed -\n",
    );
    let mut mac_hex = [0u8; 64];
    hex(&mac, &mut mac_hex);
    assert!(head.contains(&format!(
        "Authorization: X3-HMAC {}\r\n",
        std::str::from_utf8(&mac_hex).unwrap()
    )));
}

#[test]
fn a_signed_request_shows_a_non_default_port_and_refuses_bad_input() {
    let k = key("x");
    let mut out = [0u8; 1024];
    let n = signed_request(
        x3_proto::config::Remote {
            host: "a.example.org",
            port: 8443,
        },
        None,
        &k,
        &[0; 16],
        Route::Jobs,
        b"",
        &mut out,
    )
    .unwrap();
    assert!(
        std::str::from_utf8(&out[..n])
            .unwrap()
            .contains("Host: a.example.org:8443\r\n")
    );
    assert!(
        signed_request(
            x3_proto::config::Remote {
                host: "a.example.org",
                port: 0
            },
            None,
            &k,
            &[0; 16],
            Route::Jobs,
            b"",
            &mut out
        )
        .is_err()
    );
    assert!(
        signed_request(
            x3_proto::config::Remote {
                host: "a.example.org",
                port: 443
            },
            None,
            &k,
            &[0; 16],
            Route::Jobs,
            b"x",
            &mut out
        )
        .is_err()
    );
    assert!(
        signed_request(
            x3_proto::config::Remote {
                host: "a.example.org",
                port: 443
            },
            None,
            &k,
            &[0; 16],
            Route::Payload("../x"),
            b"",
            &mut out
        )
        .is_err()
    );
    assert!(
        signed_request(
            x3_proto::config::Remote {
                host: "a.example.org",
                port: 443
            },
            None,
            &k,
            &[0; 16],
            Route::Hello,
            &[0; 162],
            &mut out
        )
        .is_err()
    );
    assert!(
        signed_request(
            x3_proto::config::Remote {
                host: "a.example.org",
                port: 443
            },
            None,
            &k,
            &[0; 16],
            Route::Jobs,
            b"",
            &mut [0u8; 40]
        )
        .is_err()
    );
}

#[test]
fn the_reply_mac_header_is_read_and_strictly_checked() {
    let m = "ab".repeat(32);
    let ok = format!("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX3-Mac: {m}\r\n\r\n");
    let h = ResponseHead::parse(ok.as_bytes(), 0).unwrap().unwrap();
    assert_eq!(h.mac, Some([0xab; 32]));
    let none = ResponseHead::parse(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n", 0)
        .unwrap()
        .unwrap();
    assert_eq!(none.mac, None);
    for bad in [
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX3-Mac: {}\r\n\r\n",
            "AB".repeat(32)
        ),
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX3-Mac: {}\r\n\r\n",
            "ab".repeat(31)
        ),
        format!("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX3-Mac: {m}\r\nx3-mac: {m}\r\n\r\n"),
        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX3-Mac: \r\n\r\n".to_string(),
    ] {
        assert!(ResponseHead::parse(bad.as_bytes(), 0).is_err(), "{bad}");
    }
}

#[test]
fn the_access_service_token_is_sent_as_two_headers_and_never_the_device_token() {
    let token = "T".repeat(43);
    let k = key(&token);
    let access = x3_proto::config::Access {
        id: "abc123.access",
        secret: "s3cr3t-value_9",
    };
    let mut out = [0u8; 1024];
    let n = signed_request(
        x3_proto::config::Remote {
            host: "d.example.org",
            port: 443,
        },
        Some(access),
        &k,
        &[1; 16],
        Route::Jobs,
        b"",
        &mut out,
    )
    .unwrap();
    let head = std::str::from_utf8(&out[..n]).unwrap();
    assert!(head.contains("CF-Access-Client-Id: abc123.access\r\n"));
    assert!(head.contains("CF-Access-Client-Secret: s3cr3t-value_9\r\n"));
    assert!(!head.contains(&token));
    assert!(head.find("CF-Access-Client-Id").unwrap() < head.find("X3-Nonce").unwrap());
}

#[test]
fn server_cfg_reads_the_access_service_token() {
    let t = "A".repeat(43);
    let ok = format!(
        "url=https://d.example.org\ntoken={t}\naccess_id=abc.access\naccess_secret={}\n",
        "f".repeat(64)
    );
    let c = x3_proto::config::Companion::parse(ok.as_bytes()).unwrap();
    assert_eq!(c.access.unwrap().id, "abc.access");
    for bad in [
        format!("url=https://d.example.org\ntoken={t}\naccess_id=abc.access\n"),
        format!("url=https://d.example.org\ntoken={t}\naccess_secret=abc\n"),
        format!("url=https://d.example.org\ntoken={t}\naccess_id=a b\naccess_secret=abc\n"),
        format!(
            "url=https://d.example.org\ntoken={t}\naccess_id=a\naccess_secret={}\n",
            "f".repeat(129)
        ),
        format!("url=http://192.168.1.20:8788\ntoken={t}\naccess_id=a\naccess_secret=b\n"),
    ] {
        assert!(
            x3_proto::config::Companion::parse(bad.as_bytes()).is_err(),
            "{bad}"
        );
    }
}
