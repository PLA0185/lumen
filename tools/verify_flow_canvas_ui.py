"""Sequential flow canvas verification using real CDP mouse/key events and isolated data."""
import argparse
import base64
import json
import os
import sys
from pathlib import Path
import ui_drive as ui

sys.stdout.reconfigure(encoding='utf-8')
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--port', type=int, default=9227)
p.add_argument('--data-dir', type=Path, required=True)
p.add_argument('--screenshot', type=Path, required=True)
args = p.parse_args()
t = ui.connect(args.port, timeout=60)
def invoke(command, payload=None):
    return t.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(command)},{json.dumps(payload or {})})')
paths = invoke('app_data_paths')
assert Path(paths['dataDir']).resolve() == args.data_dir.resolve()
assert args.data_dir.name == 'flow-ai-test-data'
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert args.data_dir.resolve() != (Path(os.environ['APPDATA']) / 'com.pla0185.lumen').resolve()
assert not invoke('cloud_sync_status')['config']
checks = 0
def check(label, condition):
    global checks
    assert condition, label
    checks += 1
    print('PASS: ' + label, flush=True)
def click(label):
    probe = 'Array.from(document.querySelectorAll("button")).find(b=>!b.disabled && b.textContent.trim()===' + json.dumps(label) + ')'
    assert t.wait_for('!!(' + probe + ')'), label
    t.eval(probe + '.dataset.canvasVerify="1"')
    t.eval('document.querySelector("[data-canvas-verify]").scrollIntoView({block:"center",behavior:"instant"})')
    ui.real_click(t, '[data-canvas-verify]')
    t.eval('document.querySelectorAll("[data-canvas-verify]").forEach(b=>delete b.dataset.canvasVerify)')
try:
    click('流程'); click('新建流程')
    check('默认进入画布，记录列表不挤占画布', t.eval('!!document.querySelector(`[aria-label="流程画布"]`) && getComputedStyle(document.querySelector(".memos__list")).display === "none"'))
    check('保留左侧菜单和顶部栏，画布铺满剩余内容区', t.eval('(()=>{const r=document.querySelector(`[aria-label="流程画布"]`).getBoundingClientRect(), d=document.querySelector(".memos__document").getBoundingClientRect(), sidebar=document.querySelector(".sidebar").getBoundingClientRect(), header=document.querySelector(".topbar").getBoundingClientRect(), toolbar=document.querySelector(".memos__toolbar").getBoundingClientRect();return r.left>=sidebar.right && r.top>=header.bottom && r.top>=toolbar.bottom && Math.abs(r.right-d.right)<2 && Math.abs(r.bottom-d.bottom)<2 && !!document.elementFromPoint(sidebar.left+40,sidebar.top+40)?.closest(".sidebar") && !!document.elementFromPoint(toolbar.left+40,toolbar.top+20)?.closest(".memos__toolbar")})()'))
    click('流程信息')
    ui.set_react_input(t, '[aria-label="备忘标题"]', '隔离画布验收')
    ui.real_click(t, '.flow-canvas__node .flow-canvas__title')
    ui.set_react_input(t, '[aria-label="第 1 步标题"]', 'ERP 创建发货单')
    ui.set_react_input(t, '[aria-label="第 1 步说明"]', '先核对数量，再创建发货单。')
    click('添加步骤')
    check('新增卡片在可用画布区域居中', t.eval('(()=>{const n=document.querySelectorAll(".flow-canvas__node")[1].getBoundingClientRect(), v=document.querySelector(`[aria-label="流程画布"]`).getBoundingClientRect(), i=document.querySelector(".flow-canvas__inspector").getBoundingClientRect(), a=document.querySelector(".memos__document-actions").getBoundingClientRect(), h=document.querySelector(".flow-canvas__hint").getBoundingClientRect();return Math.abs((n.left+n.right)/2-(v.left+32+i.left-24)/2)<3 && Math.abs((n.top+n.bottom)/2-(document.querySelector(".flow-canvas__nav").getBoundingClientRect().bottom+h.top)/2)<3})()'))
    ui.set_react_input(t, '[aria-label="第 2 步标题"]', '通知仓库')
    check('新增步骤与连线立即更新', t.eval('document.querySelectorAll(".flow-canvas__node").length === 2 && document.querySelectorAll(".flow-canvas__edges > path").length === 1'))
    t.eval('document.querySelector(`[aria-label="第 2 步上移"]`).scrollIntoView({block:"center",behavior:"instant"})')
    ui.real_click(t, '[aria-label="第 2 步上移"]')
    check('调序后画布顺序更新', t.wait_for('document.querySelector(".flow-canvas__node h3")?.textContent === "通知仓库"'))
    check('停顿后自动保存真实顺序', t.wait_for('Array.from(document.querySelectorAll("[role=status]")).some(e=>e.textContent.includes("已保存到本机"))'))
    doc = invoke('memo_get', {'id': next(r['id'] for r in invoke('memo_list', {'query': '', 'deletedOnly': False}) if r['title'] == '隔离画布验收')})
    check('库中顺序与空负责人保留', [s['title'] for s in doc['steps']] == ['通知仓库', 'ERP 创建发货单'] and all(s['owner'] == '' for s in doc['steps']))
    ui.set_react_input(t, '[aria-label="搜索流程步骤"]', 'ERP 数量')
    check('模糊搜索跨关键词定位操作说明', t.wait_for('document.querySelectorAll(".flow-canvas__node--match").length === 1'))
    click('定位下一项')
    check('搜索定位展开对应步骤', t.wait_for('!!document.querySelector(`[aria-label="第 2 步标题"]`)'))
    t.eval('(()=>{const e=document.querySelector(`[aria-label="搜索方式"]`);e.value="exact";e.dispatchEvent(new Event("change",{bubbles:true}))})()')
    check('精准搜索要求连续文字', t.wait_for('document.querySelectorAll(".flow-canvas__node--match").length === 0'))
    ui.set_react_input(t, '[aria-label="搜索流程步骤"]', '')
    click('收起详情'); click('查看全图')
    t.eval('document.querySelector(`[aria-label="流程画布"]`).scrollIntoView({block:"center",behavior:"instant"});document.querySelector(`[aria-label="流程画布"]`).focus()')
    rect = t.eval('(()=>{const r=document.querySelector(`[aria-label="流程画布"]`).getBoundingClientRect();return {x:r.left+40,y:r.top+r.height/2}})()')
    scene = 'document.querySelector(".flow-canvas__scene").style.transform'
    before = t.eval(scene)
    t.call('Input.dispatchMouseEvent', {'type': 'mouseWheel', 'x': rect['x'], 'y': rect['y'], 'deltaX': 0, 'deltaY': -100})
    check('普通真滚轮直接缩放画布', t.wait_for(scene + ' !== ' + json.dumps(before)))
    before = t.eval(scene)
    t.call('Input.dispatchMouseEvent', {'type': 'mouseWheel', 'x': rect['x'], 'y': rect['y'], 'deltaX': 0, 'deltaY': -100, 'modifiers': 1})
    check('Alt 真滚轮缩放', t.wait_for(scene + ' !== ' + json.dumps(before)))
    before = t.eval(scene)
    t.call('Input.dispatchMouseEvent', {'type': 'mousePressed', 'x': rect['x'], 'y': rect['y'], 'button': 'middle', 'clickCount': 1})
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'x': rect['x'] + 50, 'y': rect['y'] - 40, 'button': 'middle', 'buttons': 4})
    t.call('Input.dispatchMouseEvent', {'type': 'mouseReleased', 'x': rect['x'] + 50, 'y': rect['y'] - 40, 'button': 'middle', 'clickCount': 1})
    check('中键真拖动平移', t.wait_for(scene + ' !== ' + json.dumps(before)))
    check('松开中键结束拖动模式', t.wait_for('!document.querySelector(".flow-canvas__viewport--pan")'))
    click('查看全图')
    args.screenshot.parent.mkdir(parents=True, exist_ok=True)
    args.screenshot.write_bytes(base64.b64decode(t.call('Page.captureScreenshot', {'format': 'png'})['data']))
    print(f'{checks} real native canvas checks passed; isolated records only.', flush=True)
finally:
    t.close()

