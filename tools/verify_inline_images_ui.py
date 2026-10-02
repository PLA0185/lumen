"""Real isolated WebView2 clicks: inline editing, step menu, image annotations and wrapping."""
import argparse
import base64
import hashlib
import json
import re
import time
import uuid
from pathlib import Path
import ui_drive as ui

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--port', type=int, required=True)
p.add_argument('--data-dir', type=Path, required=True)
p.add_argument('--output-dir', type=Path, required=True)
a = p.parse_args()
t = ui.connect(a.port)

def invoke(name, args=None):
    return t.eval('window.__TAURI_INTERNALS__.invoke(' + json.dumps(name) + ',' + json.dumps(args or {}) + ')')

def check(label, ok):
    assert ok, label
    checks.append(label)
    print('PASS: ' + label, flush=True)

def click(label, scope='document'):
    probe = 'Array.from(' + scope + '.querySelectorAll("button")).find(b=>!b.disabled&&b.getBoundingClientRect().width>0&&(b.textContent.trim()===' + json.dumps(label) + '||b.getAttribute("aria-label")===' + json.dumps(label) + '||b.querySelector("strong")?.textContent===' + json.dumps(label) + '))'
    assert t.wait_for(probe), label
    t.eval(probe + '.dataset.verifyInline="1"')
    ui.real_click(t, '[data-verify-inline]')
    t.eval('document.querySelectorAll("[data-verify-inline]").forEach(b=>delete b.dataset.verifyInline)')

def double_click(selector):
    r = t.eval('(()=>{const r=document.querySelector(' + json.dumps(selector) + ').getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()')
    for count in [1, 2]:
        t.call('Input.dispatchMouseEvent', {'type': 'mousePressed', 'button': 'left', 'buttons': 1, 'clickCount': count, **r})
        t.call('Input.dispatchMouseEvent', {'type': 'mouseReleased', 'button': 'left', 'buttons': 0, 'clickCount': count, **r})

def select(selector, value):
    t.eval('(()=>{const s=document.querySelector(' + json.dumps(selector) + ');s.value=' + json.dumps(str(value)) + ';s.dispatchEvent(new Event("change",{bubbles:true}))})()')

def drag(start, end):
    t.call('Input.dispatchMouseEvent', {'type': 'mousePressed', 'button': 'left', 'buttons': 1, 'clickCount': 1, **start})
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'button': 'left', 'buttons': 1, **end})
    t.call('Input.dispatchMouseEvent', {'type': 'mouseReleased', 'button': 'left', 'buttons': 0, 'clickCount': 1, **end})

def wait_async(expression):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if t.eval(expression):
            return True
        time.sleep(.1)
    return False

def reveal_in_canvas(selector):
    for _ in range(5):
        r = t.eval('(()=>{const r=document.querySelector('+json.dumps(selector)+').getBoundingClientRect(),v=document.querySelector(".flow-canvas__viewport").getBoundingClientRect();return {y:r.top+r.height/2,top:v.top,bottom:v.bottom,x:v.left+12}})()')
        if r['top'] + 130 < r['y'] < r['bottom'] - 80:
            return
        y = r['top'] + 150
        delta = (r['top'] + r['bottom']) / 2 - r['y']
        t.call('Input.dispatchMouseEvent', {'type': 'mousePressed', 'button': 'middle', 'buttons': 4, 'clickCount': 1, 'x': r['x'], 'y': y})
        t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'button': 'middle', 'buttons': 4, 'x': r['x'], 'y': y + delta})
        t.call('Input.dispatchMouseEvent', {'type': 'mouseReleased', 'button': 'middle', 'buttons': 0, 'clickCount': 1, 'x': r['x'], 'y': y + delta})
        time.sleep(.1)
    raise AssertionError('画布无法平移到元素：' + selector)

checks = []
repo = Path(__file__).resolve().parents[1]
assert a.data_dir.resolve() != (Path.home() / 'AppData/Roaming/com.pla0185.lumen').resolve()
assert not a.data_dir.resolve().is_relative_to(repo)
assert Path(invoke('app_data_paths')['dataDir']).resolve() == a.data_dir.resolve()
assert not invoke('cloud_sync_status')['config']
a.output_dir.mkdir(parents=True, exist_ok=True)
try:
    encoded = t.eval('(()=>{const c=document.createElement("canvas");c.width=500;c.height=300;const x=c.getContext("2d");x.fillStyle="#fff";x.fillRect(0,0,500,300);x.fillStyle="#1677ff";x.fillRect(350,150,100,100);return c.toDataURL("image/png").split(",")[1]})()')
    asset = invoke('content_asset_import', {'name': 'image1.png', 'dataBase64': encoded})
    original_asset = invoke('content_asset_get', {'id': asset['id']})
    step_ids = [str(uuid.uuid4()) for _ in range(4)]
    original_text = '例外情况：若总数不一致，需回查货件与出货计划数据。'
    flow = invoke('memo_save', {'input': {'id': None, 'expectedRevision': None, 'title': '卡片图片原生验收-' + str(uuid.uuid4())[:8], 'category': '', 'kind': 'flow', 'bodyMd': '![原图](lumen-asset:' + asset['id'] + ')', 'steps': [
        {'id': step_ids[0], 'title': '下载本周出货计划Excel表格', 'owner': '', 'detail': original_text + '\n\n![原图](lumen-asset:' + asset['id'] + ')'},
        *[{'id': step_ids[i], 'title': '步骤' + str(i + 1), 'owner': '', 'detail': '原始正文' + str(i + 1)} for i in range(1, 4)],
    ]}})
    click('备忘与流程', 'document.querySelector(".sidebar")')
    if t.eval('!!document.querySelector(".flow-canvas")'):
        click('返回列表')
    click('刷新列表')
    click(flow['title'])
    assert t.wait_for('document.querySelectorAll(".flow-canvas__node").length===4')
    ui.real_click(t, '.flow-canvas__node h3')
    check('单击卡片不显示步骤详情或编辑输入', t.eval('!document.querySelector(".flow-canvas__inspector")&&!document.querySelector(".flow-canvas__inline-editor")'))
    double_click('.flow-canvas__node h3')
    check('真实双击在卡片内编辑，未出现步骤详情侧栏', t.wait_for('document.querySelector(".flow-canvas__node .flow-canvas__inline-editor")&&!document.querySelector(".flow-canvas__inspector")'))
    ui.real_click(t, '[aria-label="第 1 步标题"]')
    t.call('Input.dispatchKeyEvent', {'type': 'keyDown', 'key': 'a', 'code': 'KeyA', 'modifiers': 2, 'windowsVirtualKeyCode': 65})
    t.call('Input.dispatchKeyEvent', {'type': 'keyUp', 'key': 'a', 'code': 'KeyA', 'modifiers': 2, 'windowsVirtualKeyCode': 65})
    t.call('Input.insertText', {'text': '实际卡片内编辑'})
    field_selector = json.dumps('[aria-label="第 1 步说明"]')
    field_size = t.eval('(()=>{const r=document.querySelector(' + field_selector + ').getBoundingClientRect();return {width:r.width,height:r.height}})()')
    bottom = t.eval('(()=>{const r=document.querySelector(' + json.dumps('[aria-label="调整第 1 步说明高度"]') + ').getBoundingClientRect();return {x:r.left+r.width*.2,y:r.top+r.height/2}})()')
    drag(bottom, {'x': bottom['x'] + 50, 'y': bottom['y'] + 60})
    check('说明整条底边左侧可拖动，只改变高度', t.wait_for('(()=>{const r=document.querySelector(' + field_selector + ').getBoundingClientRect();return r.height>' + str(field_size['height'] + 40) + '&&Math.abs(r.width-' + str(field_size['width']) + ')<1})()'))
    click('完成编辑')
    check('卡片直接更新编辑后的标题', t.wait_for('document.querySelector(".flow-canvas__node h3")?.textContent==="实际卡片内编辑"'))
    wrapping = t.eval(r'''(()=>{
      const card=document.querySelector('.flow-canvas__node'),para=card.querySelector('.flow-canvas__body p'),oldWidth=card.style.width,oldSize=para.style.fontSize,results=[];
      for(const width of [280,420,660]) for(const font of [12,14,18,24]) {
        card.style.width=width+'px';para.style.fontSize=font+'px';const lines=new Map(),walker=document.createTreeWalker(para,NodeFilter.SHOW_TEXT);
        while(walker.nextNode()){const node=walker.currentNode;for(let i=0;i<node.length;i++){const ch=node.textContent[i];if(!/[\p{Letter}\p{Number}]/u.test(ch))continue;const range=document.createRange();range.setStart(node,i);range.setEnd(node,i+1);const r=range.getBoundingClientRect(),key=Math.round(r.top*2)/2;lines.set(key,(lines.get(key)||'')+ch)}}
        results.push({width,font,lines:[...lines.values()]});
      }
      card.style.width=oldWidth;para.style.fontSize=oldSize;return results;
    })()''')
    check('三种卡片宽度和四档字体逐行测量均无单字残行', all(item['lines'] and all(len(line) >= 2 for line in item['lines']) for item in wrapping))
    (a.output_dir / 'wrapping-lines.json').write_text(json.dumps(wrapping, ensure_ascii=False, indent=2), encoding='utf8')
    check('导航是紧凑刻度带且包含真实过渡动画', t.eval('(()=>{const n=document.querySelector(".flow-canvas__nav"),r=n.getBoundingClientRect(),m=getComputedStyle(n.querySelector(".flow-canvas__nav-mark"));return r.width<120&&r.height<=26&&m.transitionDuration!=="0s"})()'))
    resize_handle = t.eval('(()=>{const r=document.querySelector(".flow-canvas__resize-handle").getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()')
    old_width = t.eval('document.querySelector(".flow-canvas__node").offsetWidth')
    drag(resize_handle, {'x': resize_handle['x'] + 60, 'y': resize_handle['y'] + 10})
    check('真实拖动调整卡片宽度', t.wait_for('document.querySelector(".flow-canvas__node").offsetWidth>' + str(old_width + 40)))
    click('自动排版')
    click('定位第 2 步：步骤2')
    double_click('.flow-canvas__node:nth-of-type(2) h3')
    reveal_in_canvas('.flow-image-picker summary'); ui.real_click(t, '.flow-image-picker summary')
    check('关联图片显示可选择的缩略图，没有文件名下拉框', t.wait_for('document.querySelector(".flow-image-picker details")?.open&&document.querySelector(".flow-image-picker img")?.naturalWidth===500&&!document.querySelector(".flow-image-picker select")'))
    reveal_in_canvas('.flow-image-picker img'); ui.real_click(t, '.flow-image-picker img')
    reveal_in_canvas('[aria-label="添加到第 2 步"]'); click('添加到第 2 步')
    check('缩略图确认添加后真正写入步骤正文', t.wait_for('document.querySelector('+json.dumps('[aria-label="第 2 步说明"]')+').value.includes('+json.dumps(asset['id'])+')'))
    reveal_in_canvas('.memos__step-actions button'); click('完成编辑'); click('定位第 1 步：实际卡片内编辑')
    click('第 1 步操作')
    select('[aria-label="交换目标步骤"]', step_ids[3])
    click('交换位置')
    check('交换后首尾真正互换，正文随节点保留', t.wait_for('document.querySelectorAll(".flow-canvas__node")[0]?.dataset.stepId===' + json.dumps(step_ids[3]) + '&&document.querySelectorAll(".flow-canvas__node")[3]?.dataset.stepId===' + json.dumps(step_ids[0])))
    click('第 4 步操作')
    select('[aria-label="移动目标位置"]', 0)
    click('移动到此位置')
    check('移到第一步顺移中间节点', t.wait_for('document.querySelector(".flow-canvas__node")?.dataset.stepId===' + json.dumps(step_ids[0])))
    click('添加步骤')
    check('添加新步骤居中并直接在卡片内编辑', t.wait_for('document.querySelectorAll(".flow-canvas__node").length===5&&document.querySelectorAll(".flow-canvas__node")[4]?.querySelector(".flow-canvas__inline-editor")'))
    click('第 5 步操作'); click('删除步骤')
    check('删除前仍保留节点直到确认', t.eval('document.querySelectorAll(".flow-canvas__node").length===5'))
    click('确认删除此步骤')
    check('确认删除后路线重新编号', t.wait_for('document.querySelectorAll(".flow-canvas__node").length===4&&document.querySelectorAll(".flow-canvas__nav-step").length===4'))
    click('定位第 1 步：实际卡片内编辑')
    other = invoke('memo_save', {'input': {'id': None, 'expectedRevision': None, 'title': '切换目标-' + str(uuid.uuid4())[:8], 'category': '', 'kind': 'flow', 'bodyMd': '', 'steps': [{'id': str(uuid.uuid4()), 'title': '切换实际节点', 'owner': '', 'detail': '原文'}]}})
    click('切换流程：' + flow['title']); click(other['title'], 'document.querySelector(".flow-switcher__menu")')
    check('名称按钮实际切换到另一份流程', t.wait_for('document.querySelector(".flow-canvas__node h3")?.textContent==="切换实际节点"'))
    click('切换流程：' + other['title']); click(flow['title'], 'document.querySelector(".flow-switcher__menu")')
    check('切回流程保留顺序与卡片大小', t.wait_for('document.querySelector(".flow-canvas__node h3")?.textContent==="实际卡片内编辑"&&document.querySelector(".flow-canvas__node").offsetWidth>' + str(old_width + 40)))
    assert t.wait_for('document.querySelector(".flow-canvas__node img")?.naturalWidth===500')
    check('图片下没有文件名或大小', not t.eval('document.querySelector(".flow-canvas__node").textContent.includes("image1.png")'))
    ui.real_click(t, '.flow-canvas__node img')
    check('真实单击图片打开大图，原图完整解码', t.wait_for('document.querySelector(".image-viewer__overlay")'))
    click('箭头')
    r = t.eval('(()=>{const r=document.querySelector(".image-viewer__overlay").getBoundingClientRect();return {left:r.left,top:r.top,width:r.width,height:r.height}})()')
    start = {'x': r['left'] + r['width'] * .2, 'y': r['top'] + r['height'] * .2}
    end = {'x': r['left'] + r['width'] * .6, 'y': r['top'] + r['height'] * .4}
    t.call('Input.dispatchMouseEvent', {'type': 'mousePressed', 'button': 'left', 'buttons': 1, 'clickCount': 1, **start})
    t.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'button': 'left', 'buttons': 1, **end})
    t.call('Input.dispatchMouseEvent', {'type': 'mouseReleased', 'button': 'left', 'buttons': 0, 'clickCount': 1, **end})
    check('鼠标绘制箭头有真实坐标及箭头端点', t.wait_for('document.querySelector(".image-viewer__overlay polygon")'))
    click('撤销'); check('撤销实际移除批注', not t.eval('!!document.querySelector(".image-viewer__overlay polygon")'))
    click('重做'); check('重做恢复批注', t.eval('!!document.querySelector(".image-viewer__overlay polygon")'))
    click('文字')
    text_position = {'x': r['left'] + r['width'] * .15, 'y': r['top'] + r['height'] * .55}
    drag(text_position, text_position)
    check('点击图片位置直接出现文字编辑器', t.wait_for('document.activeElement?.getAttribute("aria-label")==="就地编辑批注文字"'))
    t.call('Input.insertText', {'text': '实际发货批注'})
    t.call('Input.dispatchKeyEvent', {'type': 'keyDown', 'key': 'Enter', 'code': 'Enter', 'modifiers': 2, 'windowsVirtualKeyCode': 13})
    t.call('Input.dispatchKeyEvent', {'type': 'keyUp', 'key': 'Enter', 'code': 'Enter', 'modifiers': 2, 'windowsVirtualKeyCode': 13})
    check('完成输入后图片上显示文字', t.wait_for('document.querySelector(".image-viewer__overlay text")?.textContent==="实际发货批注"&&!document.querySelector(".image-viewer textarea")'))
    old_x = t.eval('Number(document.querySelector(".image-viewer__overlay text").getAttribute("x"))')
    drag({'x': text_position['x'] + 5, 'y': text_position['y'] + 5}, {'x': text_position['x'] + 45, 'y': text_position['y'] + 25})
    check('真实鼠标拖动改变文字位置', t.eval('Number(document.querySelector(".image-viewer__overlay text").getAttribute("x"))') > old_x + 20)
    handle = t.eval('(()=>{const r=document.querySelector("[aria-label=等比缩放文字]").getBoundingClientRect();return {x:r.left+r.width/2,y:r.top+r.height/2}})()')
    old_font = t.eval('Number(document.querySelector(".image-viewer__overlay text").getAttribute("font-size"))')
    drag(handle, {'x': handle['x'] + 45, 'y': handle['y'] + 15})
    check('拖动缩放控制点等比调整字号', t.eval('Number(document.querySelector(".image-viewer__overlay text").getAttribute("font-size"))') > old_font)
    double_click('.image-viewer__overlay text')
    check('真实双击文字重新就地编辑', t.wait_for('document.querySelector("[aria-label=就地编辑批注文字]")?.value==="实际发货批注"'))
    click('选择')
    click('关闭图片'); ui.real_click(t, '.flow-canvas__node img')
    check('收起后重新打开保留批注草稿', t.wait_for('document.querySelector(".image-viewer__overlay polygon")'))
    ui.real_click(t, '[aria-label="图片备注"]'); t.call('Input.insertText', {'text': '发货入口箭头备注'})
    click('保存备注'); click('保存到流程')
    check('保存批注替换当前引用并关闭查看器', t.wait_for('!document.querySelector(".image-viewer")&&document.querySelector(".content-asset__caption")?.textContent==="发货入口箭头备注"'))
    assert t.wait_for('document.querySelector(".flow-canvas__node img")?.complete')
    assert wait_async('window.__TAURI_INTERNALS__.invoke("memo_get",{id:' + json.dumps(flow['id']) + '}).then(m=>m.steps[0].detail.includes("发货入口箭头备注")&&!m.steps[0].detail.includes(' + json.dumps(asset['id']) + '))')
    actual = invoke('memo_get', {'id': flow['id']})
    new_id = re.search(r'lumen-asset:([0-9a-f-]{36})', actual['steps'][0]['detail'])[1]
    changed = invoke('content_asset_get', {'id': new_id})
    comparison = {'newId': new_id, 'oldId': asset['id'], 'originalUnchanged': invoke('content_asset_get', {'id': asset['id']}) == original_asset, 'newSha': changed['sha256'], 'oldSha': asset['sha256']}
    (a.output_dir / 'image-comparison.json').write_text(json.dumps(comparison, ensure_ascii=False, indent=2), encoding='utf8')
    check('保存产生新图片，原图字节和摘要完全不变', new_id != asset['id'] and comparison['originalUnchanged'] and changed['sha256'] != asset['sha256'])
    pixel = t.eval('(async()=>{const i=document.querySelector(".flow-canvas__node img");await i.decode();const c=document.createElement("canvas");c.width=i.naturalWidth;c.height=i.naturalHeight;const x=c.getContext("2d");x.drawImage(i,0,0);return [...x.getImageData(200,90,1,1).data]})()')
    check('新图片真实像素包含红色批注而非仅有菜单', pixel[0] > 200 and pixel[1] < 100 and pixel[2] < 100)
    target = a.output_dir / '实际批注.png'
    invoke('content_image_export', {'path': str(target.resolve()), 'dataBase64': changed['dataBase64']})
    check('实际导出的 PNG 字节与批注图一致', hashlib.sha256(target.read_bytes()).hexdigest() == changed['sha256'])
    click('今天', 'document.querySelector(".sidebar")'); click('备忘与流程', 'document.querySelector(".sidebar")'); click(actual['title'])
    check('保存重开保留顺序、卡片编辑、备注及批注图', t.wait_for('document.querySelector(".flow-canvas__node h3")?.textContent==="实际卡片内编辑"&&document.querySelector(".content-asset__caption")?.textContent==="发货入口箭头备注"'))
    (a.output_dir / 'native-inline-images.json').write_text(json.dumps({'checks': checks, 'flowId': actual['id'], 'originalAsset': asset['id'], 'annotatedAsset': new_id, 'pixel': pixel}, ensure_ascii=False, indent=2), encoding='utf8')
    (a.output_dir / 'native-inline-images.png').write_bytes(base64.b64decode(t.call('Page.captureScreenshot', {'format': 'png'})['data']))
finally:
    t.close()
