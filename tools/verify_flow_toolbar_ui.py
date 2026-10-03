"""Verify the fitted toolbar, chapter navigation and save stability in an isolated WebView2."""
import argparse
import base64
import json
import time
import uuid
from pathlib import Path
import ui_drive as ui

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--port', type=int, required=True)
parser.add_argument('--data-dir', type=Path, required=True)
parser.add_argument('--output-dir', type=Path, required=True)
args = parser.parse_args()
t = ui.connect(args.port)
checks = []


def invoke(command, params=None):
    return t.eval('window.__TAURI_INTERNALS__.invoke(' + json.dumps(command) + ',' + json.dumps(params or {}) + ')')


def check(label, condition):
    assert condition, label
    checks.append(label)
    print('PASS: ' + label, flush=True)


def click(label):
    probe = '[...document.querySelectorAll("button")].find(b=>!b.disabled&&(b.textContent.trim()===' + json.dumps(label) + '||b.getAttribute("aria-label")===' + json.dumps(label) + '||b.querySelector("strong")?.textContent===' + json.dumps(label) + '))'
    assert t.wait_for(probe), label
    t.eval(probe + '.dataset.toolbarQa="1"')
    ui.real_click(t, '[data-toolbar-qa]')
    t.eval('document.querySelectorAll("[data-toolbar-qa]").forEach(b=>delete b.dataset.toolbarQa)')


def rect(selector):
    return t.eval('(()=>{const r=document.querySelector(' + json.dumps(selector) + ').getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()')


def drag(start, end):
    for kind, point in [('mousePressed', start), ('mouseMoved', end), ('mouseReleased', end)]:
        t.call('Input.dispatchMouseEvent', {'type': kind, 'button': 'left', 'buttons': 0 if kind == 'mouseReleased' else 1, 'clickCount': 1, **point})


def geometry():
    return t.eval('(()=>{const bar=document.querySelector(".memos__toolbar"),r=bar.getBoundingClientRect(),buttons=[...bar.querySelectorAll("button,select")].map(e=>{const b=e.getBoundingClientRect();return {label:e.textContent.trim(),left:b.left,right:b.right,center:b.top+b.height/2}});return {left:r.left,right:r.right,buttons,scale:document.querySelector(".memos__toolbar-row").style.transform,scroll:bar.scrollLeft,overflow:getComputedStyle(bar).overflowX}})()')


assert args.data_dir.resolve() != (Path.home() / 'AppData/Roaming/com.pla0185.lumen').resolve()
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert Path(invoke('app_data_paths')['dataDir']).resolve() == args.data_dir.resolve()
assert not invoke('cloud_sync_status')['config']
args.output_dir.mkdir(parents=True, exist_ok=True)
try:
    encoded = t.eval('(()=>{const c=document.createElement("canvas");c.width=320;c.height=100;const x=c.getContext("2d");x.fillStyle="#fff";x.fillRect(0,0,320,100);x.fillStyle="#4f46e5";x.fillText("Original image",20,40);return c.toDataURL("image/png").split(",")[1]})()')
    asset = invoke('content_asset_import', {'name': '原图.png', 'dataBase64': encoded})
    groups = [('6. 发票', ['6.1 美国发票']), ('6. 发票', ['6.1 美国发票']), ('6. 发票', ['6.2 英国发票']), ('7. 追踪编码', []), ('7. 追踪编码', [])]
    doc = invoke('memo_save', {'input': {'id': None, 'expectedRevision': None, 'title': 'Amazon出货SOP-工具栏验收', 'category': '', 'kind': 'flow', 'bodyMd': '', 'steps': [
        {'id': str(uuid.uuid4()), 'title': '原操作' + str(i + 1), 'owner': '', 'detail': '原文说明。' + ('\n\n![原图](lumen-asset:' + asset['id'] + ')' if i == 0 else ''), 'group': {'id': title, 'title': title, 'path': path}}
        for i, (title, path) in enumerate(groups)]}})
    click('备忘与流程'); click('刷新列表'); click(doc['title'])
    assert t.wait_for('document.querySelectorAll(".flow-canvas__nav-step").length===5')
    check('章节、阶段、步骤导航层级正确', t.eval('[...document.querySelectorAll(".flow-canvas__nav-step")].map(e=>e.dataset.level)') == ['chapter', 'step', 'stage', 'chapter', 'step'])
    sizes = t.eval('[...document.querySelectorAll(".flow-canvas__nav-mark")].map(e=>parseFloat(getComputedStyle(e).height))')
    check('章节刻度大于阶段、阶段大于普通步骤', sizes[0] > sizes[2] > sizes[4])
    hover = rect('.flow-canvas__nav-step')
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', **hover})
    time.sleep(.25)
    tooltip = t.eval('(()=>{const e=document.querySelector(".flow-canvas__nav-label");return {text:e.textContent,visible:getComputedStyle(e).visibility}})()')
    check('真实悬停章节显示章节名及全部小章节名', tooltip['visible'] == 'visible' and all(s in tooltip['text'] for s in ['6. 发票', '6.1 美国发票', '6.2 英国发票']))
    ui.real_click(t, '.flow-canvas__nav-step', index=2)
    check('阶段导航真实点击定位对应步骤', t.wait_for('document.querySelectorAll(".flow-canvas__nav-step")[2].getAttribute("aria-current")==="step"'))
    ui.real_click(t, '.flow-canvas__nav-step', index=0)
    assert t.wait_for('document.querySelector(".content-asset__image")?.complete')
    check('卡片图片下不再显示另存为', t.eval('!document.querySelector(".flow-canvas__body").textContent.includes("另存为")'))
    ui.real_click(t, '.content-asset__image')
    check('放大图片仍保留导出和批注功能', t.wait_for('document.querySelector(".image-viewer")') and t.eval('document.querySelector(".image-viewer").textContent.includes("另存为")&&document.querySelector(".image-viewer").textContent.includes("箭头")'))
    ui.real_click(t, '.image-viewer__close')
    positions = []
    for width in [1800, 1200, 900]:
        t.call('Emulation.setDeviceMetricsOverride', {'width': width, 'height': 1000, 'deviceScaleFactor': 1, 'mobile': False})
        time.sleep(.2)
        g = geometry(); positions.append({'width': width, **g})
        check(f'{width}px窗口全部菜单在同一行且完整可见', all(g['left'] - 1 <= b['left'] and b['right'] <= g['right'] + 1 for b in g['buttons']) and max(b['center'] for b in g['buttons']) - min(b['center'] for b in g['buttons']) < 1 and len(g['buttons']) >= 13 and g['overflow'] == 'hidden')
        click('刷新列表')
        check(f'{width}px点击最右命令不引起横向滚动', geometry()['scroll'] == 0)
    t.call('Emulation.clearDeviceMetricsOverride')
    ui.real_click(t, '.flow-canvas__nav-step', index=0)
    time.sleep(.2)
    start = rect('[aria-label="调整第 1 步卡片大小"]')
    drag(start, {'x': start['x'] + 80, 'y': start['y'] + 30})
    assert t.wait_for('document.querySelector(".memos__toolbar").textContent.includes("保存并查看")')
    assert t.wait_for('window.__TAURI_INTERNALS__.invoke("memo_get",{id:' + json.dumps(doc['id']) + '}).then(d=>!!d.steps[0].layout)')
    saved = invoke('memo_get', {'id': doc['id']})
    g = geometry()
    time.sleep(4)
    after = invoke('memo_get', {'id': doc['id']})
    check('实际调整卡片只保存一次，等待四秒版本不再重复增长', after['revision'] == saved['revision'] == doc['revision'] + 1)
    check('自动保存后整行菜单位置稳定', geometry() == g)
    check('保存前后原文、图片引用和分组不变', all({k: s[k] for k in ['id', 'title', 'owner', 'detail', 'group']} == {k: d[k] for k in ['id', 'title', 'owner', 'detail', 'group']} for s, d in zip(after['steps'], doc['steps'])))
    ui.real_click(t, '.flow-switcher__title')
    check('缩放后的流程切换菜单完整弹出', t.wait_for('document.querySelector(".flow-switcher__menu")') and t.eval('document.querySelector(".flow-switcher__menu").parentElement===document.body'))
    (args.output_dir / 'native.png').write_bytes(base64.b64decode(t.call('Page.captureScreenshot', {'format': 'png'})['data']))
    (args.output_dir / 'native.json').write_text(json.dumps({'checks': checks, 'geometry': positions, 'savedRevision': after['revision']}, ensure_ascii=False, indent=2), encoding='utf8')
finally:
    t.call('Emulation.clearDeviceMetricsOverride')
    t.close()
