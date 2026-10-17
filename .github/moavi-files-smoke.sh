#!/usr/bin/env bash
# MOAVI: end-to-end test of the web-SSH file panel API against a real OpenSSH target.
#   moavi-files-smoke.sh <warpgate image>
# Warpgate and an OpenSSH container share a docker network. As admin: create an
# SSH target, open a web-SSH session, then list / upload / download / mkdir /
# delete through the session, check the file inside the target container, check
# the transfer is in Warpgate's log, and check that web_file_transfer=false
# turns the panel off.
set -euo pipefail

image="$1"
net=moavi-files
sshd=moavi-sshd
wg=moavi-wgf
port=38888
base="https://127.0.0.1:$port/@warpgate"
jar="$(mktemp)"
work="$(mktemp -d)"

pw="$(openssl rand -hex 16)"
ssh_pw="$(openssl rand -hex 12)"
echo "::add-mask::$pw"
echo "::add-mask::$ssh_pw"

cleanup() {
    docker logs "$wg" > /tmp/warpgate-files.log 2>&1 || true
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
    -p "127.0.0.1:$port:8888" "$image" run >/dev/null

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

target_json() { # web_file_transfer
    jq -nc --arg pw "$ssh_pw" --argjson ft "$1" '{
        name: "sftp-e2e", description: "", require_approval: false,
        ticket_requests_disabled: false, ticket_require_approval: false,
        options: {kind: "Ssh", host: "moavi-sshd", port: 2222, username: "moavi",
                  allow_insecure_algos: false, web_file_transfer: $ft,
                  auth: {kind: "Password", password: $pw}}}'
}
r="$(api POST /admin/api/targets "$(target_json true)")"
[ "$(status_of "$r")" -lt 300 ] || fail "create target: $r"
target_id="$(body_of "$r" | jq -r .id)"

# give the admin's roles access to the target
for role_id in $(body_of "$(api GET /admin/api/roles)" | jq -r '.[].id'); do
    api POST "/admin/api/targets/$target_id/roles/$role_id" '{}' >/dev/null || true
done

new_session() {
    local r
    r="$(api POST /api/web-ssh/sessions "$(jq -nc --arg t "$target_id" '{target_id:$t}')")"
    [ "$(status_of "$r")" = 201 ] || fail "create web-SSH session: $r"
    body_of "$r" | jq -r .session_id
}

sid="$(new_session)"
files="/api/web-ssh/sessions/$sid"

# the target connection comes up asynchronously
listing=""
for _ in $(seq 1 30); do
    r="$(api GET "$files/files")"
    if [ "$(status_of "$r")" = 200 ]; then listing="$(body_of "$r")"; break; fi
    sleep 1
done
[ -n "$listing" ] || fail "listing never succeeded: $r"
home="$(jq -r .path <<<"$listing")"
echo "home directory: $home"

r="$(api GET "$files/info")"
[ "$(status_of "$r")" = 200 ] || fail "info: $r"
body_of "$r" | jq -c '{target_name, username, recording, file_transfer, max_upload_bytes}'
[ "$(body_of "$r" | jq -r .file_transfer)" = true ] || fail "file_transfer should be on"

# upload ~3 MB with a Korean name
head -c 3145728 /dev/urandom > "$work/up.bin"
name='업로드 테스트.bin'
enc_name="$(jq -rn --arg v "$name" '$v|@uri')"
enc_home="$(jq -rn --arg v "$home" '$v|@uri')"
code="$(curl -sk -b "$jar" -o "$work/up.json" -w '%{http_code}' -X POST \
    -H 'Content-Type: application/octet-stream' --data-binary @"$work/up.bin" \
    "$base$files/files/upload?dir=$enc_home&name=$enc_name")"
[ "$code" = 200 ] || fail "upload: $code $(cat "$work/up.json")"
cat "$work/up.json"; echo

# visible in the target (as the terminal's `ls` would see it)
docker exec "$sshd" ls -l "$home/$name" || fail "uploaded file not on the target"
remote_sum="$(docker exec "$sshd" sha256sum "$home/$name" | cut -d' ' -f1)"
[ "$remote_sum" = "$(sha256sum "$work/up.bin" | cut -d' ' -f1)" ] || fail "content differs on the target"

# listed with its size
r="$(api GET "$files/files?path=$enc_home")"
size="$(body_of "$r" | jq -r --arg n "$name" '.entries[] | select(.name==$n) | .size')"
[ "$size" = 3145728 ] || fail "listed size $size"

# download gives the same bytes
enc_file="$(jq -rn --arg v "$home/$name" '$v|@uri')"
code="$(curl -sk -b "$jar" -o "$work/down.bin" -w '%{http_code}' "$base$files/files/download?path=$enc_file")"
[ "$code" = 200 ] || fail "download: $code"
cmp "$work/up.bin" "$work/down.bin" || fail "downloaded bytes differ"

# mkdir, then delete file and folder
r="$(api POST "$files/files/mkdir?dir=$enc_home&name=e2e-dir")"
[ "$(status_of "$r")" = 204 ] || fail "mkdir: $r"
docker exec "$sshd" test -d "$home/e2e-dir" || fail "folder not created"
r="$(api POST "$files/files/delete?path=$enc_file")"
[ "$(status_of "$r")" = 204 ] || fail "delete file: $r"
r="$(api POST "$files/files/delete?path=$(jq -rn --arg v "$home/e2e-dir" '$v|@uri')&dir=true")"
[ "$(status_of "$r")" = 204 ] || fail "delete folder: $r"
docker exec "$sshd" test ! -e "$home/$name" || fail "file still there"

# path tricks are refused
r="$(api POST "$files/files/mkdir?dir=$enc_home&name=..")"
[ "$(status_of "$r")" = 400 ] || fail "'..' accepted: $r"

# transfers are in Warpgate's log
sleep 1
docker logs "$wg" 2>&1 | grep -q 'File transfer finished' || fail "transfer not logged"
docker logs "$wg" 2>&1 | grep 'File transfer' | tail -n 4

# turning the flag off closes the panel for new sessions
r="$(api PUT "/admin/api/targets/$target_id" "$(target_json false)")"
[ "$(status_of "$r")" -lt 300 ] || fail "update target: $r"
sid2="$(new_session)"
r="$(api GET "/api/web-ssh/sessions/$sid2/files")"
[ "$(status_of "$r")" = 403 ] || fail "file transfer still allowed: $r"
r="$(api GET "/api/web-ssh/sessions/$sid2/info")"
[ "$(body_of "$r" | jq -r .file_transfer)" = false ] || fail "info should say off"

echo "OK: web-SSH file panel works (list/upload/download/mkdir/delete, audit log, per-target switch)"
echo "- web-SSH file panel: list/upload/download/mkdir/delete OK, logged, per-target switch OK" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
