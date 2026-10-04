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


def rect(selector, index=0):
	return t.eval('(()=>{const r=document.querySelectorAll(' + json.dumps(selector) + ')[' + str(index) + '].getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()')


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
    groups.extend((f'{i}. 验收章节', []) for i in range(8, 15))
    doc = invoke('memo_save', {'input': {'id': None, 'expectedRevision': None, 'title': 'Amazon出货SOP-工具栏验收-' + str(uuid.uuid4())[:8], 'category': '', 'kind': 'flow', 'bodyMd': '', 'steps': [
        {'id': str(uuid.uuid4()), 'title': '原操作' + str(i + 1), 'owner': '', 'detail': '原文说明。' + ('\n\n![原图](lumen-asset:' + asset['id'] + ')' if i == 0 else ''), 'group': {'id': title, 'title': title, 'path': path}}
        for i, (title, path) in enumerate(groups)]}})
    click('日历')
    assert t.wait_for('document.querySelector(".calcell[role=gridcell]")')
    check('月历可选日期使用手型指针', t.eval('getComputedStyle(document.querySelector(".calcell[role=gridcell]")).cursor==="pointer"'))
    click('备忘与流程')
    if t.eval('!!document.querySelector(".flow-canvas")'):
        click('返回列表')
    click('刷新列表'); click(doc['title'])
    assert t.wait_for('document.querySelectorAll(".flow-canvas__nav-step").length===' + str(len(groups)))
    check('侧栏内容在当前窗口高度内时不显示多余滚动', t.eval('(()=>{const e=document.querySelector(".sidebar__nav");return e.scrollHeight<=e.clientHeight})()'))
    t.call('Emulation.setDeviceMetricsOverride', {'width': 1250, 'height': 620, 'deviceScaleFactor': 1, 'mobile': False})
    check('窗口高度不足时侧栏仍可滚动到完整导航', t.eval('(()=>{const e=document.querySelector(".sidebar__nav");return e.scrollHeight>e.clientHeight&&getComputedStyle(e).overflowY==="auto"})()'))
    t.call('Emulation.clearDeviceMetricsOverride')
    check('顶部按钮固定8px间距靠拢，搜索相邻且顶栏高度不超过60px', t.eval('(()=>{const controls=document.querySelector(".topbar__controls"),end=document.querySelector(".topbar__end"),style=getComputedStyle(controls);return style.justifyContent==="flex-start"&&parseFloat(style.gap)===8&&end.getBoundingClientRect().left-controls.getBoundingClientRect().right<=13&&document.querySelector(".topbar").getBoundingClientRect().height<=60})()'))
    check('流程切换按钮使用与主要操作相同的紫色底', t.eval('getComputedStyle(document.querySelector(".flow-switcher__title")).backgroundColor===getComputedStyle(document.querySelector(".memos__toolbar .btn--primary:not(.flow-switcher__title)")).backgroundColor&&document.querySelector(".flow-switcher__title").classList.contains("btn--primary")'))
    check('流程名称完整显示在画布左上角，不参与工具栏缩放', t.eval('(()=>{const button=document.querySelector(".flow-canvas__switcher .flow-switcher__title"),head=document.querySelector(".flow-canvas__head"),toolbar=document.querySelector(".memos__toolbar"),b=button.getBoundingClientRect(),h=head.getBoundingClientRect();return button.textContent.includes(' + json.dumps(doc['title']) + ')&&button.scrollWidth<=button.clientWidth&&!toolbar.contains(button)&&Math.abs(b.left-h.left)<1})()'))
    check('普通操作按钮均有可见底色', t.eval('[...document.querySelectorAll(".memos__toolbar .btn:not(.btn--primary)")].every(e=>{const bg=getComputedStyle(e).backgroundColor;return bg!=="transparent"&&bg!=="rgba(0, 0, 0, 0)"&&bg!==getComputedStyle(document.querySelector(".memos__toolbar")).backgroundColor})'))
    check('返回列表与同行普通按钮高度、字号和内边距一致', t.eval('(()=>{const buttons=[...document.querySelectorAll(".memos__toolbar .btn--ghost")],back=buttons.find(b=>b.textContent.trim()==="返回列表"),other=buttons.find(b=>b.textContent.trim()==="细分流程"),a=getComputedStyle(back),b=getComputedStyle(other);return a.height===b.height&&a.fontSize===b.fontSize&&a.padding===b.padding})()'))
    check('保存完成时移除状态文字和占位，不挤工具栏', t.eval('!document.querySelector(".memos__save-status")&&parseFloat(getComputedStyle(document.querySelector(".memos__toolbar-row")).gap)===8'))
    check('章节、阶段、步骤导航层级正确', t.eval('[...document.querySelectorAll(".flow-canvas__nav-step")].slice(0,5).map(e=>e.dataset.level)') == ['chapter', 'step', 'stage', 'chapter', 'step'])
    check('所有可点候选、导航、流程节点及其正文统一显示手型', t.eval('(()=>{const selectors=[".memos__toolbar button:not(:disabled)",".memos__toolbar select:not(:disabled)",".flow-switcher__title",".flow-canvas__nav-step",".flow-canvas__node",".flow-canvas__node .flow-canvas__body"];return selectors.every(selector=>{const e=document.querySelector(selector);return !!e&&getComputedStyle(e).cursor==="pointer"})})()'))
    check('九个章节采用九种不同柔和色相，同章节不变色', t.eval('(()=>{const marks=[...document.querySelectorAll(".flow-canvas__nav-step")],roots=marks.filter(e=>e.dataset.level==="chapter"),hue=e=>e.style.getPropertyValue("--flow-chapter-hue");return roots.length===9&&new Set(roots.map(hue)).size===9&&hue(marks[0])===hue(marks[1])&&hue(marks[1])===hue(marks[2])})()'))
    check('尚未选择节点时不把整条导航误压暗', t.eval('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],marks=steps.map(e=>parseFloat(getComputedStyle(e.querySelector(".flow-canvas__nav-mark")).opacity));return !steps.some(e=>e.hasAttribute("aria-current"))&&marks.every(value=>value>=.5)})()'))
    ui.real_click(t, '.flow-canvas__nav-step', index=0)
    assert t.wait_for('document.querySelector(".flow-canvas__nav-step[aria-current=step]")')
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'x': 20, 'y': 20})
    assert t.wait_for('!document.querySelector(".flow-canvas__nav-step:hover")')
    assert t.wait_for('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],active=steps.find(e=>e.getAttribute("aria-current")==="step"),alpha=e=>parseFloat(getComputedStyle(e.querySelector(".flow-canvas__nav-mark")).opacity);return !!active&&alpha(active)>=.9&&steps.filter(e=>e!==active).every(e=>alpha(e)<=.35)})()')
    check('未悬停时当前查看节点高亮、其他导航刻度变淡', t.eval('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],active=steps.find(e=>e.getAttribute("aria-current")==="step");if(!active)return false;const alpha=e=>parseFloat(getComputedStyle(e.querySelector(".flow-canvas__nav-mark")).opacity);return alpha(active)>=.9&&steps.filter(e=>e!==active).every(e=>alpha(e)<=.35)})()'))
    hovered = rect('.flow-canvas__nav-step', index=3)
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', **hovered})
    assert t.wait_for('document.querySelectorAll(".flow-canvas__nav-step")[3]?.matches(":hover")')
    assert t.wait_for('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],active=steps.find(e=>e.matches(":hover")),alpha=e=>parseFloat(getComputedStyle(e.querySelector(".flow-canvas__nav-mark")).opacity);return !!active&&alpha(active)>=.9&&steps.filter(e=>e!==active).every(e=>alpha(e)<=.35)})()')
    check('悬停的导航刻度成为焦点并使其余刻度变淡', t.eval('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],active=steps.find(e=>e.matches(":hover"));if(!active)return false;const alpha=e=>parseFloat(getComputedStyle(e.querySelector(".flow-canvas__nav-mark")).opacity);return alpha(active)>=.9&&steps.filter(e=>e!==active).every(e=>alpha(e)<=.35)})()'))
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'x': 20, 'y': 20})
    assert t.wait_for('!document.querySelector(".flow-canvas__nav-step:hover")')
    assert t.wait_for('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],active=steps.find(e=>e.getAttribute("aria-current")==="step"),alpha=e=>parseFloat(getComputedStyle(e.querySelector(".flow-canvas__nav-mark")).opacity);return !!active&&alpha(active)>=.9&&steps.filter(e=>e!==active).every(e=>alpha(e)<=.35)})()')
    check('鼠标离开导航后高亮恢复到正在查看的节点', t.eval('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],active=steps.find(e=>e.getAttribute("aria-current")==="step");if(!active)return false;const alpha=e=>parseFloat(getComputedStyle(e.querySelector(".flow-canvas__nav-mark")).opacity);return alpha(active)>=.9&&steps.filter(e=>e!==active).every(e=>alpha(e)<=.35)})()'))
    sizes = t.eval('[...document.querySelectorAll(".flow-canvas__nav-mark")].map(e=>parseFloat(getComputedStyle(e).height))')
    check('章节刻度大于阶段、阶段大于普通步骤', sizes[0] > sizes[2] > sizes[4])
    time.sleep(.2)
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'x': 20, 'y': 20})
    hover = rect('.flow-canvas__nav-step')
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', **hover})
    assert t.wait_for('document.querySelector(".flow-canvas__nav-step[data-level=chapter]:hover")')
    assert t.wait_for('getComputedStyle(document.querySelector(".flow-canvas__nav-label")).visibility==="visible"')
    time.sleep(.2)
    tooltip = t.eval('(()=>{const e=document.querySelector(".flow-canvas__nav-label");return {text:e.textContent,visible:getComputedStyle(e).visibility}})()')
    check('真实悬停只显示当前章节路径、不展开其他小章节', tooltip['visible'] == 'visible' and all(s in tooltip['text'] for s in ['6. 发票', '6.1 美国发票']) and '6.2 英国发票' not in tooltip['text'])
    check('大章节悬停明显放大，提示框避开刻度', t.eval('(()=>{const step=document.querySelector(".flow-canvas__nav-step[data-level=chapter]:hover");if(!step)return false;const mark=step.querySelector(".flow-canvas__nav-mark").getBoundingClientRect(),label=step.querySelector(".flow-canvas__nav-label").getBoundingClientRect();return mark.height>=30&&mark.width>=5&&label.top-mark.bottom>=20})()'))
    check('导航提示框与刻度保留20px间距', t.eval('document.querySelector(".flow-canvas__nav-label").getBoundingClientRect().top-document.querySelector(".flow-canvas__nav-step").getBoundingClientRect().bottom>=19.5'))
    ui.real_click(t, '.flow-canvas__nav-step', index=2)
    check('阶段导航真实点击定位对应步骤', t.wait_for('document.querySelectorAll(".flow-canvas__nav-step")[2].getAttribute("aria-current")==="step"'))
    assert t.wait_for('(()=>{const e=document.querySelector(".flow-canvas__nav-step[data-level=chapter][data-current-chapter=true]"),m=e?.querySelector(".flow-canvas__nav-mark");return !!m&&parseFloat(getComputedStyle(m).height)>=28})()')
    check('选中小章节时所属大章节刻度仍明显更大', t.eval('(()=>{const steps=[...document.querySelectorAll(".flow-canvas__nav-step")],parent=steps.find(e=>e.dataset.level==="chapter"&&e.hasAttribute("data-current-chapter")),child=steps.find(e=>e.getAttribute("aria-current")==="step");if(!parent||!child)return false;const a=parent.querySelector(".flow-canvas__nav-mark"),b=child.querySelector(".flow-canvas__nav-mark");return parseFloat(getComputedStyle(a).height)>=28&&parseFloat(getComputedStyle(a).height)>=parseFloat(getComputedStyle(b).height)+5})()'))
    ui.real_click(t, '.flow-canvas__nav-step', index=0)
    assert t.wait_for('(()=>{const e=document.querySelector(".flow-canvas__nav-step[data-level=chapter][aria-current=step]"),m=e?.querySelector(".flow-canvas__nav-mark");return !!m&&parseFloat(getComputedStyle(m).height)>=28})()')
    check('选中大章节时大章节刻度保持突出', t.eval('(()=>{const e=document.querySelector(".flow-canvas__nav-step[data-level=chapter][aria-current=step]"),m=e?.querySelector(".flow-canvas__nav-mark");return !!m&&parseFloat(getComputedStyle(m).height)>=28})()'))
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
    check('自动保存完成后彻底移除已保存占位', t.eval('!document.querySelector(".memos__save-status")'))
    check('自动保存后整行菜单位置稳定', geometry() == g)
    check('保存前后原文、图片引用和分组不变', all({k: s[k] for k in ['id', 'title', 'owner', 'detail', 'group']} == {k: d[k] for k in ['id', 'title', 'owner', 'detail', 'group']} for s, d in zip(after['steps'], doc['steps'])))
    click('流程信息')
    selector = '[aria-label="备忘标题"]'
    assert t.wait_for('document.querySelector(' + json.dumps(selector) + ')')
    ui.real_click(t, selector)
    t.eval('document.querySelector(' + json.dumps(selector) + ').select()')
    t.call('Input.insertText', {'text': '  ' + doc['title'] + '  '})
    assert t.wait_for('document.querySelector(' + json.dumps(selector) + ').value===' + json.dumps(doc['title']))
    normalized = invoke('memo_get', {'id': doc['id']})
    time.sleep(3)
    check('真实保存接受后端标题规范化且不重复写入', invoke('memo_get', {'id': doc['id']}) == normalized and normalized['revision'] == after['revision'] + 1 and normalized['title'] == doc['title'])
    click('收起信息')
    ui.real_click(t, '.flow-switcher__title')
    check('缩放后的流程切换菜单完整弹出', t.wait_for('document.querySelector(".flow-switcher__menu")') and t.eval('document.querySelector(".flow-switcher__menu").parentElement===document.body'))
    check('当前流程以柔和底色和边线标识，文字保持正常正文色', t.eval('(()=>{const menu=document.querySelector(".flow-switcher__menu"),item=menu.querySelector(".flow-switcher__item[aria-current=page]");return !!item&&getComputedStyle(item).color===getComputedStyle(menu).color&&getComputedStyle(item).backgroundColor!==getComputedStyle(menu).backgroundColor&&getComputedStyle(item).boxShadow.includes("inset")})()'))
    (args.output_dir / 'native.png').write_bytes(base64.b64decode(t.call('Page.captureScreenshot', {'format': 'png'})['data']))
    (args.output_dir / 'native.json').write_text(json.dumps({'checks': checks, 'geometry': positions, 'savedRevision': normalized['revision']}, ensure_ascii=False, indent=2), encoding='utf8')
finally:
    t.call('Emulation.clearDeviceMetricsOverride')
    t.close()
