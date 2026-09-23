"""通过 CDP 驱动 Lumen 的 WebView2 界面（实机验收用）。

## 为什么用这条路

Lumen 的界面跑在 WebView2（Chromium）里，而 Windows 的 UI Automation
**看不到 WebView2 内部的元素**（实测只能枚举到 16 个外壳元素），
所以"点某个按钮"这种验收没法用 UI Automation 做。

WebView2 支持 Chromium 的远程调试协议（CDP）：只要给进程设置环境变量
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`，
就能连上每个窗口，用 `Runtime.evaluate` 查 DOM、派发真实事件、
读取渲染结果——这比"截图猜坐标"可靠得多，也能给出可复现的证据。

本模块只做连接与通用操作；具体的验收断言写在 `verify_*.py` 里。
"""

from __future__ import annotations

import json
import time
import urllib.request

import websocket  # websocket-client


class CdpError(RuntimeError):
    pass


class Target:
    """一个可执行 JS 的调试目标（对应一个窗口的 WebView）。"""

    def __init__(self, ws_url: str, info: dict):
        self.info = info
        # Chromium 会拒绝带 Origin 头的 WebSocket 连接
        # （报 403 "Rejected an incoming WebSocket connection from ..."），
        # 而 websocket-client 默认会带上，所以必须显式关掉。
        self._ws = websocket.create_connection(ws_url, timeout=30, suppress_origin=True)
        self._id = 0

    def close(self) -> None:
        try:
            self._ws.close()
        except Exception:
            pass

    def call(self, method: str, params: dict | None = None) -> dict:
        self._id += 1
        msg_id = self._id
        self._ws.send(json.dumps({"id": msg_id, "method": method, "params": params or {}}))
        while True:
            raw = self._ws.recv()
            if not raw:
                raise CdpError("调试连接被关闭")
            data = json.loads(raw)
            if data.get("id") == msg_id:
                if "error" in data:
                    raise CdpError(f"{method} 失败：{data['error']}")
                return data.get("result", {})

    def eval(self, expression: str, await_promise: bool = True):
        """执行 JS 表达式并返回其值（支持 Promise）。"""
        res = self.call(
            "Runtime.evaluate",
            {
                "expression": expression,
                "awaitPromise": await_promise,
                "returnByValue": True,
                "userGesture": True,
            },
        )
        if "exceptionDetails" in res:
            detail = res["exceptionDetails"]
            text = detail.get("exception", {}).get("description") or detail.get("text")
            raise CdpError(f"JS 异常：{text}")
        return res.get("result", {}).get("value")

    def wait_for(self, expression: str, timeout: float = 10.0, interval: float = 0.2):
        """轮询等待某个 JS 表达式返回真值。"""
        probe = "(() => { try { return Boolean(" + expression + ") } catch (e) { return false } })()"
        deadline = time.time() + timeout
        while time.time() < deadline:
            if self.eval(probe):
                return True
            time.sleep(interval)
        return False


def _http_json(url: str):
    with urllib.request.urlopen(url, timeout=5) as r:
        return json.loads(r.read().decode("utf-8"))


def list_targets(port: int = 9222) -> list[dict]:
    return _http_json(f"http://127.0.0.1:{port}/json/list")


def connect(port: int = 9222, probe: str = "document.querySelector('.app') ? 'main' : (document.querySelector('.floating') ? 'floating' : 'other')", want: str = "main",
            timeout: float = 20.0) -> Target:
    """连接指定窗口。

    三个窗口共用同一份前端产物，URL 完全相同，只能靠 DOM 特征区分，
    因此这里对每个目标执行 probe 表达式，取第一个匹配 want 的。
    """
    deadline = time.time() + timeout
    while time.time() < deadline:
        try:
            targets = [t for t in list_targets(port) if t.get("type") == "page" and t.get("webSocketDebuggerUrl")]
        except Exception:
            time.sleep(0.5)
            continue
        for t in targets:
            try:
                cand = Target(t["webSocketDebuggerUrl"], t)
            except Exception:
                continue
            try:
                kind = cand.eval(probe, await_promise=False)
            except CdpError:
                cand.close()
                continue
            if kind == want:
                return cand
            cand.close()
        time.sleep(0.5)
    raise CdpError(f"在 {timeout}s 内没找到 {want} 窗口（调试端口 {port}）")


# --------------------------------------------------------------------------
# 常用操作
# --------------------------------------------------------------------------

def set_react_input(target: Target, selector: str, value: str) -> None:
    """给受控输入框赋值。

    React 会跟踪 input 的 value；直接改 `.value` 不会触发 onChange，
    必须用原型上的原生 setter 再派发 input 事件。
    """
    js = """
        (() => {
          const el = document.querySelector(%s);
          if (!el) throw new Error('找不到输入框：' + %s);
          const proto = el instanceof HTMLTextAreaElement
            ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
          const setter = Object.getOwnPropertyDescriptor(proto, 'value').set;
          setter.call(el, %s);
          el.dispatchEvent(new Event('input', { bubbles: true }));
          return el.value;
        })()
    """ % (
        json.dumps(selector),
        json.dumps(selector),
        json.dumps(value),
    )
    target.eval(js)


def press_key(target: Target, selector: str, key: str = "Enter") -> None:
    js = """
        (() => {
          const el = document.querySelector(%s);
          if (!el) throw new Error('找不到元素：' + %s);
          el.dispatchEvent(new KeyboardEvent('keydown', { key: %s, bubbles: true, cancelable: true }));
          return true;
        })()
    """ % (
        json.dumps(selector),
        json.dumps(selector),
        json.dumps(key),
    )
    target.eval(js)


def click(target: Target, selector: str, index: int = 0) -> None:
    """点击第 index 个匹配元素（用 `.click()`，走真实的事件冒泡）。"""
    js = """
        (() => {
          const list = document.querySelectorAll(%s);
          const el = list[%d];
          if (!el) throw new Error('找不到元素：' + %s + '[' + %d + ']');
          el.click();
          return true;
        })()
    """ % (
        json.dumps(selector),
        index,
        json.dumps(selector),
        index,
    )
    target.eval(js)


def real_click(target: Target, selector: str, index: int = 0) -> None:
    """用 CDP 的 Input 域派发**真实的鼠标按下/抬起**。

    与 `element.click()` 的区别很关键：`click()` 只发一个 click 事件，
    不经过 mousedown。而"按钮点了没反应"这类缺陷恰恰出在 mousedown 上
    （例如窗口拖动把 mousedown 抢走，click 就永远不会发生）。
    要验证按钮真的能用，必须走这条路径。
    """
    box = target.eval(
        """
        (() => {
          const el = document.querySelectorAll(%s)[%d];
          if (!el) throw new Error('找不到元素：' + %s + '[' + %d + ']');
          const r = el.getBoundingClientRect();
          return { x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width, h: r.height };
        })()
        """
        % (json.dumps(selector), index, json.dumps(selector), index)
    )
    if not box or box["w"] <= 0 or box["h"] <= 0:
        raise CdpError(f"元素不可点击（尺寸为 0）：{selector}[{index}]")
    x, y = box["x"], box["y"]
    for kind in ("mousePressed", "mouseReleased"):
        target.call(
            "Input.dispatchMouseEvent",
            {
                "type": kind,
                "x": x,
                "y": y,
                "button": "left",
                "buttons": 1 if kind == "mousePressed" else 0,
                "clickCount": 1,
            },
        )
        time.sleep(0.05)


def text_of(target: Target, selector: str) -> str | None:
    return target.eval(
        "(() => { const el = document.querySelector(%s); return el ? el.innerText : null; })()"
        % json.dumps(selector)
    )
