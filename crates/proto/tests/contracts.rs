use x3_proto::{config::*, http::*, lines::*, payload::*, *};
const ID: &str = "01JB8ZK3Q4M8D6F2T0V9X7W5YH";
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const TOKEN: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
#[test]
fn identifiers() {
    for s in ["a", "a-_09", &"a".repeat(48)] {
        assert!(item_id(s).is_ok());
    }
    for s in ["", "..", "a/b", "a\\b", "ą", "a\0", &"a".repeat(49)] {
        assert!(item_id(s).is_err());
    }
    assert!(job_id(ID).is_ok());
    for s in [
        "",
        "81JB8ZK3Q4M8D6F2T0V9X7W5YH",
        "01JB8ZK3Q4M8D6F2T0V9X7W5YI",
        &ID.to_lowercase(),
    ] {
        assert!(job_id(s).is_err());
    }
    assert!(hash(HASH).is_ok());
    for s in ["", &HASH.to_uppercase(), &"g".repeat(64)] {
        assert!(hash(s).is_err());
    }
}
#[test]
fn line_boundaries() {
    assert!(validate_lines(format!("{}\n", "x".repeat(192)).as_bytes(), 4096).is_ok());
    assert_eq!(
        validate_lines(format!("{}\n", "x".repeat(193)).as_bytes(), 4096),
        Err(Error::TooLarge)
    );
    for b in [
        b"a\r\n".as_slice(),
        b"a\t\n",
        b"a\0\n",
        b" a\n",
        b"a  b\n",
        b"a \n",
        b"\n",
        b"\xff\n",
    ] {
        assert!(validate_lines(b, 100).is_err());
    }
    assert!(validate_lines(b"a", 100).is_err());
    assert!(validate_lines(b"a\n", 1).is_err());
}
#[test]
fn hello_time_errors() {
    let mut out = [0; 512];
    for battery in [None, Some(0), Some(100)] {
        let hello = Hello {
            firmware: "0.1.0",
            width: 792,
            height: 528,
            formats: "tk1,ar1",
            free_kib: u64::MAX,
            battery,
        };
        let n = hello.write(&mut out).unwrap();
        assert_eq!(Hello::parse(&out[..n]).unwrap(), hello);
        assert!(hello.write(&mut [0; 3]).is_err());
    }
    let good = "hello 1 fw=0.1.0 w=792 h=528 formats=tk1,ar1 free_kib=9 battery=87\n";
    for (a, b) in [
        ("hello", "bad"),
        ("hello 1", "hello 2"),
        ("fw=0.1.0", "fw="),
        ("fw=0.1.0", "fw=~"),
        ("w=792", "w=0"),
        ("w=792", "w=65536"),
        ("h=528", "h=-1"),
        ("formats=tk1,ar1", "formats=no"),
        ("free_kib=9", "free_kib=18446744073709551616"),
        ("battery=87", "battery=101"),
        ("battery=87", "battery=256"),
        ("battery=87", "battery=+1"),
    ] {
        assert!(Hello::parse(good.replace(a, b).as_bytes()).is_err(), "{b}");
    }
    for s in ["ok 1 time=0\n", "ok 1 time=18446744073709551615\n"] {
        let t = parse_time(s.as_bytes()).unwrap();
        let n = write_time(t, &mut out).unwrap();
        assert_eq!(&out[..n], s.as_bytes());
    }
    for s in [
        "ok 2 time=0\n",
        "ok 1 time=\n",
        "ok 1 time=+1\n",
        "ok 1 time=0 extra\n",
        "ok 1 time=0\nextra\n",
    ] {
        assert!(parse_time(s.as_bytes()).is_err());
    }
    for e in [
        "invalid",
        "unauthorised",
        "not_found",
        "conflict",
        "too_large",
        "unavailable",
    ] {
        let n = write_error(e, &mut out).unwrap();
        assert_eq!(parse_error(&out[..n]).unwrap(), e);
    }
    assert!(write_error("oops", &mut out).is_err());
}
#[test]
fn jobs_roundtrip_worst_case() {
    let item = "t-".to_owned() + &"a".repeat(46);
    let mut out = [0; 4096];
    for (kind, len, show) in [
        ("token", TOKEN_CAP, true),
        ("article", ARTICLE_CAP, false),
        ("delete", 0, false),
        ("future", 12, false),
    ] {
        let j = Job {
            id: ID,
            kind,
            item: &item,
            len,
            sha256: if kind == "delete" { None } else { Some(HASH) },
            show,
        };
        let n = j.write(&mut out).unwrap();
        // Including LF: the 6-digit 2-bit token length (ADR 018) adds one byte.
        match kind {
            "token" => assert_eq!(n, 163),
            "article" => assert_eq!(n, 162),
            _ => {}
        }
        assert_eq!(j.supported(), kind != "future");
        let mut jobs = Jobs {
            jobs: [None; 16],
            more: true,
        };
        jobs.jobs[0] = Some(j);
        let n = jobs.write(&mut out).unwrap();
        assert_eq!(Jobs::parse(&out[..n]).unwrap(), jobs);
        assert!(jobs.write(&mut [0; 10]).is_err());
    }
    for more in [false, true] {
        let j = Jobs {
            jobs: [None; 16],
            more,
        };
        let n = j.write(&mut out).unwrap();
        assert_eq!(Jobs::parse(&out[..n]).unwrap(), j);
    }
}
#[test]
fn hostile_jobs() {
    let valid = format!("x3jobs 1\njob {ID} token t-a 52288 {HASH} show\nend more=0\n");
    for (a, b) in [
        ("x3jobs 1", "x3jobs 2"),
        ("job ", "unknown "),
        ("token", "article"),
        ("52288", "104561"),
        ("52288", "999999999999999999999999"),
        ("t-a", "../a"),
        ("show", "unknown"),
        ("end more=0", "end more=2"),
        (HASH, "-"),
        (ID, "bad"),
    ] {
        assert!(Jobs::parse(valid.replace(a, b).as_bytes()).is_err(), "{b}");
    }
    for suffix in ["junk\n", "end more=0\n"] {
        assert!(Jobs::parse((valid.clone() + suffix).as_bytes()).is_err());
    }
    for n in 0..valid.len() {
        assert!(Jobs::parse(&valid.as_bytes()[..n]).is_err());
    }
    let line = format!("job {ID} delete a 0 - -\n");
    assert!(Jobs::parse(format!("x3jobs 1\n{line}{line}end more=0\n").as_bytes()).is_err());
    for line in [
        format!("job {ID} delete a 1 - -\n"),
        format!("job {ID} delete a 0 {HASH} -\n"),
        format!("job {ID} article a 264477 {HASH} -\n"),
        format!("job {ID} future a 1 {HASH} show\n"),
    ] {
        assert!(Jobs::parse(format!("x3jobs 1\n{line}end more=0\n").as_bytes()).is_err());
    }
    let mut s = "x3jobs 1\n".to_owned();
    for i in 0..17 {
        s += &format!("job 01JB8ZK3Q4M8D6F2T0V9X7W5{:02} delete a 0 - -\n", i);
    }
    s += "end more=0\n";
    assert_eq!(Jobs::parse(s.as_bytes()), Err(Error::TooLarge));
    assert!(Jobs::parse(&vec![b'x'; 4097]).is_err());
}
#[test]
fn ack_roundtrip() {
    let mut out = [0; 193];
    for a in [
        Ack::Committed(None),
        Ack::Committed(Some(HASH)),
        Ack::Displayed(HASH),
        Ack::Rejected("checksum"),
        Ack::Rejected("too_large"),
        Ack::Rejected("invalid"),
        Ack::Rejected("unsupported"),
        Ack::Rejected("storage"),
    ] {
        let n = a.write(&mut out).unwrap();
        assert_eq!(Ack::parse(&out[..n]).unwrap(), a);
        assert!(a.write(&mut [0; 2]).is_err());
    }
    for s in [
        "committed bad\n",
        "displayed -\n",
        "rejected other\n",
        "other storage\n",
        "committed - extra\n",
    ] {
        assert!(Ack::parse(s.as_bytes()).is_err());
    }
}
#[test]
fn config_validation() {
    let wifi = Wifi::parse(b"ssid=My WiFi\npsk=some=password\n").unwrap();
    assert_eq!(wifi.ssid, "My WiFi");
    assert_eq!(wifi.psk, "some=password");
    assert!(Wifi::parse(b"psk=12345678\nssid=a\n").is_ok());
    for s in [
        "ssid=a\npsk=short\n",
        "ssid=\npsk=12345678\n",
        "ssid=a\nssid=b\npsk=12345678\n",
        "ssid=a\nother=12345678\n",
        "ssid=a\npsk=12345678",
        "ssid=a\r\npsk=12345678\n",
        "ssid=a\n",
        "ssid\npsk=12345678\n",
    ] {
        assert!(Wifi::parse(s.as_bytes()).is_err());
    }
    assert!(Wifi::parse(&vec![b'a'; 513]).is_err());
    assert!(Wifi::parse(b"ssid=\xff\npsk=12345678\n").is_err());
    for url in [
        "https://1.2.3.4:80",
        "http://host:80",
        "http://1.2.3.4:0",
        "http://1.2.3.256:80",
        "http://01.2.3.4:80",
        "http://1.2.3:80",
        "http://1.2.3.4.5:80",
        "http://1.2.3.4:65536",
        "http://1.2.3.4:80/path",
        "http://1.2.3.4",
    ] {
        assert!(Address::parse(url).is_err(), "{url}");
    }
    let cfg = format!("url=http://192.168.1.20:8788\ntoken={TOKEN}\n");
    let c = Companion::parse(cfg.as_bytes()).unwrap();
    assert_eq!(
        c.address,
        Some(Address {
            ip: [192, 168, 1, 20],
            port: 8788
        })
    );
    assert_eq!(c.remote, None);
    assert_eq!(c.token, TOKEN);
    assert!(token("secret").is_err());
    assert!(token(&"!".repeat(43)).is_err());
}
#[test]
fn http_framing() {
    let good = b"HTTP/1.1 200 OK\r\ncontent-length: 4\r\nX-Test: fine\r\n\r\n";
    for n in 0..good.len() {
        assert_eq!(ResponseHead::parse(&good[..n], 4).unwrap(), None);
    }
    let h = ResponseHead::parse(good, 4).unwrap().unwrap();
    assert_eq!(h.content_length, 4);
    let mut b = Body::new(h);
    b.accept(b"ab").unwrap();
    b.accept(b"cd").unwrap();
    b.finish().unwrap();
    assert_eq!(Body::new(h).finish(), Err(Error::Truncated));
    assert!(Body::new(h).accept(b"12345").is_err());
    assert!(ResponseHead::parse(good, 3).is_err());
    let mut data = good.to_vec();
    data.extend_from_slice(b"1234");
    assert!(ResponseHead::parse(&data, 4).is_ok());
    data.push(0);
    assert!(ResponseHead::parse(&data, 4).is_err());
    for s in [
        "HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n",
        "HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n",
        "HTTP/1.1 100 Continue\r\nContent-Length: 0\r\n\r\n",
        "HTTP/1.1 204 OK\r\nContent-Length: 1\r\n\r\n",
        "HTTP/1.1 200 OK\r\n\r\n",
        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n",
    ] {
        assert!(ResponseHead::parse(s.as_bytes(), 100).is_err(), "{s}");
    }
    for header in [
        "Transfer-Encoding: chunked",
        "Content-Encoding: identity",
        "Content-Length: -1",
        "Content-Length: +1",
        "Content-Length: 1, 1",
        "Bad Header: x",
        "Folded line",
        "X: a\nb",
        "X: \0",
    ] {
        let s = format!("HTTP/1.1 200 OK\r\n{header}\r\n\r\n");
        assert!(ResponseHead::parse(s.as_bytes(), 100).is_err(), "{header}");
    }
    assert!(ResponseHead::parse(&vec![b'a'; HEAD_CAP], 1).is_err());
    let big = format!(
        "HTTP/1.1 200 OK\r\nX: {}\r\nContent-Length: 0\r\n\r\n",
        "a".repeat(HEAD_CAP)
    );
    assert!(ResponseHead::parse(big.as_bytes(), 0).is_err());
    let h = ResponseHead::parse(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n", 0)
        .unwrap()
        .unwrap();
    Body::new(h).finish().unwrap();
}
#[test]
fn request_safe_paths() {
    let a = Address {
        ip: [192, 168, 1, 20],
        port: 8788,
    };
    let mut out = [0; 1024];
    for (r, len, path) in [
        (Route::Hello, 70, "hello"),
        (Route::Jobs, 0, "jobs"),
        (
            Route::Payload(ID),
            0,
            "jobs/01JB8ZK3Q4M8D6F2T0V9X7W5YH/payload",
        ),
        (Route::Ack(ID), 76, "jobs/01JB8ZK3Q4M8D6F2T0V9X7W5YH/ack"),
    ] {
        let n = request(a, TOKEN, r, len, &mut out).unwrap();
        let s = std::str::from_utf8(&out[..n]).unwrap();
        assert!(s.contains(&format!("/device/v1/{path} HTTP/1.1\r\n")));
        assert!(s.contains("Connection: close\r\n"));
        assert!(s.contains(&format!("Content-Length: {len}\r\n")));
    }
    assert!(request(a, TOKEN, Route::Jobs, 1, &mut out).is_err());
    assert!(request(a, TOKEN, Route::Payload("../bad"), 0, &mut out).is_err());
    assert!(request(a, "bad", Route::Jobs, 0, &mut out).is_err());
    assert!(request(a, TOKEN, Route::Jobs, 0, &mut [0; 2]).is_err());
}
#[test]
fn crc_and_token() {
    assert_eq!(crc32(b"123456789"), 0xcbf43926);
    let mut c = Crc32::default();
    c.update(b"1234");
    c.update(b"56789");
    assert_eq!(c.finish(), 0xcbf43926);
    for (bpp, width, height) in [(1, 792, 528), (1, 528, 792), (2, 792, 528), (2, 528, 792)] {
        let len = if bpp == 1 { 52_288 } else { TOKEN_CAP };
        let mut b = vec![0xff; len];
        let h = TokenHeader {
            bpp,
            width,
            height,
            crc: crc32(&b[16..]),
        };
        assert_eq!(h.payload_len(), len);
        h.encode(&mut b).unwrap();
        assert_eq!(TokenHeader::payload(&b).unwrap(), h);
        for n in 0..16 {
            assert!(TokenHeader::decode(&b[..n]).is_err());
        }
        assert!(h.encode(&mut [0; 15]).is_err());
        for i in [0, 4, 5, 6, 8, 10] {
            let mut bad = b.clone();
            bad[i] ^= 0x80;
            assert!(TokenHeader::payload(&bad).is_err());
        }
        b[16] ^= 1;
        assert_eq!(TokenHeader::payload(&b), Err(Error::Checksum));
        assert!(TokenHeader::payload(&b[..b.len() - 1]).is_err());
    }
    // Unknown depths, and a header whose depth disagrees with the length.
    let mut b = vec![0xff; TOKEN_CAP];
    for bpp in [0, 3, 4, 8, 255] {
        b[..16].copy_from_slice(b"X3TK\x01\x00\x18\x03\x10\x02\x00\x00\x00\x00\x00\x00");
        b[5] = bpp;
        assert!(TokenHeader::decode(&b).is_err());
    }
    let h = TokenHeader {
        bpp: 1,
        width: 792,
        height: 528,
        crc: crc32(&b[16..]),
    };
    h.encode(&mut b).unwrap();
    assert!(TokenHeader::payload(&b).is_err());
    let h = TokenHeader { bpp: 2, ..h };
    let mut short = vec![0xff; 52_288];
    h.encode(&mut short).unwrap();
    assert!(TokenHeader::payload(&short).is_err());
}
#[test]
fn article_validation() {
    let mut b = vec![0; ARTICLE_CAP];
    for summary in [false, true] {
        let n = encode_article(
            "Zażółć",
            "https://example.org/a",
            "# Heading\n\nĄ\tsoft\u{ad}hyphen",
            summary,
            MAX_TIME,
            &mut b,
        )
        .unwrap();
        let a = Article::decode(&b[..n]).unwrap();
        assert_eq!(a.title, "Zażółć");
        assert_eq!(a.header.summary, summary);
        assert_eq!(a.header.len(), n);
        assert!(!a.header.is_empty());
        for i in 0..n {
            assert!(Article::decode(&b[..i]).is_err());
        }
        for i in [0, 4, 5, 6, 8, 10, 12, 16, 24, n - 1] {
            let mut bad = b[..n].to_vec();
            bad[i] ^= 0x80;
            assert!(Article::decode(&bad).is_err(), "{i}");
        }
        assert!(a.header.encode(&mut [0; 27]).is_err());
    }
    for (title, url, content) in [
        ("", "", ""),
        ("bad\n", "", ""),
        ("a", "file:///a", ""),
        ("a", "http://", ""),
        ("a", "https://host/a b", ""),
        ("a", "https://host\\evil", ""),
        ("a", "", "bad\r"),
        ("a", "", "bad\u{85}"),
    ] {
        assert!(encode_article(title, url, content, false, 0, &mut b).is_err());
    }
    assert!(encode_article("a", "", "", false, MAX_TIME + 1, &mut b).is_err());
    assert!(encode_article("a", "", "", false, 0, &mut [0; 28]).is_err());
    assert!(encode_article(&"a".repeat(257), "", "", false, 0, &mut b).is_err());
    assert!(encode_article("a", "", &"a".repeat(262145), false, 0, &mut b).is_err());
    let n = encode_article("a", "", "", false, 0, &mut b).unwrap();
    assert_eq!(Article::decode(&b[..n]).unwrap().text, "");
    b[28] = 255;
    let crc = crc32(&b[28..n]);
    b[24..28].copy_from_slice(&crc.to_le_bytes());
    assert!(Article::decode(&b[..n]).is_err());
    assert!(Article::decode(&vec![0; ARTICLE_CAP + 1]).is_err());
}
#[test]
fn arbitrary_bytes_never_panic() {
    let mut seed = 42u32;
    for len in 0..1100 {
        let mut b = vec![0; len];
        for v in &mut b {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            *v = (seed >> 24) as u8;
        }
        let _ = Jobs::parse(&b);
        let _ = Hello::parse(&b);
        let _ = Ack::parse(&b);
        let _ = Companion::parse(&b);
        let _ = Wifi::parse(&b);
        let _ = ResponseHead::parse(&b, ARTICLE_CAP);
        let _ = Article::decode(&b);
        let _ = TokenHeader::payload(&b);
    }
}

#[test]
fn empty_204_ack_amendment() {
    for header in ["", "Content-Length: 0\r\n"] {
        let raw = format!("HTTP/1.1 204 No Content\r\n{header}\r\n");
        let head = ResponseHead::parse(raw.as_bytes(), 0).unwrap().unwrap();
        assert_eq!(head.content_length, 0);
        Body::new(head).finish().unwrap();
        assert!(ResponseHead::parse((raw + "x").as_bytes(), 0).is_err());
    }
    for header in [
        "Content-Length: 1",
        "Transfer-Encoding: chunked",
        "Content-Encoding: gzip",
    ] {
        let raw = format!("HTTP/1.1 204 No Content\r\n{header}\r\n\r\n");
        assert!(ResponseHead::parse(raw.as_bytes(), 100).is_err());
    }
    assert!(ResponseHead::parse(b"HTTP/1.1 200 OK\r\n\r\n", 0).is_err());
}

#[test]
fn wifi_list_reads_one_or_several_networks() {
    use x3_proto::config::WifiList;
    let one = WifiList::parse(b"ssid=Home\npsk=12345678\n").unwrap();
    assert_eq!(one.len(), 1);
    let many = WifiList::parse(b"ssid=A\npsk=aaaaaaaa\nssid=B b\npsk=b=b=b=b=\n").unwrap();
    assert_eq!(many.len(), 2);
    assert_eq!(many.find("B b").unwrap().psk, "b=b=b=b=");
    assert!(many.find("C").is_none());
}

#[test]
fn wifi_list_rejects_bad_files() {
    use x3_proto::config::WifiList;
    for bad in [
        &b""[..],
        b"\n",
        b"ssid=A\npsk=aaaaaaaa",            // no final newline
        b"ssid=A\n",                        // missing psk
        b"psk=aaaaaaaa\nssid=A\n",          // wrong order
        b"ssid=A\npsk=short\n",             // psk under 8
        b"ssid=\npsk=aaaaaaaa\n",           // empty name
        b"ssid=A\npsk=aaaaaaaa\nextra=1\n", // unknown key
        b"ssid=A\npsk=aaaa\x00aaaa\n",      // control byte
        b"ssid=\xff\npsk=aaaaaaaa\n",       // not UTF-8
        b"ssid=A\r\npsk=aaaaaaaa\n",        // CR
    ] {
        assert!(WifiList::parse(bad).is_err(), "{bad:?}");
    }
    let five = "ssid=A\npsk=aaaaaaaa\n".repeat(5);
    assert!(WifiList::parse(five.as_bytes()).is_err());
    assert!(WifiList::parse(&vec![b'a'; 513]).is_err());
    let long = format!("ssid={}\npsk=aaaaaaaa\n", "x".repeat(33));
    assert!(WifiList::parse(long.as_bytes()).is_err());
}

fn saved(existing: Option<&[u8]>, ssid: &str, psk: &str) -> String {
    use x3_proto::config::{WifiList, save_network};
    let list = existing.map(|b| WifiList::parse(b).unwrap());
    let mut out = [0u8; 512];
    let n = save_network(list.as_ref(), ssid, psk, &mut out).unwrap();
    String::from_utf8(out[..n].to_vec()).unwrap()
}

#[test]
fn saving_a_network_adds_replaces_and_drops_the_oldest() {
    let first = saved(None, "Home", "homepass1");
    assert_eq!(first, "ssid=Home\npsk=homepass1\n");
    let two = saved(Some(first.as_bytes()), "Cafe", "cafepass1");
    assert_eq!(two, "ssid=Home\npsk=homepass1\nssid=Cafe\npsk=cafepass1\n");
    // The same name replaces the password and moves to the end.
    let again = saved(Some(two.as_bytes()), "Home", "newpass12");
    assert_eq!(
        again,
        "ssid=Cafe\npsk=cafepass1\nssid=Home\npsk=newpass12\n"
    );
    // A fifth network pushes out the oldest.
    let mut file = String::new();
    for n in ["A", "B", "C", "D", "E"] {
        file = saved(
            Some(file.as_bytes()).filter(|f| !f.is_empty()),
            n,
            "password1",
        );
    }
    assert!(!file.contains("ssid=A\n"));
    assert_eq!(file.matches("ssid=").count(), 4);
    assert!(file.ends_with("ssid=E\npsk=password1\n"));
}

#[test]
fn saving_refuses_bad_values_and_always_fits() {
    use x3_proto::config::save_network;
    let mut out = [0u8; 512];
    assert!(save_network(None, "", "password1", &mut out).is_err());
    assert!(save_network(None, "ok", "short", &mut out).is_err());
    assert!(save_network(None, "a\nb", "password1", &mut out).is_err());
    assert!(save_network(None, "ok", "pass\nword1", &mut out).is_err());
    assert!(save_network(None, &"x".repeat(33), "password1", &mut out).is_err());
    // Four networks at the largest size still fit and read back.
    let mut file = String::new();
    for c in ['a', 'b', 'c', 'd', 'e'] {
        let ssid = c.to_string().repeat(32);
        file = saved(
            Some(file.as_bytes()).filter(|f| !f.is_empty()),
            &ssid,
            &"p".repeat(63),
        );
    }
    assert!(file.len() <= 512);
    assert!(x3_proto::config::WifiList::parse(file.as_bytes()).is_ok());
}

#[test]
fn server_cfg_accepts_the_internet_form_with_an_optional_lan_address() {
    let t = TOKEN;
    let one = format!("url=https://device.example.org\ntoken={t}\n");
    let c = Companion::parse(one.as_bytes()).unwrap();
    assert_eq!(
        c.remote,
        Some(Remote {
            host: "device.example.org",
            port: 443
        })
    );
    assert_eq!(c.address, None);
    let two =
        format!("token={t}\nlan=http://192.168.1.20:8788\nurl=https://a-b.example.org:8443\n");
    let c = Companion::parse(two.as_bytes()).unwrap();
    assert_eq!(
        c.remote,
        Some(Remote {
            host: "a-b.example.org",
            port: 8443
        })
    );
    assert_eq!(c.address.unwrap().port, 8788);
}

#[test]
fn server_cfg_rejects_bad_forms() {
    let t = TOKEN;
    for body in [
        format!("url=https://localhost\ntoken={t}\n"),
        format!("url=https://1.2.3.4\ntoken={t}\n"),
        format!("url=https://Example.org\ntoken={t}\n"),
        format!("url=https://exa mple.org\ntoken={t}\n"),
        format!("url=https://-a.example.org\ntoken={t}\n"),
        format!("url=https://a..example.org\ntoken={t}\n"),
        format!("url=https://example.org:0\ntoken={t}\n"),
        format!("url=https://example.org:65536\ntoken={t}\n"),
        format!("url=https://example.org/path\ntoken={t}\n"),
        format!("url=https://{}.org\ntoken={t}\n", "a".repeat(64)),
        format!("url=http://example.org:80\ntoken={t}\n"),
        format!("url=http://192.168.1.20:8788\nlan=http://192.168.1.20:8788\ntoken={t}\n"),
        format!("url=https://example.org\nlan=https://example.org\ntoken={t}\n"),
        format!("url=https://example.org\nurl=https://example.org\ntoken={t}\n"),
        "url=https://example.org\n".to_string(),
        format!("token={t}\n"),
        format!("url=https://example.org\ntoken={t}\nx=1\n"),
        format!("url=https://example.org\ntoken={t}"),
    ] {
        assert!(Companion::parse(body.as_bytes()).is_err(), "{body}");
    }
}

#[test]
fn chosen_network_file() {
    assert_eq!(chosen_network(b"").unwrap(), None);
    assert_eq!(
        chosen_network(b"ssid=My phone\n").unwrap(),
        Some("My phone")
    );
    let mut out = [0u8; 64];
    let n = write_chosen_network("Caf\u{e9} 5G", &mut out).unwrap();
    assert_eq!(chosen_network(&out[..n]).unwrap(), Some("Caf\u{e9} 5G"));
    let n = write_chosen_network(&"x".repeat(32), &mut out).unwrap();
    assert_eq!(n, 38);
    // Malformed, truncated, oversized and hostile input.
    for bad in [
        &b"ssid=\n"[..],
        b"ssid=phone",
        b"name=phone\n",
        b"ssid=a\x00b\n",
        b"ssid=a\nssid=b\n",
        b"\xff\xfe\n",
    ] {
        assert!(chosen_network(bad).is_err(), "{bad:?}");
    }
    assert!(chosen_network(format!("ssid={}\n", "x".repeat(33)).as_bytes()).is_err());
    assert!(chosen_network(&[b'a'; 65]).is_err());
    assert!(write_chosen_network("", &mut out).is_err());
    assert!(write_chosen_network(&"x".repeat(33), &mut out).is_err());
    assert!(write_chosen_network("a\nb", &mut out).is_err());
}

#[test]
fn hello_formats_and_image_jobs() {
    let line =
        |f: &str| format!("hello 1 fw=0.1.0 w=792 h=528 formats={f} free_kib=9 battery=87\n");
    for ok in [
        "tk1",
        "ar1",
        "im1",
        "tk1,ar1",
        "ar1,tk1",
        "tk1,ar1,im1",
        "im1,tk1",
    ] {
        let l = line(ok);
        let h = Hello::parse(l.as_bytes()).unwrap();
        assert_eq!(h.takes("im1"), ok.contains("im1"), "{ok}");
    }
    for bad in [
        "",
        ",",
        "tk1,",
        ",tk1",
        "tk1,tk1",
        "im2",
        "tk1,ar1,im1,tk1",
        "TK1",
        "tk1 ar1",
    ] {
        assert!(Hello::parse(line(bad).as_bytes()).is_err(), "{bad:?}");
    }
    assert_eq!(format_of("image"), Some("im1"));
    assert_eq!(format_of("token"), Some("tk1"));
    assert_eq!(format_of("article"), Some("ar1"));
    assert_eq!(format_of("delete"), None);
    // An image job: picture cap and the show flag, like a token.
    let job = |kind: &str, len: usize, flag: &str| {
        format!("x3jobs 1\njob {ID} {kind} i-cat-0123456789ab {len} {HASH} {flag}\nend more=0\n")
    };
    let ok = job("image", TOKEN_CAP, "show");
    let jobs = Jobs::parse(ok.as_bytes()).unwrap();
    let j = jobs.jobs[0].as_ref().unwrap();
    assert!(j.supported() && j.show && j.kind == "image");
    assert!(Jobs::parse(job("image", TOKEN_CAP + 1, "-").as_bytes()).is_err());
    assert!(Jobs::parse(job("image", 10, "maybe").as_bytes()).is_err());
    assert!(Jobs::parse(job("article", 10, "show").as_bytes()).is_err());
}
