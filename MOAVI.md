# MOAVI 빌드 (MAKUS SYSTEMS 수정판)

Warpgate 0.29.2(Apache-2.0)를 MOAVI Desk의 "보안 게이트웨이"로 쓰기 위해 고친 빌드입니다.
원본 LICENSE는 그대로 둡니다.

- 이미지: `ghcr.io/dskim1979/warpgate:0.29.2-moavi2` (linux/amd64, `latest` 없음)
  - moavi1: 키보드 수정 + 한국어 화면
  - moavi2: + SSO 자동 시작
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
