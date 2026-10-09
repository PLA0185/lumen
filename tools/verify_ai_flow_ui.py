"""AI 流程原生验收：本机 HTTP 故障服务；仅使用空白隔离 profile，不代表真实模型验收。"""
import argparse
import base64
import json
import os
import sys
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import ui_drive as ui
sys.stdout.reconfigure(encoding='utf-8')

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--port', type=int, default=9226)
p.add_argument('--data-dir', type=Path, required=True)
p.add_argument('--image', type=Path, required=True)
args = p.parse_args()
main = ui.connect(args.port, timeout=60)

def invoke(command, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(command)}, {json.dumps(payload or {})})')

paths = invoke('app_data_paths')
assert args.data_dir.resolve() == Path(paths['dataDir']).resolve()
assert args.data_dir.name == 'flow-ai-test-data'
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert args.data_dir.resolve() != (Path(os.environ['APPDATA']) / 'com.pla0185.lumen').resolve()
assert not invoke('memo_list', {'query': '', 'deletedOnly': False})
assert not invoke('cloud_sync_status')['config'], 'Test profile must not connect to user cloud'
assert not invoke('ai_provider_key_status')['custom'], 'Never overwrite an existing custom API credential'

requests = []
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass
    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(request)
        user_parts = request['messages'][-1]['content']
        resource_text = next(part['text'] for part in user_parts if part.get('type') == 'text')
        manifest = json.loads(resource_text.split('随后提供的图片、文件按以下资源顺序表排列：\n', 1)[1])
        content = '不是 JSON' if len(requests) == 1 else json.dumps({
            'title': '隔离验收：订单处理', 'category': '验收', 'bodyMd': '负责人未提供时留空。',
            'steps': [{'title': '核对订单', 'owner': '销售', 'detail': '核对型号与数量。', 'assetIds': [manifest[0]['id']]},
                      {'title': '通知仓库', 'owner': '', 'detail': '发送确认后的订单。'}],
        }, ensure_ascii=False)
        body = json.dumps({'choices': [{'message': {'content': content}, 'finish_reason': 'stop'}]}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
config = {'provider': 'custom', 'baseUrl': f'http://127.0.0.1:{server.server_port}/v1', 'model': 'local-http-fixture', 'timeoutSeconds': 15, 'maxOutputTokens': 2048, 'hasApiKey': False}
original_config = invoke('ai_get_config')
checks = 0
def check(label, condition):
    global checks
    assert condition, label
    checks += 1
    print('PASS: ' + label, flush=True)

def button(label):
    probe = 'Array.from(document.querySelectorAll("button")).find(b=>!b.disabled && b.textContent.trim()===' + json.dumps(label) + ')'
    assert main.wait_for('(' + probe + ')?.disabled === false'), label
    main.eval(probe + '.dataset.flowVerify="1"')
    assert main.wait_for('Array.from(document.querySelectorAll("dialog[open] img")).every(i=>i.complete && i.naturalWidth>0)')
    assert main.wait_for('!document.querySelector("dialog[open]")?.textContent.includes("正在读取本地内容")')
    main.eval('document.querySelector("[data-flow-verify]").scrollIntoView({block:"center",behavior:"instant"})')
    main.eval('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))')
    assert main.wait_for('(()=>{const b=document.querySelector("[data-flow-verify]"); const r=b.getBoundingClientRect(); return b.contains(document.elementFromPoint(r.left+r.width/2,r.top+r.height/2))})()'), 'Button must be visible and receive the click'
    ui.real_click(main, '[data-flow-verify]')
    main.eval('document.querySelectorAll("[data-flow-verify]").forEach(b=>delete b.dataset.flowVerify)')

try:
    # Temporary generated test credential, only after asserting the entry was absent.
    assert invoke('ai_set_config', {'config': config, 'apiKey': str(uuid.uuid4())})['hasApiKey']
    assert invoke('ai_provider_key_status')['custom']
    private = invoke('memo_save', {'input': {'id': None, 'expectedRevision': None, 'kind': 'memo', 'title': '未选择的私人记录', 'category': '', 'bodyMd': '不应发送给生成请求的私人正文', 'steps': []}})
    asset = invoke('content_asset_import_path', {'path': str(args.image.resolve())})
    source = '张三：先核对订单，再通知仓库。\n![聊天截图](lumen-asset:' + asset['id'] + ')'
    button('流程')
    button('AI 生成流程')
    check('原生材料对话框实际打开', main.wait_for('document.querySelector("dialog[open] [aria-label=流程原始材料]")'))
    ui.set_react_input(main, '[aria-label="流程原始材料"]', source)
    assert main.wait_for('document.querySelector("dialog[open] img")?.complete && document.querySelector("dialog[open] img")?.naturalWidth>0')
    button('生成流程草稿')
    if not main.wait_for('document.querySelector("dialog[open] [role=alert]")?.textContent.includes("JSON")', timeout=20):
        print(json.dumps({'requestCount': len(requests), 'alert': main.eval('document.querySelector("dialog[open] [role=alert]")?.textContent'), 'keyExists': invoke('ai_provider_key_status')['custom']}, ensure_ascii=False), flush=True)
        raise AssertionError('Invalid JSON response must be reported')
    check('错误模型输出明确提示并保留材料', main.eval('document.querySelector("[aria-label=流程原始材料]").value') == source)
    check('生成失败没有保存流程', len(invoke('memo_list', {'query': '', 'deletedOnly': False})) == 1)
    button('生成流程草稿')
    assert main.wait_for('!document.querySelector("dialog[open]") && document.querySelector("[aria-label=备忘标题]")?.value.includes("隔离验收")')
    time.sleep(2)
    check('生成成功也不会自动保存', len(invoke('memo_list', {'query': '', 'deletedOnly': False})) == 1)
    check('生成后默认显示顺序画布', main.eval('!!document.querySelector(`[aria-label="流程画布"]`)'))
    button('返回列表')
    check('负责人未提供时保持空白', main.eval('document.querySelector(`[aria-label="第 2 步负责人"]`).value') == '')
    check('模型指定原图实际出现在对应步骤', main.wait_for('document.querySelector(".memos__step-fields img")?.naturalWidth>0') and asset['id'] in main.eval('document.querySelector(`[aria-label="第 1 步说明"]`).value'))
    picker = '[aria-label="第 2 步关联原图"]'
    main.eval('(()=>{const e=document.querySelector('+json.dumps(picker)+');Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype,"value").set.call(e,'+json.dumps(asset['id'])+');e.dispatchEvent(new Event("change",{bubbles:true}))})()')
    check('已有原图补选可预览', main.wait_for('document.querySelectorAll(".flow-image-picker img").length===1 && document.querySelector(".flow-image-picker img")?.naturalWidth>0'))
    button('放到此步骤')
    check('补选仅改变对应步骤且不重新请求 AI', asset['id'] in main.eval('document.querySelector(`[aria-label="第 2 步说明"]`).value') and len(requests)==2)
    ui.set_react_input(main, '[aria-label="第 2 步负责人"]', '仓库')
    time.sleep(2)
    check('修改预览仍不自动保存', len(invoke('memo_list', {'query': '', 'deletedOnly': False})) == 1)
    button('确认保存流程')
    assert main.wait_for('Array.from(document.querySelectorAll("[role=status]")).some(e=>e.textContent.includes("已保存到本机"))')
    rows = invoke('memo_list', {'query': '', 'deletedOnly': False})
    flow = invoke('memo_get', {'id': next(r['id'] for r in rows if r['kind'] == 'flow')})
    check('确认后新建流程，原材料及手动修改保留', len(rows) == 2 and flow['bodyMd'].endswith(source) and flow['steps'][1]['owner'] == '仓库')
    check('确认保存后两步原图引用均保留', all('![' in step['detail'] and 'lumen-asset:'+asset['id'] in step['detail'] for step in flow['steps']))
    check('已有私人记录没有覆盖', invoke('memo_get', {'id': private['id']}) == private)
    check('真实 HTTP 仅发送选中的文字和图片字节', len(requests) == 2 and all('不应发送给生成请求的私人正文' not in json.dumps(r, ensure_ascii=False) for r in requests))
    parts = requests[-1]['messages'][-1]['content']
    image = next(part['image_url']['url'] for part in parts if part.get('type') == 'image_url')
    check('图片请求保留真实原始 PNG 内容', base64.b64decode(image.split(',', 1)[1]) == args.image.read_bytes())
    print(f'{checks} native checks passed; HTTP fixture, not a real AI model.', flush=True)
finally:
    invoke('ai_clear_key', {'provider': 'custom'})
    assert not invoke('ai_provider_key_status')['custom']
    if original_config:
        invoke('ai_set_config', {'config': original_config, 'apiKey': None})
    server.shutdown()
    server.server_close()
    main.close()
