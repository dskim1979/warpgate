// Minimal UI translation layer (MOAVI build by MAKUS SYSTEMS).
//
// Strings are keyed by their English text, so an untranslated string simply
// shows in English. Korean is the default; `?lang=en` (or the footer switch)
// picks English and is remembered in localStorage.

export type Lang = 'ko' | 'en'

const STORAGE_KEY = 'warpgate-lang'
const DEFAULT_LANG: Lang = 'ko'

const ko: Record<string, string> = {
    // Product / chrome
    Warpgate: '보안 게이트웨이',
    'Security gateway': '보안 게이트웨이',
    'Security gateway · MOAVI Desk': '보안 게이트웨이 · MOAVI Desk',
    'Security gateway admin · MOAVI Desk': '보안 게이트웨이 관리 · MOAVI Desk',
    Admin: '관리',
    'Switch theme': '테마 바꾸기',
    Acknowledge: '확인',
    Language: '언어',

    // Login
    Welcome: '로그인',
    'Continue login': '로그인 계속',
    Username: '사용자 이름',
    Password: '비밀번호',
    'Log in': '로그인',
    'One-time password': '일회용 비밀번호(OTP)',
    'The security policy requires you to sign in again before accessing this function.':
        '보안 정책에 따라 다시 로그인해야 합니다.',
    'Incorrect credentials': '사용자 이름 또는 비밀번호가 틀렸습니다.',
    'Login denied: your IP address is not in the allowed range for this user':
        '허용되지 않은 IP 주소라 로그인할 수 없습니다.',
    'Password login': '비밀번호로 로그인',
    Cancel: '취소',
    or: '또는',

    // Auth bar / logout
    '(ticket auth)': '(티켓 로그인)',
    'Log out options': '로그아웃 선택',
    'Log out of Warpgate': '보안 게이트웨이에서 로그아웃',
    'Log out everywhere': '모든 곳에서 로그아웃',
    'Log out': '로그아웃',

    // Web auth requests
    '{protocol} authentication request': '{protocol} 인증 요청',
    'From {address}': '{address}에서 요청',

    // Target list
    'Connecting to {name}. If this target needs approval, it will start once an administrator approves it.':
        '{name}에 연결 중입니다. 승인이 필요하면 관리자 승인 후 시작됩니다.',
    Preferences: '설정',
    'Open targets in a new tab': '새 탭에서 열기',
    'Managed by the administrator': '관리자가 설정함',
    "You don't have access to any targets yet": '접속할 수 있는 대상이 아직 없습니다',
    'Once you add targets and assign access, they will appear here':
        '대상을 추가하고 권한을 주면 여기에 표시됩니다',
    'No other targets yet': '다른 대상이 없습니다',
    'Web terminal': '웹 터미널',
    'Web desktop': '웹 데스크톱',
    'Connection instructions': '접속 방법',
    'Edit target': '대상 편집',
    Close: '닫기',
    'Search...': '검색...',
    Groups: '그룹',
    'Collapse all': '모두 접기',
    'Expand all': '모두 펼치기',
    '{count} user request(s)': '사용자 요청 {count}건',
    'awaiting your action': '처리 대기 중',

    // Out-of-band approval
    'This request is no longer waiting. It may have been answered already, timed out, or belong to a different account than the one you are signed in as.':
        '이미 처리됐거나 만료된 요청입니다. 다른 계정의 요청일 수도 있습니다.',
    'authorization request': '접속 승인 요청',
    'Ensure this security key matches your authentication prompt:':
        '접속 창에 보이는 보안 코드와 같은지 확인하세요:',
    'Authorize this {protocol} session?': '이 {protocol} 접속을 허용할까요?',
    Requested: '요청 시각',
    'from {address}': '({address}에서)',
    Approved: '허용했습니다',
    Rejected: '거부했습니다',
    'Authorize & remember for {duration}': '허용하고 {duration} 동안 기억',
    'Authorize for all targets & remember for {duration}':
        '모든 대상 허용하고 {duration} 동안 기억',
    'Authorize this time only': '이번만 허용',
    Authorize: '허용',
    Reject: '거부',

    // Session (web desktop / terminal)
    Connecting: '연결 중',
    Connected: '연결됨',
    Disconnected: '연결 끊김',
    Error: '오류',
    '(attempt {attempt})': '(재시도 {attempt}회)',
    Fullscreen: '전체 화면',
    'Exit fullscreen': '전체 화면 끝내기',
    Disconnect: '연결 끊기',
    'Session not found. It may have expired or been closed.':
        '세션이 없습니다. 만료됐거나 이미 닫혔습니다.',
    'Connecting. If this target needs administrator approval, the session will start once an administrator approves it.':
        '연결 중입니다. 승인이 필요하면 관리자 승인 후 시작됩니다.',
    'Zoom out': '축소',
    'Zoom in': '확대',
    'Connect from your machine': '내 PC에서 접속',
    'Trust this key?': '이 서버 키를 신뢰할까요?',
    'Reject and disconnect': '거부하고 연결 끊기',
    'Accept and connect': '수락하고 연결',
    Accept: '수락',
    'Shell {n}': '셸 {n}',
    // web desktop paste box
    Paste: '붙여넣기',
    'Paste text into the desktop': '데스크톱에 글 붙여넣기',
    'Paste text here (Ctrl+V), then choose how to send it.': '여기에 붙여넣은(Ctrl+V) 뒤 보낼 방법을 고르세요.',
    'Paste into desktop': '데스크톱에 붙여넣기',
    'Uses the desktop clipboard, then presses Ctrl+V.': '데스크톱 클립보드에 넣고 Ctrl+V를 누릅니다.',
    'Type it': '직접 입력',
    'Types the text key by key. Works when the clipboard is blocked.': '한 글자씩 입력합니다. 클립보드가 막혀도 됩니다.',
    Clear: '지우기',
    'Pasted into the desktop.': '데스크톱에 붙여넣었습니다.',
    'Typed into the desktop.': '데스크톱에 입력했습니다.',
    'Text is too long to type ({max} characters max).': '글이 너무 깁니다(최대 {max}자).',
    'Copied in the desktop:': '데스크톱에서 복사한 글:',
    'Copy to this PC': '내 PC로 복사',
    'Copied to this PC.': '내 PC에 복사했습니다.',
    'The browser blocked the clipboard. Select the text and press Ctrl+C.': '브라우저가 막았습니다. 글을 선택하고 Ctrl+C를 누르세요.',
    // web terminal file panel
    'Info and files': '정보·파일',
    'Info': '정보',
    'Files': '파일',
    'Target': '대상',
    'User': '사용자',
    'Connected since': '접속 시각',
    'Recording': '녹화',
    'File transfer': '파일 전송',
    'On': '켜짐',
    'Off': '꺼짐',
    'File transfer is turned off for this server.': '이 서버는 파일 전송이 꺼져 있습니다.',
    'Files appear once connected.': '연결되면 파일이 보입니다.',
    'Up': '상위 폴더',
    'Refresh': '새로 고침',
    'New folder': '새 폴더',
    'Upload': '업로드',
    'Folder name': '폴더 이름',
    'Create': '만들기',
    'Download': '다운로드',
    'Delete': '삭제',
    'This folder is empty.': '빈 폴더입니다.',
    'Only the first 5000 items are shown.': '처음 5000개만 보입니다.',
    'Done': '완료',
    'Failed': '실패',
    'Waiting': '대기 중',
    'Clear finished': '끝난 항목 지우기',
    'Drop files here or on the terminal. Up to {size} per file.': '여기나 터미널에 파일을 끌어 놓으세요. 파일당 최대 {size}.',
    'Permission denied.': '권한이 없습니다.',
    'File or folder not found.': '파일이나 폴더가 없습니다.',
    'The request could not be completed.': '요청을 처리하지 못했습니다.',
    'The file is too large.': '파일이 너무 큽니다.',
    'Could not reach the server for file transfer.': '파일 전송용 서버 연결에 실패했습니다.',
    'Something went wrong.': '오류가 발생했습니다.',
    'Only empty folders can be deleted.': '빈 폴더만 삭제할 수 있습니다.',
    'Upload failed.': '업로드에 실패했습니다.',
    'You are signed in with a different account.': '다른 계정으로 로그인되어 있습니다.',
    'Sign in again with {name}': '{name}로 다시 로그인',
    'Signing in again with {name}...': '{name}로 다시 로그인하는 중...',
    'You do not have access to this desktop.': '이 데스크톱에 접근 권한이 없습니다.',
    'You do not have access to this server.': '이 서버에 접근 권한이 없습니다.',
    'There is currently no trusted {type} key for the SSH server at {host}:{port}.':
        '{host}:{port} 서버의 {type} 키를 아직 신뢰하지 않았습니다.',
    'The remote side wants to start a ZMODEM file transfer. Accept?':
        '서버가 ZMODEM 파일 전송을 시작하려 합니다. 수락할까요?',

    // MFA setup
    'Multi-factor authentication setup': '2단계 인증 설정',
    'Before you can use Warpgate, you need to add a second authentication factor to protect your account.':
        '보안 게이트웨이를 쓰려면 먼저 2단계 인증을 설정하세요.',
    'Set up one-time password': '일회용 비밀번호(OTP) 설정',
    'OTP QR code': 'OTP QR 코드',
    Copy: '복사',
    'Install a TOTP authenticator app, such as Microsoft Authenticator, or use your password manager.':
        'OTP 앱(예: Microsoft Authenticator)을 설치하세요.',
    'Initialize it using the code on the left.': '왼쪽 QR 코드를 앱으로 스캔하세요.',
    'Type the one-time password generated by the app below:':
        '앱에 나온 6자리 숫자를 입력하세요:',
    Confirm: '확인',

    // Profile
    'API tokens': 'API 토큰',
    'Manage your API tokens': 'API 토큰 관리',
    Credentials: '자격 증명',
    'Manage your passwords and keys': '비밀번호와 키 관리',
    'Ticket requests': '티켓 요청',
    'Request and manage self-service access tickets': '접속 티켓 요청 및 관리',
}

const dictionaries: Record<Lang, Record<string, string>> = { ko, en: {} }

function isLang(value: string | null | undefined): value is Lang {
    return value === 'ko' || value === 'en'
}

function readStored(): string | null {
    try {
        return localStorage.getItem(STORAGE_KEY)
    } catch {
        return null
    }
}

function detectLang(): Lang {
    try {
        const fromQuery = new URLSearchParams(location.search).get('lang')
        if (isLang(fromQuery)) {
            setLang(fromQuery, false)
            return fromQuery
        }
    } catch {
        // ignore
    }
    const stored = readStored()
    if (isLang(stored)) {
        return stored
    }
    return DEFAULT_LANG
}

export const lang: Lang = detectLang()

if (typeof document !== 'undefined') {
    document.documentElement.lang = lang
}

export function setLang(value: Lang, reload = true): void {
    try {
        localStorage.setItem(STORAGE_KEY, value)
    } catch {
        // ignore
    }
    if (reload) {
        location.reload()
    }
}

/** Translate an English UI string; `{name}` placeholders are filled from `params`. */
export function t(text: string, params?: Record<string, string | number>): string {
    let out = dictionaries[lang][text] ?? text
    if (params) {
        for (const [key, value] of Object.entries(params)) {
            out = out.split(`{${key}}`).join(String(value))
        }
    }
    return out
}
