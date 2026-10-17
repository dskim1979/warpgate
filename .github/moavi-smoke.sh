#!/usr/bin/env bash
# Smoke test for a Warpgate image (MOAVI build).
#   moavi-smoke.sh <image> <name> expect-ok|expect-reject
# expect-ok:     container starts, /@warpgate answers 200, the embedded web UI carries
#                the Korean strings, and an RDP client sending keyboardType 8 is NOT
#                rejected with a ClientCoreData keyboardType decode error.
# expect-reject: only the RDP part; the decode error MUST appear (control run against
#                the unpatched upstream image, proving the probe reaches GCC decoding).
set -euo pipefail

image="$1"
name="wg-smoke-$2"
mode="$3"
http_port=18888
rdp_port=13389
[ "$mode" = expect-reject ] && { http_port=28888; rdp_port=23389; }

pw="$(openssl rand -hex 16)"
echo "::add-mask::$pw"

docker rm -f "$name" >/dev/null 2>&1 || true
docker volume rm -f "$name-data" >/dev/null 2>&1 || true
docker volume create "$name-data" >/dev/null

docker run --rm -v "$name-data:/data" -e WARPGATE_ADMIN_PASSWORD="$pw" "$image" \
    unattended-setup --data-path /data --http-port 8888 --ssh-port 2222 --rdp-port 3389 \
    --record-sessions --external-host 127.0.0.1

docker run -d --name "$name" -v "$name-data:/data" \
    -p "127.0.0.1:$http_port:8888" -p "127.0.0.1:$rdp_port:3389" "$image" run >/dev/null

up=0
for _ in $(seq 1 60); do
    if curl -skf "https://127.0.0.1:$http_port/@warpgate/api/info" >/dev/null; then up=1; break; fi
    sleep 1
done
if [ "$up" != 1 ]; then
    docker logs "$name" || true
    echo "::error::$image did not come up"
    exit 1
fi
echo "== $image is up"

if [ "$mode" = expect-ok ]; then
    code="$(curl -sk -o /tmp/wg-index.html -w '%{http_code}' -L "https://127.0.0.1:$http_port/@warpgate")"
    echo "GET /@warpgate -> $code"
    [ "$code" = 200 ] || { echo "::error::/@warpgate returned $code"; exit 1; }

    # The web UI is embedded in the binary; the Korean dictionary must be in it.
    docker run --rm --entrypoint sh "$image" -c \
        'grep -c -a -e "보안 게이트웨이" -e "\\\\ubcf4\\\\uc548" /usr/local/bin/warpgate' \
        || { echo "::error::Korean UI strings not found in the binary"; exit 1; }
fi

# RDP probe: FreeRDP with keyboardType 8 (Korean), TLS security like mstsc without NLA.
timeout 25 xvfb-run -a xfreerdp "/v:127.0.0.1:$rdp_port" /u:smoke-probe /p:x \
    /sec:tls /cert:ignore /kbd:0x412 /kbd-type:8 /log-level:INFO >/tmp/freerdp-$2.log 2>&1 || true
sleep 2
docker logs "$name" >/tmp/warpgate-$2.log 2>&1 || true
echo "---- FreeRDP (tail)"; tail -n 20 "/tmp/freerdp-$2.log" || true
echo "---- Warpgate log (RDP lines)"; grep -i -e rdp -e keyboard "/tmp/warpgate-$2.log" | tail -n 30 || true

rejected=0
grep -qi 'keyboardType' "/tmp/warpgate-$2.log" && rejected=1

docker rm -f "$name" >/dev/null 2>&1 || true
docker volume rm -f "$name-data" >/dev/null 2>&1 || true

if [ "$mode" = expect-ok ]; then
    [ "$rejected" = 0 ] || { echo "::error::keyboardType 8 was rejected"; exit 1; }
    echo "OK: keyboardType 8 accepted"
    echo "- moavi image: keyboardType 8 accepted" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
else
    [ "$rejected" = 1 ] || { echo "::warning::control did not reproduce the upstream rejection"; exit 1; }
    echo "OK: upstream rejects keyboardType 8 (control reproduced)"
    echo "- upstream 0.29.2: keyboardType 8 rejected (control reproduced)" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
fi
