"""Single isolated WebView2: source-only hierarchy, preview/cancel/save and original images.

The loopback fixture makes structural choices on synthetic material only. It is
not a real-model evaluation. Refuses the user's profile and existing custom key.
"""
import argparse
import base64
import json
import re
import threading
import time
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import ui_drive as ui

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--port', type=int, required=True)
p.add_argument('--data-dir', type=Path, required=True)
p.add_argument('--output-dir', type=Path, required=True)
a = p.parse_args()
t = ui.connect(a.port, timeout=60)


def invoke(name, payload=None):
    return t.eval('window.__TAURI_INTERNALS__.invoke(' + json.dumps(name) + ',' + json.dumps(payload or {}) + ')')


def click(label, scope='document'):
    probe = 'Array.from(' + scope + '.querySelectorAll("button")).find(b=>!b.disabled&&b.getBoundingClientRect().width>0&&(b.textContent.trim()===' + json.dumps(label) + '||b.querySelector("strong")?.textContent===' + json.dumps(label) + '))'
    assert t.wait_for(probe), label
    t.eval(probe + '.dataset.hierarchyCheck="1"')
    t.eval('document.querySelector("[data-hierarchy-check]").scrollIntoView({block:"center",behavior:"instant"})')
    ui.real_click(t, '[data-hierarchy-check]')
    t.eval('document.querySelectorAll("[data-hierarchy-check]").forEach(b=>delete b.dataset.hierarchyCheck)')


def check(label, condition):
    assert condition, label
    checks.append(label)
    print('PASS: ' + label, flush=True)


assert a.data_dir.resolve() != (Path.home() / 'AppData/Roaming/com.pla0185.lumen').resolve()
assert not a.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert Path(invoke('app_data_paths')['dataDir']).resolve() == a.data_dir.resolve()
assert not invoke('cloud_sync_status')['config']
assert not invoke('ai_provider_key_status')['custom']
requests = []
checks = []


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        requests.append(request)
        parts = request['messages'][-1]['content']
        text = next(part['text'] for part in parts if part.get('type') == 'text')
        blocks = json.loads(text.split('编号原文块（只能引用这些块进行分层）：\n', 1)[1].split('\n\n随后提供', 1)[0])
        if len(requests) == 1:
            content = {'schemaVersion': 2, 'steps': [{'begin': 1, 'end': len(blocks), 'detail': '模型新增完成标准和补图要求'}]}
        else:
            placements = []
            begin = 1
            chapter = None
            title = None
            for block in blocks:
                heading = re.match(r'^(#{1,6})\s+(.+)', block['text'].strip())
                if heading:
                    if re.match(r'^\d+[.、．]\s*[^\d\s]', heading[2]):
                        chapter = block['id']
                    else:
                        title = block['id']
                else:
                    placements.append({'begin': begin, 'end': block['id'], 'titleBlock': title, 'groupPath': [chapter] if chapter else []})
                    begin = block['id'] + 1
                    title = None
            assert begin == len(blocks) + 1, 'Fixture includes all blocks'
            content = {'schemaVersion': 2, 'steps': placements}
        response = json.dumps({'choices': [{'message': {'content': json.dumps(content, ensure_ascii=False)}, 'finish_reason': 'stop'}]}).encode()
        self.send_response(200)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(response)))
        self.end_headers()
        self.wfile.write(response)


server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
previous = invoke('ai_get_config')
try:
    config = {'provider': 'custom', 'baseUrl': f'http://127.0.0.1:{server.server_port}/v1', 'model': 'synthetic-hierarchy-fixture', 'timeoutSeconds': 30, 'maxOutputTokens': 4096, 'hasApiKey': False}
    invoke('ai_set_config', {'config': config, 'apiKey': str(uuid.uuid4())})
    assets = []
    for color in ['red', 'blue', 'green', 'orange']:
        encoded = t.eval('(()=>{const c=document.createElement("canvas");c.width=240;c.height=100;const x=c.getContext("2d");x.fillStyle=' + json.dumps(color) + ';x.fillRect(0,0,240,100);return c.toDataURL("image/png").split(",")[1]})()')
        assets.append(invoke('content_asset_import', {'name': color + '.png', 'dataBase64': encoded}))
    details = [f'### {i+1}.1 操作甲\n\n点击甲入口。\n\n![原图甲](lumen-asset:{assets[i*2]["id"]})\n图{i*2+1} 甲入口\n### {i+1}.2 操作乙\n\n点击乙入口。\n\n![原图乙](lumen-asset:{assets[i*2+1]["id"]})\n图{i*2+2} 乙入口' for i in range(2)]
    original = invoke('memo_save', {'input': {'id': None, 'expectedRevision': None, 'title': '原文分层验收-' + str(uuid.uuid4())[:8], 'category': '原分类', 'kind': 'flow', 'bodyMd': '## 原始材料\n\n' + '\n\n'.join(details), 'steps': [{'id': str(uuid.uuid4()), 'title': f'{i+1}. 原章节' + ('甲' if i == 0 else '乙'), 'owner': '', 'detail': details[i]} for i in range(2)]}})
    click('备忘与流程', 'document.querySelector(".sidebar")')
    click('刷新列表')
    click(original['title'])
    click('细分流程')
    click('生成细分预览')
    check('拒绝模型添写并保留原流程', t.wait_for('document.querySelector(".ai-flow-dialog [role=alert]")?.textContent.includes("不能新增正文")') and invoke('memo_get', {'id': original['id']}) == original)
    click('生成细分预览')
    check('预览按章节显示四个小步骤', t.wait_for('document.querySelectorAll(".flow-restructure-dialog .flow-canvas__node").length===4&&document.querySelectorAll(".flow-restructure-dialog .flow-canvas__group").length===2'))
    check('图片在预览中实际载入', t.wait_for('Array.from(document.querySelectorAll(".flow-restructure-dialog img")).length===4&&Array.from(document.querySelectorAll(".flow-restructure-dialog img")).every(i=>i.naturalWidth>0)'))
    check('预览确认按钮可见且未被画布覆盖', t.eval('(()=>{const b=Array.from(document.querySelectorAll(".flow-restructure-dialog button")).find(b=>b.textContent==="确认保存细分");const r=b.getBoundingClientRect();return document.elementFromPoint(r.left+r.width/2,r.top+r.height/2)===b})()'))
    time.sleep(1.2)
    check('预览不提前写库', invoke('memo_get', {'id': original['id']}) == original)
    click('取消细分')
    check('取消保持原章节卡片', t.wait_for('!document.querySelector(".flow-restructure-dialog")&&document.querySelectorAll(".flow-canvas__node").length===2'))
    click('细分流程')
    click('生成细分预览')
    assert t.wait_for('!!document.querySelector(".flow-restructure-dialog")')
    click('确认保存细分')
    check('确认保存真实新版本', t.wait_for('!document.querySelector(".flow-restructure-dialog")&&document.querySelectorAll(".flow-canvas__node").length===4'))
    saved = invoke('memo_get', {'id': original['id']})
    check('标题分类原始材料保留', all(saved[k] == original[k] for k in ['id', 'title', 'category', 'bodyMd']) and saved['revision'] == original['revision'] + 1)
    for i, step in enumerate(saved['steps']):
        ids = re.findall(r'lumen-asset:([0-9a-f-]{36})', step['detail'])
        check(f'第{i+1}步原图和图注不串位', ids == [assets[i]['id']] and f'图{i+1} ' in step['detail'] and step['group']['title'] == original['steps'][i//2]['title'])
    check('原图字节和摘要未改变', all(invoke('content_asset_get', {'id': asset['id']}) == asset for asset in assets))
    history = invoke('cloud_sync_history', {'id': original['id']})
    check('历史保留旧章节和细分新版本', sorted(len(v['document']['steps']) for v in history['versions']) == [2, 4])
    click('今天', 'document.querySelector(".sidebar")')
    click('备忘与流程', 'document.querySelector(".sidebar")')
    click(original['title'])
    check('重新打开保留章节分组和四步', t.wait_for('document.querySelectorAll(".flow-canvas__node").length===4&&document.querySelectorAll(".flow-canvas__group").length===2') and invoke('memo_get', {'id': original['id']}) == saved)
    check('图片后原图注按原文换行在下方显示', t.eval('Array.from(document.querySelectorAll(".flow-canvas__body")).every(b=>{const image=b.querySelector("img"),br=b.querySelector(".content-asset")?.nextSibling;return image&&br?.nodeName==="BR"})'))
    plain = '打开货件页面。\n核对数量。\n保存货件。'
    draft = invoke('ai_generate_flow', {'config': config, 'input': {'text': plain, 'assetIds': []}})
    check('普通无标题无空行材料也能细分且不添写', [s['detail'] for s in draft['steps']] == plain.splitlines())
    a.output_dir.mkdir(parents=True, exist_ok=True)
    screenshot = t.call('Page.captureScreenshot')['data']
    (a.output_dir / 'native-hierarchy.png').write_bytes(base64.b64decode(screenshot))
    (a.output_dir / 'native-hierarchy.json').write_text(json.dumps({'checks': checks, 'requestCount': len(requests), 'flowId': saved['id'], 'model': 'synthetic loopback fixture, not real AI'}, ensure_ascii=False, indent=2), encoding='utf8')
finally:
    invoke('ai_clear_key', {'provider': 'custom'})
    if previous:
        invoke('ai_set_config', {'config': previous, 'apiKey': None})
    server.shutdown()
    server.server_close()
    t.close()
