"""按真实鼠标操作验证创建位置；仅允许隔离工作日验收库。"""
import argparse
import json
import time
from pathlib import Path
import ui_drive as ui

p = argparse.ArgumentParser()
p.add_argument('--data-dir', required=True)
p.add_argument('--port', type=int, default=9223)
args = p.parse_args()
main = ui.connect(args.port, timeout=60)

def invoke(name, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(name)},{json.dumps(payload or {})})')

if Path(invoke('app_data_paths')['dataDir']).resolve() != Path(args.data_dir).resolve() or 'workday-test-data' not in args.data_dir:
    raise SystemExit('Refusing unexpected profile')

def check(name, value):
    print(('PASS: ' if value else 'FAIL: ') + name, flush=True)
    if not value:
        print(main.eval('JSON.stringify({text:document.querySelector(".content").innerText.slice(-3500),buttons:Array.from(document.querySelectorAll(".orgrow button")).map(b=>b.getAttribute("aria-label"))})'), flush=True)
        raise AssertionError(name)

def click(selector):
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main, selector)

def nav(label):
    main.eval(f'(() => {{ document.querySelectorAll("[data-context-nav]").forEach(e=>delete e.dataset.contextNav); Array.from(document.querySelectorAll(".sidebar button")).find(b=>b.textContent.trim().startsWith({json.dumps(label)})).dataset.contextNav="1"; }})()')
    click('[data-context-nav]')
    time.sleep(0.4)

def new_form():
    if not main.eval('Boolean(document.querySelector(".quickadd"))'):
        click('button[title="新建任务（Ctrl+N）"]')

stamp = str(time.time_ns())[-8:]

def save(title, should_show=True):
    ui.set_react_input(main, '.quickadd__input', title)
    click('.quickadd__row .btn--primary')
    check('保存成功：' + title, main.wait_for('document.querySelector(".quickadd__input").value===""'))
    rows = invoke('task_list', {'query': {'search':title, 'limit':100}})
    check('数据库存在该任务', len(rows) == 1)
    if should_show:
        check('保存后留在创建位置', main.wait_for(f'Array.from(document.querySelectorAll(".task__title,.caltask__title")).some(e=>e.textContent==={json.dumps(title)})'))
    return rows[0]

main.call('Page.reload')
main.wait_for('document.querySelector(".sidebar")')
for index, label in enumerate(['今天', '明天', '本周安排']):
    nav(label)
    new_form()
    check(label + '自动带上计划日期', bool(main.eval('document.querySelector("[aria-label=\\"计划执行日期\\"]").value')))
    save('CTX-date-' + str(index) + '-' + stamp)

for label, period in [('周任务','week'), ('月任务','month'), ('季度任务','quarter'), ('年任务','year')]:
    nav(label)
    new_form()
    check(label + '默认带上周期', main.eval('document.querySelector("[aria-label=\\"周期跨度\\"]").value') == period)
    task = save('CTX-period-' + period + '-' + stamp)
    check('真实周期写入', task['periodType'] == period)

nav('日历')
check('日历载入成功', main.wait_for('document.querySelector(".calcell")'))
click('[aria-label^="10 月 8 日，"]')
new_form()
check('新建沿用所选日历日期', main.wait_for('document.querySelector("[aria-label=\\"计划执行日期\\"]").value==="2026-10-08"'))
save('日历-归属-' + stamp, should_show=False)
check('新任务出现在选中日期的日历', main.wait_for(f'document.querySelector(".calendar")?.textContent.includes({json.dumps("日历-归属-" + stamp)})'))

for kind, command, field in [('项目','project_create','projectId'),('分类','category_create','categoryId'),('标签','tag_create','tagIds')]:
    name = kind + '-归属-' + stamp
    item = invoke(command, {'input': {'name':name}})
    # 原始 IPC 测试夹具没有经过前端 mutation 广播，重新加载后再检查新归属。
    main.call('Page.reload')
    main.wait_for('document.querySelector(".sidebar")')
    nav('标签' if kind == '标签' else '项目与分类')
    if kind == '分类':
        main.eval('Array.from(document.querySelectorAll("[role=tab]")).find(b=>b.textContent.includes("分类")).dataset.contextCategory="1"')
        click('[data-context-category]')
    selector = f'[aria-label={json.dumps("在" + kind + " " + name + " 新建任务", ensure_ascii=False)}]'
    check('对应归属有新建入口', main.wait_for(f'document.querySelector({json.dumps(selector)})'))
    click(selector)
    check('表单明确显示归属', main.eval(f'document.querySelector(".quickadd").textContent.includes({json.dumps(name)})'))
    task = save(kind + '-任务-' + stamp)
    if kind != '标签':
        check('任务保存到对应归属', task[field] == item['id'])
    else:
        tags = invoke('task_tags_get', {'taskId':task['id']})
        check('任务保存到对应标签', any(t['id'] == item['id'] for t in tags))

nav('周任务')
new_form()
main.eval('(() => { const e=document.querySelector("[aria-label=\\"任务重复\\"]"); e.value="daily"; e.dispatchEvent(new Event("change",{bubbles:true})); })()')
main.eval('(() => { const e=document.querySelector("[aria-label=\\"结束条件\\"]"); e.value="count"; e.dispatchEvent(new Event("change",{bubbles:true})); })()')
ui.set_react_input(main, '[aria-label="重复次数"]', '2')
title = 'CTX-recurring-period-' + stamp
ui.set_react_input(main, '.quickadd__input', title)
check('周期归属进入重复规则', main.wait_for('document.querySelector(".rulepreview code").textContent.includes("X-LUMEN-PERIOD=WEEK")'))
click('.quickadd__row .btn--primary')
check('重复任务也留在当前周期视图', main.wait_for(f'Array.from(document.querySelectorAll(".task__title")).filter(e=>e.textContent==={json.dumps(title)}).length===2'))
rows = invoke('task_list', {'query': {'search':title, 'limit':100}})
check('所有重复实例保留周期归属', len(rows)==2 and all(t['periodType']=='week' for t in rows))

nav('收件箱')
new_form()
check('收件箱不强加日期和周期', main.wait_for('document.querySelector("[aria-label=\\"计划执行日期\\"]").value==="" && document.querySelector("[aria-label=\\"周期跨度\\"]").value==="none"'))
task = save('收件箱-归属-' + stamp)
check('无日期任务仍保留在收件箱', task['plannedAt'] is None and task['periodType'] == 'none')
main.close()
