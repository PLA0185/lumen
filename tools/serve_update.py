"""本地更新服务器：给自动更新做"确实能发现新版本"的实机验证。

## 为什么需要它

自动更新的正常端点是 GitHub Releases，验证时要先发一个更高的版本出去，
既慢又会污染真实仓库。Tauri 的 updater 插件在**调试构建**下允许 `http://`
端点（源码里用 `#[cfg(debug_assertions)]` 放行，发布版会直接拒绝非
HTTPS），因此用 `tauri dev` + 本地 HTTP 服务就能把
「拉清单 → 解析 → 比版本号 → 显示新版本」这条链路真跑一遍。

下载/安装那一步会**故意失败**：本地清单里的签名是假的，插件校验不过。
这本身也是证据——说明签名校验确实在生效，而不是摆设。

用法：
    python tools/serve_update.py --port 8788 --version 9.9.9
"""

from __future__ import annotations

import argparse
import json
import time
from http.server import BaseHTTPRequestHandler, HTTPServer


def build_manifest(version: str, repo: str) -> dict:
    return {
        "version": version,
        "notes": "这是本地验证用的假清单，只用来验证「能发现新版本」。\n下载安装会因签名校验失败而被拒绝，属预期行为。",
        "pub_date": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "platforms": {
            "windows-x86_64": {
                "url": f"http://127.0.0.1:8788/fake-setup.nsis.zip",
                # 故意给一个格式正确但内容为假的签名
                "signature": "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNpZ25lcgo=",
            }
        },
    }


def make_handler(manifest: dict):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            if self.path.startswith("/latest.json"):
                body = json.dumps(manifest, ensure_ascii=False).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "application/json; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                print(f"[serve_update] 已返回 latest.json（version={manifest['version']}）", flush=True)
            else:
                self.send_response(404)
                self.end_headers()

        def log_message(self, *args):  # 静音默认日志
            pass

    return Handler


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8788)
    ap.add_argument("--version", default="9.9.9")
    ap.add_argument("--repo", default="PLA0185/lumen")
    args = ap.parse_args()

    manifest = build_manifest(args.version, args.repo)
    httpd = HTTPServer(("127.0.0.1", args.port), make_handler(manifest))
    print(f"[serve_update] 监听 http://127.0.0.1:{args.port}/latest.json （版本 {args.version}）", flush=True)
    httpd.serve_forever()


if __name__ == "__main__":
    main()
