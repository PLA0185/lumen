"""Native clipboard / content / weekly arrangement acceptance, isolated profile only.

The drag route test emits Tauri's file-drop event; it is NOT an Explorer OLE-drag test.
"""
import argparse, ctypes, json, struct, time
from pathlib import Path
import ui_drive as ui

p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--data-dir',type=Path,required=True)
p.add_argument('--output-dir',type=Path,required=True)
p.add_argument('--port',type=int,default=9223)
args=p.parse_args()
main=ui.connect(args.port,timeout=45)
def invoke(name,payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(name)}, {json.dumps(payload or {})})')
assert args.data_dir.name=='memo-test-data'
assert args.data_dir.resolve()==Path(invoke('app_data_paths')['dataDir']).resolve()
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert not args.output_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
args.output_dir.mkdir(parents=True,exist_ok=True)
checks=[]
def check(name,condition):
    assert condition,name
    checks.append(name); print('PASS: '+name,flush=True)
def wait(expr):
    if not main.wait_for(expr,timeout=15):
        (args.output_dir/'content-failure-dom.html').write_text(main.eval('document.documentElement.outerHTML'),encoding='utf-8')
        raise AssertionError(expr)
def click(selector):
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main,selector)
def button(text):
    selector='[data-content-check-click]'
    main.eval(f'Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim()==={json.dumps(text)}).dataset.contentCheckClick="1"')
    click(selector)
    main.eval('document.querySelectorAll("[data-content-check-click]").forEach(b=>delete b.dataset.contentCheckClick)')
def input_value(label,value):
    main.eval(f'''(() => {{const n=document.querySelector('[aria-label={json.dumps(label)}]'); Object.getOwnPropertyDescriptor(n.tagName==='TEXTAREA'?HTMLTextAreaElement.prototype:HTMLInputElement.prototype,'value').set.call(n,{json.dumps(value)});n.dispatchEvent(new Event('input',{{bubbles:true}}));}})()''')

# CF_DIB, four colored pixels. readImage must read the real Windows clipboard.
u=ctypes.WinDLL('user32',use_last_error=True); k=ctypes.WinDLL('kernel32',use_last_error=True)
u.OpenClipboard.argtypes=[ctypes.c_void_p];u.SetClipboardData.argtypes=[ctypes.c_uint,ctypes.c_void_p];u.SetClipboardData.restype=ctypes.c_void_p
k.GlobalAlloc.argtypes=[ctypes.c_uint,ctypes.c_size_t];k.GlobalAlloc.restype=ctypes.c_void_p
k.GlobalLock.argtypes=[ctypes.c_void_p];k.GlobalLock.restype=ctypes.c_void_p;k.GlobalUnlock.argtypes=[ctypes.c_void_p]
def clipboard_image():
    data=struct.pack('<IiiHHIIiiII',40,2,2,1,24,0,16,0,0,0,0)+bytes([0,255,0,0,0,255,0,0,255,0,0,255,255,255,0,0])
    assert u.OpenClipboard(None);u.EmptyClipboard()
    try:
        mem=k.GlobalAlloc(0x42,len(data));assert mem
        ptr=k.GlobalLock(mem);ctypes.memmove(ptr,data,len(data));k.GlobalUnlock(mem)
        assert u.SetClipboardData(8,mem)
    finally:u.CloseClipboard()
def paste_image(label):
    selector=f'[aria-label={json.dumps(label,ensure_ascii=False)}]'
    click(selector);clipboard_image()
    for vk in [0x11,0x10,ord('V')]:u.keybd_event(vk,0,0,0)
    time.sleep(.08)
    for vk in [ord('V'),0x10,0x11]:u.keybd_event(vk,0,2,0)
    wait(f'document.querySelector({json.dumps(selector)}).value.includes("lumen-asset:")')

invoke('window_apply_action',{'action':'show_main'})
main.eval('window.__TAURI_INTERNALS__.invoke("plugin:window|unminimize",{label:"main"})')
button('备忘与流程');button('新建流程')
input_value('备忘标题','图片与流程实机验收')
input_value('第 1 步标题','核对资料')
paste_image('备忘内容')
check('Ctrl+Shift+V 从真实 Windows 图片剪贴板导入备忘', 'lumen-asset:' in main.eval('document.querySelector("[aria-label=备忘内容]").value'))
paste_image('第 1 步说明')
check('流程步骤支持真实剪贴板图片', 'lumen-asset:' in main.eval("document.querySelector('[aria-label=\"第 1 步说明\"]').value"))

source=args.output_dir/'流程文件.txt';source.write_text('操作事项：发货前核对订单\n审批通过后执行',encoding='utf-8')
field='[aria-label="第 1 步说明"]';click(field)
main.eval(f'''(() => {{const n=document.querySelector({json.dumps(field)}),r=n.getBoundingClientRect();window.__TAURI_INTERNALS__.invoke('plugin:event|emit',{{event:'tauri://drag-drop',payload:{{paths:[{json.dumps(str(source))}],position:{{x:(r.x+10)*devicePixelRatio,y:(r.y+10)*devicePixelRatio}}}}}});}})()''')
wait(f'document.querySelector({json.dumps(field)}).value.includes("流程文件.txt")')
check('Tauri 文件拖入事件路由到命中的流程步骤',True)
button('保存并查看')
wait('document.querySelectorAll(".content-asset__image").length===2')
wait('Array.from(document.querySelectorAll(".content-asset__image")).every(i=>i.complete&&i.naturalWidth===2)')
check('备忘与流程图片解码显示，非只保存文件名',True)
docs=invoke('memo_list',{'query':'图片与流程实机验收','deletedOnly':False})
# API list is an array, not the shape of a task paginated query.
doc=invoke('memo_get',{'id':docs[0]['id']})
check('正文和流程步骤引用已写入数据库', 'lumen-asset:' in doc['bodyMd'] and '流程文件.txt' in doc['steps'][0]['detail'])
source.unlink()
import re
asset_ids=re.findall(r'lumen-asset:([0-9a-f-]+)',doc['steps'][0]['detail'])
file_asset=next(invoke('content_asset_get',{'id':i}) for i in asset_ids if invoke('content_asset_get',{'id':i})['name']=='流程文件.txt')
check('原文件移走后内容资源仍保留实际字节', file_asset['byteSize']>0)
out=args.output_dir/'流程另存.txt';invoke('content_asset_export',{'id':file_asset['id'],'path':str(out)})
check('导出资源恢复原文件内容',out.read_text(encoding='utf-8')=='操作事项：发货前核对订单\n审批通过后执行')

# Reuse QuickAdd / the existing rule editor, with a weekly default in this view.
button('每周重复');button('新建')
wait('document.querySelector("[aria-label=重复频率]")')
check('每周重复视图新建时默认按周重复',main.eval('document.querySelector("[aria-label=重复频率]").value')=='weekly')
input_value('任务标题，可包含日期、标签与优先级','每周业务检查')
button('添加')
wait('document.querySelector(".weekly-recurring__item")')
check('每周重复视图每个系列仅显示一条',main.eval('Array.from(document.querySelectorAll(".weekly-recurring__item")).filter(n=>n.textContent.includes("每周业务检查")).length')==1)
button('修改重复规则')
wait('document.querySelector("#series-rule-title")')
check('已有系列可直接编辑星期及节假日选项',main.eval('document.body.textContent.includes("法定节假日不执行") && document.querySelector("[aria-label=周一]")!==null'))
button('取消')

# Status colors are measured in WebView, not inferred from class names.
invoke('window_apply_action',{'action':'show_floating'})
floating=ui.connect(args.port,want='floating',timeout=15)
try:
    invoke('window_apply_action',{'action':'toggle_floating_top'})
    time.sleep(.4)
    off=floating.eval('getComputedStyle(document.querySelector("[aria-pressed=false].floating__btn")).color')
    invoke('window_apply_action',{'action':'toggle_floating_top'})
    time.sleep(.4)
    on=floating.eval('getComputedStyle(document.querySelector("[aria-pressed=true].floating__btn")).color')
    check('浮窗开关启用绿色、关闭灰色',on=='rgb(34, 197, 94)' and off=='rgb(136, 145, 159)')
    invoke('window_apply_action',{'action':'toggle_floating_click_through'})
    time.sleep(.4)
    check('鼠标穿透启用后绿色状态仍可见',floating.eval('document.querySelector("[aria-label=鼠标穿透已开启]")?.getAttribute("aria-pressed")')=='true')
    invoke('window_apply_action',{'action':'toggle_floating_click_through'})
finally:floating.close()

backup=invoke('backup_export',{'path':str(args.output_dir/'content.lumen-backup.json')})
payload=json.loads(Path(backup['path']).read_text(encoding='utf-8'))
check('格式 5 备份保存图片和文件本体', payload['formatVersion']==5 and len(payload['data']['contentAssets'])>=3)
check('备份确实含导入文件的 Base64 内容',any(a['data_base64']==file_asset['dataBase64'] for a in payload['data']['contentAssets']))
status=invoke('ai_status')
(args.output_dir/'content-native.json').write_text(json.dumps({'checks':checks,'passed':len(checks),'aiConfigured':status['configured'],'note':'未执行 Explorer OLE 拖动；Tauri 拖入事件路由已验证。'},ensure_ascii=False,indent=2),encoding='utf-8')
main.close()
