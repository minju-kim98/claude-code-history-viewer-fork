# Fork 운영 가이드

이 fork(`minju-kim98/claude-code-history-viewer-fork`)는 업스트림
[`jhlee0409/claude-code-history-viewer`](https://github.com/jhlee0409/claude-code-history-viewer)에
**DITCodeAgent provider**를 추가한 사내용 fork다. 업스트림에 기여하지 않고
fork 내부에서만 운영한다.

## 1. 월간 릴리즈 한 사이클

`v*` 태그를 푸시하면 GitHub Actions(`.github/workflows/fork-release.yml`)가
Windows 설치본을 빌드해 fork release에 업로드한다. 이미 설치된 팀원 앱은
Tauri 업데이터가 `latest.json`을 보고 자동으로 새 버전을 받는다.

> 이 fork는 단일 feature 브랜치만 운영한다 (default branch
> = `feature/ditcodeagent-provider`, origin에 `develop` 없음, 로컬 `develop`은
> `upstream/develop`을 직접 추적). 그래서 feature 브랜치에서
> `upstream/develop`으로 바로 rebase한다.

**예시**: 1.13.0 → 1.14.0

```powershell
cd D:\development\personal\claude-code-history-viewer-fork

# 1) upstream 동기화 + rebase
git fetch upstream
git checkout feature/ditcodeagent-provider
git rebase upstream/develop          # 충돌 시 §2 참고

# 2) 버전 bump
#    단순 tag만 달면 latest.json의 version이 안 올라가 Tauri 업데이터가
#    새 버전으로 인식하지 못한다. 반드시 npm version + just sync-version.
npm version 1.14.0 --no-git-tag-version    # patch면 1.13.1
just sync-version                          # Cargo.toml + tauri.conf.json 동기화

# 3) release commit + tag + push
git add package.json src-tauri/Cargo.toml src-tauri/tauri.conf.json
git commit -m "chore: release v1.14.0"
git push --force-with-lease                # rebase 결과 + 새 commit 한 번에
git tag v1.14.0
git push origin v1.14.0                    # 이 push가 GitHub Actions trigger

# 4) ~15분 후 자동 빌드 완료. 모니터링 + 검증:
gh run watch -R minju-kim98/claude-code-history-viewer-fork
gh release view v1.14.0 -R minju-kim98/claude-code-history-viewer-fork
#    release에 다음 5종 첨부 확인:
#    - *_x64-setup.exe + .sig    (NSIS 설치본 + 서명)
#    - *_x64_en-US.msi + .sig    (MSI 설치본 + 서명)
#    - *_x64-portable.zip        (휴대용)
#    - latest.json               (Tauri 업데이터 메타데이터)
```

**팀원 경험**: 아무 작업도 필요 없다. 앱 실행 시 `useUpdater.ts`가 endpoint
(`latest.json`)를 폴링하고 새 버전 발견 시 `SimpleUpdateModal` 팝업이 뜬다.
"업데이트" 클릭 → 다운로드 → minisign 서명 검증 → 설치 → 재시작.

업데이트 주기는 자유. 업스트림에 큰 기능 추가가 있을 때만 받아도 충분하다.

### ⚠️ 운영 시 반드시 지킬 두 가지

1. **`.tauri/` 폴더 외부 백업 필수** — `cchv-fork.key`, `cchv-fork.key.pub`,
   `key-password.txt`. 분실하면 자동 업데이트가 영구 깨지고 팀원 전원 재설치
   해야 한다. 1Password / 회사 비밀 저장소에 보관.
2. **버전은 항상 SemVer 단조 증가** — 1.13.0 → 1.13.1 → 1.14.0. Tauri 업데이터는
   `>` 비교만 하므로 같은 버전 재발행 시 다운로드되지 않는다. 잘못 발행했다면
   다음 patch 버전으로 재발행.

### 신규 팀원 첫 설치

자동 업데이트는 **이미 설치된 앱**부터 적용된다. 신규 팀원에게는 release 페이지
링크를 전달:

```
https://github.com/minju-kim98/claude-code-history-viewer-fork/releases/latest
```

거기서 `*-setup.exe`를 받아 한 번 수동 설치하면, 이후 새 태그 push마다 자동으로
받아간다. 서명 키 검증 때문에 다른 키로 만든 빌드를 쓰던 팀원은 재설치 필요.

> 자동 업데이트 활성화 절차(GitHub Secrets, 키 생성)는 §7 참고. 최초 1회만 필요.

---

## 2. 업스트림 머지 충돌 해결

`upstream` remote는 이미 설정되어 있다. rebase 시 다음 파일에서 충돌이 자주
발생한다. 대부분 양쪽 변경 모두 보존하면 된다 — 우리 `ditcodeagent` 라인을
upstream 추가분과 함께 살리는 식.

| 파일 | 사유 |
|---|---|
| `src-tauri/src/providers/mod.rs` | upstream이 새 provider 추가 가능 |
| `src-tauri/src/commands/multi_provider.rs` | dispatch arm 추가 |
| `src-tauri/src/commands/stats.rs` | `StatsProvider` enum + dispatch |
| `src/utils/providers.ts` | capability flags, badge styles |
| `src/components/ProjectTree/index.tsx` | `providerCounts` 초기화 객체 |
| `src/i18n/locales/{en,ko,ja,zh-CN,zh-TW}/common.json` | provider 키 |

자세한 변경 위치는 §6 참고.

### i18n 타입 재생성

i18n 키가 바뀌면 타입 재생성:

```powershell
pnpm install
pnpm run generate:i18n-types
pnpm run i18n:validate
```

---

## 3. 로컬 Production 빌드 (수동 / 디버깅용)

자동 업데이트가 정상이면 GitHub Actions가 빌드해주므로 로컬 빌드는 평소엔
불필요하다. 자동 빌드 fail 디버깅이나 머지 전 사전 검증할 때만 쓴다.
`pnpm exec tauri dev`는 개발용(느림 + Vite + 콘솔), 평소 사용은 production
빌드 `.exe`로 설치해 일반 앱처럼 쓴다.

```powershell
cd D:\development\personal\claude-code-history-viewer-fork
$env:CARGO_BUILD_JOBS=2
pnpm exec tauri build
```

- 첫 빌드: **30분 ~ 1시간** (release optimization)
- 이후 incremental: 5 ~ 10분
- `CARGO_BUILD_JOBS=2`로 메모리 안전성 확보. 더 빠른 머신이면 `4` 가능, OOM 나면 `1`.

### 결과물 위치

`src-tauri\target\release\bundle\` 아래:

| 파일 | 경로 | 용도 |
|---|---|---|
| `claude-code-history-viewer_X.Y.Z_x64-setup.exe` | `bundle/nsis/` | **권장** — 설치 후 시작 메뉴 등록 |
| `claude-code-history-viewer_X.Y.Z_x64_en-US.msi` | `bundle/msi/` | 회사 IT 정책상 MSI 필요할 때 |
| `claude-code-history-viewer.exe` | `target/release/` | portable (의존성 위험, 비추) |

### .exe 설치

1. `bundle/nsis/*-setup.exe` 더블클릭
2. 설치 진행 → 시작 메뉴에 "Claude Code History Viewer" 등록
3. 평소엔 시작 메뉴에서 실행

새 `-setup.exe`를 그대로 더블클릭하면 기존 설치 위에 덮어쓰기 가능.
별도 uninstall 불필요.

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

### 로컬 빌드

| 증상 | 원인 | 해결 |
|---|---|---|
| `rustc-LLVM ERROR: out of memory` | 페이지 파일 부족 | §4. 또는 `$env:CARGO_BUILD_JOBS=1` |
| `STATUS_STACK_BUFFER_OVERRUN` | 페이지 파일 부족 | §4. |
| `os error 1455` | 페이지 파일 부족 | §4. |
| `can't find crate for X` (다수 crate 연속) | 이전 incomplete build 잔재 | `cd src-tauri && cargo clean` |
| `crate X required to be available in rlib format` | incremental cache 손상 | `cargo clean` 후 재빌드 |
| `cannot find None/Ok/Err in this scope` | OOM의 2차 증상 | 페이지 파일 fix가 root cause |
| husky pre-commit이 cargo clippy 막힘 | 환경에서 cargo 빌드 불가 | `package.json`의 lint-staged에서 Rust hook 임시 제거 → commit → 복원 |
| Tauri 버전 mismatch 경고 (`tauri v2.11.1 vs api v2.10.1`) | upstream develop 진행 중 alignment | **무시 가능** — 빌드/실행은 됨 |
| `Found version mismatched` 빌드 진행 후 멈춤 | 위와 동일 | 무시하고 진행 |

### 자동 업데이트 / GitHub Actions

| 증상 | 원인 | 해결 |
|---|---|---|
| Actions에서 `TAURI_SIGNING_PRIVATE_KEY not set` | secret 미등록 | §7.2 |
| 자동 업데이트가 안 됨 (앱에서 "최신 버전입니다") | endpoint/pubkey가 upstream을 가리킴 | `src-tauri/tauri.conf.json` 확인 후 재빌드, 팀원 재설치 |
| `Signature error: Failed to verify` | 구 pubkey로 설치된 앱이 새 키로 서명된 업데이트 수신 | 팀원이 release 페이지에서 새 `*-setup.exe`를 직접 받아 재설치 |
| release는 만들어지는데 `latest.json` 없음 | `includeUpdaterJson: false`로 잘못 설정 | `fork-release.yml`에서 `includeUpdaterJson: true` 확인 |
| `'just' is not recognized` | Windows runner에 just 미설치 | `fork-release.yml`의 `Install just` step 확인 |
| upstream의 `updater-release.yml`이 동시 실행되어 실패 | 자동 trigger 살아있음 | 이미 `workflow_dispatch` only로 변경됨 + fork-guard 처리 |

### 빠른 reset (로컬 빌드 마지막 수단)

```powershell
cd D:\development\personal\claude-code-history-viewer-fork\src-tauri
cargo clean
cd ..
$env:CARGO_BUILD_JOBS=1
pnpm exec tauri build
```

---

## 6. fork 내 DITCodeAgent 변경 위치 (참고)

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

## 7. 자동 업데이트 설정 (최초 1회)

`.github/workflows/fork-release.yml`이 fork 전용 Windows 릴리즈를 빌드한다.
첫 릴리즈 발행 전에 아래 1회성 설정이 필요하다 (이미 완료된 상태).

### 7.1. 서명 키 (이미 생성됨)

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

### 7.2. GitHub Secrets 등록

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

---

## 8. DITCodeAgent 토큰 백필 스크립트

`scripts/ditcodeagent_backfill_tokens.py` — DITCodeAgent 로컬 세션 로그의
`tokens` 필드를 채워 viewer의 토큰/비용 통계가 0으로 뜨는 문제를 우회한다.

### 배경

DITCodeAgent CLI는 세션 파일
(`~/.ditcodeagent/tmp/<project>/chats/session-*.json`)의 assistant(`type: "gemini"`)
메시지에 `tokens`를 **항상 `null`로** 남긴다. 그래서 viewer가 이 provider의
사용량/비용을 전부 0으로 표시한다. 실제 사용량은 애초에 기록되지 않아 복원이
불가능하므로, 이 스크립트는 Anthropic의 `POST /v1/messages/count_tokens`로
**근사치를 재구성**해 `null` 자리를 채운다.

> ⚠️ **근사치**다. 시스템 프롬프트·툴 정의가 로그에 없고 실제 캐시 hit/miss도
> 알 수 없어, "차트가 0 대신 의미 있는 추정치를 보여준다" 수준이지 청구서 수준
> 정확도가 아니다. 이 스크립트로 채운 값은 viewer 표시에만 쓰고 비용 정산 근거로
> 쓰지 않는다.

채워지는 값 (assistant 메시지마다):

| 필드 | 의미 |
|---|---|
| `input` | 직전 응답 이후 새로 들어간 user/tool 턴 토큰 |
| `cached` | 모델이 이미 본 이전 컨텍스트 토큰 (캐시 동작 모방 → 세션이 길어도 `input` 폭증 안 함) |
| `output` | 그 응답이 생성한 내용 토큰 (텍스트 + thinking) |
| `thoughts` / `tool` | 0 (viewer가 안 읽음. thinking은 `output`에 포함됨) |
| `total` | `input + cached + output` |

### 사용법

```powershell
# 0) 의존성 (선택): .env 로딩용
pip install python-dotenv

# 1) API 키 설정 — 아래 둘 중 하나
#    (a) 레포 루트에 .env 파일 (커밋 금지, .gitignore 확인)
#        ANTHROPIC_API_KEY=sk-ant-...
#    (b) 환경변수 직접 설정
$env:ANTHROPIC_API_KEY = "sk-ant-..."

# 2) 키 없이 현황만 확인
python scripts/ditcodeagent_backfill_tokens.py --scan

# 3) 소규모 시험 (파일 2개만)
python scripts/ditcodeagent_backfill_tokens.py --limit 2 --backup

# 4) 전체 백필 (.bak 백업 권장)
python scripts/ditcodeagent_backfill_tokens.py --backup

# 미리보기만 (쓰기 안 함)
python scripts/ditcodeagent_backfill_tokens.py --dry-run
```

주요 플래그: `--scan`(API 무호출 현황만), `--dry-run`(계산만, 쓰기 X),
`--force`(이미 채워진 것도 재계산), `--backup`(`.bak` 보존), `--rpm N`(분당 호출 제한),
`--limit N`(파일 N개만), `--root PATH`(`DITCODEAGENT_HOME` 오버라이드).

### 안전성 / 멱등성

- 이미 채워진 메시지는 건너뛴다 (`--force`로 강제 재계산). **주기적으로 재실행해도
  안전** — 새로 생긴 `null`만 채운다.
- atomic write (temp + `os.replace`), `tokens` 외 필드는 건드리지 않는다.
- **DITCodeAgent가 해당 세션을 동시에 쓰지 않을 때** 돌릴 것.
- 의존성 0 (표준 라이브러리만, `.env`만 `python-dotenv` 선택).

### Rate limit

`count_tokens`는 **무료**이고 Messages API와 **별개 한도**라 실제 사용량/예산에
영향이 없다. 메시지당 ~1회 호출(현재 데이터 ~2,300회 수준).

| Usage tier | RPM | ~2,300회 소요(스로틀 없이) |
|---|---|---|
| 1 | 100 | ~23분 (429 자동 백오프) |
| 2 | 2,000 | ~1.2분 |
| 3 | 4,000 | ~35초 |
| 4 | 8,000 | ~17초 |

Tier 1이면 `--rpm 90`으로 선제 스로틀해 429를 피한다. 어느 경우든 429/5xx는
지수 백오프로 자동 재시도하므로 실패하지 않고 느려질 뿐이다.

### 근본 해결

이건 어디까지나 우회책이다. 근본 해결은 DITCodeAgent CLI가 API 응답의 usage를
세션 로그 `tokens`에 직접 기록하는 것이며, 해당 피드백을 CLI 팀에 전달했다.
