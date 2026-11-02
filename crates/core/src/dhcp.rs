//! The smallest DHCP server that works, for the setup hotspot (ADR 025). It
//! offers one address, `CLIENT`, to whoever asks. The hotspot is WPA2 and
//! takes one phone, so there is no lease table.
//!
//! `reply` reads one request datagram and writes the answer into `out`. Both
//! the offer and the acknowledgement are broadcast by the caller (the client
//! has no address yet).

pub const SERVER: [u8; 4] = [192, 168, 4, 1];
pub const CLIENT: [u8; 4] = [192, 168, 4, 2];
const MASK: [u8; 4] = [255, 255, 255, 0];
const LEASE_SECONDS: u32 = 3600;
const COOKIE: [u8; 4] = [99, 130, 83, 99];
/// Fixed BOOTP header plus the magic cookie.
const HEADER: usize = 240;
/// Largest reply: header plus the options below.
pub const REPLY_CAP: usize = HEADER + 40;

const DISCOVER: u8 = 1;
const REQUEST: u8 = 3;
const OFFER: u8 = 2;
const ACK: u8 = 5;

/// Message type (option 53) of a well-formed client request, if any.
fn message_type(packet: &[u8]) -> Option<u8> {
    if packet.len() < HEADER || packet[0] != 1 || packet[1] != 1 || packet[2] != 6 {
        return None;
    }
    if packet[236..240] != COOKIE {
        return None;
    }
    let mut options = &packet[HEADER..];
    while let [code, rest @ ..] = options {
        match code {
            0 => options = rest,
            255 => return None,
            _ => {
                let (&len, rest) = rest.split_first()?;
                let value = rest.get(..len as usize)?;
                if *code == 53 {
                    return value.first().copied();
                }
                options = &rest[len as usize..];
            }
        }
    }
    None
}

/// Answers a DISCOVER with an offer and a REQUEST with an acknowledgement.
/// Anything else, or anything malformed, gets no reply. Returns the length
/// written to `out`.
pub fn reply(request: &[u8], out: &mut [u8; REPLY_CAP]) -> Option<usize> {
    let kind = match message_type(request)? {
        DISCOVER => OFFER,
        REQUEST => ACK,
        _ => return None,
    };
    out.fill(0);
    out[0] = 2; // BOOTREPLY
    out[1] = 1;
    out[2] = 6;
    out[4..8].copy_from_slice(&request[4..8]); // xid
    out[10..12].copy_from_slice(&[0x80, 0x00]); // broadcast flag
    out[16..20].copy_from_slice(&CLIENT); // yiaddr
    out[20..24].copy_from_slice(&SERVER); // siaddr
    out[28..44].copy_from_slice(&request[28..44]); // chaddr
    out[236..240].copy_from_slice(&COOKIE);
    let mut n = HEADER;
    for option in [
        &[53, 1, kind][..],
        &[54, 4, SERVER[0], SERVER[1], SERVER[2], SERVER[3]],
        &[51, 4, 0, 0, (LEASE_SECONDS >> 8) as u8, LEASE_SECONDS as u8],
        &[1, 4, MASK[0], MASK[1], MASK[2], MASK[3]],
        &[3, 4, SERVER[0], SERVER[1], SERVER[2], SERVER[3]],
        // The X3 answers DNS too (dns.rs), so a phone's internet check reaches it.
        &[6, 4, SERVER[0], SERVER[1], SERVER[2], SERVER[3]],
        &[255],
    ] {
        out[n..n + option.len()].copy_from_slice(option);
        n += option.len();
    }
    Some(n)
}
