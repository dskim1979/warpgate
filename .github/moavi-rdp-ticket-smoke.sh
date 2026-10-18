#!/usr/bin/env bash
# MOAVI: end-to-end test of the native RDP ticket path and the clientName binding.
#   moavi-rdp-ticket-smoke.sh <warpgate image>
# Warpgate + an xrdp target share a docker network. As admin: create an RDP target and a
# one-use ticket, connect FreeRDP to Warpgate's RDP port as "ticket-<secret>", then check
#  - Warpgate logged binding=ticket (debug) and the target session was authorized, and
#  - xrdp saw client computer name == first 13 hex chars of the ticket UUID.
# A second connection through the web-login path is not needed: the unit tests cover
# the non-ticket default ("warpgate").
set -euo pipefail

image="$1"
net=moavi-rdpt
xrdp=moavi-xrdp
wg=moavi-wgr
http_port=48888
rdp_port=43389
base="https://127.0.0.1:$http_port/@warpgate"
jar="$(mktemp)"
work="$(mktemp -d)"

pw="$(openssl rand -hex 16)"
echo "::add-mask::$pw"

cleanup() {
    docker rm -f "$wg" "$xrdp" >/dev/null 2>&1 || true
    docker volume rm -f "$wg-data" >/dev/null 2>&1 || true
    docker network rm "$net" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
    echo "::error::$*"
    echo "---- Warpgate log (tail)"
    docker logs "$wg" > "$work/wg.log" 2>&1 || true
    tail -n 60 "$work/wg.log" || true
    echo "---- xrdp log (tail)"
    docker exec "$xrdp" sh -c 'tail -n 40 /var/log/xrdp.log 2>/dev/null' || true
    exit 1
}

docker network create "$net" >/dev/null
# the linuxserver/rdesktop image's own default user abc/abc; a throwaway container
docker run -d --name "$xrdp" --network "$net" --shm-size=1gb \
    -e PUID=1000 -e PGID=1000 -e TZ=Etc/UTC \
    lscr.io/linuxserver/rdesktop:ubuntu-openbox-c5644f3f-ls194 >/dev/null

docker volume create "$wg-data" >/dev/null
docker run --rm -v "$wg-data:/data" -e WARPGATE_ADMIN_PASSWORD="$pw" "$image" \
    unattended-setup --data-path /data --http-port 8888 --ssh-port 2222 --rdp-port 3389 \
    --record-sessions --external-host 127.0.0.1 >/dev/null
docker run -d --name "$wg" --network "$net" -v "$wg-data:/data" \
    -e RUST_LOG=audit=info,warpgate=info,warpgate_protocol_rdp=debug \
    -p "127.0.0.1:$http_port:8888" -p "127.0.0.1:$rdp_port:3389" "$image" run >/dev/null

for _ in $(seq 1 60); do
    curl -skf "$base/api/info" >/dev/null && break
    sleep 1
done
curl -skf "$base/api/info" >/dev/null || fail "Warpgate did not come up"
for _ in $(seq 1 90); do
    docker exec "$xrdp" sh -c '(exec 3<>/dev/tcp/127.0.0.1/3389) 2>/dev/null' && break
    sleep 1
done
sleep 10 # let xrdp finish starting after the port opens

api() { # method path [json]
    local method="$1" path="$2" data="${3:-}"
    if [ -n "$data" ]; then
        curl -sk -b "$jar" -c "$jar" -X "$method" -H 'Content-Type: application/json' \
            --data "$data" -w '\n%{http_code}' "$base$path"
    else
        curl -sk -b "$jar" -c "$jar" -X "$method" -w '\n%{http_code}' "$base$path"
    fi
}
status_of() { tail -n 1 <<<"$1"; }
body_of() { sed '$d' <<<"$1"; }

r="$(api POST /api/auth/login "$(jq -nc --arg p "$pw" '{username:"admin", password:$p}')")"
[ "$(status_of "$r")" -lt 300 ] || fail "admin login: $r"

r="$(api POST /admin/api/targets "$(jq -nc '{
    name: "rdp-ticket-e2e", description: "", require_approval: false,
    ticket_requests_disabled: false, ticket_require_approval: false,
    options: {kind: "Rdp", host: "moavi-xrdp", port: 3389, username: "abc",
              auth: {kind: "Password", password: "abc"}, verify_tls: false,
              compression: "lossless", interactive_logon: false, tls_security: "Tls12"}}')")"
[ "$(status_of "$r")" -lt 300 ] || fail "create RDP target: $r"

r="$(api POST /admin/api/tickets "$(jq -nc '{username: "admin", target_name: "rdp-ticket-e2e", number_of_uses: 1}')")"
[ "$(status_of "$r")" -lt 300 ] || fail "create ticket: $(status_of "$r")"
ticket_id="$(body_of "$r" | jq -r .ticket.id)"
secret="$(body_of "$r" | jq -r .secret)"
echo "::add-mask::$secret"
expected="$(tr -d '-' <<<"$ticket_id" | tr 'A-F' 'a-f' | cut -c1-13)"
echo "ticket id: $ticket_id -> expected clientName: $expected"

# native RDP through Warpgate as the ticket (TLS security, like the portal .rdp)
timeout 40 xvfb-run -a xfreerdp "/v:127.0.0.1:$rdp_port" "/u:ticket-$secret" /p:unused \
    /sec:tls /cert:ignore /log-level:WARN >"$work/freerdp.log" 2>&1 || true

docker logs "$wg" > "$work/wg.log" 2>&1 || true
grep -E 'RDP clientName|Authorized|Ticket' "$work/wg.log" | tail -n 10 || true
grep -q 'binding="ticket"\|binding=ticket' "$work/wg.log" || fail "Warpgate did not bind clientName to the ticket"

found=""
for _ in $(seq 1 10); do
    docker exec "$xrdp" sh -c 'cat /var/log/xrdp.log 2>/dev/null' > "$work/xrdp.log" || true
    if grep -qi "client computer name: $expected" "$work/xrdp.log"; then found=1; break; fi
    sleep 1
done
grep -i 'client computer name' "$work/xrdp.log" | tail -n 3 || true
[ -n "$found" ] || fail "xrdp did not see clientName $expected"

echo "OK: native RDP ticket session reached the target with clientName $expected"
echo "- native RDP ticket path: target saw clientName = ticket UUID[:13]" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
