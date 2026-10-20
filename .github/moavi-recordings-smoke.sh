#!/usr/bin/env bash
# MOAVI: end-to-end test of per-target recording, recording delete and storage usage.
#   moavi-recordings-smoke.sh <warpgate image>
# Warpgate (recordings on) and an OpenSSH container share a docker network. Two SSH
# targets: one with options.record=false, one without the field. Native ssh with a
# ticket runs a command through each; the usage endpoint shows that only the second
# made a recording. A recording is refused (409) while its session is open, then
# deleted (204): its files leave the data volume and the count goes back down.
set -euo pipefail

image="$1"
net=moavi-rec
sshd=moavi-rec-sshd
wg=moavi-wgr
port=38889
ssh_port=32222
base="https://127.0.0.1:$port/@warpgate"
jar="$(mktemp)"

pw="$(openssl rand -hex 16)"
ssh_pw="$(openssl rand -hex 12)"
echo "::add-mask::$pw"
echo "::add-mask::$ssh_pw"

cleanup() {
    docker logs "$wg" > /tmp/warpgate-recordings.log 2>&1 || true
    docker rm -f "$wg" "$sshd" >/dev/null 2>&1 || true
    docker volume rm -f "$wg-data" >/dev/null 2>&1 || true
    docker network rm "$net" >/dev/null 2>&1 || true
}
trap cleanup EXIT

fail() {
    echo "::error::$*"
    echo "---- Warpgate log (tail)"
    docker logs "$wg" 2>&1 | tail -n 60 || true
    exit 1
}

docker network create "$net" >/dev/null
docker run -d --name "$sshd" --network "$net" \
    -e PUID=1000 -e PGID=1000 -e PASSWORD_ACCESS=true \
    -e USER_NAME=moavi -e USER_PASSWORD="$ssh_pw" \
    lscr.io/linuxserver/openssh-server:latest >/dev/null

docker volume create "$wg-data" >/dev/null
docker run --rm -v "$wg-data:/data" -e WARPGATE_ADMIN_PASSWORD="$pw" "$image" \
    unattended-setup --data-path /data --http-port 8888 --ssh-port 2222 \
    --record-sessions --host-key-verification auto-accept --external-host 127.0.0.1 >/dev/null
docker run -d --name "$wg" --network "$net" -v "$wg-data:/data" \
    -p "127.0.0.1:$port:8888" -p "127.0.0.1:$ssh_port:2222" "$image" run >/dev/null

for _ in $(seq 1 60); do
    curl -skf "$base/api/info" >/dev/null && break
    sleep 1
done
curl -skf "$base/api/info" >/dev/null || fail "Warpgate did not come up"
for _ in $(seq 1 60); do
    docker exec "$sshd" sh -c 'nc -z 127.0.0.1 2222 2>/dev/null || (exec 3<>/dev/tcp/127.0.0.1/2222) 2>/dev/null' && break
    sleep 1
done

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

# ---- the contract is visible in the served OpenAPI document
spec="$(curl -skf -b "$jar" "$base/admin/api/openapi.json")" || fail "openapi.json"
for schema in TargetRdpOptions TargetSSHOptions TargetVncOptions; do
    jq -e --arg s "$schema" '.components.schemas[$s].properties.record.type == "boolean"
        and .components.schemas[$s].properties.record_keyboard.type == "boolean"' <<<"$spec" >/dev/null \
        || fail "$schema has no record/record_keyboard in openapi.json"
done
jq -e '.paths["/recordings/{id}"].delete and .paths["/recordings-storage/usage"].get' <<<"$spec" >/dev/null \
    || fail "openapi.json lacks the recordings delete / usage operations"
jq -e '.components.schemas.Recording.properties.size_bytes.type == "integer"' <<<"$spec" >/dev/null \
    || fail "Recording has no size_bytes in openapi.json"

# ---- two targets: record=false, and no record field (follow the global switch)
target_json() { # name [extra options json]
    local extra="${2:-}"
    [ -n "$extra" ] || extra='{}'
    jq -nc --arg pw "$ssh_pw" --arg name "$1" --argjson extra "$extra" '{
        name: $name, description: "", require_approval: false,
        ticket_requests_disabled: false, ticket_require_approval: false,
        options: ({kind: "Ssh", host: "moavi-rec-sshd", port: 2222, username: "moavi",
                  allow_insecure_algos: false, auth: {kind: "Password", password: $pw}} + $extra)}'
}
create_target() { # name [extra]
    local r id
    r="$(api POST /admin/api/targets "$(target_json "$1" "${2:-}")")"
    [ "$(status_of "$r")" -lt 300 ] || fail "create target $1: $r"
    id="$(body_of "$r" | jq -r .id)"
    for role_id in $(body_of "$(api GET /admin/api/roles)" | jq -r '.[].id'); do
        api POST "/admin/api/targets/$id/roles/$role_id" '{}' >/dev/null || true
    done
    echo "$id"
}
off_id="$(create_target rec-off '{"record": false}')"
on_id="$(create_target rec-on)"

[ "$(body_of "$(api GET "/admin/api/targets/$off_id")" | jq -r .options.record)" = false ] \
    || fail "record=false not stored"
[ "$(body_of "$(api GET "/admin/api/targets/$on_id")" | jq -r '.options.record // "absent"')" = absent ] \
    || fail "a target without the field should not get one"

usage() { body_of "$(api GET /admin/api/recordings-storage/usage)"; }
count() { usage | jq -r .count; }
u0="$(usage)"
echo "usage at start: $u0"
jq -e '.count == 0 and .bytes >= 0 and (.free_bytes | type) == "number" and (.total_bytes | type) == "number"
       and .total_bytes >= .free_bytes' <<<"$u0" >/dev/null || fail "unexpected starting usage: $u0"

ticket() { # target-name -> secret
    local r
    r="$(api POST /admin/api/tickets "$(jq -nc --arg t "$1" '{username:"admin", target_name:$t, number_of_uses:1}')")"
    [ "$(status_of "$r")" -lt 300 ] || fail "ticket for $1: $r"
    body_of "$r" | jq -r .secret
}
ssh_via() { # target-name command
    local secret
    secret="$(ticket "$1")"
    echo "::add-mask::$secret"
    sshpass -p x ssh -p "$ssh_port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \
        -o LogLevel=ERROR -o ConnectTimeout=20 -o PubkeyAuthentication=no \
        -o PreferredAuthentications=password,keyboard-interactive \
        "ticket-$secret@127.0.0.1" "$2"
}

# ---- record=false: the command runs, nothing is recorded
out="$(ssh_via rec-off 'echo off-ok')" || fail "ssh through rec-off failed: $out"
grep -q off-ok <<<"$out" || fail "rec-off command output: $out"
sleep 3
[ "$(count)" = 0 ] || fail "record=false still made a recording: $(usage)"

# ---- no field: recorded (global switch on)
out="$(ssh_via rec-on 'echo on-ok')" || fail "ssh through rec-on failed: $out"
grep -q on-ok <<<"$out" || fail "rec-on command output: $out"
for _ in $(seq 1 20); do [ "$(count)" -ge 1 ] && break; sleep 1; done
[ "$(count)" = 1 ] || fail "the default target was not recorded: $(usage)"

all_recordings() { # every recording row, newest first
    for sid in $(body_of "$(api GET '/admin/api/sessions?limit=100')" | jq -r '(.items // .)[].id'); do
        body_of "$(api GET "/admin/api/sessions/$sid/recordings")"
    done | jq -s 'add // [] | sort_by(.started) | reverse'
}

# ---- an open session's recording cannot be deleted (409)
ssh_via rec-on 'echo live; sleep 20' >/dev/null 2>&1 &
live_pid=$!
for _ in $(seq 1 20); do [ "$(count)" -ge 2 ] && break; sleep 1; done
[ "$(count)" = 2 ] || fail "the second session did not start recording: $(usage)"
live_rec="$(all_recordings | jq -r '[.[] | select(.ended == null)][0].id')"
[ -n "$live_rec" ] && [ "$live_rec" != null ] || fail "no in-progress recording found"
[ "$(all_recordings | jq -r --arg id "$live_rec" '.[] | select(.id == $id) | .size_bytes')" = null ] \
    || fail "an in-progress recording should have size_bytes null"
r="$(api DELETE "/admin/api/recordings/$live_rec")"
[ "$(status_of "$r")" = 409 ] || fail "deleting a live recording should be 409: $r"
wait "$live_pid" || true

# ---- a finished recording: 204, files gone, row gone, count down
done_rec="$(all_recordings | jq -c '[.[] | select(.id != $live)][-1]' --arg live "$live_rec")"
rec_id="$(jq -r .id <<<"$done_rec")"
rec_dir="$(jq -r '"\(.session_id)/\(.name)"' <<<"$done_rec")"
size="$(jq -r .size_bytes <<<"$done_rec")"
[ "$size" != null ] && [ "$size" -gt 0 ] || fail "an ended recording should have size_bytes > 0: $done_rec"
[ "$(body_of "$(api GET "/admin/api/recordings/$rec_id")" | jq -r .size_bytes)" = "$size" ] \
    || fail "GET /recordings/{id} size_bytes differs from the session listing"
on_disk="$(docker exec "$wg" sh -c "find /data -path '*$rec_dir*' -type f -exec stat -c %s {} +" | awk '{s+=$1} END {print s+0}')"
[ "$on_disk" = "$size" ] || fail "size_bytes $size but the files on disk take $on_disk"
docker exec "$wg" sh -c "find /data -path '*$rec_dir*' | head -n 1" | grep -q . \
    || fail "recording files not found on the data volume before delete ($rec_dir)"
bytes_before="$(usage | jq -r .bytes)"
[ "$bytes_before" -gt 0 ] || fail "usage bytes should be > 0 with recordings present"
r="$(api DELETE "/admin/api/recordings/$rec_id")"
[ "$(status_of "$r")" = 204 ] || fail "delete recording: $r"
[ "$(status_of "$(api GET "/admin/api/recordings/$rec_id")")" = 404 ] || fail "row still there"
docker exec "$wg" sh -c "find /data -path '*$rec_dir*' | head -n 1" | grep -q . \
    && fail "recording files still on the data volume after delete"
[ "$(count)" = 1 ] || fail "count after delete: $(usage)"

# the live one, once its session ended, can be deleted too
deleted=""
for _ in $(seq 1 30); do
    r="$(api DELETE "/admin/api/recordings/$live_rec")"
    if [ "$(status_of "$r")" = 204 ]; then deleted=1; break; fi
    sleep 1
done
[ -n "$deleted" ] || fail "the ended recording was still refused: $r"
u1="$(usage)"
jq -e '.count == 0 and .oldest == null' <<<"$u1" >/dev/null || fail "usage after deleting all: $u1"

r="$(api DELETE "/admin/api/recordings/$(cat /proc/sys/kernel/random/uuid)")"
[ "$(status_of "$r")" = 404 ] || fail "unknown recording should be 404: $r"

echo "OK: per-target record switch, delete (409 while live, 204 after), storage usage, size_bytes"
echo "- recordings: per-target record=false skips recording, delete 409 live / 204 ended, usage counts and disk space OK" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
