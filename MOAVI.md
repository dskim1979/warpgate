# MOAVI 빌드 (MAKUS SYSTEMS 수정판)

Warpgate 0.29.2(Apache-2.0)를 MOAVI Desk의 "보안 게이트웨이"로 쓰기 위해 고친 빌드입니다.
원본 LICENSE는 그대로 둡니다.

- 이미지: `ghcr.io/dskim1979/warpgate:0.29.2-moavi9` (linux/amd64, `latest` 없음)
  - moavi1: 키보드 수정 + 한국어 화면
  - moavi2: + SSO 자동 시작
  - moavi3: + 다른 계정으로 로그인된 상태에서 바로가기 링크 처리, MOAVI Desk 파비콘·탭 제목
  - moavi4: + 웹 터미널 오른쪽 정보·파일 패널(SFTP 업로드/다운로드)
  - moavi5: + 웹 세션 탭 제목에 데스크톱 이름
  - moavi6: 설명 끝에 이미 "(RDP)" 같은 프로토콜이 있으면 탭 제목에 두 번 붙이지 않음
  - moavi7: + 웹 데스크톱 "붙여넣기" 상자(붙여넣기 / 직접 입력 / 내 PC로 복사), 클립보드 방향 정책
  - moavi8: RDP 연결 대기 화면 로고를 MOAVI Desk 마크 + "보안 게이트웨이"로
  - moavi9: 티켓으로 들어온 RDP 세션의 clientName = 티켓 UUID 앞 13자
- 브랜치: `moavi/0.29.2-ko` (태그 `v0.29.2`에서 시작)
- CI: `.github/workflows/moavi-image.yml` (웹 타입검사 → 키보드 단위 테스트 → 이미지 빌드 → 스모크 테스트 → 푸시)

## 바꾼 것

1. **한국어 키보드(keyboardType 8)**: `vendor/ironrdp-pdu`에 둔 IronRDP PDU 크레이트
   (`src/gcc/core_data/client.rs`). `KeyboardType::Korean = 8`을 추가했고, 모르는 값은 실패 대신
   `IbmEnhanced`로 받습니다. 루트 `Cargo.toml`의 `[patch.crates-io]`로 연결합니다.
   설명은 `vendor/ironrdp-pdu/PATCHES.md`, 원본과의 차이는 `vendor/ironrdp-pdu/warpgate.patch`에 있습니다.
   테스트: 벤더 크레이트 안 테스트와 `warpgate-protocol-rdp/src/keyboard_type_tests.rs`.
2. **한국어 화면**: `warpgate-web/src/common/i18n.ts`(영어 문장을 키로 쓰는 사전 + `t()`).
   기본은 한국어입니다. `?lang=en` 또는 화면 아래 "English" 버튼으로 바꾸면 브라우저에 기억됩니다.
   번역이 없는 문장은 영어로 나옵니다. 관리자 화면은 대부분 영어로 둡니다.
3. **SSO 자동 시작**(moavi2): `warpgate-web/src/gateway/Login.svelte`. SSO 공급자가 하나뿐이면 로그인 화면에서
   바로 SSO로 넘어갑니다. `next=`는 그대로 넘겨서 `#/web-desktop/start/<id>` 같은 링크는 로그인 뒤 그 화면으로 갑니다.
   - 비밀번호로 로그인하려면 `/@warpgate?local=1` 또는 `/@warpgate#/login?local=1`
   - SSO 오류(`login_error`)가 있거나, 60초 안에 이미 자동으로 보냈으면 다시 보내지 않습니다(무한 반복 방지).
4. **다른 계정 세션 처리**(moavi3): `warpgate-web/src/gateway/lib/accountSwitch.ts`, WebDesktop/WebSsh.
   `#/web-desktop/start/<id>`·`#/web-ssh/start/<id>`가 403이면 "다른 계정으로 로그인되어 있습니다." +
   "<SSO 이름>로 다시 로그인" 버튼(로그아웃 → 같은 SSO → 같은 링크로 복귀).
   - 링크에 `?sso=<공급자 이름>`(예: `#/web-desktop/start/<id>?sso=moavi-desk`)을 붙이면 버튼 없이 바로 바꿉니다.
   - 이미 그 SSO로 로그인한 사용자이거나, 2분 안에 다시 로그인한 뒤에도 403이면 "이 데스크톱에 접근 권한이 없습니다."만 보여 줍니다(반복 없음).
   - 서버: `/@warpgate/api/info`에 `sso_provider`(이 브라우저 세션이 로그인한 SSO 이름)를 추가했습니다(`warpgate-protocol-http/src/api/info.rs`).
5. **MOAVI Desk 표시**(moavi3): 파비콘 `warpgate-web/public/assets/favicon.svg`(콘솔 `public/favicon.svg` 복사),
   탭 제목 "보안 게이트웨이 · MOAVI Desk"(영어: "Security gateway · MOAVI Desk"), 사용자 화면 왼쪽 위 로고도 MOAVI 아이콘 +
   "보안 게이트웨이". Warpgate 이름은 관리자 화면 아래쪽에 "Warpgate (Apache-2.0) 기반 · MAKUS SYSTEMS 수정판"으로만 남깁니다.
6. **웹 터미널 정보·파일 패널**(moavi4): `warpgate-web/src/gateway/SshSidePanel.svelte`(화면),
   `warpgate-web-ssh/src/sftp.rs`(SFTP v3 클라이언트), `warpgate-web-ssh/src/files.rs`(HTTP), `session.rs`·`manager.rs`(채널 연결).
   - 같은 대상 SSH 연결에 `sftp` 서브시스템 채널을 하나 더 엽니다. 새 자격 증명은 필요 없고, 대상 접근 규칙도 그대로 적용됩니다.
   - API(같은 출처, 쿠키 로그인): `GET .../web-ssh/sessions/<id>/info`, `GET .../files?path=`, `GET .../files/download?path=`,
     `POST .../files/upload?dir=&name=`(본문 = 파일), `POST .../files/mkdir?dir=&name=`, `POST .../files/delete?path=&dir=true|false`
   - 파일당 최대 2GB(`MAX_UPLOAD_BYTES`). 스트리밍으로 처리하고 동시에 16개×32KB 요청만 오가서 메모리를 거의 쓰지 않습니다.
   - 감사: 전송마다 세션 로그에 `File transfer started/finished/failed`(경로·크기·방향), 폴더 생성·삭제도 남습니다.
   - 끄기: SSH 대상 옵션 `web_file_transfer`(기본 true). 관리 API `PUT /@warpgate/admin/api/targets/<id>`의
     `options`(kind `Ssh`)에 `"web_file_transfer": false`. 새 세션부터 적용됩니다.
   - 세션이 다른 클러스터 노드에 있으면 409(단일 노드 전제).
7. **탭 제목**(moavi5): `warpgate-web/src/gateway/lib/targetTitle.ts`. 웹 데스크톱·웹 터미널 탭은
   "<데스크톱 이름> (RDP|VNC|SSH) · 보안 게이트웨이". 이름은 대상 설명(description) 첫 줄을 쓰고, 없으면 대상 이름에서
   앞의 `moavi-`와 뒤의 `-desk_<id>[-ssh|-vnc]`를 뺍니다. 로그인·대상 목록은 "보안 게이트웨이 · MOAVI Desk" 그대로입니다.
8. **웹 데스크톱 붙여넣기 상자**(moavi7): `WebDesktop.svelte`, `common/desktopClipboard.ts`(`pasteText`),
   `warpgate-protocol-rdp/src/client/input.rs`(유니코드 keysym). 툴바 "붙여넣기" 버튼으로 상자를 엽니다.
   - "데스크톱에 붙여넣기": 원격 클립보드(cliprdr)에 넣고 Ctrl+V를 대신 누릅니다.
   - "직접 입력": 한 글자씩 유니코드 입력(X11 유니코드 keysym `0x01000000+코드`)으로 보냅니다. 원격 클립보드가 막혀도 됩니다. 최대 2만 자.
   - 원격에서 복사한 글은 상자에 보이고 "내 PC로 복사" 버튼으로 가져옵니다(브라우저가 자동 쓰기를 막을 때 대비).
   - 기존 Ctrl+V 붙여넣기와 원격→로컬 자동 복사도 그대로 둡니다.
   - **클립보드 정책**: RDP/VNC 대상 옵션 `web_clipboard` = `off`|`in`|`out`|`both`(기본 both). `in` = 내 PC → 데스크톱, `out` = 데스크톱 → 내 PC.
     서버에서 막습니다: RDP는 대상 쪽 클라이언트(`client/mod.rs`, 웹·mstsc 모두), VNC와 "직접 입력"은 `warpgate-web-desktop/src/session.rs`.
     화면도 따릅니다: in이 꺼지면 붙여넣기·직접 입력·Ctrl+V 글 전달이 없고, out이 꺼지면 "데스크톱에서 복사한 글"과 자동 복사가 없습니다.
     기록: `Clipboard transfer`(direction, bytes), `Clipboard transfer blocked by policy`, `Text typed into the desktop`(chars).
     관리 API `PUT /@warpgate/admin/api/targets/<id>`의 `options`(kind `Rdp`/`Vnc`)에 `"web_clipboard": "in"` 식으로 넣습니다.
9. **RDP 대기 화면 로고**(moavi8): 데스크톱이 뜨기 전에 RDP 화면에 그리는 대기 화면(연결 중, 승인 대기, 배너, OTP)의 로고
   `warpgate-desktop-ui/assets/logo.png`(354×90, 배경 #14141a 불투명 RGB)를 MOAVI Desk 마크 + "보안 게이트웨이"로 바꿨습니다.
   웹 RDP와 mstsc 모두 같은 화면입니다. 화면 글자는 ASCII 글꼴이라 영어로 두고 "Warpgate"만 "security gateway"로 바꿨습니다.
   그 밖의 사용자 화면에서 남은 Warpgate 표시(HTTP 대상 위 떠 있는 버튼 대체 텍스트, API 토큰·접속 방법 문구)도 바꿨습니다.
   Warpgate 이름은 관리자 화면 아래쪽 안내에만 남습니다.
10. **RDP clientName = 티켓**(moavi9): `warpgate-protocol-rdp/src/client/mod.rs` `rdp_client_name`, `lib.rs` `connect`.
    티켓(`ticket-<secret>`)으로 인증된 세션이면 대상 Windows로 가는 ClientCoreData clientName을 티켓 UUID에서 하이픈을 뺀
    소문자 16진수 앞 13자로 보냅니다(예: `0f1e2d3c4b5a6`). Windows에서 `%CLIENTNAME%`/WTSClientName으로 보입니다.
    티켓이 아니면(SSO·웹 로그인·비밀번호) 그대로 `warpgate`라서 `^[0-9a-f]{13}$`와 절대 맞지 않습니다. 웹 RDP와 mstsc 모두 같습니다.
    사용자의 mstsc는 이 값에 영향을 줄 수 없습니다(Warpgate가 RDP를 끝내고 새로 연결). 로그는 debug로 `binding=ticket|none`만 남깁니다.
    티켓 UUID = `POST /@warpgate/admin/api/tickets` 응답 `TicketAndSecret`의 `ticket.id`.

## 새 Warpgate 버전으로 다시 빌드하기

1. 업스트림 태그를 받아 새 브랜치를 만듭니다.
   `git fetch upstream tag vX.Y.Z --no-tags && git checkout -b moavi/X.Y.Z-ko vX.Y.Z`
2. 이 브랜치의 커밋들을 cherry-pick 합니다. (`git cherry-pick v0.29.2..moavi/0.29.2-ko`)
3. 키보드 수정이 아직 필요한지 확인합니다.
   - 업스트림 `Cargo.lock`의 `ironrdp-pdu`가 바뀌었으면, 새 버전 crate를 `vendor/ironrdp-pdu`에 풀고
     `warpgate.patch`를 다시 적용합니다. (`patch -p1 -d vendor/ironrdp-pdu < warpgate.patch`)
     `Cargo.toml`의 `[lints.rust] warnings = allow`도 다시 넣고, `Cargo.lock`의 ironrdp-pdu 항목에서
     `source`/`checksum` 두 줄을 지웁니다.
   - IronRDP가 keyboardType 8을 이미 받으면 `vendor/ironrdp-pdu`와 `[patch.crates-io]` 줄을 지웁니다.
     (`warpgate-protocol-rdp`의 테스트는 남겨 두면 회귀 검사가 됩니다.)
4. 화면 문장이 바뀌었으면 `i18n.ts`에 새 영어 문장을 추가합니다. (키가 안 맞으면 영어로만 나옵니다.)
5. `moavi-image.yml`의 `MOAVI_TAG`(예: `X.Y.Z-moavi1`)와 `UPSTREAM_IMAGE`를 바꾸고 브랜치를 push 합니다.
6. CI가 통과하면 MOAVI Desk `deploy/docker-compose.yml`의 이미지 태그를 바꿉니다.
