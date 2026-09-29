"""已安装 Windows 程序的开机自启验收；恢复本账户原有启动项，不修改任务。

先通过 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 开启 CDP，再运行：
python tools/verify_autostart.py --exe <已安装的 lumen.exe> --output <仓库外验收目录>
"""
import argparse
import base64
import json
from pathlib import Path
import winreg

import ui_drive as ui

RUN = r"Software\Microsoft\Windows\CurrentVersion\Run"
APPROVED = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"
NAME = "lumen"


def read_value(path):
    try:
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path) as key:
            return winreg.QueryValueEx(key, NAME)
    except FileNotFoundError:
        return None


def restore_value(path, saved):
    if saved is not None:
        with winreg.CreateKey(winreg.HKEY_CURRENT_USER, path) as key:
            winreg.SetValueEx(key, NAME, 0, saved[1], saved[0])
    else:
        try:
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, path, 0, winreg.KEY_SET_VALUE) as key:
                winreg.DeleteValue(key, NAME)
        except FileNotFoundError:
            pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--port', type=int, default=9222)
    args = parser.parse_args()
    assert args.exe.is_file(), '已安装的程序不存在'
    args.output.mkdir(parents=True, exist_ok=True)
    before = {path: read_value(path) for path in (RUN, APPROVED)}
    backup = args.output / 'autostart-registry-before.json'
    assert not backup.exists(), '拒绝覆盖原有自启快照'
    backup.write_text(json.dumps(before, default=lambda v: {'base64': base64.b64encode(v).decode()}, indent=2), encoding='utf-8')
    target = ui.connect(args.port, timeout=45)
    checks = []

    def invoke(command):
        return target.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(command)})')

    def verify_state(expected):
        assert target.wait_for(f'(() => {{ const e=document.querySelector("[aria-label=\\"开机自动启动\\"]"); return e && !e.disabled && e.checked === {json.dumps(expected)} }})()')
        assert invoke('plugin:autostart|is_enabled') == expected

    try:
        assert target.wait_for('document.querySelector(".sidebar")')
        target.eval('Array.from(document.querySelectorAll(".sidebar button")).find(b=>b.textContent.trim()==="设置").dataset.verifySettings="1"')
        target.eval('document.querySelector("[data-verify-settings]").scrollIntoView({block:"center"})')
        ui.real_click(target, '[data-verify-settings]')
        assert target.wait_for('document.querySelector(".settings")')
        target.eval('Array.from(document.querySelectorAll("[role=tab]")).find(b=>b.textContent.trim()==="窗口与启动").dataset.verifyStartup="1"')
        ui.real_click(target, '[data-verify-startup]')
        initial = invoke('plugin:autostart|is_enabled')
        verify_state(initial)
        checks.append('设置入口显示系统实际状态')
        toggle = '[aria-label="开机自动启动"]'
        if initial:
            ui.real_click(target, toggle)
            verify_state(False)
        ui.real_click(target, toggle)
        verify_state(True)
        value = read_value(RUN)
        assert value and value[0].strip().strip('"').lower() == str(args.exe.resolve()).lower(), '自启路径不是已安装程序'
        assert read_value(APPROVED)[0] == bytes([2] + [0] * 11)
        checks.append('真实开启并注册已安装程序路径')
        ui.real_click(target, '[aria-label="重新读取开机自启状态"]')
        verify_state(True)
        checks.append('重新读取保留开启状态')

        # 模拟用户在任务管理器中禁用，只触碰本程序自己的值。
        with winreg.OpenKey(winreg.HKEY_CURRENT_USER, APPROVED, 0, winreg.KEY_SET_VALUE) as key:
            winreg.SetValueEx(key, NAME, 0, winreg.REG_BINARY, bytes([3, 0, 0, 0, 1] + [0] * 7))
        ui.real_click(target, '[aria-label="重新读取开机自启状态"]')
        verify_state(False)
        checks.append('识别系统外部禁用')
        ui.real_click(target, toggle)
        verify_state(True)
        checks.append('重新开启恢复系统允许状态')
        ui.real_click(target, toggle)
        verify_state(False)
        assert read_value(RUN) is None
        checks.append('真实关闭并删除启动项')
    finally:
        for path, saved in before.items():
            restore_value(path, saved)
        assert all(read_value(path) == saved for path, saved in before.items()), '原有自启状态恢复失败'
        target.close()

    checks.append('原有两个注册表值完整恢复')
    target = ui.connect(args.port)
    try:
        ui.real_click(target, '[aria-label="重新读取开机自启状态"]')
        verify_state(initial)
        (args.output / 'installed-autostart.png').write_bytes(base64.b64decode(target.call('Page.captureScreenshot', {'format': 'png'})['data']))
        target.eval("setTimeout(()=>window.__TAURI_INTERNALS__.invoke('app_quit'),200);true")
    finally:
        target.close()
    report = {'checks': checks, 'originalEnabled': initial, 'registryRestored': True, 'taskWrites': False, 'actualWindowsLoginTested': False}
    (args.output / 'installed-verification.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False))


if __name__ == '__main__':
    main()
