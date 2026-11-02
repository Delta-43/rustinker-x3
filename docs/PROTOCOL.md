# Protocol v2 — companion and device contracts

Status: **approved by the owner on 2026-10-22 (ADR 009), amended by ADR 010 and ADR 012**. Implement this. Change
it only through a new ADR. Version 1 (device push to a device HTTP server) is
withdrawn; see git history before commit b90fe79 for the old draft.

There are two separate contracts:

1. **Device API** (section A). The X3 is the HTTP client. It pulls jobs from the
   companion's LAN listener during a Receive session. Plain HTTP on the trusted
   home LAN, with a per-device bearer token.
2. **Companion API** (section B). Browser and iPhone Shortcut call it over HTTPS
   through Cloudflare Tunnel, Access and Caddy. JSON.

Payload formats (section C) are shared by the companion, tools, simulator and
firmware. Parsers live in `crates/proto` (no_std, no allocation).

## A. Device API (X3 → companion, LAN only)

### A.1 Transport rules

- HTTP/1.1 over TCP, IPv4. The device sends `Connection: close` on every request.
- Companion address comes from the SD config (A.6): `http://<IPv4>:<port>`. No
  hostnames, DNS or mDNS in v1. No redirects: any 3xx is an error.
- Every request carries `Authorization: Bearer <token>`. The token identifies
  the device. Never put it in a URL or log.
- Responses must have `Content-Length`, except `204` (ADR 012): a 204 must have
  no body, no `Transfer-Encoding`, and either no `Content-Length` or
  `Content-Length: 0`; any other value is an error. The device rejects chunked responses,
  `Content-Encoding`, a status line or header block over 1,024 bytes, and any
  body longer than the cap for that route.
- Device timeouts: connect 5 s, idle read 10 s. A whole Receive session stops
  after 180 s (a setting, 30–600 s), even if jobs remain.
- One request at a time. No request pipelining, no resumable downloads.
- Text bodies are `text/plain; charset=utf-8`, lines end with `\n` (no `\r`),
  fields are separated by one ASCII space. Any line over 192 bytes (excluding
  the `\n`; ADR 010), any control
  character other than `\n`, or any unknown leading word invalidates the whole body.

### A.2 Receive session

```text
button → join Wi-Fi → POST hello → loop { GET jobs → for each job: process → POST ack }
       → stop when the job list is empty, on timeout, or on error → Wi-Fi off
```

Process a job:

| Kind | Device action |
| --- | --- |
| `token` / `article` | If this item already holds this exact SHA-256, skip the download and ack `committed`. Otherwise download the payload into staging on SD while hashing it. Check length, SHA-256 and the payload's own header and CRC. Commit (C.3). Then ack. |
| `delete` | Remove the item and its reading position if present (absent counts as success). Ack `committed -`. |

Token jobs with the `show` flag: after the session ends, the device shows the
last committed `show` token and acks `displayed <sha256>` at the next session's
start (the radio is off by then). Articles never open automatically.

### A.3 `POST /device/v1/hello`

Request body (one line, max 160 bytes excluding `\n`; the longest valid line is
84 bytes), keys in this order:

```text
hello 1 fw=0.1.0 w=792 h=528 formats=tk1,ar1,im1 free_kib=1843200 battery=87
```

`battery` is `0`–`100` or `na`. `formats` lists the payload formats from section C
(`tk1`, `ar1`, `im1`; each at most once, any order). The companion offers `image` jobs
only to a device whose last hello lists `im1` (ADR 028).
Response `200`, body `ok 1 time=<unix seconds>`. The device may set its RTC from
`time` when the difference is over 60 s. The companion records last contact,
firmware version and capabilities.

### A.4 `GET /device/v1/jobs`

Response `200`, max 4,096 bytes, max 16 `job` lines:

```text
x3jobs 1
job 01JB8ZK3Q4M8D6F2T0V9X7W5YH token t-9f3k2m7q1z8x 52300 3b2c…64 hex…9e show
job 01JB8ZK9A1C2D3E4F5G6H7J8KM article a-7d1q0p4n6r2s 10234 a1f0…64 hex…02 -
job 01JB8ZKB0N1P2Q3R4S5T6V7W8X delete a-0c5h8j2k4l6m 0 - -
end more=0
```

Fields of a `job` line:

| Field | Rule |
| --- | --- |
| job | ULID: 26 characters, Crockford base32, upper case |
| kind | `token`, `image`, `article` or `delete` |
| item | Logical item ID: 1–48 bytes of `[A-Za-z0-9_-]`. Companion uses `t-…` for tokens, `i-…` for images, `a-…` for articles. A named token is `t-<name slug>-<12 hex>` (e.g. `t-goblin-warrior-0123456789ab`), a named image `i-<name slug>-<12 hex>`; the device shows the slug as its name. Devices must still accept any valid ID |
| len | Decimal payload length; `0` for delete; at most the cap in section C |
| sha256 | 64 lower-case hex digits; `-` for delete |
| flags | `show` (tokens and images only) or `-` |

`end more=1` means more jobs are waiting; the device lists again after the
batch. The companion orders jobs oldest first. An unknown `kind` in an otherwise
valid line: the device skips it and acks `rejected unsupported`.

### A.5 `GET /device/v1/jobs/{job}/payload` and `POST /device/v1/jobs/{job}/ack`

Payload: `200 application/octet-stream`, `Content-Length` equal to `len`. Serving
it marks the job `sending` with a 300 s lease and, in the same transaction, sets
the job's `delivery_started` flag (permanent; ADR 012). `404` means the job was
cancelled or no longer exists: discard staging, do not ack, continue.

Ack body, one line:

```text
committed <sha256|->
displayed <sha256>
rejected <checksum|too_large|invalid|unsupported|storage>
```

Response `204` with no body (no `Content-Length`; see A.1). Acks are
idempotent: repeating the same ack returns `204`.
A `committed` ack whose hash differs from the job's returns `409`. `404` for an
unknown or cancelled job; the device treats it as done. (A device that skipped
the download because it already held the hash may ack a job cancelled in the
meantime; the 404 is correct, as that content came from an earlier delivery.)

**Lost ack.** If the ack response never arrives, the job lease expires and the
job is listed again. The device sees it already holds that hash for that item,
skips the download and acks again. One logical item, no duplicate.

### A.8 Signed requests (internet route, ADR 026)

When `SERVER.CFG` has an `https://` address, the device uses TLS 1.3 to port 443 (or the
given port) **without checking the certificate** and signs instead. The token is never sent.

- Key: `SHA-256(token)` (32 bytes). The companion already stores this value.
- Each request carries `X3-Nonce: <32 lower-case hex, 16 random bytes>` and
  `Authorization: X3-HMAC <64 hex>`, where the MAC is HMAC-SHA-256 over
  `"x3req1\n" METHOD "\n" PATH "\n" NONCE "\n" hex(SHA-256(body))`. `PATH` is the exact path,
  and a request with a query string is refused. If an Access service token is configured, the
  headers `CF-Access-Client-Id` and `CF-Access-Client-Secret` are sent too.
- Each reply carries `X3-Mac: <64 hex>`, the HMAC over
  `"x3res1\n" NONCE "\n" STATUS "\n" hex(SHA-256(reply body))`. The device checks it
  after reading the whole reply and treats a missing or wrong MAC as a failed Receive. Nothing
  from the reply is committed or acted on before that. Replies to a bad request (401) carry no MAC.
- The companion remembers the last 1,024 nonces and refuses a repeat.
- The connection is kept open for the whole session (HTTP keep-alive); the device closes it at
  the end. A TLS handshake takes about a second, so a handshake per request would be slow.
- The signed listener (`X3_SIGNED_BIND`) accepts signed requests only. The LAN listener keeps
  accepting only `Authorization: Bearer`.
- What this does not give: confidentiality against an active attacker who terminates the TLS
  connection. They can read the articles and tokens being fetched, and can stop the session.

### A.6 Device files on SD

```text
/x3/wifi.cfg        ssid=<1–32 bytes>\npsk=<8–63 bytes>\n
/x3/server.cfg      url=http://192.168.1.20:8788\ntoken=<43 chars base64url>\n
/x3/data/           articles and tokens: NNNNNNNN.DAT payloads and NNNNNNNN.MET
                    112-byte records (x3_core::storage); generation NNNNNNNN in hex
```

`/x3/data` holds only generation files. A reading position is a new `.MET`
generation pointing at the same payload; the newest record per item wins and
a deleted record hides the item. `x3-tools put-article DIR ID FILE.ar1` writes
the same files into a mounted card. Old generations are not yet collected.

File names are 8.3 (the device opens short names only). `key=value` lines,
unknown keys rejected, max 512 bytes per file. Samples live in
`config/*.example.cfg`. Real files are never committed. If `server.cfg` is
missing, the Receive button explains that no companion is configured.

### A.7 Errors from the companion's device listener

Status code only matters; the body is `error <code>`.
`400 invalid`, `401 unauthorised`, `404 not_found`, `409 conflict`,
`413 too_large`, `503 unavailable`. The device treats `401` and `503` as
"stop the session and show the reason"; others are per job.

## B. Companion API (browser and Shortcut → companion, HTTPS)

### B.1 Authentication and listeners

- The **public listener** serves the UI and `/api/v1/*`. It is reached only
  through Cloudflare Tunnel → Caddy on the Docker network, never a host port.
  Every request must carry a valid `Cf-Access-Jwt-Assertion` (RS256, verified
  against the team's JWKS and the application's audience tag). Browser users get
  it from an Access login; the Shortcut gets it by sending an Access service
  token (`CF-Access-Client-Id`, `CF-Access-Client-Secret`). Never trust other
  identity headers. A `--dev-insecure` flag allows a static bearer for local
  tests and refuses to start on a non-loopback address.
- The **device listener** serves only `/device/v1/*` on the server's reserved
  LAN address. Device bearer tokens are stored as SHA-256 hashes.
  `x3-companion device add <name>` prints a new token once.

### B.2 Routes

| Method and path | Purpose | Success |
| --- | --- | --- |
| POST /api/v1/articles | Submit an article or summary | 202 new job, 200 repeat |
| POST /api/v1/articles/link | Submit an article the companion reads from a web page (ADR 015) | 202 new job, 200 repeat |
| POST /api/v1/tokens | Queue a token from a Scryfall card | 202 new job, 200 repeat |
| POST /api/v1/images?… | Queue an uploaded picture (ADR 028); body is the image | 202 new job, 200 repeat |
| POST /api/v1/images/preview.png?fit=&orientation= | Preview an uploaded picture; creates nothing | 200 image/png |
| GET /api/v1/scryfall/search?q= | Card search, proxied and cached | 200 |
| GET /api/v1/jobs?state=&limit= | Job list, newest first, limit 1–100 | 200 |
| GET /api/v1/jobs/{job} | One job with state history | 200 |
| GET /api/v1/tokens/preview.png?… | Preview a token before queueing; creates nothing | 200 image/png |
| GET /api/v1/jobs/{job}/preview.png | Exact device pixels of a queued job | 200 image/png |
| POST /api/v1/jobs/{job}/cancel | Cancel before delivery has started | 200, or 409 |
| DELETE /api/v1/items/{item} | Queue a device delete job | 202 |
| GET /api/v1/device | Last contact, firmware, capabilities | 200 |

Article request (max 1 MiB encoded JSON; unknown and duplicate fields rejected):

```json
{"submission_id":"6f1c…uuid","kind":"article","title":"Zażółć gęślą jaźń",
 "source_url":"https://example.org/a","text":"# Heading\n\nParagraph.","item":null}
```

`item` is optional: give an existing article item to replace its content.
Token request: `{"submission_id", "scryfall_id", "face": 0, "fit": "contain|cover",
"orientation": "auto|landscape|portrait", "show": true}`. Custom image upload is
not in v1.

Token requests may add `"expected_sha256"` (64 lower-case hex). If the payload
the companion produces differs (for example Scryfall replaced the image since
the preview), the request fails with `409` and nothing is queued.

Article from a link (ADR 015): `{"submission_id", "kind": "article|summary",
"url", "item": null}`. The URL must be `http` or `https` on the default port,
with no user name, password or private host. The companion fetches the page in
the background: an upstream or 5xx failure retries like a token; an unreadable
page (4xx, not HTML or text, too little text) fails the job. The job title is
the URL until the page's title is read.

Token requests may also carry an optional `"name"` (card name, at most 200
bytes, no control characters). It is the job's display title only.

Response for all: `{"job":"01J…","item":"a-…","state":"accepted"}`. Job objects
in `GET /api/v1/jobs` include `title` (article title, card name, link, or the
removed item's title for a delete job; may be null for older jobs).

**Token preview (ADR 012).** `GET /api/v1/tokens/preview.png?scryfall_id=<uuid>&face=<0|1>&fit=<contain|cover>&orientation=<auto|landscape|portrait>`.
Same authentication, validation and limits as token submission; unknown or
repeated query parameters return `400`. Returns the exact converted pixels as
PNG and the `tk1` payload's SHA-256 in the `X3-Payload-Sha256` header. It creates
no job, submission or item. It shares the Scryfall cache and throttle with
submission. At most 2 conversions run at once across preview and submission;
extra requests wait briefly, then return `503`. The UI enables "Queue" only after
a preview succeeds and sends that hash as `expected_sha256`.

**Cancellation (ADR 012).** Cancel succeeds only while `delivery_started` is
unset. For token and article jobs the flag is set when a payload response is
first authorised; for delete jobs, when the job first appears in a jobs
response. It never clears, even after a lease expires, because the device may
already hold the bytes. Before that point cancel is idempotent (repeat → `200`),
the job leaves job listings, and payload requests return `404`. Afterwards
cancel returns `409 conflict`; the UI then offers "Remove from device", which
queues a delete job via `DELETE /api/v1/items/{item}`.

**Idempotency.** `submission_id` is a client UUID that the Shortcut or page keeps
across retries. Same ID and same canonical body → `200` with the original job.
Same ID and a different body → `409`. Nothing is returned as accepted until the
SQLite transaction holding the job and content has committed.

Errors: `{"error":"<code>"}` with `400 invalid_request`, `401 unauthorised`,
`404 not_found`, `409 conflict`, `413 too_large`, `503 busy` (conversion limit), `415 unsupported_media_type`,
`422 conversion_failed`, `502 upstream_failed` (Scryfall), `507 storage_full`.
Never echo submitted text or credentials in errors or logs.

### B.3 Job states

`accepted → preparing → ready → sending → delivered → displayed`, plus
`failed` and `cancelled`. `sending` returns to `ready` when its 300 s lease
expires. `delivered` needs a `committed` ack; `displayed` needs a `displayed`
ack. Pending jobs are never removed by retention. Retention keeps each item's
newest delivered job, without its payload, so the item can still be replaced or
removed (ADR 014). An upstream or capacity failure during preparation returns
the job to `accepted` for up to five attempts before `failed`.

### B.4 Scryfall

The companion alone calls `api.scryfall.com` and fetches images from
`cards.scryfall.io` (allowlist; no other image hosts). It sends the headers
Scryfall requires, caches responses and images, and throttles requests. Verify
Scryfall's current API rules and image terms before Phase 7 sign-off. No card
images are stored in the repo.

## C. Payload formats

All integers little-endian. CRC32 is CRC-32/ISO-HDLC. The SHA-256 in the job list
covers the whole payload, header included.

### C.1 Token `tk1` (52,288 bytes at 1 bpp, 104,560 at 2 bpp)

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 4 | `X3TK` |
| 4 | 1 | version `1` |
| 5 | 1 | bits per pixel: `1` or `2` (ADR 018); reject anything else |
| 6 | 2 | width: 792 (landscape) or 528 (portrait) |
| 8 | 2 | height: 528 or 792; width × height must be 792 × 528 |
| 10 | 2 | flags: `0` |
| 12 | 4 | CRC32 of the pixel bytes |
| 16 | 52,272 or 104,544 | pixels, row-major, top-left first, MSB first. 1 bpp: 0 black, 1 white. 2 bpp: 0 black, 1 dark grey, 2 light grey, 3 white, four pixels per byte |

The payload length must equal 16 + 792 × 528 × bpp / 8. The companion sends
2 bpp (even grey steps, ADR 018); devices also accept 1 bpp, so tokens queued
before ADR 018 still deliver. A worst-case token job line is now 163 bytes
including LF, within the 192-byte line cap.

The device rotates portrait tokens for display. The companion fits the card art
with aspect ratio kept (`contain` pads white, `cover` crops centre).


### C.1a Picture `im1` (ADR 028)

An `image` job carries an `im1` payload. Its bytes are laid out exactly as `tk1` (same
`X3TK` header, caps, CRC and checks), so a device decodes and stores both the same way and
lists them together as pictures. The separate name lets a device say it can take pictures
other than tokens; the companion offers `image` jobs only to devices whose hello lists `im1`.
Older firmware never sees them; if one reached it, it answers `rejected unsupported`.

**Upload.** `POST /api/v1/images?submission_id=<uuid>&fit=<contain|cover>&orientation=<auto|portrait|landscape>`
with optional `show=<0|1>`, `name=` (1–200 characters, no control characters) and
`expected_sha256=` (the hash a preview returned). The body is the image itself, at most
8 MiB, with `Content-Type` `image/jpeg`, `image/png`, `image/webp` or `image/gif` (anything
else is `415`; an HTML form cannot send these types, which keeps cross-site posts out).
The companion decodes it with limits (4096 px a side, 64 MiB decoded; otherwise `422`),
converts it like a token and stores the finished payload with the job. A repeat with the
same submission ID, settings and image bytes returns the first job (`200`); different bytes
are a conflict (`409`). `POST /api/v1/images/preview.png?fit=&orientation=` takes the same
body and returns the device pixels as PNG with `X3-Payload-Sha256`.

### C.2 Article `ar1` (max 264,476 bytes)

| Offset | Bytes | Field |
| --- | --- | --- |
| 0 | 4 | `X3AR` |
| 4 | 1 | version `1` |
| 5 | 1 | kind: `0` article, `1` summary |
| 6 | 2 | flags: `0` |
| 8 | 2 | title length, 1–256 |
| 10 | 2 | source URL length, 0–2,048 |
| 12 | 4 | text length, 0–262,144 |
| 16 | 8 | fetched_at, Unix seconds, at most 253,402,300,799 |
| 24 | 4 | CRC32 of every byte after the header |
| 28 | … | title, then source URL, then text, all UTF-8 |

Title and URL: no control characters. URL is empty or absolute `http`/`https`;
display only, never fetched by the device. Text: newline and tab allowed, other
controls rejected. A blank line separates paragraphs; a line starting `# ` is a
heading; U+00AD is a soft hyphen. No HTML. Total length must equal 28 plus the
three lengths. The companion normalises line endings to `\n` and strips other
controls before encoding.

### C.3 Device storage rules

- Physical SD file names derive from validated item IDs, never titles or URLs.
- Download into a staging file; verify length, SHA-256, header and CRC; then
  commit by writing a new metadata generation that points at the new payload.
  The previous payload stays until the new generation is written and verified.
  The index can be rebuilt from validated payload files.
- Replacing an article's content resets its reading position to the start.
- Token cache keeps the last 20 tokens (setting); eviction never removes the
  token currently on screen.
