"""Synthetic local document/OCR fixtures in an isolated profile; no user files or real AI."""
import argparse
import json
import os
import re
import sys
import threading
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import ui_drive as ui

sys.stdout.reconfigure(encoding='utf-8')
p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--port', type=int, default=9227)
p.add_argument('--data-dir', type=Path, required=True)
p.add_argument('--fixtures', type=Path, required=True)
args = p.parse_args()
t = ui.connect(args.port, timeout=60)


def invoke(command, payload=None):
    return t.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(command)},{json.dumps(payload or {})})')


paths = invoke('app_data_paths')
assert Path(paths['dataDir']).resolve() == args.data_dir.resolve()
assert args.data_dir.name == 'flow-ai-test-data'
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert args.data_dir.resolve() != (Path(os.environ['APPDATA']) / 'com.pla0185.lumen').resolve()
assert not invoke('cloud_sync_status')['config']
assert not invoke('ai_provider_key_status')['custom']


def click(label, area='', aria_label=None):
    match = 'b.getAttribute("aria-label")===' + json.dumps(aria_label) if aria_label else 'b.textContent.trim()===' + json.dumps(label)
    probe = 'Array.from(document.querySelectorAll(' + json.dumps(area + ' button') + ')).find(b=>!b.disabled && ' + match + ')'
    assert t.wait_for(probe), label
    t.eval(probe + '.dataset.fileVerify="1"')
    t.eval('document.querySelector("[data-file-verify]").scrollIntoView({block:"center",behavior:"instant"})')
    t.eval('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))')
    ui.real_click(t, '[data-file-verify]')
    t.eval('document.querySelectorAll("[data-file-verify]").forEach(b=>delete b.dataset.fileVerify)')


def import_file(area, filename, expected, recognize=True):
    selector = area + ' input[type=file]'
    before = t.eval('document.querySelector(' + json.dumps(area + ' textarea') + ').value')
    t.eval('(()=>{const f=document.querySelector(' + json.dumps(area + ' textarea') + ');f.focus();f.setSelectionRange(f.value.length,f.value.length)})()')
    root = t.call('DOM.getDocument')['root']['nodeId']
    node = t.call('DOM.querySelector', {'nodeId': root, 'selector': selector})['nodeId']
    assert node
    t.call('DOM.setFileInputFiles', {'nodeId': node, 'files': [str((args.fixtures / filename).resolve())]})
    assert t.wait_for('(()=>{const f=document.querySelector(' + json.dumps(area + ' textarea') + ');return f.getAttribute("aria-busy")==="false" && f.value!==' + json.dumps(before) + ' && f.value.includes(' + json.dumps(filename) + ')})()', timeout=30), filename
    imported = t.eval('document.querySelector(' + json.dumps(area + ' textarea') + ').value')
    assert imported.startswith(before), filename
    assert re.fullmatch(r'\n!?\[' + re.escape(filename) + r'\]\(lumen-asset:[0-9a-f-]{36}\)\n', imported[len(before):]), 'Import must append only the original reference: ' + filename
    assert expected not in imported[len(before):], 'Import unexpectedly parsed ' + filename
    assert t.wait_for('Array.from(document.querySelectorAll(' + json.dumps(area + ' .content-asset') + ')).some(card=>card.textContent.includes(' + json.dumps(filename) + '))')
    if filename.endswith('.png'):
        assert t.wait_for('Array.from(document.querySelectorAll(' + json.dumps(area + ' .content-asset img') + ')).some(img=>img.alt===' + json.dumps(filename) + ' && img.naturalWidth>0)')
    print('PASS: native file input adds only original ' + filename + ' with a visible resource card.', flush=True)
    if recognize:
        click('识别内容', area, '识别内容：' + filename)
        assert t.wait_for('(()=>{const f=document.querySelector(' + json.dumps(area + ' textarea') + ');return f.getAttribute("aria-busy")==="false" && f.value.includes(' + json.dumps(expected) + ')})()', timeout=30), filename
        recognized = t.eval('document.querySelector(' + json.dumps(area + ' textarea') + ').value')
        assert recognized.startswith(imported), 'Manual recognition must preserve the original input: ' + filename
        print('PASS: real mouse click explicitly recognizes ' + filename + ' and retains original reference.', flush=True)


click('流程')
click('新建流程')
assert t.eval('!!document.querySelector(`[aria-label="流程画布"]`)')
click('返回列表')
ui.set_react_input(t, '[aria-label="备忘标题"]', '隔离验收：普通文件导入')
area = '.memos__editor .content-editor'
for name, text in [('orders.docx', '先核对订单'), ('orders.xlsx', 'ABC\t\t12'), ('orders.pdf', 'Ship order ABC'), ('printed.png', 'ORDER')]:
    import_file(area, name, text)
body = t.eval('document.querySelector(`[aria-label="备忘内容"]`).value')
assert all(name in body for name in ['orders.docx', 'orders.xlsx', 'orders.pdf', 'printed.png'])
assert '![' in body
click('保存并查看')
rows = invoke('memo_list', {'query': '', 'deletedOnly': False})
saved = next(r for r in rows if r['title'] == '隔离验收：普通文件导入')
assert invoke('memo_get', {'id': saved['id']})['bodyMd'] == body
print('PASS: ordinary flow persists extracted text, original files and embedded image references.', flush=True)

requests = []


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(request)
        text = request['messages'][-1]['content'][0]['text']
        manifest = json.loads(text.split('随后提供的图片、文件按以下资源顺序表排列：\n', 1)[1])
        content = json.dumps({'title': '隔离验收：文件生成流程', 'bodyMd': '核对后保存。', 'steps': [{'title': '核对订单', 'detail': '核对数量。', 'assetIds': [a['id'] for a in manifest if a['mime'].startswith('image/')]}]}, ensure_ascii=False)
        response = json.dumps({'choices': [{'message': {'content': content}, 'finish_reason': 'stop'}]}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(response)))
        self.end_headers()
        self.wfile.write(response)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
original = invoke('ai_get_config')
try:
    config = {'provider': 'custom', 'baseUrl': f'http://127.0.0.1:{server.server_port}/v1', 'model': 'local-http-file-fixture', 'timeoutSeconds': 15, 'maxOutputTokens': 2048, 'hasApiKey': False}
    invoke('ai_set_config', {'config': config, 'apiKey': str(uuid.uuid4())})
    click('AI 生成流程')
    import_file('.ai-flow-dialog .content-editor', 'orders.docx', '先核对订单', recognize=False)
    click('生成流程草稿')
    assert t.wait_for('document.querySelector(".memos__step-fields img")?.naturalWidth>0', timeout=30)
    click('流程信息')
    assert t.wait_for('document.querySelector(`[aria-label="备忘标题"]`)?.value==="隔离验收：文件生成流程"', timeout=30)
    ui.real_click(t, '.flow-canvas__node .flow-canvas__title')
    assert t.wait_for('document.querySelector(".memos__step-fields img")?.naturalWidth>0')
    parts = requests[0]['messages'][-1]['content']
    assert len(requests) == 1
    assert sum(p.get('text', '').count('先核对订单') for p in parts) == 1, 'Extracted document text must be sent once'
    assert any(p.get('type') == 'image_url' for p in parts)
    assert not any(p.get('type') == 'file' for p in parts)
    assert len(invoke('memo_list', {'query': '', 'deletedOnly': False})) == len(rows)
    print('PASS: local Word text conversion and embedded image reach real HTTP fixture; generated step displays image and remains unconfirmed.', flush=True)
    click('确认保存流程')
    assert t.wait_for('document.querySelector(".memos")?.textContent.includes("已保存到本机")')
    print('Native file-import and generated-image checks passed; generated fixtures and local HTTP, not a real model or user document.', flush=True)
finally:
    invoke('ai_clear_key', {'provider': 'custom'})
    assert not invoke('ai_provider_key_status')['custom']
    if original:
        invoke('ai_set_config', {'config': original, 'apiKey': None})
    server.shutdown()
    server.server_close()
    t.close()
