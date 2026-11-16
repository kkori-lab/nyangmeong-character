"""대조 테스트 실행기 : 가짜 Java·Python·러스트 서버를 띄우고 compare.py로 비교한 뒤 모두 끈다

scripts/contract.sh가 nyangmeong-ai 가상환경의 Python으로 부른다.
사용: <ai 가상환경 python> -I contract/run.py <nyangmeong-ai 경로>
"""

import http.client
import io
import os
import socket
import subprocess
import sys
import tarfile
import tempfile
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


def export_original(ai: Path, target: Path) -> str:
    """원본은 nyangmeong-ai의 develop이다(작업 중인 파일이 아님). `app/`·`data/`를 그 커밋에서 꺼내 target에 푼다 -> 커밋 줄"""
    ref = os.environ.get("AI_REF", "origin/develop")
    tar = subprocess.run(["git", "-C", str(ai), "archive", "--format=tar", ref, "app", "data"], capture_output=True)
    if tar.returncode != 0:
        raise SystemExit(f"{ai}에서 {ref}를 꺼내지 못했습니다(AI_REF로 바꿀 수 있음): {tar.stderr.decode(errors='replace')}")
    with tarfile.open(fileobj=io.BytesIO(tar.stdout)) as archive:
        archive.extractall(target, filter="data")
    return subprocess.run(
        ["git", "-C", str(ai), "log", "-1", "--format=%h %s", ref],
        capture_output=True,
        encoding="utf-8",
        errors="replace",
    ).stdout.strip()


def rust_binary() -> Path:
    name = "nyangmeong-character.exe" if os.name == "nt" else "nyangmeong-character"
    return ROOT / "target" / "debug" / name


def main() -> int:
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    ai = Path(sys.argv[1]).resolve()
    # 가짜는 서버마다 따로 둔다 : 두 서버가 밖으로 보낸 요청을 따로 기록해 비교한다
    ports = {name: free_port() for name in ("py_java", "rs_java", "py_openai", "rs_openai", "py", "rs")}
    url = {name: f"http://127.0.0.1:{port}" for name, port in ports.items()}
    base_env = os.environ.copy()
    # Python은 .env.local보다 환경변수가 우선 : 개인 키·로그 폴더를 비운다
    base_env.update({"OPENAI_API_KEY": "test-key", "CHARACTER_TIMEOUT_SEC": "5", "LOG_DIR": ""})
    quiet = {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL}
    procs: list[tuple[str, subprocess.Popen]] = []
    with tempfile.TemporaryDirectory() as original:
        commit = export_original(ai, Path(original))
        try:
            for name, script in (("py_java", "fake_java.py"), ("rs_java", "fake_java.py"), ("py_openai", "fake_openai.py"), ("rs_openai", "fake_openai.py")):
                proc = subprocess.Popen([sys.executable, "-I", str(HERE / script), str(ports[name])], **quiet)
                procs.append((f"가짜 {name}", proc))
            python = subprocess.Popen(
                [sys.executable, "-m", "uvicorn", "app.main:app", "--port", str(ports["py"]), "--log-level", "warning"],
                cwd=original,
                env=base_env | {"BACKEND_BASE_URL": url["py_java"], "OPENAI_BASE_URL": url["py_openai"] + "/v1"},
                **quiet,
            )
            procs.append(("Python", python))
            rust = subprocess.Popen(
                [str(rust_binary())],
                env=base_env | {"BACKEND_BASE_URL": url["rs_java"], "OPENAI_BASE_URL": url["rs_openai"] + "/v1", "PORT": str(ports["rs"])},
                **quiet,
            )
            procs.append(("러스트", rust))

            for name in ("py_java", "rs_java"):
                wait_ready(ports[name], "/internal/auth/check", procs[0][1], name)
            for name in ("py_openai", "rs_openai"):
                wait_ready(ports[name], "/__log", procs[2][1], name)
            wait_ready(ports["py"], "/health", python, "Python")
            wait_ready(ports["rs"], "/health", rust, "러스트")
            print(f"Python 원본: {ai.name} {commit}")
            fakes = {
                "python": {"java": url["py_java"], "openai": url["py_openai"]},
                "rust": {"java": url["rs_java"], "openai": url["rs_openai"]},
            }
            return compare.main(url["py"], url["rs"], verbose=bool(os.environ.get("CONTRACT_VERBOSE")), fakes=fakes)
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
