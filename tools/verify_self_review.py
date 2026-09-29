"""自查修复的实机回归，只允许仓库外 memo-test-data 验收库。"""
import argparse
import json
import sqlite3
from pathlib import Path
import ui_drive as ui

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--data-dir', type=Path, required=True)
parser.add_argument('--output-dir', type=Path, required=True)
parser.add_argument('--port', type=int, default=9223)
args = parser.parse_args()
main = ui.connect(args.port, timeout=45)

def invoke(name, payload=None):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke({json.dumps(name)}, {json.dumps(payload or {})})')

assert args.data_dir.name == 'memo-test-data'
assert args.data_dir.resolve() == Path(invoke('app_data_paths')['dataDir']).resolve()
assert not args.data_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
assert not args.output_dir.resolve().is_relative_to(Path(__file__).resolve().parents[1])
args.output_dir.mkdir(parents=True, exist_ok=True)
checks = []

def check(name, result):
    assert result, name
    checks.append(name)
    print('PASS: ' + name, flush=True)

def button(text):
    main.eval(f'Array.from(document.querySelectorAll("button")).find(b=>b.textContent.trim()==={json.dumps(text)}).dataset.auditClick="1"')
    selector = '[data-audit-click]'
    main.eval(f'document.querySelector({json.dumps(selector)}).scrollIntoView({{block:"center"}})')
    ui.real_click(main, selector)
    main.eval('document.querySelectorAll("[data-audit-click]").forEach(b=>delete b.dataset.auditClick)')

def write_backup(name, file):
    path = args.output_dir / (name + '.lumen-backup.json')
    path.write_text(json.dumps(file, ensure_ascii=False), encoding='utf-8')
    return str(path)

def rejected(path):
    return main.eval(f'window.__TAURI_INTERNALS__.invoke("backup_restore", {{path:{json.dumps(path)}}}).then(()=>false,()=>true)')

def snapshot():
    with sqlite3.connect(args.data_dir / 'lumen.db') as db:
        tables = [r[0] for r in db.execute("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")]
        return {table: sorted(db.execute('SELECT * FROM "'+table.replace('"','""')+'"').fetchall(), key=repr) for table in tables}

try:
    button('备忘与流程')
    assert main.wait_for('document.querySelectorAll(".memos__item").length>0')
    main.eval('const s=document.querySelector("[aria-label=\\"备忘分类筛选\\"]");s.value="业务学习";s.dispatchEvent(new Event("change",{bubbles:true}));true')
    ui.set_react_input(main, '[aria-label="搜索备忘与流程"]', '不存在的验收词')
    assert main.wait_for('document.querySelectorAll(".memos__item").length===0')
    check('无搜索结果时分类筛选仍清楚可见', main.eval('document.querySelector("[aria-label=\\"备忘分类筛选\\"]").value') == '业务学习')
    ui.set_react_input(main, '[aria-label="搜索备忘与流程"]', '')
    button('设置'); button('窗口与启动')
    assert main.wait_for('Array.from(document.querySelectorAll("button")).some(b=>b.textContent.trim()==="恢复默认大小")')
    invoke('window_set_floating_size', {'width': 320, 'height': 460})
    button('恢复默认大小')
    assert main.wait_for('document.body.textContent.includes("当前 450 × 600")')
    cfg = invoke('window_get_config')
    check('恢复默认大小与后端统一为450×600', cfg['config']['floatingWidth'] == cfg['defaultFloatingSize']['width'] == 450 and cfg['config']['floatingHeight'] == 600)
    # Fixture writes are confined to the explicit isolated profile; export/preview/restore use real IPC.
    with sqlite3.connect(args.data_dir / 'lumen.db') as db:
        db.execute('PRAGMA foreign_keys=ON')
        for sql in [
            "INSERT INTO projects(id,name,created_at,updated_at) VALUES('audit-project','验收项目','2026-09-29','2026-09-29')",
            "INSERT INTO tags(id,name,created_at,updated_at) VALUES('audit-tag','验收标签','2026-09-29','2026-09-29')",
            "INSERT INTO task_series(id,rrule,tzid,dtstart_local,recurrence_end_kind,recurrence_count,created_at,updated_at) VALUES('audit-series','FREQ=DAILY','Asia/Shanghai','2020-01-01T09:00:00','count',1,'2026-09-29','2026-09-29')",
            "INSERT INTO task_series_template(series_id,title,note_md,project_id) VALUES('audit-series','验收模板','模板说明','audit-project')",
            "INSERT INTO task_series_tags(series_id,tag_id) VALUES('audit-series','audit-tag')",
            "INSERT INTO task_series_skips(id,series_id,occurrence_key,created_at) VALUES('audit-skip','audit-series','2026-10-01T01:00:00.000Z','2026-09-29')",
            "INSERT INTO task_series_rebuilds(series_id,range_start_utc,range_end_utc,requested_at) VALUES('audit-series','2026-09-29','2026-12-29','2026-09-29')",
            "INSERT INTO tasks(id,title,created_at,updated_at) VALUES('audit-task','验收任务','2026-09-29','2026-09-29')",
            "INSERT INTO focus_sessions(id,task_id,state,elapsed_seconds,created_at,updated_at) VALUES('audit-focus','audit-task','finished',1200,'2026-09-29','2026-09-29')",
            "INSERT INTO goals(id,title,target_count,created_at) VALUES('audit-goal','验收目标',10,'2026-09-29')",
        ]:
            db.execute(sql)
    original = snapshot()
    exported = invoke('backup_export', {'path': str(args.output_dir / 'complete.lumen-backup.json')})
    file = json.loads(Path(exported['path']).read_text(encoding='utf-8'))
    check('新版完整备份覆盖六类曾遗漏的数据', file['formatVersion'] == 3 and all(exported['stats'][k] == 1 for k in ['seriesTemplates','seriesTags','seriesSkips','seriesRebuilds','focusSessions','goals']))
    file['stats']['tasks'] = 99999
    preview = invoke('backup_preview', {'path': write_backup('false-stats', file)})
    check('预览统计使用实际内容而非文件声明', preview['stats']['tasks'] == 1 and not preview['blockingIssues'])
    with sqlite3.connect(args.data_dir / 'lumen.db') as db:
        db.execute("UPDATE focus_sessions SET elapsed_seconds=1 WHERE id='audit-focus'")
        db.execute("INSERT INTO goals(id,title,target_count,created_at) VALUES('audit-extra','额外验收目标',1,'2026-09-29')")
    restored = invoke('backup_restore', {'path': exported['path']})
    check('实际恢复逐行还原所有数据并清除额外旧目标', snapshot() == original and restored['imported']['goals'] == 1)
    check('恢复前生成真实一致性快照', Path(restored['safetyBackup']).is_file())
    file['formatVersion'] = 0
    zero = write_backup('zero-version', file)
    check('不经过预览也不能恢复版本0', rejected(zero) and snapshot() == original)
    file['formatVersion'] = 3
    file['checksum'] = '非法校验字符串验收'
    broken = write_backup('unicode-checksum', file)
    preview = invoke('backup_preview', {'path': broken})
    check('非法中文校验和返回阻断提示不崩溃', not preview['checksumOk'] and bool(preview['blockingIssues']))
    check('非法备份恢复被拒绝且原库不变', rejected(broken) and snapshot() == original)
    # Parse/restore old formats without series. Preserve their original field order/checksum contract.
    import hashlib
    legacy = json.loads(Path(exported['path']).read_text(encoding='utf-8'))
    for key in ['seriesTemplates','seriesTags','seriesSkips','seriesRebuilds','focusSessions','goals']:
        legacy['data'].pop(key, None)
    legacy['formatVersion'] = 2
    legacy['checksum'] = hashlib.sha256(json.dumps(legacy['data'], ensure_ascii=False, separators=(',', ':')).encode()).hexdigest()
    old = write_backup('legacy-missing-template', legacy)
    preview = invoke('backup_preview', {'path': old})
    check('旧格式缺少重复模板时明确阻断', preview['checksumOk'] and any('模板' in issue for issue in preview['blockingIssues']) and rejected(old))
    first = invoke('backup_export')
    second = invoke('backup_export')
    check('连续备份路径不冲突且两份都保留', first['path'] != second['path'] and Path(first['path']).is_file() and Path(second['path']).is_file())
    legacy['data']['series'] = []
    legacy['checksum'] = hashlib.sha256(json.dumps(legacy['data'], ensure_ascii=False, separators=(',', ':')).encode()).hexdigest()
    old = write_backup('legacy-without-series', legacy)
    preview = invoke('backup_preview', {'path': old})
    restored = invoke('backup_restore', {'path': old})
    check('没有重复系列的旧格式2备份可真实恢复', not preview['blockingIssues'] and restored['imported']['tasks'] == 1 and restored['imported']['series'] == 0)
    print(json.dumps({'passed': len(checks), 'isolatedProfile': True}), flush=True)
finally:
    main.close()
