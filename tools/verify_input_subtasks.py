"""桌面回归：只允许指定的隔离 profile；使用真实鼠标与键盘事件。"""
from __future__ import annotations
import argparse
import base64
import json
import subprocess
import time
from pathlib import Path
import ui_drive as ui

parser = argparse.ArgumentParser()
parser.add_argument('--port', type=int, default=9223)
parser.add_argument('--data-dir', required=True)
args = parser.parse_args()
main = ui.connect(args.port, timeout=60)

def invoke(command, payload=None):
    return main.eval(f"window.__TAURI_INTERNALS__.invoke({json.dumps(command)}, {json.dumps(payload or {})})")

paths = invoke('app_data_paths')
expected = Path(args.data_dir).resolve()
actual = Path(paths['dataDir']).resolve()
if actual != expected or 'input-fixes' not in str(expected):
    raise SystemExit('Refusing to modify a production or unexpected profile')

def check(name, passed):
    print(f"{'PASS' if passed else 'FAIL'}: {name}", flush=True)
    if not passed:
        raise AssertionError(name)

def select(selector, value):
    main.eval(f"""(() => {{ const el=document.querySelector({json.dumps(selector)}); el.value={json.dumps(value)}; el.dispatchEvent(new Event('change',{{bubbles:true}})); }})()""")

def real_click(target, selector):
    target.eval(f"document.querySelector({json.dumps(selector)}).scrollIntoView({{block:'center'}})")
    ui.real_click(target, selector)

def key(target, name, modifiers=0):
    for kind in ('keyDown', 'keyUp'):
        target.call('Input.dispatchKeyEvent', {'type': kind, 'key': name, 'code': 'Key' + name.upper(), 'windowsVirtualKeyCode': ord(name.upper()), 'modifiers': modifiers})

def set_clipboard(value):
    # Known test strings only; clipboard backup/restore is handled by the caller.
    subprocess.run(['powershell.exe', '-NoProfile', '-STA', '-Command', f"Set-Clipboard -Value '{value}'"], check=True, capture_output=True)

def drag_text(selector, fraction=0.8):
    box = main.eval(f"""(() => {{ const el=document.querySelector({json.dumps(selector)}); el.scrollIntoView({{block:'center'}}); const r=el.getBoundingClientRect(); return {{x:r.left+12,y:r.top+r.height/2,w:r.width}}; }})()""")
    x, y = box['x'], box['y']
    main.call('Input.dispatchMouseEvent', {'type':'mousePressed','x':x,'y':y,'button':'left','buttons':1,'clickCount':1})
    for step in range(1, 9):
        main.call('Input.dispatchMouseEvent', {'type':'mouseMoved','x':x+min(box['w']*fraction,160)*step/8,'y':y,'button':'left','buttons':1})
    main.call('Input.dispatchMouseEvent', {'type':'mouseReleased','x':x+min(box['w']*fraction,160),'y':y,'button':'left','buttons':0,'clickCount':1})

stamp = str(time.time_ns())[-8:]
title = '输入验收-' + stamp
card_node = f"Array.from(document.querySelectorAll('.task')).find(x=>x.querySelector('.task__title')?.textContent==={json.dumps(title)})"
planned = main.eval('new Date().toISOString()')
task = invoke('task_create', {'input': {'title': title, 'plannedAt': planned, 'hasPlannedTime': False}})
child = invoke('subtask_create', {'taskId': task['id'], 'title': '子任务验收-' + stamp})
sibling = invoke('subtask_create', {'taskId': task['id'], 'title': '第二个子任务-' + stamp})
main.eval("Array.from(document.querySelectorAll('.sidebar button')).find(b=>b.innerText.includes('全部任务')).click()")
check('折叠卡片显示子任务名称', main.wait_for(f"Array.from(document.querySelectorAll('.subtask-preview')).some(x=>x.innerText.includes({json.dumps(child['title'])}))", timeout=15))
main.eval(f"{card_node}.dataset.inputTest={json.dumps(stamp)}")
card = f'[data-input-test="{stamp}"]'
check('整张卡片不是拖拽源', main.eval(f"!document.querySelector('{card}').draggable && document.querySelector('{card} .task__grip').draggable"))
drag_text(card + ' .subtask-preview__title')
check('子任务文字可用鼠标拖动选择', main.eval("window.getSelection().toString().length>0"))
main.eval('window.getSelection().removeAllRanges()')
real_click(main, card + ' button[aria-expanded]')
field_selector = card + ' input[aria-label="新子任务标题"]'
main.wait_for(f"document.querySelector({json.dumps(field_selector)})")
ui.set_react_input(main, field_selector, '拖动选中文字 123456789')
drag_text(field_selector)
check('输入框可用鼠标拖动选择文字', main.eval(f"(() => {{ const e=document.querySelector({json.dumps(field_selector)}); return e.selectionEnd>e.selectionStart; }})()"))

real_click(main, field_selector)
key(main, 'a', 2)
set_clipboard('FRESH_COPY_1')
key(main, 'v', 2)
check('Ctrl+V 读取当前系统剪贴板', main.wait_for(f"document.querySelector({json.dumps(field_selector)}).value==='FRESH_COPY_1'"))
key(main, 'a', 2)
set_clipboard('FRESH_COPY_2')
key(main, 'v', 10)
check('Ctrl+Shift+V 读取新复制的内容', main.wait_for(f"document.querySelector({json.dumps(field_selector)}).value==='FRESH_COPY_2'"))
key(main, 'z', 2)
check('粘贴保留原生撤销', main.wait_for(f"document.querySelector({json.dumps(field_selector)}).value==='FRESH_COPY_1'"))
real_click(main, card + ' button[aria-expanded]')
invoke('window_apply_action', {'action': 'show_floating'})
floating = ui.connect(args.port, want='floating', timeout=15)
# Fixtures are created through raw IPC, outside the UI's mutation broadcaster.
floating.call('Page.reload')
check('悬浮窗显示子任务', floating.wait_for(f"Array.from(document.querySelectorAll('.subtask-preview')).some(x=>x.innerText.includes({json.dumps(child['title'])}))"))
floating.eval(f"Array.from(document.querySelectorAll('.subtask-preview')).find(x=>x.innerText.includes({json.dumps(child['title'])})).dataset.inputTest={json.dumps(stamp)}")
floating_child = f'[data-input-test="{stamp}"] button[role=checkbox]'
real_click(floating, floating_child)
check('悬浮窗勾选后基础列表同步', main.wait_for(f"{card_node}?.querySelector('.subtask-preview button')?.getAttribute('aria-checked')==='true'"))
check('只完成一个子任务时父任务保持未完成', invoke('task_get', {'id':task['id']})['status']=='todo')
def progress_ratio():
    return f"(() => {{const card={card_node}, track=card?.querySelector('.task__progress .progress'), bar=track?.querySelector('.progress__bar'); return track && bar ? bar.getBoundingClientRect().width/track.getBoundingClientRect().width : -1}})()"
check('完成一个子任务后进度条真实绘制一半', main.wait_for(f"Math.abs({progress_ratio()}-0.5)<0.02"))
check('完成按钮是无描边绿色圆与居中的粗白色对勾', main.eval(f"(() => {{const b={card_node}.querySelector('.subtask-preview button'),svg=b.querySelector('svg'),r=b.getBoundingClientRect(),s=svg.getBoundingClientRect(),c=getComputedStyle(b);return b.querySelector('svg path')!==null && b.querySelector('svg circle')===null && c.color==='rgb(255, 255, 255)' && c.backgroundColor==='rgb(75, 169, 79)' && c.borderTopColor==='rgba(0, 0, 0, 0)' && Number(svg.querySelector('path').getAttribute('stroke-width'))>=2.5 && Math.abs(s.x+s.width/2-r.x-r.width/2)<1 && Math.abs(s.y+s.height/2-r.y-r.height/2)<1}})()"))
main.eval(f"{card_node}.scrollIntoView({{block:'center'}})")
clip=main.eval(f"(() => {{const r={card_node}.getBoundingClientRect();return {{x:r.x,y:r.y,width:r.width,height:r.height,scale:1}}}})()")
shot=main.call('Page.captureScreenshot', {'format':'png','clip':clip})
(expected.parent/'half-progress.png').write_bytes(base64.b64decode(shot['data']))
floating.eval(f"Array.from(document.querySelectorAll('.subtask-preview__item')).find(x=>x.innerText.includes({json.dumps(sibling['title'])})).dataset.siblingTest={json.dumps(stamp)}")
real_click(floating, f'[data-sibling-test="{stamp}"] button')
check('完成第二个子任务后进度条真实绘制全部', main.wait_for(f"Math.abs({progress_ratio()}-1)<0.02"))
parent = invoke('task_get', {'id': task['id']})
check('最后一个子任务完成后父任务自动完成并记录时间', parent['status']=='done' and parent['completedAt'] is not None)
check('基础列表同步父任务完成状态', main.wait_for(f"{card_node}?.querySelector('.task__main > button[role=checkbox]')?.getAttribute('aria-checked')==='true'"))
check('悬浮窗同步父任务完成状态', floating.wait_for(f"Array.from(document.querySelectorAll('.floating__item')).find(x=>x.innerText.includes({json.dumps(title)}))?.querySelector('.floating__check')?.getAttribute('aria-checked')==='true'"))
main.eval(f"{card_node}.dataset.inputTest={json.dumps(stamp)}")
real_click(main, card + ' .subtask-preview button')
check('基础列表取消勾选后悬浮窗同步', floating.wait_for(f"Array.from(document.querySelectorAll('.subtask-preview')).find(x=>x.innerText.includes({json.dumps(child['title'])}))?.querySelector('button')?.getAttribute('aria-checked')==='false'"))

real_click(main, '.topbar__actions button[title^="新建任务"]')
ui.set_react_input(main, '.quickadd__input', '统一入口重复-' + stamp)
select('select[aria-label="任务重复"]', 'daily')
select('select[aria-label="结束条件"]', 'count')
ui.set_react_input(main, 'input[aria-label="重复次数"]', '2')
check('重复规则预览显示', main.wait_for("document.querySelector('.rulepreview')?.innerText.includes('接下来')"))
real_click(main, '.quickadd__row .btn--primary')
check('同一新建入口创建重复任务', main.wait_for(f"Array.from(document.querySelectorAll('.task__title')).some(x=>x.textContent==='统一入口重复-{stamp}')", timeout=15))
rows = invoke('task_list', {'query': {'search': '统一入口重复-' + stamp}})
check('重复任务实际生成两次且属于同一系列', len(rows)==2 and rows[0]['seriesId'] is not None and rows[0]['seriesId']==rows[1]['seriesId'])
check('新建成功后恢复不重复默认值', main.eval("document.querySelector('select[aria-label=任务重复]').value==='none'"))
check('顶栏不再有单独的重复新建入口', main.eval("!Array.from(document.querySelectorAll('.topbar__actions button')).some(x=>x.innerText.includes('重复任务'))"))
main.close()
floating.close()
print('ALL DESKTOP CHECKS PASSED', flush=True)
