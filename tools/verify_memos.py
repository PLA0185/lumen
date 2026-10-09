"""备忘与流程实机验收。只允许 TEMP 下显式命名的 memo-test-data-* 隔离 profile。"""
import argparse
import base64
import json
import os
from pathlib import Path
import sys
import ui_drive as ui

sys.stdout.reconfigure(encoding='utf-8')

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--data-dir', required=True, type=Path)
parser.add_argument('--output-dir', required=True, type=Path)
parser.add_argument('--port', type=int, default=9223)
args = parser.parse_args()
main = ui.connect(args.port, timeout=45)

def invoke(command, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(command)}, {json.dumps(payload or {})})')

profile = args.data_dir.resolve()
assert profile.name == 'memo-test-data' or profile.name.startswith('memo-test-data-')
assert profile.is_relative_to(Path(os.environ['TEMP']).resolve()), '仅允许 TEMP 下的显式隔离 profile'
assert profile != (Path(os.environ['APPDATA']) / 'com.pla0185.lumen').resolve(), '拒绝使用真实用户数据'
assert Path(invoke('app_data_paths')['dataDir']).resolve() == profile, '拒绝非隔离数据'
assert not args.output_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
args.output_dir.mkdir(parents=True, exist_ok=True)
checks = []

def check(name, condition):
    assert condition, name
    checks.append(name)
    print('PASS: ' + name, flush=True)

def click(selector):
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main, selector)

def double_click(selector):
    box = main.eval(f'(()=>{{const r=document.querySelector({json.dumps(selector)}).getBoundingClientRect();return {{x:r.left+r.width/2,y:r.top+r.height/2}}}})()')
    for count in (1, 2):
        for kind in ('mousePressed', 'mouseReleased'):
            main.call('Input.dispatchMouseEvent', {'type': kind, 'x': box['x'], 'y': box['y'], 'button': 'left', 'buttons': 1 if kind == 'mousePressed' else 0, 'clickCount': count})

def button(text):
    main.eval(f'Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim()==={json.dumps(text)}).dataset.verifyMemo="1"')
    click('[data-verify-memo]')
    main.eval('document.querySelectorAll("[data-verify-memo]").forEach(b=>delete b.dataset.verifyMemo)')

def fill(label, value):
    ui.set_react_input(main, f'[aria-label="{label}"]', value)

def saved():
    assert main.wait_for('document.querySelector(".memos__reading")||document.querySelector(".flow-canvas__node")')

main.eval('window.__memoOrigConfirm=window.confirm; window.__memoAllowConfirm=false; window.__memoConfirmMessage=""; window.confirm=(message)=>{window.__memoConfirmMessage=message;return window.__memoAllowConfirm}; true')
try:
    check('隔离 profile 没有任务', invoke('task_count', {'query': {}})['total'] == 0)
    button('流程')
    check('备忘和流程使用独立页面', main.wait_for('document.querySelector(".memos__list[aria-label=流程列表]")'))
    button('新建流程')
    button('流程信息')
    fill('备忘标题', '验收订单处理流程')
    fill('备忘分类', '业务学习')
    fill('备忘内容', '## 核对清单\n- 确认客户信息\n- 保存凭证')
    check('正文编辑区有足够高度', main.eval('parseFloat(getComputedStyle(document.querySelector("[aria-label=\\\"备忘内容\\\"]")).minHeight)>=150'))
    double_click('.flow-canvas__node .flow-canvas__title')
    assert main.wait_for('document.querySelector("[aria-label=\\\"第 1 步标题\\\"]")')
    fill('第 1 步标题', '审核资料')
    fill('第 1 步负责人', '验收运营部')
    fill('第 1 步说明', '核对金额和联系方式')
    button('添加步骤')
    fill('第 2 步标题', '接收订单')
    fill('第 2 步负责人', '验收销售部')
    fill('第 2 步说明', '先收集完整材料')
    click('[aria-label="第 2 步操作"]')
    button('上移')
    check('画布菜单调序立即更新节点顺序', main.eval('Array.from(document.querySelectorAll(".flow-canvas__node")).map(n=>n.querySelector(".flow-canvas__inline-editor [aria-label$=\\\"步标题\\\"]")?.value||n.querySelector("h3")?.textContent).join(",")==="接收订单,审核资料"'))
    button('今天')
    check('拒绝放弃时保留当前草稿', main.eval('document.querySelector(".flow-switcher__title")?.textContent.includes("验收订单处理流程")&&document.querySelector(".flow-canvas__inline-editor [aria-label=\\\"第 1 步标题\\\"]")?.value==="接收订单"'))
    button('保存并查看')
    saved()
    check('可视路线按正确顺序显示', main.eval('Array.from(document.querySelectorAll(".memos__node h3")).map(e=>e.textContent)') == ['接收订单', '审核资料'])
    check('流程卡片上的负责人和说明完整可读', main.eval('(()=>{const text=Array.from(document.querySelectorAll(".flow-canvas__node")).map(e=>e.textContent).join(" ");return text.includes("验收运营部")&&text.includes("核对金额和联系方式")})()'))
    rows = invoke('memo_list', {'query': '', 'deletedOnly': False})
    assert len(rows) == 1
    original = invoke('memo_get', {'id': rows[0]['id']})
    check('流程真实落库', original['steps'][0]['title'] == '接收订单' and original['bodyMd'].startswith('##'))
    check('流程节点使用可视化卡片布局', main.eval('getComputedStyle(document.querySelector(".memos__node")).display==="grid"'))
    (args.output_dir / 'memo-flow.png').write_bytes(base64.b64decode(main.call('Page.captureScreenshot', {'format': 'png'})['data']))
    fill('搜索流程', '验收运营部')
    check('按负责人搜索', main.wait_for('document.querySelectorAll(".memos__item").length===1'))
    fill('搜索流程', '无匹配的验收词')
    check('搜索无结果', main.wait_for('document.querySelectorAll(".memos__item").length===0'))
    fill('搜索流程', '')
    assert main.wait_for('document.querySelectorAll(".memos__item").length===1')
    button('今天'); button('流程')
    assert main.wait_for('document.querySelectorAll(".memos__item").length===1')
    click('.memos__item'); saved()
    check('切换页面后仍可读取保存的流程', main.eval('document.querySelectorAll(".memos__node").length===2'))
    main.eval('window.__memoAllowConfirm=true;true')
    button('删除记录')
    check('流程删除确认明确指向流程回收站', main.eval('window.__memoConfirmMessage.includes("流程回收站")'))
    check('删除进入流程回收站', main.wait_for('document.querySelectorAll(".memos__item").length===0'))
    button('流程回收站')
    assert main.wait_for('document.querySelectorAll(".memos__item").length===1')
    click('.memos__item'); saved(); button('恢复记录')
    button('返回流程')
    check('恢复回到正常列表', main.wait_for('document.querySelectorAll(".memos__item").length===1'))
    backup_path = args.output_dir / 'memo-roundtrip.lumen-backup.json'
    exported = invoke('backup_export', {'path': str(backup_path)})
    check('完整备份包含流程', exported['stats']['memoDocuments'] == 1)
    button('备忘')
    check('备忘页不显示流程记录', main.wait_for('document.querySelector(".memos__list[aria-label=备忘列表]")'))
    button('新建备忘')
    fill('备忘标题', '验收术语备忘')
    body='\n\n'.join(f'## 长正文第 {i} 节\n\n这是一段用于实机检查的正文，验证内容跨过摘要高度后仍然可以完整滚动阅读。末尾标记：全文结束。' for i in range(1,13))
    fill('备忘内容', body)
    button('保存并查看'); saved()
    check('长备忘正文无内部截断且显示末尾', main.eval('(()=>{const e=document.querySelector(".memos__reading .mdpreview");return e&&getComputedStyle(e).maxHeight==="none"&&getComputedStyle(e).overflow==="visible"&&e.textContent.includes("全文结束。")&&e.scrollHeight===e.clientHeight})()'))
    (args.output_dir / 'memo-reading.png').write_bytes(base64.b64decode(main.call('Page.captureScreenshot', {'format': 'png'})['data']))
    check('备忘独立于流程保存', len(invoke('memo_list', {'query': '', 'deletedOnly': False})) == 2)
    preview = invoke('backup_preview', {'path': str(backup_path)})
    check('备份预览对比真实数量', preview['checksumOk'] and preview['current']['memoDocuments'] == 2 and preview['stats']['memoDocuments'] == 1)
    restored = invoke('backup_restore', {'path': str(backup_path)})
    check('实际恢复完整备份', restored['imported']['memoDocuments'] == 1)
    doc = invoke('memo_get', {'id': original['id']})
    check('恢复后正文和步骤完整', doc['bodyMd'] == original['bodyMd'] and doc['steps'] == original['steps'])
    check('全过程不污染任务', invoke('task_count', {'query': {}})['total'] == 0)
    print(json.dumps({'passed': len(checks), 'taskWrites': False, 'isolatedProfile': True}), flush=True)
finally:
    main.eval('window.confirm=window.__memoOrigConfirm;true')
    main.close()
