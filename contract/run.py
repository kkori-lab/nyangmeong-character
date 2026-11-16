"""대조 테스트 실행기 : 가짜 Java·Python·러스트 서버를 띄우고 compare.py로 비교한 뒤 모두 끈다

scripts/contract.sh가 nyangmeong-ai 가상환경의 Python으로 부른다.
사용: <ai 가상환경 python> -I contract/run.py <nyangmeong-ai 경로>
"""

import http.client
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

import compare  # noqa: E402


def free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def wait_ready(port: int, path: str, proc: subprocess.Popen, name: str) -> None:
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise SystemExit(f"{name} 서버가 떠 있지 않습니다(종료 코드 {proc.returncode})")
        try:
            conn = http.client.HTTPConnection("127.0.0.1", port, timeout=2)
            conn.request("GET", path)
            conn.getresponse().read()
            conn.close()
            return
        except OSError:
            time.sleep(0.3)
    raise SystemExit(f"{name} 서버가 60초 안에 뜨지 않았습니다")


def rust_binary() -> Path:
    name = "nyangmeong-character.exe" if os.name == "nt" else "nyangmeong-character"
    return ROOT / "target" / "debug" / name


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    ai = Path(sys.argv[1]).resolve()
    java_port, py_port, rs_port = free_port(), free_port(), free_port()
    java_url = f"http://127.0.0.1:{java_port}"
    env = os.environ.copy()
    # Python은 .env.local보다 환경변수가 우선 : 개인 키·로그 폴더를 비운다
    env.update({"BACKEND_BASE_URL": java_url, "OPENAI_API_KEY": "", "LOG_DIR": ""})
    quiet = {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    procs: list[tuple[str, subprocess.Popen]] = []
    try:
        java = subprocess.Popen([sys.executable, "-I", str(HERE / "fake_java.py"), str(java_port)], **quiet)
        procs.append(("가짜 Java", java))
        python = subprocess.Popen(
            [sys.executable, "-m", "uvicorn", "app.main:app", "--port", str(py_port), "--log-level", "warning"],
            cwd=ai,
            env=env,
            **quiet,
        )
        procs.append(("Python", python))
        rust = subprocess.Popen([str(rust_binary())], env=env | {"PORT": str(rs_port)}, **quiet)
        procs.append(("러스트", rust))

        wait_ready(java_port, "/internal/auth/check", java, "가짜 Java")
        wait_ready(py_port, "/health", python, "Python")
        wait_ready(rs_port, "/health", rust, "러스트")
        commit = subprocess.run(
            ["git", "-C", str(ai), "log", "-1", "--format=%h %s"],
            capture_output=True,
            encoding="utf-8",
            errors="replace",
        ).stdout.strip()
        print(f"Python 원본: {ai.name} {commit}")
        return compare.main(f"http://127.0.0.1:{py_port}", f"http://127.0.0.1:{rs_port}")
    finally:
        for _, proc in procs:
            proc.terminate()
        for _, proc in procs:
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()


if __name__ == "__main__":
    sys.exit(main())
