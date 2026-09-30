"""云同步设置、自动保存、历史恢复及重复规则实机验收；仅操作隔离 profile。"""
import argparse
import json
from pathlib import Path
import ui_drive as ui

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--port', type=int, default=9223)
p.add_argument('--data-dir', type=Path, required=True)
p.add_argument('--output-dir', type=Path, required=True)
args = p.parse_args()
main = ui.connect(args.port, timeout=60)

def invoke(name, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(name)}, {json.dumps(payload or {})}).catch(e=>{{throw new Error(JSON.stringify(e))}})')

assert args.data_dir.name == 'memo-test-data'
assert args.data_dir.resolve() == Path(invoke('app_data_paths')['dataDir']).resolve()
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert not args.output_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
args.output_dir.mkdir(parents=True, exist_ok=True)
checks = []

def check(name, ok):
    assert ok, name
    checks.append(name)
    print('PASS: ' + name, flush=True)

def click(selector):
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main, selector)

def button(text):
    probe = f'Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim()==={json.dumps(text)})'
    assert main.wait_for(probe), text
    main.eval(f'{probe}.dataset.verifyCloud="1"')
    click('[data-verify-cloud]')
    main.eval('document.querySelectorAll("[data-verify-cloud]").forEach(b=>delete b.dataset.verifyCloud)')

def refresh():
    main.call('Page.reload')
    assert main.wait_for('document.querySelector(".app")')

series = invoke('recurring_create', {'input': {
    'title': '关键词规则验收', 'rrule': 'FREQ=DAILY;COUNT=10', 'tzid': 'UTC',
    'dtstartLocal': '2026-09-30T09:00:00', 'hasStartTime': True, 'materializeDays': 20,
}})
tasks = invoke('task_list', {'query': {'search': '关键词规则验收', 'limit': 300}})
tasks = sorted([t for t in tasks if t.get('seriesId') == series['seriesId']], key=lambda t: t['occurrenceKey'])
assert len(tasks) >= 3
subs = invoke('subtask_change', {'input': {'taskId': tasks[0]['id'], 'scope': 'whole_series', 'action': {'kind': 'create', 'title': 'CA 关键词'}}})
invoke('subtask_update', {'id': subs[0]['id'], 'isDone': True})
refresh()
button('已完成')
card = 'Array.from(document.querySelectorAll(".task")).find(t=>t.querySelector(".task__title")?.textContent==="关键词规则验收")'
assert main.wait_for(card)
main.eval(f'{card}.dataset.verifyTask="1"')
check('折叠已完成卡片保留完成样式', main.eval('Number(getComputedStyle(document.querySelector("[data-verify-task]")).opacity)<1'))
click('[data-verify-task] button[aria-expanded]')
check('展开已完成任务恢复正常亮度', main.wait_for('getComputedStyle(document.querySelector("[data-verify-task]")).opacity==="1"'))
button('修改重复规则')
assert main.wait_for('document.querySelectorAll("input[name=rule-scope]").length===2')
click('[role=dialog] fieldset label:nth-of-type(2) input')
button('保存重复规则')
check('带模板子任务的重复规则保存成功', main.wait_for('!document.querySelector(".modal-backdrop")', timeout=20))
check('完成记录及完成时间保留', invoke('subtask_list', {'taskId': tasks[0]['id']})[0]['completedAt'] is not None)
button('收件箱')
check('收件箱同一系列只显示一条', main.wait_for('document.querySelectorAll(".task").length===1'))
button('全部任务')
check('全部任务同一系列只显示一条', main.wait_for('document.querySelectorAll(".task").length===1'))
check('折叠不删除独立发生', len(invoke('task_list', {'query': {'search': '关键词规则验收', 'limit': 300}})) > 1)
button('看板')
check('看板待办重复系列计数为1', main.wait_for('document.querySelector(".boardcol .boardcol__count")?.textContent==="1"'))
button('今天')
click('.topbar button[aria-haspopup="dialog"]')
check('顶部AI入口直接弹出输入框', main.wait_for('document.querySelector(".ai-quick-dialog[open] textarea")'))
ui.set_react_input(main, '.ai-quick-dialog [aria-label="发送给 AI 的文本"]', '明天整理业务流程')
button('关闭助手')
click('.topbar button[aria-haspopup="dialog"]')
check('AI对话框关闭再开保留输入', main.eval('document.querySelector(".ai-quick-dialog textarea").value==="明天整理业务流程"'))
button('关闭助手')

button('备忘与流程')
button('新建备忘')
ui.set_react_input(main, '.memos input[placeholder="例如：客户订单处理流程"]', '家里整理流程验收')
ui.set_react_input(main, '.memos textarea', '先核对订单，再准备材料。')
check('输入停顿后实际自动保存', main.wait_for('document.querySelector(".memos")?.textContent.includes("已保存到本机")', timeout=15))
docs = invoke('memo_list', {'query': '', 'deletedOnly': False})
doc = next(d for d in docs if d['title'] == '家里整理流程验收')
check('自动保存正文真实落库', invoke('memo_get', {'id': doc['id']})['bodyMd'] == '先核对订单，再准备材料。')
button('保存并查看')
button('历史版本')
check('历史展示真实版本', main.wait_for('document.querySelector(".cloud-history select")?.options.length>=1'))
check('历史对话框取得键盘焦点', main.eval('document.querySelector(".cloud-history")?.contains(document.activeElement)'))
button('采用这个版本')
check('恢复历史成功且关闭对话框', main.wait_for('!document.querySelector(".cloud-history")', timeout=15))
check('恢复产生新历史而不覆盖旧历史', len(invoke('cloud_sync_history', {'id': doc['id']})['versions']) >= 2)

button('设置')
button('云同步')
check('云设置真实显示继承范围', main.wait_for('document.querySelector(".cloud-settings__fields select")?.options.length===2'))
check('应用密码和恢复码输入受遮挡', main.eval('document.querySelectorAll(".cloud-settings__fields input[type=password]").length===2'))
ui.set_react_input(main, '.cloud-settings__fields label:nth-of-type(1) input', 'http://invalid.test/')
ui.set_react_input(main, '.cloud-settings__fields label:nth-of-type(2) input', 'fixture')
ui.set_react_input(main, '.cloud-settings__fields label:nth-of-type(4) input', 'fixture-app-password')
button('验证并连接云空间')
check('不安全地址明确报错', main.wait_for('document.querySelector("[role=alert]")?.textContent.includes("HTTPS")', timeout=15))
check('失败连接没有伪装成成功', invoke('cloud_sync_status')['config'] is None)
backup = invoke('backup_export', {'path': str(args.output_dir / 'native-history.json')})
file = json.loads(Path(backup['path']).read_text(encoding='utf-8'))
check('原生备份格式6包含同步历史', file['formatVersion'] == 6 and len(file['data']['syncHistory']['memo_sync_events']) >= 2)
invoke('backup_restore', {'path': backup['path']})
check('原生恢复后仍能读取备忘', invoke('memo_get', {'id': doc['id']})['title'] == '家里整理流程验收')
check('设置结构版本与实际迁移一致', int(invoke('app_data_paths')['schemaVersion']) == len(list((Path(__file__).resolve().parents[1] / 'src-tauri' / 'migrations').glob('*.sql'))))
dated = invoke('task_create', {'input': {'title': '隔离日历改期验收', 'plannedAt': '2026-09-25T01:00:00.000Z', 'hasPlannedTime': True, 'dueAt': '2026-10-08T02:00:00.000Z'}})
moved = invoke('task_reschedule', {'id': dated['id'], 'newDateUtc': '2026-10-04T16:00:00.000Z', 'timeZone': 'Asia/Shanghai'})
check('日历改到10月5日仍为本地09点', moved['plannedAt'] == '2026-10-05T01:00:00.000Z' and moved['hasPlannedTime'])
check('日历改期不改截止时间', moved['dueAt'] == dated['dueAt'])
rejected = main.eval(f'window.__TAURI_INTERNALS__.invoke("task_reschedule", {{id:{json.dumps(dated["id"])}, newDateUtc:"2026-10-06T16:00:00.000Z", timeZone:"Invalid/Zone"}}).then(()=>false,e=>e.code==="validation")')
check('无效时区拒绝且保持原任务', rejected and invoke('task_get', {'id': dated['id']})['plannedAt'] == moved['plannedAt'])
(args.output_dir / 'native-cloud-results.json').write_text(json.dumps({'passed': len(checks), 'checks': checks}, ensure_ascii=False, indent=2), encoding='utf-8')
main.close()
