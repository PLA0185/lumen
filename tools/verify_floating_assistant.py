"""浮窗与 AI 入口实机验收；只写入显式隔离 profile，不调用外部模型。"""
import argparse
import base64
import json
import time
from pathlib import Path
import ui_drive as ui

p = argparse.ArgumentParser()
p.add_argument('--data-dir', required=True)
p.add_argument('--output-dir', required=True)
p.add_argument('--port', type=int, default=9223)
args = p.parse_args()
main = ui.connect(args.port, timeout=60)

def invoke(command, payload=None, target=main):
    return target.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(command)},{json.dumps(payload or {})})')

paths = invoke('app_data_paths')
if Path(paths['dataDir']).resolve() != Path(args.data_dir).resolve() or Path(args.data_dir).name != 'floating-ai-test-data':
    raise SystemExit('Refusing unexpected profile')
out = Path(args.output_dir).resolve()
if out.is_relative_to(Path(__file__).resolve().parents[1]):
    raise SystemExit('Evidence must be outside the repository')
out.mkdir(parents=True, exist_ok=True)

def check(name, result):
    print(('PASS: ' if result else 'FAIL: ') + name, flush=True)
    if not result:
        raise AssertionError(name)

def click(target, selector):
    target.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    box = target.eval(f'(() => {{ const r=document.querySelector({json.dumps(selector)}).getBoundingClientRect(); return {{x:r.x+r.width/2,y:r.y+r.height/2,w:innerWidth,h:innerHeight}}; }})()')
    if not (0 <= box['x'] < box['w'] and 0 <= box['y'] < box['h']):
        raise AssertionError('Target outside viewport: ' + selector)
    ui.real_click(target, selector)

def button(target, text):
    target.eval(f'Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim().startsWith({json.dumps(text)})).dataset.verifyButton="1"')
    click(target, '[data-verify-button]')
    target.eval('document.querySelectorAll("[data-verify-button]").forEach(b=>delete b.dataset.verifyButton)')

def select(target, selector, value):
    target.eval(f'(() => {{ let e=document.querySelector({json.dumps(selector)}); Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,"value").set.call(e,{json.dumps(value)}); e.dispatchEvent(new Event("change",{{bubbles:true}})); }})()')

stamp = str(int(time.time()))
dates = main.eval('(() => { let d=new Date(); d.setHours(0,0,0,0); let next=new Date(d); next.setDate(d.getDate()+1); return {today:d.toISOString(),tomorrow:next.toISOString(),nextDate:next.getFullYear()+"-"+String(next.getMonth()+1).padStart(2,"0")+"-"+String(next.getDate()).padStart(2,"0")}; })()')
parent_title = '验收-关键词更新-' + stamp
parent = invoke('task_create', {'input': {'title': parent_title, 'plannedAt': dates['today']}})
for title in ['验收 CA 部分', '验收 UK 部分']:
    invoke('subtask_create', {'taskId': parent['id'], 'title': title})
invoke('task_create', {'input': {'title': '验收-运营表更新-' + stamp, 'plannedAt': dates['today']}})
next_title = '验收-明日安排-' + stamp
invoke('task_create', {'input': {'title': next_title, 'plannedAt': dates['tomorrow']}})
invoke('window_set_floating_size', {'width': 450, 'height': 600})
invoke('window_apply_action', {'action': 'show_floating'})
floating = ui.connect(args.port, want='floating', timeout=30)
check('一周七天导航可见', floating.wait_for('document.querySelectorAll(".floating__day").length===7'))
check('真实主任务与两个子任务可见', floating.wait_for(f'document.body.textContent.includes({json.dumps(parent_title)}) && document.querySelectorAll(".subtask-preview__item").length>=2'))
progress = f'Array.from(document.querySelectorAll(".floating__item")).find(e=>e.textContent.includes({json.dumps(parent_title)})).querySelector(".floating__progress").textContent'
check('进度徽标来自子任务数据', floating.eval(progress + '==="0/2"'))
click(floating, '[aria-label="完成子任务「验收 UK 部分」"]')
check('完成一项进度变为1/2', floating.wait_for(progress + '==="1/2"'))
click(floating, '[aria-label="完成子任务「验收 CA 部分」"]')
check('子任务全完成后父任务自动完成', floating.wait_for(f'document.querySelector({json.dumps("[aria-label=" + json.dumps("将「"+parent_title+"」标记为未完成", ensure_ascii=False) + "]")})?.getAttribute("aria-checked")==="true"'))
check('绿色圆内白勾不带黑色描边', floating.eval('(() => { const el=document.querySelector(".subtask-preview [aria-checked=true]"); return el && getComputedStyle(el.querySelector("path")).stroke === "rgb(255, 255, 255)" && getComputedStyle(el).backgroundColor !== "rgba(0, 0, 0, 0)"; })()'))

click(floating, '[aria-label^="' + dates['nextDate'] + '"]')
check('日期切换查询明天的真实任务', floating.wait_for(f'document.body.textContent.includes({json.dumps(next_title)}) && !document.body.textContent.includes({json.dumps(parent_title)})'))
click(floating, '[aria-label="新建任务"]')
check('点击加号直接打开编辑行', floating.wait_for('document.querySelector(".quickadd--compact input")'))
click(floating, '[aria-label="新任务选项"]')
check('新建默认沿用选中的日期', floating.eval(f'document.querySelector("[aria-label=\\"计划执行日期\\"]").value==={json.dumps(dates["nextDate"])}'))
ui.set_react_input(floating, '[aria-label="任务标题，可包含日期、标签与优先级"]', '验收-浮窗新建-' + stamp)
ui.set_react_input(floating, '[aria-label="计划执行时间（留空表示仅日期）"]', '15:30')
select(floating, '[aria-label="优先级"]', '3')
click(floating, '.quickadd .btn--primary')
check('点击添加实际保存新任务', floating.wait_for(f'window.__TAURI_INTERNALS__.invoke("task_list",{{query:{{search:{json.dumps("验收-浮窗新建-" + stamp)}}}}}).then(r=>r.length===1)'))
saved = invoke('task_list', {'query': {'search': '验收-浮窗新建-' + stamp}})[0]
check('所选日期时间优先级真实写入', saved['priority'] == 3 and saved['hasPlannedTime'] == 1 and saved['plannedAt'] != dates['today'])
check('保存后新任务在所选日期可见', floating.wait_for(f'document.querySelector(".floating__list").textContent.includes({json.dumps(saved["title"])})'))

click(floating, '[aria-label="新任务提醒"]')
ui.set_react_input(floating, '[aria-label="任务标题，可包含日期、标签与优先级"]', '验收-浮窗提醒-' + stamp)
ui.set_react_input(floating, '[aria-label="计划执行时间（留空表示仅日期）"]', '16:30')
click(floating, '.quickadd .btn--primary')
check('新建行可以选择保存后设置提醒', floating.wait_for('document.querySelector("[aria-label=\\"提醒面板\\"] .reminders")'))
click(floating, '.remnew button')
rem_task = invoke('task_list', {'query': {'search': '验收-浮窗提醒-' + stamp}})[0]
check('提醒关联到新建任务并保存', floating.wait_for(f'window.__TAURI_INTERNALS__.invoke("reminder_list",{{taskId:{json.dumps(rem_task["id"])}}}).then(r=>r.length===1)'))
if floating.eval('document.querySelector("[aria-label=\\"收起面板\\"]") !== null'):
    click(floating, '[aria-label="收起面板"]')
button(floating, '开始专注')
check('专注入口接入现有计时器', floating.wait_for('document.querySelector(".focuspanel")'))
click(floating, '.focuspanel .btn--primary')
check('专注计时真实开始', invoke('focus_current')['state'] == 'running')
session = invoke('focus_current')
invoke('focus_cancel', {'sessionId': session['id']})
click(floating, '[aria-label="收起面板"]')
click(floating, '[aria-label="AI 助手"]')
check('浮窗内打开真实AI助手', floating.wait_for('document.querySelector(".ai-assistant")'))
check('隔离配置缺少密钥时提示而非伪造结果', floating.wait_for('document.querySelector(".ai-assistant").textContent.includes("尚未配置 AI 密钥")'))
click(floating, '[aria-label="收起面板"]')
button(floating, '回到今天')
click(floating, '.quickadd button.btn--quiet')
floating.eval('document.querySelector(".floating__workspace").scrollTop=0')
invoke('window_set_floating_size', {'width':450, 'height':600})
check('参考布局在450×600窗口完整显示', floating.wait_for('innerWidth===450 && innerHeight===600'))
time.sleep(.4)
(out / 'floating-reference.png').write_bytes(base64.b64decode(floating.call('Page.captureScreenshot', {'format':'png'})['data']))

main.call('Page.reload')
main.wait_for('document.querySelector(".sidebar")')
button(main, 'AI 助手')
check('主窗口有独立AI助手入口', main.wait_for('document.querySelector(".content .ai-assistant")'))
button(main, '总结与复盘')
options = main.eval('Array.from(document.querySelector("[aria-label=\\"AI 时间范围\\"]").options).map(o=>o.value)')
check('支持日周月年四种总结', options == ['daily','weekly','monthly','yearly'])
select(main, '[aria-label="AI 时间范围"]', 'monthly')
check('总结范围可直接选择月份', main.eval('document.querySelector("[aria-label=\\"AI 时间范围\\"]").value==="monthly"'))
check('支持导入文本文件', main.eval('!!document.querySelector("input[type=file][accept*=txt]")'))
check('未配置密钥时生成按钮禁用', main.eval('Array.from(document.querySelectorAll(".ai-assistant button")).find(b=>b.textContent.includes("生成总结")).disabled'))
(out / 'ai-assistant.png').write_bytes(base64.b64decode(main.call('Page.captureScreenshot', {'format':'png'})['data']))
print('All native floating/assistant checks passed; no external model called', flush=True)
floating.close()
main.close()
