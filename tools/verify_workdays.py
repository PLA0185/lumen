"""重复工作日桌面验收；只允许外部隔离 profile，不修改用户数据。"""
import argparse
import base64
import json
import time
from pathlib import Path
import ui_drive as ui

parser = argparse.ArgumentParser()
parser.add_argument('--data-dir', required=True)
parser.add_argument('--port', type=int, default=9223)
args = parser.parse_args()
main = ui.connect(args.port, timeout=60)

def invoke(name, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(name)}, {json.dumps(payload or {})})')

actual = Path(invoke('app_data_paths')['dataDir']).resolve()
expected = Path(args.data_dir).resolve()
if actual != expected or 'workday-test-data' not in str(expected):
    raise SystemExit('Refusing unexpected or production profile')

def check(name, condition):
    print(('PASS: ' if condition else 'FAIL: ') + name, flush=True)
    if not condition:
        raise AssertionError(name)

def click(selector):
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main, selector)

def select(selector, value):
    main.eval(f'(() => {{ const e=document.querySelector({json.dumps(selector)}); e.value={json.dumps(value)}; e.dispatchEvent(new Event("change", {{bubbles:true}})); }})()')

main.wait_for('document.querySelector("button[title=\\"新建任务（Ctrl+N）\\"]")')
click('button[title="新建任务（Ctrl+N）"]')
select('[aria-label="任务重复"]', 'daily')
check('新建每日任务默认双休', main.wait_for('document.querySelector("[aria-label=\\"周六、周日不执行\\"]")?.checked'))
ui.set_react_input(main, '.ruleeditor input[type=date]', '2026-09-30')
click('[aria-label="法定节假日不执行"]')
check('调休补班默认关闭', not main.eval('document.querySelector("[aria-label=\\"调休补班也执行\\"]").checked'))
check('明确提示未知年份不生成任务', main.eval("document.querySelector('.ruleeditor').textContent.includes('其它年份没有日历时不生成任务')"))
check('节假日预览跳过整个国庆假期', main.wait_for("document.querySelector('.rulepreview__list')?.textContent.includes('2026-10-08') && !document.querySelector('.rulepreview__list')?.textContent.includes('2026-10-01')"))
check('双休预览不含周六补班', not main.eval("document.querySelector('.rulepreview__list').textContent.includes('2026-10-10')"))
click('[aria-label="调休补班也执行"]')
check('勾选补班后预览包含10月10日', main.wait_for("document.querySelector('.rulepreview__list')?.textContent.includes('2026-10-10')"))
click('[aria-label="调休补班也执行"]')
select('[aria-label="结束条件"]', 'count')
ui.set_react_input(main, '[aria-label="重复次数"]', '4')
stamp = str(time.time_ns())[-8:]
title = '工作日验收-' + stamp
ui.set_react_input(main, '.quickadd__input', title)
check('预览四次与实际可执行日一致', main.wait_for("document.querySelectorAll('.rulepreview__list li').length===4 && document.querySelector('.rulepreview__list').textContent.includes('2026-10-12')"))
main.eval('document.querySelector("[aria-label=\\"法定节假日不执行\\"]").scrollIntoView({block:"center"})')
shot = main.call('Page.captureScreenshot', {'format':'png'})['data']
Path(expected.parent, 'workday-options.png').write_bytes(base64.b64decode(shot))
click('.quickadd__row .btn--primary')
check('保存后恢复不重复', main.wait_for('document.querySelector("[aria-label=\\"任务重复\\"]").value==="none"'))
rows = invoke('task_list', {'query': {'search': title, 'limit': 100}})
if isinstance(rows, dict):
    rows = rows['items']
check('真实生成四个实例', len(rows) == 4)
dates = sorted(main.eval(f'new Intl.DateTimeFormat("sv-SE",{{timeZone:"Asia/Shanghai"}}).format(new Date({json.dumps(row["occurrenceKey"])}))') for row in rows)
check('实际日期跳过法定假期和双休', dates == ['2026-09-30', '2026-10-08', '2026-10-09', '2026-10-12'])
series_id = rows[0]['seriesId']
detail = invoke('recurring_get', {'seriesId': series_id})
check('双休及节假日规则写入系列', 'BYDAY=MO,TU,WE,TH,FR;X-LUMEN-HOLIDAYS=CN' in detail['series']['rrule'])
main.call('Page.reload')
main.wait_for("document.querySelector('.app')")
check('页面重载后仍有节假日过滤', invoke('recurring_get', {'seriesId':series_id})['series']['rrule'] == detail['series']['rrule'])
main.close()
