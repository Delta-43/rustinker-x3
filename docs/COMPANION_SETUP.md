# Companion setup

`x3-companion` is an optional server you run at home. It keeps what you send until you
press **Receive** on the X3, and it does the heavy work: reading a web page, laying out an
article, converting a card or a photo to four greys. The X3 runs no server of its own.

Without a companion the reader still works, but nothing new arrives.

```text
phone or browser ──HTTPS──► login proxy ──► companion :8787   web page and /api/v1
X3, on Receive ──── LAN ──────────────────► companion :8788   /device/v1, device token
X3, away from home ─TLS──► login proxy ───► companion :8789   /device/v1, signed requests
```

| Listener | Serves | Reached from |
| --- | --- | --- |
| **Public**, 8787 | the web page and `/api/v1/*` | only through a proxy that applies login |
| **Device**, 8788 | `/device/v1/*`, plain HTTP with a per-device token | your home LAN only. **Never put it on the internet.** |
| **Signed**, 8789 (optional) | `/device/v1/*`, signed requests only | the proxy, for Receive away from home |

## Login

The public listener accepts **Cloudflare Access** tokens and nothing else. It checks every
request's `Cf-Access-Jwt-Assertion` (RS256, against your team's keys and your
application's audience tag). So you need:

- a Cloudflare account with a **Tunnel** to your server and an **Access application** for
  the hostname you will use;
- the Access **team domain** (`yourteam.cloudflareaccess.com`) and the application's
  **audience tag**;
- a reverse proxy between the Tunnel and port 8787. A Caddy example is in
  [`infra/caddy/x3-companion.example.caddy`](../infra/caddy/x3-companion.example.caddy);
  it also refuses `/device/*` on the public side.

## 1. Build the image

On a machine that is always on, on the same network as the X3, with Docker:

```bash
git clone https://github.com/Delta-43/rustinker-x3.git
cd rustinker-x3
docker build -f companion/Dockerfile -t x3-companion:local .
```

## 2. Choose a data directory

A folder only you can read, for example `/srv/x3-companion`. It holds one SQLite database
with the inbox, the payloads and the hash of the device token.

## 3. Register the X3

```bash
docker run --rm -v /srv/x3-companion:/data x3-companion:local device add x3
```

It prints a 43-character device token **once**. Put it in `X3/SERVER.CFG` on the card
([Install](INSTALL.md#6-prepare-the-sd-card)). One companion serves one X3; a second
`device add` is refused.

## 4. Configure and run

Copy [`infra/companion/compose.example.yml`](../infra/companion/compose.example.yml) and set
these, in an env file kept out of git:

| Variable | Meaning |
| --- | --- |
| `X3_DATA_DIR` | the data directory (absolute path on the host) |
| `X3_ACCESS_TEAM` | your `yourteam.cloudflareaccess.com` domain |
| `X3_ACCESS_AUD` | the Access application's audience tag |
| `LAN_IP` | the server's LAN address; the device listener is published only there |
| `CADDY_NETWORK` | the Docker network your proxy is on |
| `X3_QUOTA_BYTES` | optional cap on stored payloads (default 512 MiB) |

The compose file also sets `X3_PUBLIC_BIND`, `X3_DEVICE_BIND` and `X3_SIGNED_BIND` (the
three listeners). `X3_FONT_DIR` optionally points at a folder with a UI font for the web
page; without it the page uses the system sans.

```bash
docker compose --env-file my.env -f infra/companion/compose.example.yml up -d
```

The container runs read-only, as an unprivileged user, with no extra capabilities.

## 5. Check it

1. Open your hostname on the phone. You should see the Cloudflare Access login, then the
   companion's page.
2. Send an article. On the X3 choose **Receive**: the result page should say 1 new.
3. From another machine on the LAN, `http://LAN_IP:8788/device/v1/jobs` without a token
   must answer `401`.

## Using the web page

| Tab | Sends |
| --- | --- |
| **Link** | an article or summary the companion reads from a web page |
| **Text** | an article you paste: title, optional source link, text |
| **Token** | a Magic: The Gathering token, found by name on Scryfall; choose fit, orientation and, for two-sided tokens, the face |
| **Picture** | your own JPEG, PNG, WebP or GIF (up to 8 MiB), converted to four greys |

Tokens and pictures show a preview of the exact device pixels before you queue them.
**Show** puts the picture on screen as soon as the Receive ends.

The **Inbox** lists the newest 10 jobs and their state; older ones page in with
"Show older". A job that has not started downloading
can be cancelled; after that, "Remove from device" queues a delete instead.
**Saved tokens** and **Saved pictures** keep the last 50 the X3 received, so **Send again**
queues one at once, without Scryfall or conversion.

Card data and images come from [Scryfall](https://scryfall.com). The companion follows its
API rules: it identifies itself, keeps to the per-endpoint rate limits, caches results for
24 h, and never crops a card's artist or copyright line.

## Receive away from home

At home the X3 uses the LAN listener. Elsewhere (a phone hotspot, a café) it can use the
signed listener over the internet. The X3 does not check the server's certificate; instead
every request and reply is signed with a key derived from the device token, and the token
itself is never sent ([Protocol](PROTOCOL.md#signed-requests), [Security](SECURITY.md)).

1. **Keep the signed listener internal.** The compose file binds it on the Docker
   network only. Do not publish port 8789 on the host.
2. **Add a hostname for the X3** to your Tunnel, for example `device.example.org`, with the
   proxy block in [`infra/caddy/x3-device.example.caddy`](../infra/caddy/x3-device.example.caddy).
   It passes only `/device/v1/*`, only to port 8789.
3. **Protect it with Access, no bypass:** an Access application for that hostname with one
   *Service Auth* policy, and a **service token** for it. Note its expiry date.
4. **Add the route to the card.** `X3/SERVER.CFG`:

   ```text
   url=https://device.example.org
   lan=http://192.168.1.20:8788
   token=<the device token>
   access_id=<service token Client ID>
   access_secret=<service token Client Secret>
   ```

   With `lan=` the X3 tries the LAN address first, so Receive at home does not need the
   internet.
5. **Check it:** join a phone hotspot (Device → Wi-Fi → Add a network), then Receive.

When the service token expires, Receive away from home fails with "The companion refused
the device token". Make a new service token and update the card.

## Backups and updates

```bash
docker exec <container> x3-companion backup /data/backup.sqlite
docker exec <container> x3-companion restore-check /data/backup.sqlite
```

`backup` writes a consistent copy while the companion runs; `restore-check` opens a copy
and counts its payloads, so you know it is usable. To update, pull the new source, rebuild
the image and run `docker compose up -d` again. The database upgrades itself; an older
image refuses a newer database, so back up before an update.

## Trying it locally

`x3-companion serve --dev-insecure` accepts a static bearer token (`X3_DEV_TOKEN`, 32 or
more characters) instead of Access, and **refuses to start unless every listener is on
loopback**. It is for development only.
