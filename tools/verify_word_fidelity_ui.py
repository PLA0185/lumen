"""Real WebView2 import/generation against a loopback model fixture, with independent DOCX XML evidence."""
import argparse
import base64
import hashlib
import json
import os
import re
import threading
import uuid
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import ui_drive as ui

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--port', type=int, required=True)
p.add_argument('--data-dir', type=Path, required=True)
p.add_argument('--evidence-dir', type=Path, required=True)
args = p.parse_args()
t = ui.connect(args.port, timeout=60)


def invoke(command, payload=None):
    return t.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(command)},{json.dumps(payload or {})})')


def click(label):
    probe = 'Array.from(document.querySelectorAll("button")).find(b=>!b.disabled && b.textContent.trim()===' + json.dumps(label) + ')'
    assert t.wait_for(probe), label
    t.eval(probe + '.dataset.wordVerify="1"')
    t.eval('document.querySelector("[data-word-verify]").scrollIntoView({block:"center",behavior:"instant"})')
    ui.real_click(t, '[data-word-verify]')
    t.eval('document.querySelectorAll("[data-word-verify]").forEach(b=>delete b.dataset.wordVerify)')


assert Path(invoke('app_data_paths')['dataDir']).resolve() == args.data_dir.resolve()
assert args.data_dir.resolve() != (Path(os.environ['APPDATA']) / 'com.pla0185.lumen').resolve()
assert not invoke('cloud_sync_status')['config']
assert not invoke('ai_provider_key_status')['custom'], 'Never replace an existing custom-provider key'
expected = json.loads((args.evidence_dir / 'expected-chapters.json').read_text(encoding='utf8'))
original_path = args.evidence_dir / 'original.docx'
requests = []


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(request)
        text = request['messages'][-1]['content'][0]['text']
        manifest = json.loads(text.split('随后提供的图片、文件按以下资源顺序表排列：\n', 1)[1])
        image_ids = [a['id'] for a in manifest if a['mime'].startswith('image/')]
        # A deliberately bad but structurally valid model response must not replace original chapters.
        content = json.dumps({'title': '原文顺序验收', 'bodyMd': '## 待确认问题\n必须补截图。', 'steps': [
            {'title': '乱序：先发货', 'detail': '**完成标准**：自行补充。', 'assetIds': image_ids},
            {'title': '重复拆分', 'assetIds': image_ids[:1]}, {'title': '最后下载表格', 'assetIds': image_ids[:1]},
        ]}, ensure_ascii=False)
        response = json.dumps({'choices': [{'message': {'content': content}, 'finish_reason': 'stop'}]}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(response)))
        self.end_headers()
        self.wfile.write(response)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
previous_config = invoke('ai_get_config')
try:
    config = {'provider': 'custom', 'baseUrl': f'http://127.0.0.1:{server.server_port}/v1', 'model': 'local-word-fidelity-fixture', 'timeoutSeconds': 30, 'maxOutputTokens': 4096, 'hasApiKey': False}
    invoke('ai_set_config', {'config': config, 'apiKey': str(uuid.uuid4())})
    click('流程')
    click('AI 生成流程')
    root = t.call('DOM.getDocument')['root']['nodeId']
    node = t.call('DOM.querySelector', {'nodeId': root, 'selector': '.ai-flow-dialog input[type=file]'})['nodeId']
    t.call('DOM.setFileInputFiles', {'nodeId': node, 'files': [str(original_path.resolve())]})
    assert t.wait_for('document.querySelector(".ai-flow-dialog textarea")?.value.includes("original.docx")')
    before = t.eval('document.querySelector(".ai-flow-dialog textarea").value')
    assert re.fullmatch(r'\s*\[original.docx\]\(lumen-asset:[0-9a-f-]{36}\)\s*', before)
    original_id = re.search(r'lumen-asset:([0-9a-f-]{36})', before)[1]
    assert len(invoke('memo_list', {'query': '', 'deletedOnly': False})) == 0
    click('生成流程草稿')
    assert t.wait_for(f'document.querySelectorAll(".flow-canvas__node").length==={len(expected)}', timeout=60)
    assert t.wait_for('Array.from(document.querySelectorAll(".flow-canvas__node img")).every(i=>i.naturalWidth>0)', timeout=30)
    assert len(requests) == 1
    parts = requests[0]['messages'][-1]['content']
    assert sum(part.get('text', '').count('## ' + expected[0]['title']) for part in parts) == 1
    assert sum(part.get('type') == 'image_url' for part in parts) == sum(len(c['images']) for c in expected)
    assert len(invoke('memo_list', {'query': '', 'deletedOnly': False})) == 0, 'Generation remains an unconfirmed draft'
    click('确认保存流程')
    assert t.wait_for('document.querySelector(".memos")?.textContent.includes("已保存到本机")')
    rows = invoke('memo_list', {'query': '', 'deletedOnly': False})
    assert len(rows) == 1
    saved = invoke('memo_get', {'id': rows[0]['id']})
    assert len(saved['steps']) == len(expected)
    actual_image_ids = []
    with zipfile.ZipFile(original_path) as archive:
        for step, chapter in zip(saved['steps'], expected):
            assert step['title'] == chapter['title']
            for paragraph in chapter['paragraphs']:
                assert paragraph in step['detail'], f'Original paragraph missing: {chapter["title"]}'
            ids = re.findall(r'!\[[^\]\n]*\]\(lumen-asset:([0-9a-f-]{36})\)', step['detail'])
            actual_image_ids.extend(ids)
            assets = [invoke('content_asset_get', {'id': asset_id}) for asset_id in ids]
            assert [a['name'] for a in assets] == chapter['images'], chapter['title']
            for asset in assets:
                assert hashlib.sha256(archive.read('word/media/' + asset['name'])).hexdigest() == asset['sha256']
            assert not any(marker in step['detail'] for marker in ['完成标准', '例外情况', '待确认'])
    assert len(actual_image_ids) == len(set(actual_image_ids)), 'No arbitrary image reuse'
    assert base64.b64decode(invoke('content_asset_get', {'id': original_id})['dataBase64']) == original_path.read_bytes()
    assert '待确认问题' not in saved['bodyMd']
    click('今天')
    click('流程')
    # Open saved item with real mouse events after leaving the view.
    probe = 'Array.from(document.querySelectorAll(".memos__list button")).find(b=>b.textContent.includes("原文顺序验收"))'
    assert t.wait_for(probe)
    t.eval(probe + '.dataset.wordOpen="1"')
    ui.real_click(t, '[data-word-open]')
    assert t.wait_for(f'document.querySelectorAll(".flow-canvas__node").length==={len(expected)}')
    assert invoke('memo_get', {'id': saved['id']}) == saved
    report = {'chapterCount': len(saved['steps']), 'imageCount': len(actual_image_ids), 'arbitraryReuse': False, 'originalUnchanged': True, 'model': 'loopback malicious-output fixture, not real AI', 'checks': 'native import, single send, chapter/paragraph/image order, image bytes, no premature memo save, save/reopen'}
    (args.evidence_dir / 'native-word-fidelity.json').write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf8')
    print(json.dumps(report, ensure_ascii=False))
finally:
    invoke('ai_clear_key', {'provider': 'custom'})
    assert not invoke('ai_provider_key_status')['custom']
    if previous_config:
        invoke('ai_set_config', {'config': previous_config, 'apiKey': None})
    server.shutdown()
    server.server_close()
    t.close()
