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

업스트림 머지 + 빌드 후 새 `-setup.exe`를 그대로 더블클릭하면 기존 설치
위에 덮어쓰기 가능. 별도 uninstall 불필요.

> Tauri의 자동 업데이터(`updater plugin`)는 GitHub Release의 `latest.json`을
> 참조하는데, fork에 release를 만들지 않으므로 자동 업데이트는 동작하지
> 않는다. **수동 빌드 → 수동 설치**가 워크플로.

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
[가끔]   git fetch upstream && git merge upstream/develop → pnpm exec tauri build → -setup.exe 덮어쓰기 설치
[일상]   시작 메뉴 → "Claude Code History Viewer"
```

업데이트 주기는 자유. 업스트림에 큰 기능 추가가 있을 때만 받아도 충분하다.

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
