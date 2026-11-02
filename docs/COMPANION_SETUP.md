# Companion setup

The companion (`x3-companion`) is an optional program for a home server. It keeps the
articles and tokens you send until you press Receive on the X3, and it does the heavy
work (reading a web page, making the token picture). The X3 never runs a server.

Without a companion the reader still works, but nothing new can arrive.

```text
phone or browser ──► (Cloudflare Tunnel + Access) ──► Caddy ──► companion :8787
X3, on Receive ─────────────────────────────────────────────► companion :8788  (LAN only)
```

Two listeners, on purpose:

- **Public listener, port 8787:** the web page and `/api/v1/*`. Reach it only through
  something that applies login. The companion accepts **Cloudflare Access** tokens and
  nothing else in normal mode, so you need a Cloudflare account, a Tunnel and an
  Access application. Caddy (or any reverse proxy) sits in between.
- **Device listener, port 8788:** `/device/v1/*` for the X3, authenticated by a
  per-device token. Plain HTTP on your home LAN. **Never put it on the internet.**

## What you need

- A machine that is always on, on the same network as the X3, with Docker.
- A Cloudflare account with a Tunnel and an Access application for the public hostname.
  Note the Access **team domain** (`yourteam.cloudflareaccess.com`) and the application's
  **audience tag**.
- A reverse proxy in front of port 8787. A Caddy example is in
  [infra/caddy/x3-companion.example.caddy](../infra/caddy/x3-companion.example.caddy);
  it also blocks `/device/*` on the public side.

## 1. Build the image

```text
docker build -f companion/Dockerfile -t x3-companion:local .
```

## 2. Choose a private data directory

A folder only you can read, for example `/srv/x3-companion`. It holds the inbox
database, which includes the device token's hash. Back it up (`x3-companion backup
PATH` writes a consistent copy).

## 3. Register the X3

```text
docker run --rm -v /srv/x3-companion:/data x3-companion:local device add x3
```

It prints a 43-character token **once**. Put it in `X3/SERVER.CFG` on the SD card
([INSTALL.md](INSTALL.md), step 5). The command refuses to add a second device.

## 4. Run it

Copy [infra/companion/compose.example.yml](../infra/companion/compose.example.yml) and
set these (an env file is fine; keep it out of git):

| Variable | Meaning |
| --- | --- |
| `X3_DATA_DIR` | your private data directory (absolute path) |
| `X3_ACCESS_TEAM` | your `yourteam.cloudflareaccess.com` hostname |
| `X3_ACCESS_AUD` | the Access application's audience tag |
| `LAN_IP` | the server's LAN address; the device listener is published only there |
| `CADDY_NETWORK` | the Docker network your Caddy uses |
| `X3_QUOTA_BYTES` | optional storage cap (default 512 MiB) |

```text
docker compose -f infra/companion/compose.example.yml up -d
```

The container runs read-only, as an unprivileged user, with no extra capabilities.

## 5. Check it

1. From your phone, open the public hostname. You should reach the Cloudflare Access
   login, then the companion's page.
2. Send an article. On the X3 choose **Receive**: the result page should say `1 new`.
3. From another machine on the LAN, `http://LAN_IP:8788/device/v1/jobs` without a token
   must answer `401`.

## Reaching the companion from any network (ADR 026)

On the home network the X3 uses the LAN listener. Away from home (a phone hotspot, a
café) it uses a second route over the internet. The X3 does not verify the server's
certificate. Instead every request and every reply is signed with a key derived from the
device token, so a person on the same Wi-Fi can read what is fetched but cannot forge a
job, read the token, or change an article or token picture. See
[PROTOCOL.md](PROTOCOL.md) A.8 and [SECURITY.md](SECURITY.md).

```text
X3, anywhere ──TLS──► Cloudflare (Access: service token) ──► Tunnel ──► Caddy ──► companion :8789
```

1. **Run the signed listener.** Set `X3_SIGNED_BIND=0.0.0.0:8789` (the compose example does).
   It accepts signed requests only. Do not publish the port on the host.
2. **Add a hostname for the X3**, for example `device.example.com`, to your Tunnel, with the
   same origin as the reader hostname. Use the block in
   [infra/caddy/x3-device.example.caddy](../infra/caddy/x3-device.example.caddy): it passes only
   `/device/v1/*`, and only to port 8789.
3. **Protect it with Access, without a bypass.** Create an Access application for that
   hostname with one policy, *Service Auth*, and create a **service token** for it. Choose a
   long expiry and note the date. Keep the Client ID and Client Secret private.
4. **Put the route on the X3.** Edit `X3/SERVER.CFG` on the card:

```text
url=https://device.example.com
lan=http://192.168.1.20:8788
token=<the device token>
access_id=<service token Client ID>
access_secret=<service token Client Secret>
```

   `lan=` is optional. When it is there, the X3 tries it first, so Receive at home does not
   need the internet. If it does not answer, the X3 uses the `https://` address for that
   session.
5. **Check it.** Join a phone hotspot (Device, Left, add the hotspot network), then Receive. The
   result page should say how many items arrived. Without the service token headers, a
   request to the hostname must be refused by Access.

If the service token expires, Receive away from home fails with "The companion refused the
device token" or a reply that is not signed. Make a new token in Cloudflare and edit the card.

## Trying it without Cloudflare

For local tests only, `x3-companion serve --dev-insecure` takes a static bearer token
(`X3_DEV_TOKEN`, 32 or more characters) and **refuses to start unless both listeners
are on loopback**. It is not a way to run a real deployment.

## Sending things

The web page and the API are described in [PROTOCOL.md](PROTOCOL.md). Packaged iPhone
For the iPhone Shortcuts (Share Sheet and summaries), see [SHORTCUTS.md](SHORTCUTS.md).
