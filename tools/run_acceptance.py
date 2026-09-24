"""Launch a disposable Lumen instance and run the CDP acceptance suite.

Usage: python tools/run_acceptance.py --exe path/to/Lumen.exe --log path/to/run.log
The selected executable must already contain the frontend assets.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8", errors="replace")
sys.stderr.reconfigure(encoding="utf-8", errors="replace")


def free_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_for_cdp(port: int, app: subprocess.Popen[bytes], deadline: float) -> None:
    endpoint = f"http://127.0.0.1:{port}/json/list"
    while time.monotonic() < deadline:
        if app.poll() is not None:
            raise RuntimeError(f"Lumen exited before CDP was ready (exit {app.returncode})")
        try:
            with urllib.request.urlopen(endpoint, timeout=1) as response:
                if response.status == 200:
                    return
        except (OSError, urllib.error.URLError):
            pass
        time.sleep(0.5)
    raise TimeoutError(f"CDP did not open at {endpoint} within 60 seconds")


def remove_successful_profile(profile: Path) -> None:
    """Remove only the unique direct child created in the system temp folder."""
    resolved = profile.resolve(strict=True)
    temp_root = Path(tempfile.gettempdir()).resolve(strict=True)
    if resolved.parent != temp_root or not resolved.name.startswith("lumen-acceptance-"):
        raise RuntimeError(f"Refusing to remove an unexpected profile path: {resolved}")
    for attempt in range(5):
        try:
            shutil.rmtree(resolved)
            return
        except OSError:
            if attempt == 4:
                raise
            time.sleep(0.5 * (attempt + 1))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exe", required=True, type=Path)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--suite", choices=("remediation3", "architecture", "scale", "recurrence", "recurrence_restart"), default="remediation3")
    args = parser.parse_args()
    exe = args.exe.resolve(strict=True)
    if not exe.is_file():
        parser.error("--exe must be a file")
    if importlib.util.find_spec("websocket") is None:
        parser.error("websocket-client is required: python -m pip install -r tools/requirements-acceptance.txt")

    args.log.parent.mkdir(parents=True, exist_ok=True)
    # The temporary profile is unique per run and is never an alias of the
    # production profile. The suite separately checks the app's actual path.
    profile = Path(tempfile.mkdtemp(prefix="lumen-acceptance-"))
    port = free_port()
    env = os.environ.copy()
    env["LUMEN_TEST_DATA_DIR"] = str(profile)
    env["LUMEN_CDP_PORT"] = str(port)
    env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={port}"
    print(f"Isolated profile: {profile}", flush=True)
    print(f"CDP port: {port}", flush=True)
    print(f"Log: {args.log.resolve()}", flush=True)

    result_code = 1
    with args.log.open("w", encoding="utf-8") as log:
        log.write(f"exe={exe}\nprofile={profile}\nport={port}\n")
        log.flush()
        app = subprocess.Popen([str(exe)], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            wait_for_cdp(port, app, time.monotonic() + 60)
            command = [
                sys.executable,
                str(Path(__file__).with_name(f"verify_{args.suite}.py")),
                "--expect-data-dir",
                str(profile),
            ]
            if args.suite == "recurrence_restart":
                command.extend(["--phase", "seed"])
            result = subprocess.run(command, env=env, capture_output=True, text=True,
                                    encoding="utf-8", errors="replace", check=False)
            log.write("\n===== acceptance stdout =====\n" + result.stdout)
            log.write("\n===== acceptance stderr =====\n" + result.stderr)
            log.flush()
            print(result.stdout, end="", flush=True)
            if result.stderr:
                print(result.stderr, file=sys.stderr, end="", flush=True)
            result_code = result.returncode
        finally:
            app.terminate()
            try:
                app.wait(timeout=10)
            except subprocess.TimeoutExpired:
                app.kill()
                app.wait(timeout=10)
    if result_code == 0 and args.suite == "recurrence_restart":
        # A second process opens the same disposable profile. This is an actual
        # app restart; both stages re-check the app's resolved data directory.
        port = free_port()
        env["LUMEN_CDP_PORT"] = str(port)
        env["WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS"] = f"--remote-debugging-port={port}"
        print(f"Restarting isolated profile on CDP port {port}", flush=True)
        with args.log.open("a", encoding="utf-8") as log:
            log.write(f"\nrestart_port={port}\n")
            log.flush()
            app = subprocess.Popen([str(exe)], env=env, stdout=log, stderr=subprocess.STDOUT)
            try:
                wait_for_cdp(port, app, time.monotonic() + 60)
                command = [sys.executable, str(Path(__file__).with_name(
                    "verify_recurrence_restart.py")), "--expect-data-dir", str(profile),
                    "--phase", "after"]
                result = subprocess.run(command, env=env, capture_output=True, text=True,
                                        encoding="utf-8", errors="replace", check=False)
                log.write("\n===== restart acceptance stdout =====\n" + result.stdout)
                log.write("\n===== restart acceptance stderr =====\n" + result.stderr)
                log.flush()
                print(result.stdout, end="", flush=True)
                if result.stderr:
                    print(result.stderr, file=sys.stderr, end="", flush=True)
                result_code = result.returncode
            finally:
                app.terminate()
                try:
                    app.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    app.kill()
                    app.wait(timeout=10)
    if result_code == 0:
        try:
            remove_successful_profile(profile)
            print("Isolated profile removed after successful acceptance", flush=True)
        except OSError as exc:
            print(f"Acceptance passed but profile cleanup failed: {exc}", file=sys.stderr)
            print(f"Profile retained: {profile}", file=sys.stderr)
            return 1
    else:
        print(f"Acceptance failed; profile retained: {profile}", file=sys.stderr)
    return result_code


if __name__ == "__main__":
    raise SystemExit(main())
