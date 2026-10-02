"""Real WebView2 checks for complete flow cards, straight layouts, navigation and steady empty views."""
import argparse, base64, ctypes, json, os, struct, sys, zlib
from ctypes import wintypes
from pathlib import Path
import ui_drive as ui

sys.stdout.reconfigure(encoding='utf-8')
p=argparse.ArgumentParser();p.add_argument('--port',type=int,required=True);p.add_argument('--pid',type=int,required=True);p.add_argument('--data-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
t=ui.connect(a.port)
def invoke(name,args=None): return t.eval('window.__TAURI_INTERNALS__.invoke('+json.dumps(name)+','+json.dumps(args or {})+')')
profile=a.data_dir.resolve();repo=Path(__file__).resolve().parents[1]
assert Path(invoke('app_data_paths')['dataDir']).resolve()==profile
assert profile.name=='flow-ai-test-data' and not profile.is_relative_to(repo)
assert profile!=(Path(os.environ['APPDATA'])/'com.pla0185.lumen').resolve()
assert not invoke('cloud_sync_status')['config']
assert not a.output_dir.resolve().is_relative_to(repo)
a.output_dir.mkdir(parents=True,exist_ok=True);checks=[];sizes=[]
previous_font=t.eval('localStorage.getItem("lumen.fontSize")')
# Only resize the explicitly named isolated process; WebView2 interactions remain CDP.
user32=ctypes.WinDLL('user32',use_last_error=True)
user32.GetDpiForWindow.argtypes=[wintypes.HWND];user32.GetDpiForWindow.restype=wintypes.UINT
user32.MoveWindow.argtypes=[wintypes.HWND,ctypes.c_int,ctypes.c_int,ctypes.c_int,ctypes.c_int,wintypes.BOOL]
user32.GetClientRect.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.RECT)]
user32.GetWindowRect.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.RECT)]
user32.GetWindowThreadProcessId.argtypes=[wintypes.HWND,ctypes.POINTER(wintypes.DWORD)]
user32.GetWindowTextW.argtypes=[wintypes.HWND,wintypes.LPWSTR,ctypes.c_int]
callback_type=ctypes.WINFUNCTYPE(wintypes.BOOL,wintypes.HWND,wintypes.LPARAM)
windows=[]
@callback_type
def collect(hwnd,_):
    pid=wintypes.DWORD();user32.GetWindowThreadProcessId(hwnd,ctypes.byref(pid))
    name=ctypes.create_unicode_buffer(256);user32.GetWindowTextW(hwnd,name,256)
    if pid.value==a.pid and name.value=='Lumen':windows.append(hwnd)
    return True
user32.EnumWindows(collect,0)
assert len(windows)==1,'Require the exact main window of the isolated process'
original_rect=wintypes.RECT();assert user32.GetWindowRect(windows[0],ctypes.byref(original_rect))
def resize(width,height):
    hwnd=windows[0];outer=wintypes.RECT();client=wintypes.RECT()
    assert user32.GetWindowRect(hwnd,ctypes.byref(outer)) and user32.GetClientRect(hwnd,ctypes.byref(client))
    dpi=user32.GetDpiForWindow(hwnd)/96
    assert user32.MoveWindow(hwnd,20,20,round(width*dpi)+outer.right-outer.left-client.right,round(height*dpi)+outer.bottom-outer.top-client.bottom,True)

def check(name,ok):
    assert ok,name
    checks.append(name);print('PASS: '+name,flush=True)
def click(label,scope='document'):
    expression='Array.from('+scope+'.querySelectorAll("button")).find(b=>!b.disabled&&b.textContent.trim()==='+json.dumps(label)+')'
    assert t.wait_for('!!('+expression+')'),label
    t.eval(expression+'.dataset.readabilityVerify="1"')
    t.eval('document.querySelector("[data-readability-verify]").scrollIntoView({block:"center",behavior:"instant"})')
    ui.real_click(t,'[data-readability-verify]')
    t.eval('document.querySelectorAll("[data-readability-verify]").forEach(e=>delete e.dataset.readabilityVerify)')
def select(selector,value):
    t.eval('(()=>{const e=document.querySelector('+json.dumps(selector)+');e.value='+json.dumps(value)+';e.dispatchEvent(new Event("change",{bubbles:true}))})()')
def screenshot(name):
    (a.output_dir/name).write_bytes(base64.b64decode(t.call('Page.captureScreenshot',{'format':'png'})['data']))
try:
    click('今天','document.querySelector(".sidebar")')
    assert t.wait_for('document.querySelector(".state__title")?.textContent==="这里还没有任务"')
    t.eval('window.__steadyEmpty=document.querySelector(".state");window.__emptyRemoved=false;window.__emptyObserver=new MutationObserver(()=>{if(!window.__steadyEmpty.isConnected)window.__emptyRemoved=true});window.__emptyObserver.observe(document.querySelector(".content"),{childList:true,subtree:true});document.activeElement?.blur()')
    for _ in range(10):
        t.call('Input.dispatchKeyEvent',{'type':'keyDown','key':'r','code':'KeyR','modifiers':2,'windowsVirtualKeyCode':82})
        t.call('Input.dispatchKeyEvent',{'type':'keyUp','key':'r','code':'KeyR','modifiers':2,'windowsVirtualKeyCode':82})
        t.eval('new Promise(resolve=>setTimeout(resolve,80))')
    check('十次真实刷新保留同一个空态节点，没有切换加载骨架',t.eval('!window.__emptyRemoved&&document.querySelector(".state")===window.__steadyEmpty&&!document.querySelector(".skeleton")'))
    t.eval('window.__emptyObserver.disconnect()')
    for page in ['今天','备忘与流程']:
        click(page,'document.querySelector(".sidebar")')
        for width in [1000,1180,1500,1920]:
            for font in [14,18]:
                resize(width,900)
                t.eval('localStorage.setItem("lumen.fontSize",'+json.dumps(str(font))+');window.dispatchEvent(new StorageEvent("storage",{key:"lumen.fontSize"}))')
                t.eval('new Promise(resolve=>setTimeout(resolve,180))')
                result=t.eval('(()=>{const h=document.querySelector(\'.topbar__heading\'),c=document.querySelector(\'.topbar__controls\'),e=document.querySelector(\'.topbar__end\'),s=e.querySelector(\'.search\'),a=document.querySelector(\'.topbar__actions\'),r=n=>n.getBoundingClientRect();const buttons=Array.from(c.children);return {width:innerWidth,font:getComputedStyle(h).fontSize,scroll:a.scrollWidth-a.clientWidth,titleAbove:r(h.querySelector(\'h1\')).bottom<=r(h.querySelector(\'.topbar__subtitle\')).top+1,spacing:r(c).left-r(h).right,searchRight:r(s).right<=r(e).right,endVisible:r(e).right<=r(a).right+1,nonOverlap:buttons.every((b,i)=>!i||r(b).left>=r(buttons[i-1]).right+7),exportLast:!e.querySelector(\'button[title^="把当前列表"]\')||e.lastElementChild.matches(\'button[title^="把当前列表"]\')}})()')
                sizes.append({'page':page,'requestedWidth':width,'font':font,**result})
                check(f'{page} {width}/{font} 标题上下排列且模块不重叠',result['titleAbove'] and result['spacing']>=8 and result['nonOverlap'] and result['searchRight'] and result['exportLast'] and result['endVisible'] and result['scroll']<=1)
    resize(1500,950)
    t.eval('localStorage.setItem("lumen.fontSize","14");window.dispatchEvent(new StorageEvent("storage",{key:"lumen.fontSize"}))')
    click('新建流程');click('流程信息');ui.set_react_input(t,'[aria-label="备忘标题"]','完整卡片与导航隔离验收')
    ui.real_click(t,'.flow-canvas__node .flow-canvas__title')
    def chunk(kind,data): return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
    raw=b''.join(b'\0'+bytes([40,80,200])*640 for _ in range(240))
    fixture=a.output_dir/'step-image.png';fixture.write_bytes(b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',640,240,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(raw))+chunk(b'IEND',b''))
    asset=invoke('content_asset_import_path',{'path':str(fixture.resolve())})
    ui.set_react_input(t,'[aria-label="第 1 步标题"]','下载本周出货计划Excel表格')
    detail='首段完整说明。\n\n**最后一段必须完整显示，不能被两行摘要截断。**\n\n![原图](lumen-asset:'+asset['id']+')'
    ui.set_react_input(t,'[aria-label="第 1 步说明"]',detail)
    assert t.wait_for('document.querySelector(".flow-canvas__body img")?.naturalWidth===640')
    check('卡片无需展开即可显示尾段和完整原图',t.eval("(()=>{const n=document.querySelector('.flow-canvas__node'),img=n.querySelector('img');return n.querySelector('strong')?.textContent.includes('不能被两行摘要截断')&&img.naturalWidth===640&&Math.abs(img.getBoundingClientRect().width/img.getBoundingClientRect().height-640/240)<.01&&n.scrollHeight<=n.clientHeight+1})()"))
    check('原步骤标题完整并未残留单字一行',t.eval("(()=>{const e=document.querySelector('.flow-canvas__title'),n=e.firstChild,lines=new Map();for(let i=0;i<n.textContent.length;i++){const r=document.createRange();r.setStart(n,i);r.setEnd(n,i+1);const b=r.getBoundingClientRect();lines.set(b.top,(lines.get(b.top)||'')+n.textContent[i])}return e.textContent==='下载本周出货计划Excel表格'&&Array.from(lines.values()).every(line=>line.length>1)})()"))
    for i in range(2,6):
        click('添加步骤');ui.set_react_input(t,f'[aria-label="第 {i} 步标题"]',f'顺序步骤 {i}');ui.set_react_input(t,f'[aria-label="第 {i} 步说明"]','完整操作说明。\n\n第二段仍然显示。')
    click('收起详情')
    check('五个节点横向一字排列且间距适当',t.eval("(()=>{const n=Array.from(document.querySelectorAll('.flow-canvas__node'));return n.every((e,i)=>!i||e.offsetLeft-n[i-1].offsetLeft-n[i-1].offsetWidth>=90)&&n.every(e=>e.offsetTop===0)})()"))
    select('[aria-label="流程排列方向"]','vertical')
    check('纵向按实际内容高度留空隙，不折返且无图片重叠',t.wait_for("(()=>{const n=Array.from(document.querySelectorAll('.flow-canvas__node'));return n.every(e=>e.offsetLeft===0)&&n.every((e,i)=>!i||e.offsetTop-n[i-1].offsetTop-n[i-1].offsetHeight>=90)})()"))
    destination='.flow-canvas__nav-step:nth-child(4)';ui.real_click(t,destination)
    check('顶部真实点击定位第四步且不强制展开编辑',t.wait_for('document.querySelectorAll(".flow-canvas__node")[3].getAttribute("aria-pressed")==="true"&&!document.querySelector(".flow-canvas__inspector")'))
    check('顶部导航当前位置高亮',t.eval('document.querySelector(".flow-canvas__nav-step[aria-current=step]").getAttribute("aria-label").includes("第 4 步")'))
    rect=t.eval('document.querySelector(".flow-canvas__nav-step:nth-child(4)").getBoundingClientRect().toJSON()')
    t.call('Input.dispatchMouseEvent',{'type':'mouseMoved','x':rect['x']+rect['width']/2,'y':rect['y']+rect['height']/2})
    check('悬停实际显示节点标题',t.wait_for('getComputedStyle(document.querySelector(".flow-canvas__nav-step:nth-child(4) .flow-canvas__nav-label")).display!=="none"'))
    check('导航定位后的卡片完整位于导航与底部工具栏之间',t.eval("(()=>{const n=document.querySelectorAll('.flow-canvas__node')[3].getBoundingClientRect(),nav=document.querySelector('.flow-canvas__nav').getBoundingClientRect(),b=document.querySelector('.flow-canvas__hint').getBoundingClientRect();return n.top>=nav.bottom&&n.bottom<=b.top})()"))
    screenshot('vertical-navigation.png');select('[aria-label="流程排列方向"]','horizontal');ui.real_click(t,'.flow-canvas__nav-step:first-child');screenshot('full-card.png')
    (a.output_dir/'checks.json').write_text(json.dumps({'checks':checks,'sizes':sizes},ensure_ascii=False,indent=2),encoding='utf-8')
    print(f'PASS: {len(checks)} real native readability checks',flush=True)
except Exception:
    screenshot('failure.png')
    (a.output_dir/'failure-sizes.json').write_text(json.dumps(sizes,ensure_ascii=False,indent=2),encoding='utf-8')
    raise
finally:
    try:
        t.eval('localStorage.'+('removeItem("lumen.fontSize")' if previous_font is None else 'setItem("lumen.fontSize",'+json.dumps(previous_font)+')')+';window.dispatchEvent(new StorageEvent("storage",{key:"lumen.fontSize"}))')
        user32.MoveWindow(windows[0],original_rect.left,original_rect.top,original_rect.right-original_rect.left,original_rect.bottom-original_rect.top,True)
    finally:
        t.close()
