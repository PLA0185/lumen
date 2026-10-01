"""Use real WebView layout to catch wrapping and overlap, without editing user input."""
import argparse
import json
from pathlib import Path

import ui_drive as ui


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', type=int, default=9226)
    parser.add_argument('--css-file', type=Path, help='Preview source CSS; omit to verify installed CSS')
    args = parser.parse_args()
    target = ui.connect(args.port)
    try:
        css = args.css_file.read_text(encoding='utf-8') if args.css_file else target.eval(r'''
          [...document.styleSheets].filter(s => s.ownerNode.id !== 'heading-layout-preview')
            .map(s => [...s.cssRules].map(r => r.cssText).join('\n')).join('\n')
        ''')
        target.eval(r'''(() => {
          const frame = document.createElement('iframe'); frame.id = 'topbar-layout-check';
          frame.style.cssText = 'position:fixed;left:0;top:0;height:650px;opacity:0;pointer-events:none;border:0';
          frame.setAttribute('aria-hidden','true'); document.body.append(frame);
        })()''')
        checked = 0
        for width in [680, 900, 1280, 1600]:
            for font in [12, 14, 16, 20]:
                for title, subtitle in [
                    ('今天', '计划时间落在今天的所有任务'),
                    ('设置', '外观、窗口、提醒、AI 与数据管理'),
                    ('备忘与流程', '随手记录业务要点，把操作步骤整理成随时可查的流程'),
                ]:
                    layout = target.eval(r'''(async () => {
                      const frame = document.querySelector('#topbar-layout-check');
                      frame.style.width = WIDTH + 'px';
                      const doc = frame.contentDocument;
                      doc.open(); doc.write('<!doctype html><html><head></head><body data-window="main"></body></html>'); doc.close();
                      const style = doc.createElement('style'); style.textContent = CSS; doc.head.append(style);
                      doc.documentElement.style.setProperty('--font-size-base', FONT + 'px');
                      const app = doc.createElement('div'); app.className = 'app';
                      const sidebar = doc.createElement('aside'); sidebar.className = 'sidebar';
                      const main = doc.createElement('div'); main.className = 'main';
                      const bar = document.querySelector('.topbar').cloneNode(true);
                      bar.querySelector('.topbar__title').textContent = TITLE;
                      bar.querySelector('.topbar__subtitle').textContent = SUBTITLE;
                      main.append(bar); app.append(sidebar, main); doc.body.append(app);
                      await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
                      const heading = bar.querySelector('.topbar__heading'), actions = bar.querySelector('.topbar__actions');
                      const single = selector => {
                        const el = bar.querySelector(selector);
                        return el.getBoundingClientRect().height < parseFloat(frame.contentWindow.getComputedStyle(el).lineHeight) * 1.2;
                      };
                      const h = heading.getBoundingClientRect(), a = actions.getBoundingClientRect(), b = bar.getBoundingClientRect();
                      return {singleTitle:single('.topbar__title'),singleSubtitle:single('.topbar__subtitle'),
                        noOverlap:h.right <= a.left,actionsInside:a.right <= b.right + 1,
                        toolbarOneRow:frame.contentWindow.getComputedStyle(actions).flexWrap === 'nowrap',
                        headingWidth:h.width};
                    })()'''.replace('WIDTH', str(width)).replace('FONT', str(font))
                        .replace('CSS', json.dumps(css)).replace('SUBTITLE', json.dumps(subtitle)).replace('TITLE', json.dumps(title)))
                    assert all(layout[key] for key in ['singleTitle', 'singleSubtitle', 'noOverlap', 'actionsInside', 'toolbarOneRow']), (width, font, title, layout)
                    assert 0 < layout['headingWidth'] <= 361, (width, font, title, layout)
                    checked += 1
        print(f'PASS: {checked} real Chromium header layouts; title/subtitle stay on one line and do not overlap toolbar.')
    finally:
        target.eval('document.querySelector("#topbar-layout-check")?.remove()')
        target.close()


if __name__ == '__main__':
    main()
