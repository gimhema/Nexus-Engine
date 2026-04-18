# NexusLauncher — 개발 의존성

Tauri 2 기반 런처 개발에 필요한 패키지 및 설치 방법을 플랫폼별로 정리합니다.

---

## 공통 (모든 플랫폼)

### 1. Rust

Tauri 백엔드 컴파일러. `rustup`을 통해 설치하고 관리한다.

```bash
# 설치
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 버전 확인
rustc --version
cargo --version
rustup --version

# 업데이트
rustup update stable
```

> **현재 환경**: rustc 1.95.0 / cargo 1.95.0 / rustup 1.27.1 ✅

---

### 2. Node.js (v18 이상)

프론트엔드 빌드 도구 실행 환경.

```bash
# 버전 확인
node --version
npm --version
```

> **현재 환경**: Node.js v22.22.2 / npm 10.9.7 ✅

---

### 3. Tauri CLI

Tauri 프로젝트 생성·개발 서버·빌드 명령을 제공하는 CLI.
Rust 방식(cargo)과 npm 방식 중 하나를 선택한다.

```bash
# Rust 방식 (권장 — cargo로 직접 빌드, 버전 일관성 보장)
cargo install tauri-cli --version "^2"

# npm 방식 (빠른 설치, npx로 실행 가능)
npm install -g @tauri-apps/cli@^2

# 버전 확인
cargo tauri --version
# 또는
npx tauri --version
```

> **현재 환경**: tauri-cli 2.10.1 ✅

---

### 주요 CLI 명령어 (공통)

| 명령어 | 설명 |
|---|---|
| `npm create tauri-app@latest` | 신규 Tauri 프로젝트 생성 (대화형) |
| `cargo tauri init` | 기존 웹 프로젝트에 Tauri 추가 |
| `cargo tauri dev` | 개발 서버 실행 (핫리로드) |
| `cargo tauri build` | 프로덕션 빌드 (플랫폼별 설치 파일 생성) |
| `cargo tauri info` | 현재 환경 의존성 진단 (누락 패키지 확인용) |
| `cargo tauri add <plugin>` | 공식 플러그인 추가 |

---

## Linux (Fedora)

### 시스템 패키지

WebView2 대신 WebKitGTK를 렌더러로 사용한다. 아래 패키지가 없으면 빌드 실패.

```bash
sudo dnf install \
    webkit2gtk4.1-devel \
    javascriptcoregtk4.1-devel \
    openssl-devel \
    libappindicator-gtk3-devel \
    librsvg2-devel \
    curl \
    wget \
    file \
    xdotool
```

> **참고**: Fedora 버전에 따라 `webkit2gtk4.1-devel` 대신 `webkit2gtk3-devel`이 필요할 수 있음.
> 누락 패키지는 `cargo tauri info` 로 진단 가능.

---

## Windows

### 시스템 요구사항

| 항목 | 요구사항 |
|---|---|
| OS | Windows 10 이상 (WebView2 내장) |
| Build Tools | Microsoft C++ Build Tools 또는 Visual Studio 2022 |
| WebView2 | Windows 10 / 11 내장. 이하 버전은 별도 설치 필요 |

```powershell
# Microsoft C++ Build Tools 설치 (winget)
winget install Microsoft.VisualStudio.2022.BuildTools

# WebView2 Runtime (Windows 10 이하 대상)
winget install Microsoft.EdgeWebView2Runtime

# Rust 설치 (Windows)
# https://rustup.rs 에서 rustup-init.exe 다운로드 후 실행
# 또는 winget
winget install Rustlang.Rustup
```

---

## 프론트엔드 빌드 도구

Tauri는 프론트엔드 번들러로 **Vite**를 권장한다.
`npm create tauri-app@latest` 실행 시 프레임워크·언어 선택에 따라 자동 구성된다.

```bash
# 신규 프로젝트 생성 (권장)
npm create tauri-app@latest

# 기존 프로젝트에 Tauri API 수동 추가
npm install @tauri-apps/api@^2
npm install -D @tauri-apps/cli@^2 vite typescript
```

| 패키지 | 용도 |
|---|---|
| `vite` | 프론트엔드 번들러 / 개발 서버 (핫리로드) |
| `@tauri-apps/api` | Tauri JS/TS API (프로세스, 파일시스템, 이벤트 등) |
| `@tauri-apps/cli` | Tauri CLI (npm 방식 사용 시) |
| `typescript` | TypeScript 컴파일러 |

---

## 유용한 Tauri 공식 플러그인

필요 시 `cargo tauri add <plugin>` 으로 추가한다.

| 플러그인 | 용도 |
|---|---|
| `shell` | 외부 프로세스 실행 (서버 spawn 등) |
| `fs` | 파일 시스템 접근 |
| `process` | 앱 재시작·종료 제어 |
| `notification` | OS 알림 (서버 크래시 감지 등) |
| `log` | 런처 로그 파일 기록 |

---

## 설치 순서 요약

```
1. Rust + rustup          공통  (이미 설치됨 ✅)
2. Node.js + npm          공통  (이미 설치됨 ✅)
3. 시스템 패키지          플랫폼별
   Linux  → sudo dnf install webkit2gtk4.1-devel ...
   Windows → winget install Microsoft.VisualStudio.2022.BuildTools
4. Tauri CLI              cargo install tauri-cli --version "^2"
5. 프로젝트 생성          npm create tauri-app@latest
```