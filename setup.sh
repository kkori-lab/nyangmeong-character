#!/usr/bin/env bash
# 냥멍케어 캐릭터 서버(러스트) 개발 환경 설치. 맥은 터미널에서 `bash setup.sh`, Windows는 cmd·PowerShell에서 `setup.bat`(또는 Git Bash에서 `bash setup.sh`)로 실행한다.
# 이미 맞는 버전이 있으면 건너뛴다. 여러 번 실행해도 된다.
# 백엔드·프론트 저장소의 setup.sh와 공통 부분(맨 위 함수와 "공통" 절)이 같다. 고칠 때 셋 다 고친다. VS Code·MCP 함수는 이 저장소에서 쓰지 않아 뺐다(DMS MCP 제외, 2026-11-17).
set -euo pipefail
cd "$(dirname "$0")"

TODO=0
NEED_RESTART=0
ok()   { printf '  [완료] %s\n' "$1"; }
skip() { printf '  [있음] %s\n' "$1"; }
todo() { printf '  [직접] %s\n' "$1"; TODO=$((TODO + 1)); }
die()  { printf '\n[중단] %s\n' "$1" >&2; exit 1; }
step() { printf '\n== %s\n' "$1"; }

case "$(uname -s)" in
  Darwin) OS=mac ;;
  MINGW* | MSYS*) OS=win ;;
  *) die "지원하지 않는 환경입니다: $(uname -s). 맥은 터미널에서 bash setup.sh, Windows는 setup.bat로 실행해 주세요." ;;
esac

# 패키지 설치. 맥은 Homebrew, Windows는 winget
# pkg <brew 이름> <formula|cask> <winget ID> [winget 버전]
pkg() {
  if [ "$OS" = mac ]; then
    command -v brew >/dev/null || die "Homebrew가 없습니다. https://brew.sh 의 설치 명령을 먼저 실행해 주세요."
    if [ "$2" = cask ]; then brew install --cask "$1"; else brew install "$1"; fi
  else
    command -v winget.exe >/dev/null || die "winget이 없습니다. Microsoft Store에서 '앱 설치 관리자'를 설치해 주세요."
    if [ -n "${4:-}" ]; then
      winget.exe install --id "$3" -e --version "$4" --accept-package-agreements --accept-source-agreements
    else
      winget.exe install --id "$3" -e --accept-package-agreements --accept-source-agreements
    fi
    # winget이 바꾼 PATH는 새 창에서만 적용된다
    NEED_RESTART=1
  fi
}

# 셸 설정 파일에 한 줄을 넣는다. 이미 있으면 그대로 둔다
persist_line() {
  mkdir -p "$(dirname "$1")"
  touch "$1"
  grep -qxF "$2" "$1" || printf '\n%s\n' "$2" >> "$1"
}

# 새 창에서도 이 폴더의 명령이 잡히도록 PATH에 영구로 넣는다. 맥은 ~/.zshrc, Windows는 사용자 PATH
persist_path() {
  if [ "$OS" = mac ]; then
    persist_line "$HOME/.zshrc" "export PATH=\"$1:\$PATH\""
    return
  fi
  # %USERPROFILE% 같은 항목이 풀리지 않도록 레지스트리 값을 ExpandString으로 그대로 다시 쓴다.
  # 이미 있는지는 양쪽 값을 풀고 끝의 역슬래시와 대소문자를 무시해 비교한다(%USERPROFILE% 형태로 들어 있어도 같은 폴더로 본다)
  # 마지막 줄은 열려 있는 탐색기에 환경변수 변경을 알리려고 빈 변수를 지우는 것이다
  WIN_DIR="$(cygpath -w "$1")" powershell.exe -NoProfile -Command '
    $key = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey("Environment", $true)
    $parts = @(([string]$key.GetValue("Path", "", "DoNotExpandEnvironmentNames")) -split ";" | Where-Object { $_ })
    $norm = { param($p) [Environment]::ExpandEnvironmentVariables($p).TrimEnd("\").ToLowerInvariant() }
    $want = & $norm $env:WIN_DIR
    if (-not ($parts | Where-Object { (& $norm $_) -eq $want })) {
      $key.SetValue("Path", (($parts + $env:WIN_DIR) -join ";"), [Microsoft.Win32.RegistryValueKind]::ExpandString)
      [Environment]::SetEnvironmentVariable("NYANGMEONG_SETUP", $null, "User")
    }'
}


# 이니셜 a가 b와 한 글자만 다르거나 이웃한 두 글자만 바뀌었으면 참
near() {
  local a=$1 b=$2 i diff=""
  [ "${#a}" = "${#b}" ] || return 1
  for ((i = 0; i < ${#a}; i++)); do
    [ "${a:i:1}" = "${b:i:1}" ] || diff="$diff $i"
  done
  set -- $diff
  [ $# = 1 ] && return 0
  [ $# = 2 ] && [ $(($2 - $1)) = 1 ] && [ "${a:$1:1}" = "${b:$2:1}" ] && [ "${a:$2:1}" = "${b:$1:1}" ]
}

# 입력을 대문자로 바꿔 INITIALS 중 하나로 맞춘다. 같으면 그것, 비슷한 것이 하나면 확인 후 그것, 아니면 다시 묻는다
ask_initials() {
  local input c found answer
  while :; do
    read -rp "  개인 브랜치로 쓸 이니셜을 입력하세요($INITIALS 중 하나): " input
    input=$(printf '%s' "$input" | tr -d '[:space:]' | tr '[:lower:]' '[:upper:]')
    [ -n "$input" ] || continue
    found=""
    for c in $INITIALS; do
      if [ "$c" = "$input" ]; then initials=$c; return 0; fi
      if near "$input" "$c"; then found="$found $c"; fi
    done
    set -- $found
    case $# in
      0) printf '  %s 은(는) 목록에 없습니다.\n' "$input" ;;
      1)
        read -rp "  -> $1 으로 진행할까요? [Y/n] " answer
        case "$answer" in [nN]*) ;; *) initials=$1; return 0 ;; esac
        ;;
      *) printf '  %s 와 비슷한 이니셜이 여럿입니다:%s. 정확히 입력해 주세요.\n' "$input" "$found" ;;
    esac
  done
}

step "공통"

command -v git >/dev/null || die "Git이 없습니다. Windows는 Git for Windows를 설치한 뒤 다시 실행해 주세요."
skip "Git $(git --version | awk '{print $3}')"

# AI 도구: 설치된 Claude Code·Codex를 감지해 그 도구에 MCP를 등록한다. 둘 다 없으면 Claude Code를 설치한다.
# Codex만 쓰려면 Codex를 먼저 설치하고 실행한다. 다시 실행하면 새로 설치한 도구도 감지한다
has_claude=n
has_codex=n
if command -v claude >/dev/null; then
  has_claude=y
  skip "Claude Code $(claude --version 2>/dev/null | awk '{print $1}')"
fi
if command -v codex >/dev/null; then
  has_codex=y
  skip "Codex $(codex --version 2>/dev/null | awk '{print $NF}')"
fi
# Claude Code: 공식 설치 스크립트가 ~/.local/bin에 설치한다. 로그인은 처음 실행할 때 직접 한다
if [ "$has_claude$has_codex" = nn ]; then
  if [ "$OS" = mac ]; then
    curl -fsSL https://claude.ai/install.sh | bash
  else
    powershell.exe -NoProfile -ExecutionPolicy Bypass -Command 'irm https://claude.ai/install.ps1 | iex'
  fi
  export PATH="$HOME/.local/bin:$PATH"
  command -v claude >/dev/null || die "Claude Code 설치에 실패했습니다. 위 출력을 확인해 주세요."
  persist_path "$HOME/.local/bin"
  has_claude=y
  ok "Claude Code $(claude --version 2>/dev/null | awk '{print $1}')"
  todo "저장소 폴더에서 claude 를 실행해 로그인해 주세요"
fi
use_claude() { [ "$has_claude" = y ]; }
use_codex() { [ "$has_codex" = y ]; }

# 전역 git 설정(회사 계정 등)은 건드리지 않고 이 저장소에만 둔다. 전역 값이 있으면 그대로 쓸지 묻는다
for key in user.name user.email; do
  if [ -n "$(git config --local "$key" || true)" ]; then
    skip "git $key = $(git config --local "$key")"
  else
    value=$(git config --global "$key" || true)
    if [ -n "$value" ]; then
      read -rp "  이 저장소에도 전역 git $key($value) 값을 사용하시겠습니까? [Y/n] " answer
      case "$answer" in [nN]*) value="" ;; esac
    fi
    while [ -z "$value" ]; do read -rp "  이 저장소에 사용할 git $key 값을 입력하세요: " value; done
    git config --local "$key" "$value"
    ok "git $key = $value"
  fi
done

# 커밋 훅: 커밋 메시지에서 AI 도구가 붙인 Co-Authored-By 줄을 지운다(.githooks/commit-msg, docs/convention/git.md)
if [ "$(git config --local core.hooksPath || true)" = .githooks ]; then
  skip "git 커밋 훅 (.githooks)"
else
  git config --local core.hooksPath .githooks
  ok "git 커밋 훅 (.githooks)"
fi

# 개인 브랜치 이름 = 이니셜. 핸드오프 파일 이름과 커밋 접두사가 여기서 나온다.
# 쓸 수 있는 이니셜은 마일스톤 담당 표(docs/convention/milestone.md "담당" 절)의 첫 열이다
INITIALS=$(awk '/^## /{f = ($0 == "## 담당")} f && /^\| [A-Z]+ \|/{print $2}' docs/convention/milestone.md | tr '\n' ' ' | sed 's/ $//')
[ -n "$INITIALS" ] || die "docs/convention/milestone.md 의 담당 표에서 이니셜을 읽지 못했습니다."
branch=$(git branch --show-current)
branch_ok=y
case "$branch" in
  "" | main | master | develop)
    ask_initials
    if git show-ref --verify --quiet "refs/heads/$initials"; then
      git checkout "$initials"
    elif git ls-remote --exit-code --heads origin "$initials" >/dev/null 2>&1; then
      git fetch origin "$initials" && git checkout -b "$initials" --track "origin/$initials"
    else
      git checkout -b "$initials"
    fi
    branch=$initials
    ok "브랜치 $branch"
    ;;
  *)
    if printf ' %s ' "$INITIALS" | grep -qF " $branch "; then
      skip "브랜치 $branch"
    else
      # 이니셜이 아닌 브랜치는 올리지 않고 이름을 바꾸라고 안내한다. 대소문자만 다르면 -M 이어야 맥·Windows에서 바뀐다
      branch_ok=n
      upper=$(printf '%s' "$branch" | tr '[:lower:]' '[:upper:]')
      if printf ' %s ' "$INITIALS" | grep -qF " $upper "; then
        todo "브랜치 $branch 는 대문자 이니셜이 아닙니다. git branch -M $upper 로 바꾼 뒤 다시 실행해 주세요"
      else
        todo "브랜치 $branch 는 이니셜($INITIALS)이 아닙니다. git checkout develop 한 뒤 다시 실행해 이니셜 브랜치를 만들어 주세요"
      fi
    fi
    ;;
esac

# 원격에 없는 개인 브랜치는 공개 저장소이므로 물어보고 올린다
if [ "$branch_ok" = y ] && ! git ls-remote --exit-code --heads origin "$branch" >/dev/null 2>&1; then
  read -rp "  origin에 $branch 브랜치가 없습니다. 지금 올리시겠습니까? [Y/n] " answer
  case "$answer" in
    [nN]*) todo "개인 브랜치를 올려 주세요: git push -u origin $branch" ;;
    *)
      if git push -q -u origin "$branch"; then ok "브랜치 $branch 푸시"
      else todo "푸시하지 못했습니다. 저장소 권한을 확인한 뒤 git push -u origin $branch 를 실행해 주세요"; fi
      ;;
  esac
fi

step "러스트"

# C++ 빌드 도구(Windows): 러스트 MSVC 툴체인이 링크에 link.exe를 쓴다. vswhere로 VC 도구가 든 Visual Studio를 찾고, 없으면 Build Tools를 설치한다.
# 수 GB이고 관리자 권한 창이 뜬다. pkg는 --override를 받지 않아 winget을 직접 부른다
if [ "$OS" = win ]; then
  vswhere="$(cygpath -u "$(printenv 'ProgramFiles(x86)' || printf 'C:\Program Files (x86)')")/Microsoft Visual Studio/Installer/vswhere.exe"
  has_msvc() {
    [ -x "$vswhere" ] && [ -n "$("$vswhere" -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath 2>/dev/null | tr -d '\r')" ]
  }
  if has_msvc; then
    skip "C++ 빌드 도구 (MSVC)"
  else
    command -v winget.exe >/dev/null || die "winget이 없습니다. Microsoft Store에서 '앱 설치 관리자'를 설치해 주세요."
    winget.exe install --id Microsoft.VisualStudio.2022.BuildTools -e --accept-package-agreements --accept-source-agreements \
      --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended" || true
    NEED_RESTART=1
    if has_msvc; then ok "C++ 빌드 도구 (MSVC)"
    else todo "C++ 빌드 도구를 설치하지 못했습니다. Visual Studio Installer에서 'C++를 사용한 데스크톱 개발'을 설치한 뒤 다시 실행해 주세요"; fi
  fi
fi

# rustup: 러스트 툴체인을 맡는다. ~/.cargo/bin에 설치되고, 새 창에서도 잡히도록 PATH에 영구로 넣는다
export PATH="$HOME/.cargo/bin:$PATH"
if command -v rustup >/dev/null; then
  skip "rustup $(rustup --version 2>/dev/null | awk '{print $2}')"
else
  if [ "$OS" = mac ]; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
  else
    pkg rustup formula Rustlang.Rustup
  fi
  command -v rustup >/dev/null || die "rustup 설치에 실패했습니다. 위 출력을 확인해 주세요."
  ok "rustup $(rustup --version 2>/dev/null | awk '{print $2}')"
fi
persist_path "$HOME/.cargo/bin"

# stable 툴체인과 검증 명령(scripts/check.sh)이 쓰는 rustfmt·clippy
installed=$(rustup component list --toolchain stable --installed 2>/dev/null | tr -d '\r' || true)
if printf '%s\n' "$installed" | grep -q '^rustfmt' && printf '%s\n' "$installed" | grep -q '^clippy'; then
  skip "Rust stable $(rustup run stable rustc --version | awk '{print $2}') (rustfmt, clippy)"
else
  rustup toolchain install stable --profile minimal --component rustfmt,clippy
  ok "Rust stable $(rustup run stable rustc --version | awk '{print $2}') (rustfmt, clippy)"
fi
if rustup default >/dev/null 2>&1; then
  skip "기본 툴체인 $(rustup default | tr -d '\r' | awk '{print $1}')"
else
  rustup default stable
  ok "기본 툴체인 stable"
fi

if [ "$NEED_RESTART" = 1 ]; then
  printf '
[다시 실행] 새로 설치한 프로그램은 새 창에서만 인식됩니다. 창을 닫았다가 새로 열고 같은 명령(setup.bat 또는 bash setup.sh)을 한 번 더 실행해 주세요.
'
  exit 2
fi

# 설정: .env.local(git 제외)을 디스코드에서 받아 쓴다. 키 목록은 .env.local.example 에 있다.
# 키가 없거나 값이 비면 빠진 것으로 본다(scripts/start-check.sh 와 같은 기준).
# 비워도 되는 키(OPENAI_BASE_URL: 없거나 비우면 https://api.openai.com/v1)는 start-check.sh 처럼 세지 않는다
if [ ! -f .env.local ]; then
  todo "디스코드의 최신 .env.local 을 받아 저장소 폴더에 두세요(키 목록은 .env.local.example)"
else
  missing=""
  for key in $(sed 's/\r$//' .env.local.example | grep -E '^[A-Za-z_][A-Za-z0-9_.-]*[[:space:]]*=' | sed 's/[[:space:]]*=.*//'); do
    case "$key" in (OPENAI_BASE_URL) continue ;; esac
    re=$(printf '%s' "$key" | sed 's/\./\\./g')
    value=$(sed 's/\r$//' .env.local | grep -E "^$re[[:space:]]*=" | tail -1 | sed -e 's/^[^=]*=[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^"\(.*\)"$/\1/' || true)
    [ -n "$value" ] || missing="$missing $key"
  done
  if [ -n "$missing" ]; then
    todo ".env.local 에 값이 없는 키가 있습니다:$missing. 디스코드의 최신 파일로 바꿔 주세요"
  else
    skip ".env.local (.env.local.example 의 키 전부)"
  fi
fi

# 대조 테스트(scripts/contract.sh)는 옆 폴더의 nyangmeong-ai 와 그 .venv 로 Python 서버를 띄운다. 찾는 순서는 contract.sh 와 같다.
# 없어도 서버 개발은 되므로 멈추지 않고 할 일로 남긴다
ai=""
for d in "${AI_REPO:-}" ../ai ../nyangmeong-ai; do
  if [ -n "$d" ] && [ -f "$d/app/main.py" ]; then ai=$d; break; fi
done
if [ -z "$ai" ]; then
  todo "대조 테스트용 원본이 없습니다. 옆 폴더에 git clone https://github.com/kkori-lab/nyangmeong-ai.git 한 뒤 그 저장소의 setup 을 실행해 주세요(폴더 이름이 ai·nyangmeong-ai 가 아니면 AI_REPO=<경로>)"
elif [ -x "$ai/.venv/Scripts/python.exe" ] || [ -x "$ai/.venv/bin/python" ]; then
  skip "대조 테스트 원본 $ai (.venv)"
else
  todo "$ai 에 .venv 가 없습니다. 그 저장소에서 setup(또는 uv sync)을 실행해 주세요"
fi

step "확인"
cargo build -q || die "cargo build 에 실패했습니다. 위 출력을 확인해 주세요."
ok "cargo build"

# 서버가 뜨고 /health 에 답하는지 보고 끈다. 포트는 비어 있는 아무 포트(0)를 쓰고, 로그의 listening on 줄에서 실제 포트를 읽는다
log=$(mktemp)
PORT=0 LOG_DIR= ./target/debug/nyangmeong-character > "$log" 2>&1 &
pid=$!
port=""
for _ in $(seq 1 30); do
  port=$(sed -n 's/.*listening on [0-9.]*:\([0-9][0-9]*\).*/\1/p' "$log" | head -1)
  [ -n "$port" ] && break
  kill -0 "$pid" 2>/dev/null || break
  sleep 1
done
health=""
[ -z "$port" ] || health=$(curl -s --max-time 5 "http://127.0.0.1:$port/health" || true)
kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true
if [ "$health" != '{"status":"ok"}' ]; then
  tail -40 "$log"
  rm -f "$log"
  die "서버가 뜨지 않았거나 /health 가 {\"status\":\"ok\"} 가 아닙니다(받은 값: ${health:-없음}). 위 로그를 확인해 주세요."
fi
rm -f "$log"
ok "서버 기동 (/health, 포트 $port)"

printf '\n설치가 끝났습니다. 직접 할 일 %d개(위 [직접] 줄).\n' "$TODO"
# 직접 할 일이 남으면 0이 아닌 코드로 끝낸다
[ "$TODO" = 0 ] || exit 3
