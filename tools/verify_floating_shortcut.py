"""浮窗全局快捷键实机验收；仅允许仓库外 memo-test-data，使用 Windows 真实按键。"""
import argparse
import ctypes
import json
import time
from pathlib import Path

import ui_drive as ui

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--data-dir', type=Path, required=True)
parser.add_argument('--output-dir', type=Path, required=True)
parser.add_argument('--port', type=int, default=9223)
parser.add_argument('--restart', action='store_true')
args = parser.parse_args()
main = ui.connect(args.port, timeout=45)


def invoke(name, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(name)}, {json.dumps(payload or {})})')


assert args.data_dir.name == 'memo-test-data'
assert args.data_dir.resolve() == Path(invoke('app_data_paths')['dataDir']).resolve()
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert not args.output_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
args.output_dir.mkdir(parents=True, exist_ok=True)
user32 = ctypes.windll.user32
user32.GetForegroundWindow.restype = ctypes.c_void_p
user32.ShowWindow.argtypes = [ctypes.c_void_p, ctypes.c_int]
user32.IsIconic.argtypes = [ctypes.c_void_p]
checks = []


def check(name, condition):
    assert condition, name
    checks.append(name)
    print('PASS: ' + name, flush=True)


def press_combo(letter):
    # keybd_event 注入系统键盘事件，不在 WebView 内伪造 KeyboardEvent。
    keys = (0x12, ord(letter))
    try:
        for key in keys:
            user32.keybd_event(key, 0, 0, 0)
        time.sleep(0.08)
    finally:
        for key in reversed(keys):
            user32.keybd_event(key, 0, 2, 0)


def floating_visible():
    return invoke('window_floating_state')['visible']


def summon(letter):
    press_combo(letter)
    wait(floating_visible)


def wait(condition):
    deadline = time.time() + 5
    while time.time() < deadline:
        if condition():
            return
        time.sleep(0.1)
    raise AssertionError('真实全局按键没有产生预期状态')


def button(text):
    main.eval(f'Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim()==={json.dumps(text)}).dataset.shortcutClick="1"')
    selector = '[data-shortcut-click]'
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main, selector)
    main.eval('document.querySelectorAll("[data-shortcut-click]").forEach(b=>delete b.dataset.shortcutClick)')


cfg = invoke('window_get_config')['config']
if args.restart:
    check('自定义浮窗快捷键重启保持', cfg['shortcutFloating'] == 'Alt+G')
    invoke('window_apply_action', {'action': 'hide_floating'})
    summon('G')
    check('重启后真实按键可呼出', floating_visible())
else:
    check('新默认快捷键可从原生配置读取', cfg['shortcutFloating'] == 'Alt+Q')
    cfg['shortcutEnabled'] = True
    invoke('window_set_config', {'config': cfg})
    invoke('window_apply_action', {'action': 'hide_floating'})
    main.eval('window.__TAURI_INTERNALS__.invoke("plugin:window|minimize",{label:"main"})')
    summon('Q')
    check('主窗最小化后真实按键呼出浮窗', floating_visible())
    floating = ui.connect(args.port, want='floating', timeout=20)
    try:
        wait(lambda: floating.eval('document.hasFocus()'))
        check('呼出后浮窗获得焦点', floating.eval('document.hasFocus()'))
        hwnd = user32.GetForegroundWindow()
        user32.ShowWindow(hwnd, 6)
        summon('Q')
        wait(lambda: not user32.IsIconic(hwnd))
        check('最小化的浮窗恢复', not user32.IsIconic(hwnd))
        invoke('window_apply_action', {'action': 'toggle_floating_click_through'})
        check('隔离验收已开启穿透', invoke('window_floating_state')['clickThrough'])
        press_combo('Q')
        wait(lambda: not floating_visible())
        check('再次按键隐藏已显示浮窗', not floating_visible())
        summon('Q')
        wait(lambda: not invoke('window_floating_state')['clickThrough'])
        check('呼出恢复鼠标操作', not invoke('window_floating_state')['clickThrough'])
    finally:
        floating.close()
    invoke('window_apply_action', {'action': 'show_main'})
    check('从浮窗打开主窗口恢复最小化状态', not main.eval('window.__TAURI_INTERNALS__.invoke("plugin:window|is_minimized",{label:"main"})'))
    wait(lambda: main.eval('document.hasFocus()'))
    check('主窗口内容恢复焦点', main.eval('document.hasFocus()'))
    button('设置')
    main.eval('Array.from(document.querySelectorAll("[role=tab]")).find(b=>b.textContent.trim()==="窗口与启动").dataset.shortcutTab="1"')
    ui.real_click(main, '[data-shortcut-tab]')
    assert main.wait_for('document.querySelector("[aria-label=\\"显示 / 隐藏悬浮窗快捷键\\"]")')
    selector = '[aria-label="显示 / 隐藏悬浮窗快捷键"]'
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main, selector)
    main.eval('const select=document.querySelector("[aria-label=\\"显示 / 隐藏悬浮窗快捷键\\"]"); select.value="Alt+G";select.dispatchEvent(new Event("change",{bubbles:true}));true')
    deadline = time.time() + 5
    while invoke('window_get_config')['config']['shortcutFloating'] != 'Alt+G':
        assert time.time() < deadline
        time.sleep(0.1)
    check('设置页面的独立选项落盘', True)
    invoke('window_apply_action', {'action': 'hide_floating'})
    press_combo('Q')
    time.sleep(0.7)
    check('换键后旧组合不再呼出', not floating_visible())
    summon('G')
    check('换键后新组合立即生效', floating_visible())
    cfg = invoke('window_get_config')['config']
    cfg['shortcutEnabled'] = False
    invoke('window_set_config', {'config': cfg})
    invoke('window_apply_action', {'action': 'hide_floating'})
    press_combo('G')
    time.sleep(0.7)
    check('关闭全局快捷键后不再响应', not floating_visible())
    cfg = invoke('window_get_config')['config']
    cfg['shortcutEnabled'] = True
    invoke('window_set_config', {'config': cfg})
    check('重新启用时保留刚才的隐藏状态', not floating_visible())
    summon('G')
    check('重新启用恢复响应', floating_visible())
    press_combo('G')
    wait(lambda: not floating_visible())
    check('自定义两键也支持再次隐藏', not floating_visible())

(args.output_dir / ('shortcut-restart.json' if args.restart else 'shortcut-native.json')).write_text(
    json.dumps({'checks': checks, 'passed': len(checks)}, ensure_ascii=False, indent=2), encoding='utf-8')
main.close()
