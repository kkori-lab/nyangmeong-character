#!/bin/sh
# 검증 명령. AGENTS.md·verify-collect.sh·end-check.sh·pr-prep.sh가 모두 이 파일을 부른다.
# CI(.github/workflows/build.yml)는 대조 테스트를 뺀 cargo 검사만 돈다(nyangmeong-ai 저장소가 없어서).
# 실패한 검사의 출력만 보여 준다. 종료 코드는 처음 실패한 검사의 것이다.
# 사용: sh scripts/check.sh
cd "$(git rev-parse --show-toplevel)" || exit 1
PATH="$HOME/.cargo/bin:$PATH"

run() {
  title=$1
  shift
  out=$("$@" 2>&1)
  code=$?
  if [ "$code" != 0 ]; then
    echo "$title → $code"
    printf '%s\n' "$out" | grep -v '^[[:space:]]*$' | tail -60
    exit "$code"
  fi
}

run "cargo fmt --check" cargo fmt --check
run "cargo clippy" cargo clippy --all-targets -q -- -D warnings
run "cargo test" cargo test -q
run "대조 테스트 (Python↔러스트, contract/)" sh scripts/contract.sh
echo "cargo fmt · clippy · test · 대조 테스트 통과"
