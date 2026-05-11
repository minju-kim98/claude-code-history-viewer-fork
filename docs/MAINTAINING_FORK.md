# Fork 운영 가이드

이 fork(`minju-kim98/claude-code-history-viewer-fork`)는 업스트림
[`jhlee0409/claude-code-history-viewer`](https://github.com/jhlee0409/claude-code-history-viewer)에
**DITCodeAgent provider**를 추가한 사내용 fork다. 업스트림에 기여하지 않고
fork 내부에서만 운영한다.

## 1. 업스트림 업데이트 받기

`upstream` remote는 이미 설정되어 있다.

```powershell
cd D:\development\personal\claude-code-history-viewer-fork

# 1) 업스트림 최신 가져오기
git fetch upstream

# 2) develop 동기화
git checkout develop
git merge upstream/develop
git push                       # fork의 develop도 갱신

# 3) feature 브랜치 rebase
git checkout feature/ditcodeagent-provider
git rebase develop
git push --force-with-lease    # rebase 후라 force-with-lease 필요
```

### 머지 충돌 가능 지점

| 파일 | 사유 |
|---|---|
| `src-tauri/src/providers/mod.rs` | upstream이 새 provider 추가 가능 |
| `src-tauri/src/commands/multi_provider.rs` | dispatch arm 추가 |
| `src-tauri/src/commands/stats.rs` | `StatsProvider` enum + dispatch |
| `src/utils/providers.ts` | capability flags, badge styles |
| `src/components/ProjectTree/index.tsx` | `providerCounts` 초기화 객체 |
| `src/i18n/locales/{en,ko,ja,zh-CN,zh-TW}/common.json` | provider 키 |

대부분 양쪽 변경 모두 보존하면 된다. 우리 `ditcodeagent` 라인을 upstream
추가분과 함께 살리는 식.

### i18n 타입 재생성

i18n 키가 바뀌면 타입 재생성:

```powershell
pnpm install
pnpm run generate:i18n-types
pnpm run i18n:validate
```

---

## 2. Production 빌드

평소 사용은 production 빌드 `.exe`로 설치해 일반 앱처럼 쓴다.
`pnpm exec tauri dev`는 개발용(느림 + Vite + 콘솔).

```powershell
cd D:\development\personal\claude-code-history-viewer-fork
$env:CARGO_BUILD_JOBS=2
pnpm exec tauri build
```

- 첫 빌드: **30 분 ~ 1 시간** (release optimization)
- 이후 incremental: 5 ~ 10 분
- `CARGO_BUILD_JOBS=2`로 메모리 안전성 확보. 더 빠른 머신이면 `4` 가능, OOM 나면 `1`.

### 결과물 위치

`src-tauri\target\release\bundle\` 아래:

| 파일 | 경로 | 용도 |
|---|---|---|
| `claude-code-history-viewer_X.Y.Z_x64-setup.exe` | `bundle/nsis/` | **권장** — 설치 후 시작 메뉴 등록 |
| `claude-code-history-viewer_X.Y.Z_x64_en-US.msi` | `bundle/msi/` | 회사 IT 정책상 MSI 필요할 때 |
| `claude-code-history-viewer.exe` | `target/release/` | portable (의존성 위험, 비추) |

---

## 3. .exe 설치

1. `bundle/nsis/*-setup.exe` 더블클릭
2. 설치 진행 → 시작 메뉴에 "Claude Code History Viewer" 등록
3. 평소엔 시작 메뉴에서 실행

### 업데이트 시

#### 방법 A — 자동 업데이트 (권장)

`v*` 태그를 푸시하면 GitHub Actions(`.github/workflows/fork-release.yml`)가
Windows 설치본을 빌드해 fork 저장소의 release에 업로드한다. 이미 설치된
팀원의 앱은 Tauri 업데이터가 `latest.json`을 보고 자동으로 새 버전을 받는다.

```powershell
# 버전 결정 (예: 1.12.0 → 1.13.0)
npm version 1.13.0 --no-git-tag-version
just sync-version

git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
git commit -m "chore: release v1.13.0"
git tag v1.13.0
git push && git push --tags
```

태그 푸시 후 5-15분이면 release가 발행되고, 팀원 앱이 다음 실행 시 (또는
설정 → 업데이트 확인 시) 새 버전을 가져온다.

> 자동 업데이트 활성화 절차는 §8 참고. 최초 1회 GitHub Secrets 등록이 필요하다.

#### 방법 B — 수동 빌드 (자동 업데이트 미설정 시)

업스트림 머지 + 빌드 후 새 `-setup.exe`를 그대로 더블클릭하면 기존 설치
위에 덮어쓰기 가능. 별도 uninstall 불필요.

---

## 4. 페이지 파일 설정 (메모리 OOM 방지)

이 머신에서 빌드 시 자주 발생한 에러들의 **공통 root cause는 페이지 파일 부족**이었다.
한 번만 설정하면 이후 안정적으로 빌드된다.

### 증상

- `rustc-LLVM ERROR: out of memory`
- `STATUS_STACK_BUFFER_OVERRUN (0xc0000409)`
- `failed to mmap file '...rlib': 이 작업을 완료하기 위한 페이징 파일이 너무 작습니다. (os error 1455)`
- `cannot find None/Ok/Err in this scope` (위 OOM의 2차 증상)

### 해결

1. `Win + R` → `sysdm.cpl` 실행
2. **고급** 탭 → 성능 영역 **설정**
3. **고급** 탭 → 가상 메모리 **변경**
4. **"모든 드라이브에 대해 페이징 파일 크기 자동 관리"** 체크 해제
5. **C:** 선택 → "페이징 파일 없음" → 설정
6. **D:** 선택 → "사용자 지정 크기"
   - 처음 크기: **16384 MB** (16 GB)
   - 최대 크기: **32768 MB** (32 GB)
   - 설정
7. **확인** → **확인** → 재부팅

C 드라이브 공간도 최소 20 GB 이상 여유 유지(Windows 자체 임시 파일 + 시스템 캐시).

---

## 5. 트러블슈팅

| 증상 | 원인 | 해결 |
|---|---|---|
| `rustc-LLVM ERROR: out of memory` | 페이지 파일 부족 | 위 §4. 또는 `$env:CARGO_BUILD_JOBS=1` |
| `STATUS_STACK_BUFFER_OVERRUN` | 페이지 파일 부족 | 위 §4. |
| `os error 1455` | 페이지 파일 부족 | 위 §4. |
| `can't find crate for X` (다수 crate 연속) | 이전 incomplete build 잔재 | `cd src-tauri && cargo clean` |
| `crate X required to be available in rlib format` | incremental cache 손상 | `cargo clean` 후 재빌드 |
| `cannot find None/Ok/Err in this scope` | OOM의 2차 증상 | 페이지 파일 fix가 root cause |
| husky pre-commit이 cargo clippy 막힘 | 환경에서 cargo 빌드 불가 | `package.json`의 lint-staged에서 Rust hook 임시 제거 → commit → 복원 |
| Tauri 버전 mismatch 경고 (`tauri v2.11.1 vs api v2.10.1`) | upstream develop 진행 중 alignment | **무시 가능** — 빌드/실행은 됨 |
| `Found version mismatched` 빌드 진행 후 멈춤 | 위와 동일 | 무시하고 진행 |

### 빠른 reset (마지막 수단)

```powershell
cd D:\development\personal\claude-code-history-viewer-fork\src-tauri
cargo clean
cd ..
$env:CARGO_BUILD_JOBS=1
pnpm exec tauri build
```

---

## 6. 평소 워크플로 한 줄 요약

```
[릴리즈] npm version X.Y.Z --no-git-tag-version → just sync-version → commit → git tag vX.Y.Z → push --tags
[수동]   pnpm exec tauri build → -setup.exe 덮어쓰기 설치 (자동 업데이트 미설정 시)
[일상]   시작 메뉴 → "Claude Code History Viewer"
```

업데이트 주기는 자유. 업스트림에 큰 기능 추가가 있을 때만 받아도 충분하다.
자동 업데이트가 활성화되면 팀원은 별도 작업 없이 다음 실행 시 새 버전을 받는다.

---

## 7. fork 내 DITCodeAgent 변경 위치 (참고)

업스트림 머지 시 충돌 해결 참고용. 모두 `ditcodeagent`/`DitCodeAgent`/`DITCodeAgent`로 검색 가능.

### Backend (Rust)

- `src-tauri/src/providers/ditcodeagent.rs` (신규 — gemini.rs 기반)
- `src-tauri/src/providers/mod.rs` — `ProviderId` enum + `parse` + `as_str` + `display_name` + `detect_providers`
- `src-tauri/src/commands/multi_provider.rs` — scan / load_sessions / load_messages / search 4 함수의 default array와 if-branch / match arm
- `src-tauri/src/commands/stats.rs` — `StatsProvider` enum, `stats_provider_id`, `all_stats_providers`, `parse_active_stats_providers`, `detect_project_provider`, `detect_session_provider`, `is_ditcodeagent_path` (신규 helper), 그 외 dispatch 다수

### Frontend (TypeScript / React)

- `src/types/core/session.ts` — `ProviderId` union
- `src/utils/providers.ts` — `PROVIDER_IDS`, `PROVIDER_TRANSLATIONS`, `PROVIDER_SESSION_CAPABILITIES`, `getProviderId` switch, `PROVIDER_BADGE_STYLES`
- `src/test/providers.utils.test.ts` — `PROVIDER_IDS` snapshot
- `src/components/ProjectTree/index.tsx` — `providerCounts` 초기화 객체
- `src/i18n/locales/{en,ko,ja,zh-CN,zh-TW}/common.json` — `common.provider.ditcodeagent`
- `src/i18n/types.generated.ts` — `pnpm run generate:i18n-types`로 재생성

---

## 8. 자동 업데이트 설정 (최초 1회)

`.github/workflows/fork-release.yml`이 fork 전용 Windows 릴리즈를 빌드한다.
첫 릴리즈 발행 전에 아래 1회성 설정이 필요하다.

### 8.1. 서명 키 (이미 생성됨)

`pnpm tauri signer generate`로 minisign 키쌍을 발급했다. 결과물 위치:

| 파일 | 용도 | 비고 |
|---|---|---|
| `.tauri/cchv-fork.key` | private key | **commit 금지** (`.gitignore` 처리됨). 백업 필수 — 분실 시 자동 업데이트 영구 불가 |
| `.tauri/cchv-fork.key.pub` | public key (base64) | `src-tauri/tauri.conf.json`의 `plugins.updater.pubkey`에 반영됨 |
| `.tauri/key-password.txt` | private key 비밀번호 | **commit 금지**. 백업 필수 |

새 키를 다시 만들고 싶다면 (예: 키 유출 시):

```powershell
$pw = -join ((48..57) + (65..90) + (97..122) | Get-Random -Count 32 | ForEach-Object {[char]$_})
$pw | Set-Content .tauri/key-password.txt -NoNewline
pnpm exec tauri signer generate -p $pw -w .tauri/cchv-fork.key -f --ci
```

그 후 `.tauri/cchv-fork.key.pub` 내용으로 `tauri.conf.json`의 `pubkey`를 교체하고
**모든 팀원이 새 .exe를 재설치**해야 자동 업데이트가 다시 동작한다 (구 pubkey로
설치된 앱은 새 키로 서명된 업데이트를 거부).

### 8.2. GitHub Secrets 등록

`minju-kim98/claude-code-history-viewer-fork` 저장소의
**Settings → Secrets and variables → Actions → New repository secret** 에 두 개 등록:

| Secret name | Value 소스 |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | `.tauri/cchv-fork.key`의 **파일 내용 전체** (경로 아님) |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | `.tauri/key-password.txt`의 내용 |

PowerShell로 클립보드에 복사하기:

```powershell
Get-Content .tauri/cchv-fork.key -Raw | Set-Clipboard
# GitHub Secrets 입력창에 붙여넣고 저장

Get-Content .tauri/key-password.txt -Raw | Set-Clipboard
# 마찬가지로 붙여넣고 저장
```

### 8.3. 첫 릴리즈 발행

위 시크릿 등록 후, 태그 푸시로 릴리즈 트리거:

```powershell
# 현재 1.12.0 → 1.13.0 예시
npm version 1.13.0 --no-git-tag-version
just sync-version

git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
git commit -m "chore: enable auto-update + release v1.13.0"
git tag v1.13.0
git push && git push --tags
```

진행 상황 확인:

```powershell
gh run watch --repo minju-kim98/claude-code-history-viewer-fork
gh release view v1.13.0 --repo minju-kim98/claude-code-history-viewer-fork
```

발행된 release에 다음 파일이 첨부되어야 한다:

- `Claude.Code.History.Viewer_X.Y.Z_x64-setup.exe` (NSIS 설치본)
- `Claude.Code.History.Viewer_X.Y.Z_x64-setup.exe.sig` (Tauri 업데이터 서명)
- `Claude.Code.History.Viewer_X.Y.Z_x64-portable.zip` (휴대용, 옵션)
- `latest.json` (Tauri 업데이터 메타데이터)

### 8.4. 팀원 첫 배포

자동 업데이트는 **이미 설치된 앱**부터 적용된다. 따라서:

1. 위 첫 릴리즈의 `*-setup.exe`를 팀원에게 공유 (또는 release 페이지 링크)
2. 팀원이 한 번 수동 설치
3. 이후 새 태그 푸시 → 팀원 앱이 자동으로 새 버전을 받음

### 8.5. 트러블슈팅

| 증상 | 원인 | 해결 |
|---|---|---|
| Actions에서 "TAURI_SIGNING_PRIVATE_KEY not set" | secret 미등록 | §8.2 |
| 자동 업데이트가 안 됨 (앱에서 "최신 버전입니다") | endpoint 또는 pubkey가 upstream을 가리킴 | `src-tauri/tauri.conf.json` 확인 후 재빌드, 팀원 재설치 |
| `Signature error: Failed to verify` | 구 pubkey로 설치된 앱이 새 키로 서명된 업데이트 수신 | 팀원이 release 페이지에서 새 `*-setup.exe`를 직접 받아 재설치 |
| release는 만들어지는데 `latest.json` 없음 | `includeUpdaterJson: false`로 잘못 설정 | `fork-release.yml`에서 `includeUpdaterJson: true` 확인 |
| upstream의 `updater-release.yml`이 동시에 실행되어 실패 | 자동 trigger가 살아있음 | 이미 `workflow_dispatch` only로 변경됨 (§§ 기존 upstream 워크플로우는 fork-guard 처리) |
