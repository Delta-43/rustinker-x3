# Protocol

How the X3, the companion and the phone talk, and the payload and file formats they share.
The parsers live in [`crates/proto`](../crates/proto) and [`crates/core`](../crates/core)
(`no_std`, no allocation) and are tested with malformed, truncated, oversized and hostile
input. Change the wire format only through a new decision record ([Decisions](DECISIONS.md)).

There are two contracts:

1. **Device API.** The X3 is the HTTP client. During a Receive it pulls jobs from the
   companion. Text bodies, a per-device token.
2. **Companion API.** The web page (or any HTTPS client) talks JSON to the companion
   behind a login proxy.

## Device API

### Transport

- HTTP/1.1. The companion address comes from `X3/SERVER.CFG`: an `http://IPv4:port` on the
  LAN, or an `https://host` for the [signed route](#signed-requests). No redirects: any 3xx
  is an error.
- On the LAN every request carries `Authorization: Bearer <token>`. The token never
  appears in a URL or a log.
- Every response has a `Content-Length`, except a `204`, which has no body and either no
  `Content-Length` or `Content-Length: 0`. The device rejects chunked or encoded bodies, a
  status line or header block over 1,024 bytes, and a body over the route's cap.
- Timeouts: connect 5 s, idle read 10 s, whole session 180 s.
- One request at a time; no pipelining, no resumed downloads.
- Text bodies are UTF-8; lines end with `\n`; fields are separated by one space. A line
  over 192 bytes (without the `\n`), a control character other than `\n`, or an unknown
  leading word makes the whole body invalid.

### Receive session

```text
key press → join Wi-Fi → POST hello → loop { GET jobs → for each job: fetch → commit → ack }
          → stop when the list is empty, on timeout or on error → Wi-Fi off
```

| Job kind | Device action |
| --- | --- |
| `token`, `image`, `article` | If the item already holds this exact SHA-256, skip the download and ack `committed`. Otherwise stream the payload to the card while hashing it, check length, SHA-256, header and CRC, commit, then ack. |
| `delete` | Remove the item and its reading position (absent counts as done). Ack `committed -`. |

A token or image with the `show` flag is shown after the session ends; the device acks
`displayed <sha256>` at the start of the next session (the radio is off by then).
Articles never open by themselves.

### `POST /device/v1/hello`

One line, at most 160 bytes:

```text
hello 1 fw=0.1.0 w=792 h=528 formats=tk1,ar1,im1 free_kib=1843200 battery=87
```

`battery` is 0–100 or `na`. `formats` lists the payload formats the device accepts. The
reply is `200` with `ok 1 time=<unix seconds>`; the device sets its clock from it when the
difference is over 60 s.

### `GET /device/v1/jobs`

`200`, at most 4,096 bytes and 16 jobs, oldest first:

```text
x3jobs 1
job 01JB8ZK3Q4M8D6F2T0V9X7W5YH token t-goblin-warrior-0123456789ab 104560 3b2c…(64 hex) show
job 01JB8ZK9A1C2D3E4F5G6H7J8KM article a-7d1q0p4n6r2s 10234 a1f0…(64 hex) -
job 01JB8ZKB0N1P2Q3R4S5T6V7W8X delete a-0c5h8j2k4l6m 0 - -
end more=0
```

| Field | Rule |
| --- | --- |
| job | ULID: 26 characters, Crockford base32, upper case |
| kind | `token`, `image`, `article` or `delete` |
| item | 1–48 bytes of `[A-Za-z0-9_-]`. The companion uses `t-…` (tokens), `i-…` (images), `a-…` (articles); a named token or image is `t-<slug>-<12 hex>`, and the device shows the slug as its name |
| len | payload length in bytes; `0` for delete |
| sha256 | 64 lower-case hex digits; `-` for delete |
| flags | `show` (tokens and images) or `-` |

`end more=1` means more are waiting; the device lists again after the batch. A line with
an unknown kind is skipped and acked `rejected unsupported`. The companion offers `image`
jobs only to a device whose last hello listed `im1`.

### Payload and ack

`GET /device/v1/jobs/{job}/payload` returns `200 application/octet-stream` with exactly
`len` bytes. Serving it starts a 300 s lease and marks the job as started (it can no longer
be cancelled). `404` means the job is gone: discard, do not ack, continue.

`POST /device/v1/jobs/{job}/ack`, one line:

```text
committed <sha256|->
displayed <sha256>
rejected <checksum|too_large|invalid|unsupported|storage>
```

The reply is `204`. Acks are idempotent. A `committed` hash that differs from the job's is
`409`; an unknown or cancelled job is `404`, which the device treats as done. If an ack is
lost, the lease runs out, the job is listed again, the device sees it already holds that
hash, skips the download and acks again: no duplicate.

### Errors

The body is `error <code>`: `400 invalid`, `401 unauthorised`, `404 not_found`,
`409 conflict`, `413 too_large`, `503 unavailable`. The device stops the session on `401`
and `503`; other codes apply to one job.

### Signed requests

When `SERVER.CFG` has an `https://` address, the device uses TLS 1.3 **without checking the
certificate** and signs every request instead. The token is never sent.

- Key: `SHA-256(token)`, 32 bytes. The companion stores the same value.
- Each request carries `X3-Nonce: <32 hex, 16 random bytes>` and
  `Authorization: X3-HMAC <64 hex>`: HMAC-SHA-256 over
  `"x3req1\n" METHOD "\n" PATH "\n" NONCE "\n" hex(SHA-256(body))`. A query string is
  refused.
- Each reply carries `X3-Mac: <64 hex>`: HMAC over
  `"x3res1\n" NONCE "\n" STATUS "\n" hex(SHA-256(body))`. The device reads the whole reply,
  checks the MAC, and acts on nothing before that. A missing or wrong MAC fails the Receive.
- The companion remembers the last 1,024 nonces and refuses a repeat.
- If `access_id` and `access_secret` are set, the device also sends
  `CF-Access-Client-Id` and `CF-Access-Client-Secret` for the login proxy.
- One TLS connection is kept open for the whole session.
- An active attacker who terminates TLS can read what is fetched and can stop the
  session, but cannot forge or change a job or payload, or learn the token.

## Files on the card

All names are 8.3. `key=value` files, at most 512 bytes, unknown keys rejected. Samples are
in [`config/`](../config).

| File | Content |
| --- | --- |
| `X3/WIFI.CFG` | up to four `ssid=` / `psk=` line pairs (SSID 1–32 bytes, password 8–63) |
| `X3/SERVER.CFG` | `url=`, optional `lan=`, `token=` (43 characters), optional `access_id=` and `access_secret=` |
| `X3/WIFI.PIN` | the network Receive tries first |
| `X3/SLEEP.CFG` | the item shown while asleep |
| `X3/SHOWN.CFG` | a `displayed` ack waiting for the next session |
| `X3/LOG.TXT` | one line per Receive |
| `X3/DATA/` | `NNNNNNNN.DAT` payloads and `NNNNNNNN.MET` 112-byte records, `NNNNNNNN` a hex generation number |

**Storage rules.** File names come only from generation numbers, never from titles, URLs or
item IDs. A download goes to a new generation; after length, SHA-256, header and CRC are
checked, a new metadata record makes it current. The newest record for an item wins, and a
delete record hides it. A reading position is a new record pointing at the same payload.
A power cut leaves at most an unfinished generation, which is ignored and removed by the
next Receive's clean-up. Replacing an article's text resets its position.

## Companion API

### Login

Every request to `/api/v1/*` and the web page needs a valid `Cf-Access-Jwt-Assertion`
(RS256, checked against the Access team's keys, issuer and audience, expiry and
not-before). A browser gets it from the Access login; a script can send an Access service
token. Other identity headers are ignored.

### Routes

| Method and path | Purpose | Success |
| --- | --- | --- |
| `POST /api/v1/articles` | an article or summary as text | 202 new, 200 repeat |
| `POST /api/v1/articles/link` | an article the companion reads from a web page | 202, 200 |
| `POST /api/v1/tokens` | a token from a Scryfall card | 202, 200 |
| `POST /api/v1/images?…` | a picture; the body is the image | 202, 200 |
| `POST /api/v1/images/preview.png?fit=&orientation=` | preview a picture; queues nothing | 200 PNG |
| `GET /api/v1/tokens/preview.png?…` | preview a token; queues nothing | 200 PNG |
| `GET /api/v1/scryfall/search?q=` | token search, proxied and cached | 200 |
| `GET /api/v1/jobs?limit=&before=` | jobs, newest first, with `more` | 200 |
| `GET /api/v1/jobs/{job}` | one job with its history | 200 |
| `GET /api/v1/jobs/{job}/preview.png` | the exact device pixels of a job | 200 PNG |
| `POST /api/v1/jobs/{job}/cancel` | cancel before the download started | 200, or 409 |
| `DELETE /api/v1/items/{item}` | queue a delete job | 202 |
| `GET /api/v1/saved`, `POST /api/v1/saved/{id}/send`, `DELETE /api/v1/saved/{id}` | the saved shelf: list, send again, forget | 200 |
| `GET /api/v1/device` | last contact, firmware, formats | 200 |

**Bodies.** JSON, at most 1 MiB, unknown and duplicate fields rejected:

```json
{"submission_id":"<uuid>","kind":"article","title":"A title",
 "source_url":"https://example.org/a","text":"# Heading\n\nParagraph.","item":null}
{"submission_id":"<uuid>","kind":"article","url":"https://example.org/a","item":null}
{"submission_id":"<uuid>","scryfall_id":"<uuid>","face":0,"fit":"contain",
 "orientation":"auto","show":true,"name":"Goblin Warrior","expected_sha256":"<64 hex>"}
```

`kind` is `article` or `summary`; `item` replaces an existing article. A link must be
`http` or `https` on the default port, with no credentials and no private address; the
companion refuses private, loopback, link-local and similar addresses when it resolves the
name, so redirects cannot reach them either.

**Pictures:** `POST /api/v1/images?submission_id=&fit=&orientation=` with optional `show`,
`name` and `expected_sha256`. The body is the image (JPEG, PNG, WebP or GIF, at most 8 MiB,
at most 4,096 px a side).

**Previews** return the exact pixels as PNG and the payload's SHA-256 in
`X3-Payload-Sha256`. Sending that hash as `expected_sha256` makes the submit fail with
`409` if the result would differ (for example, Scryfall changed the image).

**Idempotency.** `submission_id` is a client UUID kept across retries. Same ID and same
body: `200` with the first job. Same ID and a different body: `409`.

**Errors:** `{"error":"<code>"}`: `400 invalid_request`, `401 unauthorised`,
`404 not_found`, `409 conflict`, `413 too_large`, `415 unsupported_media_type`,
`422 conversion_failed`, `502 upstream_failed`, `503 busy`, `507 storage_full`. Errors
never echo submitted text or credentials.

### Job states

`accepted → preparing → ready → sending → delivered → displayed`, plus `failed` and
`cancelled`. `sending` returns to `ready` when its lease ends. A failed fetch during
preparation is retried up to five times. Old finished jobs are pruned after 30 days;
waiting jobs never are.

## Payload formats

Integers are little-endian. CRC32 is CRC-32/ISO-HDLC. The job's SHA-256 covers the whole
payload, header included.

### Token `tk1` and picture `im1`

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 4 | `X3TK` |
| 4 | 1 | version `1` |
| 5 | 1 | bits per pixel: `1` or `2` |
| 6 | 2 | width: 792 (landscape) or 528 (portrait) |
| 8 | 2 | height: 528 or 792 |
| 10 | 2 | flags: `0` |
| 12 | 4 | CRC32 of the pixels |
| 16 | … | pixels, row-major from the top left, MSB first |

At 1 bpp, 0 is black and 1 white (52,288 bytes). At 2 bpp, 0 black, 1 dark grey, 2 light
grey, 3 white, four pixels per byte (104,560 bytes). The companion sends 2 bpp. Portrait
images are rotated on the device. `im1` has exactly the same layout; the separate name lets
a device announce that it accepts pictures other than tokens.

### Article `ar1` (at most 264,476 bytes)

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 4 | `X3AR` |
| 4 | 1 | version `1` |
| 5 | 1 | kind: `0` article, `1` summary |
| 6 | 2 | flags: `0` |
| 8 | 2 | title length, 1–256 |
| 10 | 2 | source URL length, 0–2,048 |
| 12 | 4 | text length, 0–262,144 |
| 16 | 8 | fetched at, Unix seconds |
| 24 | 4 | CRC32 of everything after the header |
| 28 | … | title, source URL, text; UTF-8 |

The text is plain: a blank line separates paragraphs, a line starting `# ` is a heading,
U+00AD is a soft hyphen (the companion hyphenates English and Polish). Tabs and newlines
are the only control characters allowed. The URL is shown, never fetched by the device.
