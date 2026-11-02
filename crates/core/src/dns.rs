//! A DNS answer for the setup hotspot (ADR 025): every name resolves to the X3,
//! so a phone's "is there internet?" check lands on the setup page and the phone
//! opens it by itself. One question per packet, no compression in the question,
//! nothing recursive. Anything odd gets no reply.
use crate::dhcp::SERVER;

/// Largest request read; longer packets are ignored.
pub const REQUEST_CAP: usize = 256;
/// A reply is the request plus a 16-byte answer.
pub const REPLY_CAP: usize = REQUEST_CAP + 16;

/// Answers one query. `A` questions get the X3's address; other types (AAAA,
/// HTTPS) get an empty answer, which tells the phone to use IPv4. Returns the
/// length written to `out`.
pub fn reply(request: &[u8], out: &mut [u8; REPLY_CAP]) -> Option<usize> {
    if request.len() < 17 || request.len() > REQUEST_CAP {
        return None;
    }
    let flags = u16::from_be_bytes([request[2], request[3]]);
    let counts = |i: usize| u16::from_be_bytes([request[i], request[i + 1]]);
    // A query (QR 0), standard (opcode 0), one question. One extra record (the
    // EDNS option some phones add) is allowed and ignored.
    if flags & 0xF800 != 0 || counts(4) != 1 || counts(6) != 0 || counts(8) != 0 || counts(10) > 1 {
        return None;
    }
    // Walk the name: length-prefixed labels, ending in a zero.
    let mut i = 12;
    loop {
        let len = *request.get(i)? as usize;
        if len == 0 {
            i += 1;
            break;
        }
        if len > 63 {
            return None; // compression pointers and reserved forms
        }
        i += 1 + len;
    }
    let question_end = i.checked_add(4)?;
    let tail = request.get(i..question_end)?;
    if question_end != request.len() && counts(10) == 0 {
        return None; // trailing bytes with no extra record to explain them
    }
    let (qtype, qclass) = (
        u16::from_be_bytes([tail[0], tail[1]]),
        u16::from_be_bytes([tail[2], tail[3]]),
    );
    out[..question_end].copy_from_slice(&request[..question_end]);
    // QR, AA, keep RD, RA; no error.
    out[2] = 0x84 | (request[2] & 0x01);
    out[3] = 0x80;
    out[6..12].fill(0);
    let mut n = question_end;
    if qtype == 1 && qclass == 1 {
        out[7] = 1;
        out[n..n + 12].copy_from_slice(&[
            0xC0, 0x0C, // the name in the question
            0, 1, 0, 1, // A, IN
            0, 0, 0, 30, // 30 s
            0, 4, // four bytes follow
        ]);
        out[n + 12..n + 16].copy_from_slice(&SERVER);
        n += 16;
    }
    Some(n)
}
