#!/usr/bin/env bash
# A throwaway companion for the Phase 6 power-cut drill. Never touches the
# real one: its own container, its own empty volume, its own tokens.
#
#   drill-companion.sh up         start it, print the device token's file
#   drill-companion.sh queue N    queue N large articles (about 120 KB of text)
#   drill-companion.sh summaries  queue 2 summaries (kind "summary", short text)
#   drill-companion.sh status     jobs by state
#   drill-companion.sh down       stop it and delete everything it made
#
# Layout. `serve --dev-insecure` insists on loopback listeners, because its
# public API takes a static test bearer and no Cloudflare Access. So both its
# ports stay on the container's own loopback, and a small relay container that
# shares its network namespace forwards two ports out:
#   device port -> published on the LAN address 192.168.1.20:18789, through
#                  Docker's own port publishing like the real companion's 8788
#                  (a plain host listener timed out from the device, most likely a host
#                  firewall rule; not verified, no root here);
#   public API  -> published on 127.0.0.1:18790 only, for this script.
# The device port is authenticated by a per-device bearer token, as in
# production. The insecure API is never reachable from the network.
#
# Secrets (the device token and the test bearer) live in $X3_DRILL_DIR
# (default ~/.cache/x3-drill, mode 700), never in the repository.
set -euo pipefail

NAME=x3-companion-drill
VOLUME=x3-drill-data
IMAGE=${X3_IMAGE:-x3-companion:local}
STATE=${X3_DRILL_DIR:-$HOME/.cache/x3-drill}
LAN_IP=${LAN_IP:-192.168.1.20}
LAN_PORT=18789
RELAY=${X3_RELAY_IMAGE:-n8nio/n8n:latest}   # any image with node on this machine
API=127.0.0.1:18790

api() { # METHOD PATH [JSON]
  curl -sS -X "$1" "http://$API$2" \
    -H "Authorization: Bearer $(cat "$STATE/dev.token")" \
    -H 'Content-Type: application/json' ${3:+--data "$3"}
}

case "${1:-}" in
up)
  [ -e "$STATE/dev.token" ] && { echo "already up: run 'down' first" >&2; exit 1; }
  umask 077
  mkdir -p "$STATE"
  docker volume create "$VOLUME" >/dev/null
  docker run --rm -v "$VOLUME:/data" "$IMAGE" device add drill >"$STATE/device.token"
  head -c 32 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n' >"$STATE/dev.token"
  docker run -d --name "$NAME" --init --read-only --tmpfs /tmp \
    --cap-drop ALL --security-opt no-new-privileges \
    -p "$LAN_IP:$LAN_PORT:$LAN_PORT" -p "127.0.0.1:18790:18790" \
    -e X3_DATA_DIR=/data -e X3_PUBLIC_BIND=127.0.0.1:18787 -e X3_DEVICE_BIND=127.0.0.1:18788 \
    -e X3_DEV_TOKEN="$(cat "$STATE/dev.token")" \
    -v "$VOLUME:/data" "$IMAGE" serve --dev-insecure >/dev/null
  # The relay shares the companion's network namespace.
  docker run -d --name "$NAME-relay" --network "container:$NAME" --read-only \
    --cap-drop ALL --security-opt no-new-privileges --entrypoint node "$RELAY" -e '
const net = require("net");
const fwd = (from, to) => net.createServer((c) => {
  const u = net.connect(to, "127.0.0.1");
  c.pipe(u); u.pipe(c);
  c.on("error", () => u.destroy()); u.on("error", () => c.destroy());
}).listen(from, "0.0.0.0");
fwd(18789, 18788);   // device port
fwd(18790, 18787);   // public API, published on host loopback only
' >/dev/null
  sleep 2
  docker logs "$NAME" 2>&1 | tail -2
  echo "device token file: $STATE/device.token (43 characters)"
  ;;
queue)
  n=${2:?how many}
  for i in $(seq 1 "$n"); do
    body=$(python3 - "$i" <<'PY'
import json, sys, uuid, random
i = int(sys.argv[1]); rng = random.Random(i)
words = "the quick brown fox jumps over a lazy dog while quiet rivers carry small boats past old stone bridges".split()
paras = []
size = 0
while size < 120_000:
    p = " ".join(rng.choice(words) for _ in range(rng.randint(60, 140))).capitalize() + "."
    paras.append(p); size += len(p) + 2
print(json.dumps({"submission_id": str(uuid.uuid4()), "kind": "article",
                  "title": f"Drill article {i:02d}", "source_url": f"https://example.org/drill/{i}",
                  "text": "\n\n".join(paras), "item": None}))
PY
)
    printf 'article %02d: ' "$i"; api POST /api/v1/articles "$body"; echo
  done
  ;;
summaries)
  for i in 1 2; do
    body=$(python3 - "$i" <<'PY'
import json, sys, uuid
i = int(sys.argv[1])
text = "\n\n".join([
    f"Summary {i}: three points from a longer message, shortened for the small screen.",
    "The meeting moved to Thursday. Bring the draft budget and the revised timeline.",
    "Zażółć gęślą jaźń: Polish letters, curly quotes “like this” and a dash – all survive.",
    "Next step: reply by Friday if the new date does not work.",
])
print(json.dumps({"submission_id": str(uuid.uuid4()), "kind": "summary",
                  "title": f"Drill summary {i}", "source_url": f"https://example.org/summary/{i}",
                  "text": text, "item": None}))
PY
)
    printf 'summary %d: ' "$i"; api POST /api/v1/articles "$body"; echo
  done
  ;;
status)
  api GET '/api/v1/jobs?limit=100' | python3 -c '
import json, sys, collections
jobs = json.load(sys.stdin)
jobs = jobs.get("jobs", jobs) if isinstance(jobs, dict) else jobs
print(dict(collections.Counter(j.get("state") for j in jobs)), len(jobs), "jobs")'
  ;;
down)
  docker rm -f "$NAME-relay" "$NAME" >/dev/null 2>&1 || true
  docker volume rm "$VOLUME" >/dev/null 2>&1 || true
  rm -rf "$STATE"
  echo "removed the test companion, its volume and its tokens"
  ;;
*)
  sed -n 2,13p "$0"; exit 2 ;;
esac
