"""AI 状态/模型选择与顶部同步验收，仅使用明确的隔离 profile 和本机模型夹具。"""
import argparse, base64, json, sqlite3, threading, uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import ui_drive as ui
p=argparse.ArgumentParser();p.add_argument('--port',type=int,required=True);p.add_argument('--data-dir',type=Path,required=True);p.add_argument('--output-dir',type=Path,required=True);args=p.parse_args()
t=ui.connect(args.port)
def invoke(name,args=None):return t.eval('window.__TAURI_INTERNALS__.invoke('+json.dumps(name)+','+json.dumps(args or {})+')')
paths=invoke('app_data_paths');profile=args.data_dir.resolve();repo=Path(__file__).resolve().parents[1]
assert profile.name=='ai-sync-test-data' and profile==Path(paths['dataDir']).resolve()
assert not profile.is_relative_to(repo) and not args.output_dir.resolve().is_relative_to(repo)
assert invoke('cloud_sync_status')['config'] is None
assert invoke('ai_get_config') is None
assert not invoke('ai_provider_key_status')['custom'], 'Do not overwrite an existing custom-provider key'
args.output_dir.mkdir(parents=True,exist_ok=True);checks=[]
def check(label,value):assert value,label;checks.append(label);print('PASS: '+label,flush=True)
def button(label,scope='document'):
    q=f'Array.from({scope}.querySelectorAll("button")).find(b=>!b.disabled && b.textContent.trim()==={json.dumps(label)})'
    assert t.wait_for(q),label
    t.eval(f'{q}.dataset.verifyAiSync="1"');t.eval('document.querySelector("[data-verify-ai-sync]").scrollIntoView({block:"center",behavior:"instant"})')
    ui.real_click(t,'[data-verify-ai-sync]');t.eval('document.querySelectorAll("[data-verify-ai-sync]").forEach(e=>delete e.dataset.verifyAiSync)')
def select(selector,value):
    t.eval('(()=>{const e=document.querySelector('+json.dumps(selector)+');if(!e)throw Error("Missing select");Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,"value").set.call(e,'+json.dumps(value)+');e.dispatchEvent(new Event("change",{bubbles:true}))})()')
seen=[]
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*a):pass
    def respond(self,body):
        data=json.dumps(body).encode();self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(data)));self.end_headers();self.wfile.write(data)
    def do_GET(self):self.respond({'data':[{'id':'native-model-a'},{'id':'native-model-b'}]})
    def do_POST(self):
        seen.append(json.loads(self.rfile.read(int(self.headers['Content-Length']))));self.respond({'choices':[{'message':{'content':'可用'},'finish_reason':'stop'}]})
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
try:
    button('AI 助手','document.querySelector(".topbar")')
    check('初始缺少配置显示原因',t.wait_for('document.querySelector(".ai-quick-dialog").textContent.includes("尚未配置 AI 密钥")'))
    button('关闭助手');button('设置','document.querySelector(".sidebar")');button('AI','document.querySelector(".settings__tabs")' if t.eval('!!document.querySelector(".settings__tabs")') else 'document.querySelector(".content")')
    assert t.wait_for('document.querySelector(".content select")')
    select('.content .formgrid select','custom')
    ui.set_react_input(t,'.content input[list="ai-models"]','native-model-a')
    ui.set_react_input(t,'.content input[placeholder="https://…"]',f'http://127.0.0.1:{server.server_port}/v1')
    ui.set_react_input(t,'.content input[type=password]','local-native-model-fixture')
    button('保存配置','document.querySelector(".content")')
    check('保存通过真实 IPC 和凭据存储',t.wait_for('document.querySelector(".content .chip--ok")?.textContent.includes("已配置")') or invoke('ai_get_config')['hasApiKey'])
    button('AI 助手','document.querySelector(".topbar")')
    check('保留的助手自动更新配置，无需重新打开或填写密钥',t.wait_for('document.querySelector(".ai-quick-dialog .ai-model-picker")?.textContent.includes("native-model-a")') and not t.eval('document.querySelector(".ai-quick-dialog").textContent.includes("尚未配置 AI 密钥")'))
    button('选择模型','document.querySelector(".ai-quick-dialog")')
    check('实际服务返回的模型可选',t.wait_for('document.querySelector(".ai-quick-dialog [aria-label=助手使用的模型]")?.options.length===3'))
    select('.ai-quick-dialog [aria-label="助手使用的模型"]','native-model-b')
    check('选中的模型真实保存',t.wait_for('document.querySelector(".ai-quick-dialog .ai-model-picker span")?.textContent.includes("native-model-b")') and invoke('ai_get_config')['model']=='native-model-b')
    invoke('ai_test_connection',{'config':invoke('ai_get_config')})
    check('请求采用选择后的模型',seen[-1]['model']=='native-model-b')
    button('关闭助手')
    connection=str(uuid.uuid4());space=str(uuid.uuid4())
    cfg={'server':'https://example.invalid','account':'isolated-test','folder':'Lumen','connectionId':connection,'workspaceId':space,'enabled':False,'inheritAll':True,'defaultSync':{'scope':'memos','direction':'download'}}
    assert Path(paths['dbPath']).resolve().parent == profile
    with sqlite3.connect(paths['dbPath']) as db:
        db.execute("INSERT INTO settings(key,value_json,updated_at) VALUES('memo_cloud_sync',?,datetime('now')) ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json",(json.dumps(cfg),))
    button('云同步','document.querySelector(".content")')
    check('设置显示当前默认类型',t.wait_for('document.querySelector("[aria-label=默认同步内容]")?.value==="memos"'))
    select('[aria-label="默认同步内容"]','tasks');select('[aria-label="默认同步方向"]','upload');button('保存默认同步类型','document.querySelector(".content")')
    check('默认类型通过真实 IPC 保存且连接与暂停状态保留',t.wait_for('document.querySelector(".content").textContent.includes("默认同步类型已保存")') and invoke('cloud_sync_status')['config']['defaultSync']=={'scope':'tasks','direction':'upload'} and invoke('cloud_sync_status')['config']['connectionId']==connection and not invoke('cloud_sync_status')['config']['enabled'])
    for page in ['今天','备忘与流程','AI 助手','设置']:
        button(page,'document.querySelector(".sidebar")');button('同步','document.querySelector(".topbar")')
        check(page+'顶部同步按钮实际可点且默认类型一致',t.wait_for('document.querySelector(".sync-dialog[open] [aria-label=本次同步内容]")?.value==="tasks"'))
        if page=='今天':
            select('.sync-dialog [aria-label="本次同步方向"]','download');button('开始同步','document.querySelector(".sync-dialog")')
            check('暂停时手动操作真实报缺少测试凭据，不伪报同步完成',t.wait_for('document.querySelector(".sync-dialog [role=alert]")?.textContent.includes("本机缺少")') and not t.eval('document.querySelector(".sync-dialog").textContent.includes("本次同步已执行")'))
            check('本次选择不覆盖默认设置',invoke('cloud_sync_status')['config']['defaultSync']=={'scope':'tasks','direction':'upload'})
        button('关闭','document.querySelector(".sync-dialog")')
    t.call('Page.reload');assert t.wait_for('document.querySelector(".app")')
    button('同步','document.querySelector(".topbar")')
    check('重载后默认类型仍保留',t.wait_for('document.querySelector(".sync-dialog[open] [aria-label=本次同步方向]")?.value==="upload"'))
    screenshot=t.call('Page.captureScreenshot',{'format':'png'})['data'];(args.output_dir/'sync.png').write_bytes(base64.b64decode(screenshot))
    (args.output_dir/'checks.json').write_text(json.dumps(checks,ensure_ascii=False,indent=2),encoding='utf-8')
finally:
    invoke('ai_clear_key',{'provider':'custom'});server.shutdown();t.close()
print(f'PASS: {len(checks)} native checks; local HTTP model fixture, no real model or cloud credentials used.')
