"""描述、重复子任务范围及完成时间实机验收；仅允许仓库外 memo-test-data。"""
import argparse
import json
import time
from datetime import datetime, timezone
from pathlib import Path

import ui_drive as ui

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--port', type=int, default=9223)
parser.add_argument('--data-dir', type=Path, required=True)
parser.add_argument('--output-dir', type=Path, required=True)
args = parser.parse_args()
main = ui.connect(args.port, timeout=45)


def invoke(name, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(name)}, {json.dumps(payload or {})})')


assert args.data_dir.name == 'memo-test-data'
assert args.data_dir.resolve() == Path(invoke('app_data_paths')['dataDir']).resolve()
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
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


def button(text):
    main.eval(f'Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim()==={json.dumps(text)}).dataset.featureClick="1"')
    click('[data-feature-click]')
    main.eval('document.querySelectorAll("[data-feature-click]").forEach(b=>delete b.dataset.featureClick)')


def refresh(title):
    main.call('Page.reload')
    assert main.wait_for('document.querySelector(".app")')
    button('全部任务')
    expression = f'Array.from(document.querySelectorAll(".task")).find(t=>t.querySelector(".task__title")?.textContent==={json.dumps(title)})'
    assert main.wait_for(expression, timeout=15)
    main.eval(f'{expression}.dataset.featureCard="1"')
    return '[data-feature-card]'


def choose(scope):
    assert main.wait_for('document.querySelector("[role=dialog]")')
    check('范围没有默认预选', main.eval('document.querySelector("input[type=radio]:checked")===null'))
    check('子任务范围仅有本次与整个系列', main.eval('document.querySelectorAll("input[type=radio]").length===2'))
    click(f'input[value="{scope}"]')
    button('确认修改')
    assert main.wait_for('!document.querySelector("[role=dialog]")')


stamp = str(time.time_ns())[-9:]
title = '描述验收-' + stamp
invoke('window_apply_action', {'action': 'show_main'})
button('今天')
button('新建')
assert main.wait_for('document.querySelector("[aria-label=\\"任务描述\\"]")')
ui.set_react_input(main, '[aria-label="任务标题，可包含日期、标签与优先级"]', title)
description = '要完成：包装标贴\n注意：批号和交付尺寸\n交付前复核'
ui.set_react_input(main, '[aria-label="任务描述"]', description)
button('添加')
card = refresh(title)
click(card + ' button[aria-expanded]')
check('详情显示完整多行描述', main.wait_for(f'document.querySelector("{card} .task-description__content")?.textContent==={json.dumps(description)}'))
click(card + ' .task-description button')
check('编辑描述不必展开高级选项', main.wait_for('document.querySelector("#ed-desc") && !document.querySelector("#ed-desc").closest("details")'))
button('取消')

series_title = '重复子任务验收-' + stamp
created = invoke('recurring_create', {'input': {
    'title': series_title, 'rrule': 'FREQ=DAILY;COUNT=4', 'tzid': 'Asia/Shanghai',
    'dtstartLocal': '2026-09-28T00:00:00', 'hasStartTime': False, 'materializeDays': 4,
}})
rows = invoke('task_list', {'query': {'search': series_title, 'isRecurring': True, 'limit': 100}})
instances = rows['items'] if isinstance(rows, dict) else rows
instances = [row for row in instances if row['seriesId'] == created['seriesId']]
instances.sort(key=lambda t: t['occurrenceKey'])
check('隔离测试创建四次发生', len(instances) == 4)
source, current = instances[:2]
for name in ['CA 验收', 'UK 验收']:
    step = invoke('subtask_change', {'input': {'taskId': source['id'], 'scope': 'this_only', 'action': {'kind': 'create', 'title': name}}})[-1]
    invoke('subtask_update', {'id': step['id'], 'isDone': True})
historical = invoke('subtask_list', {'taskId': source['id']})
check('昨天的子任务完成时间已记录', all(s['completedAt'] for s in historical))

# Today has a distinct occurrence title for deterministic real-mouse targeting.
invoke('recurring_edit_instance', {'taskId': current['id'], 'scope': 'this_only',
    'patch': {'title': series_title + '-今天'}, 'confirmHistory': False})
card = refresh(series_title + '-今天')
click(card + ' button[aria-expanded]')
assert main.wait_for(f'document.querySelector("{card} .subtasks") && !document.querySelector("{card} .skeleton")')
button('从上一次复制子任务')
choose('whole_series')
copied = invoke('subtask_list', {'taskId': current['id']})
check('选择整个系列复制两条子任务', len(copied) == 2)
check('新实例完成状态及时间独立清空', all(s['isDone'] == 0 and s['completedAt'] is None for s in copied))
check('昨天的子任务逐字段保持不变', invoke('subtask_list', {'taskId': source['id']}) == historical)
for next_task in instances[2:]:
    check('已生成的未来发生也带独立子任务', len(invoke('subtask_list', {'taskId': next_task['id']})) == 2)

ui.set_react_input(main, card + ' [aria-label="新子任务标题"]', '只今天补充')
click(card + ' .subnew button')
choose('this_only')
check('仅本次添加不影响其它日期', len(invoke('subtask_list', {'taskId': instances[2]['id']})) == 2)
click(card + ' .subtask button[role=checkbox]')
completed = invoke('subtask_list', {'taskId': current['id']})[0]
now = datetime.now(timezone.utc)
actual = datetime.fromisoformat(completed['completedAt'].replace('Z', '+00:00'))
check('完成时间为实际点击时刻', 0 <= (now - actual).total_seconds() < 5)
check('详情显示完成时间到秒', main.wait_for(f'document.querySelector("{card} .subtask__completed")?.textContent.includes("完成")'))
click(card + ' button[aria-expanded]')
check('折叠简览也显示完成时间', main.wait_for(f'document.querySelector("{card} .subtask-preview .subtask__completed")?.textContent.includes("完成")'))

backup = invoke('backup_export', {'path': str(args.output_dir / 'subtasks-native.lumen-backup.json')})
check('正式备份写入真实文件', Path(backup['path']).is_file())
data = json.loads(Path(backup['path']).read_text(encoding='utf-8'))
check('备份格式 5 带子任务模板', data['formatVersion'] == 5 and any(json.loads(t['subtasks_json']) for t in data['data']['seriesTemplates']))
(args.output_dir / 'recurring-subtasks-native.json').write_text(json.dumps({'passed': len(checks), 'checks': checks}, ensure_ascii=False, indent=2), encoding='utf-8')
main.close()
