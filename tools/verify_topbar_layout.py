"""Use real WebView layout to catch wrapping and overlap, without editing user input."""
import argparse
import base64
import json
from pathlib import Path

import ui_drive as ui


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--port', type=int, default=9226)
    parser.add_argument('--css-file', type=Path, help='Preview source CSS; omit to verify installed CSS')
    parser.add_argument('--screenshot', type=Path, help='Save the live WebView screenshot for visual review')
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
                    ('备忘', '记录业务要点、术语和常用资料，随时查阅完整内容'),
                ]:
                    layout = target.eval(r'''(async () => {
                      const frame = document.querySelector('#topbar-layout-check');
                      frame.style.width = WIDTH + 'px';
                      const doc = frame.contentDocument;
                      doc.open(); doc.write('<!doctype html><html><head></head><body data-window="main"></body></html>'); doc.close();
                      const style = doc.createElement('style'); style.textContent = CSS; doc.head.append(style);
                      doc.documentElement.style.setProperty('--font-size-base', FONT + 'px');
                      const app = doc.createElement('div'); app.className = 'app app--windows';
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
                        noOverlap:h.right <= a.left + 1 || h.bottom <= a.top + 1,
                        actionsInside:a.left >= b.left - 1 && a.right <= b.right + 1 && a.bottom <= b.bottom + 1,
                        toolbarWraps:frame.contentWindow.getComputedStyle(actions).flexWrap === 'wrap',
                        toolbarNotClipped:actions.scrollWidth <= actions.clientWidth + 1,
                        headingInside:h.left >= b.left - 1 && h.right <= b.right + 1,
                        headingWidth:h.width};
                    })()'''.replace('WIDTH', str(width)).replace('FONT', str(font))
                        .replace('CSS', json.dumps(css)).replace('SUBTITLE', json.dumps(subtitle)).replace('TITLE', json.dumps(title)))
                    assert all(layout[key] for key in ['singleTitle', 'singleSubtitle', 'noOverlap', 'actionsInside', 'toolbarWraps', 'toolbarNotClipped', 'headingInside']), (width, font, title, layout)
                    assert layout['headingWidth'] > 0, (width, font, title, layout)
                    checked += 1
        print(f'PASS: {checked} real Chromium header layouts; title/subtitle stay on one line, toolbar wraps cleanly without clipping.')

        for width in [390, 760, 900, 1280]:
            layout = target.eval(r'''(async () => {
              const frame = document.querySelector('#topbar-layout-check');
              frame.style.width = WIDTH + 'px';
              const doc = frame.contentDocument;
              doc.open(); doc.write('<!doctype html><html><head></head><body data-window="main"></body></html>'); doc.close();
              const style = doc.createElement('style'); style.textContent = CSS; doc.head.append(style);
              const app = doc.createElement('div'); app.className = 'app app--android';
              const sidebar = document.createElement('aside'); sidebar.className = 'sidebar';
              const main = document.createElement('div'); main.className = 'main';
              const bar = document.querySelector('.topbar').cloneNode(true);
              const content = document.createElement('main'); content.className = 'content';
              const nav = document.createElement('nav'); nav.className = 'mobile-nav';
              nav.innerHTML = '<button class="mobile-nav__item">今天</button><button class="mobile-nav__item">全部</button><button class="mobile-nav__item">流程</button><button class="mobile-nav__item">饮食</button><button class="mobile-nav__item">更多</button>';
              main.append(bar, content); app.append(sidebar, main, nav); doc.body.append(app);
              await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
              const compact = WIDTH < 900;
              const actionBar = bar.querySelector('.topbar__actions');
              const item = nav.querySelector('button');
              const ss = frame.contentWindow.getComputedStyle(sidebar).display;
              const ns = frame.contentWindow.getComputedStyle(nav).display;
              const appDisplay = frame.contentWindow.getComputedStyle(app).display;
              return {compact, sidebarDisplay:ss, navDisplay:ns, appDisplay,
                tapHeight:parseFloat(frame.contentWindow.getComputedStyle(item).minHeight),
                noToolbarOverflow:actionBar.scrollWidth <= actionBar.clientWidth + 1,
                safeContent:parseFloat(frame.contentWindow.getComputedStyle(content).paddingBottom) >= (compact ? 76 : 28)};
            })()'''.replace('WIDTH', str(width)).replace('CSS', json.dumps(css)))
            assert layout['sidebarDisplay'] == ('none' if layout['compact'] else 'flex'), (width, layout)
            assert layout['navDisplay'] == ('grid' if layout['compact'] else 'none'), (width, layout)
            assert layout['appDisplay'] == ('flex' if layout['compact'] else 'grid'), (width, layout)
            assert layout['tapHeight'] >= 48, (width, layout)
            assert layout['noToolbarOverflow'] and layout['safeContent'], (width, layout)
            checked += 1
        print('PASS: Android compact navigation and touch targets switch to a desktop rail at 900px without toolbar clipping.')
        for label in ['明天', '今天']:
            point = target.eval(r'''(() => {
              const item = [...document.querySelectorAll('.nav-item')].find(el => el.innerText.trim() === LABEL);
              if (!item) throw new Error('找不到导航项：' + LABEL);
              const rect = item.getBoundingClientRect();
              return {x:rect.left + rect.width / 2, y:rect.top + rect.height / 2, cursor:getComputedStyle(item).cursor};
            })()'''.replace('LABEL', json.dumps(label)))
            assert point['cursor'] == 'pointer', (label, point)
            target.call('Input.dispatchMouseEvent', {'type': 'mouseMoved', 'x': point['x'], 'y': point['y']})
            target.call('Input.dispatchMouseEvent', {'type': 'mousePressed', 'x': point['x'], 'y': point['y'], 'button': 'left', 'clickCount': 1})
            target.call('Input.dispatchMouseEvent', {'type': 'mouseReleased', 'x': point['x'], 'y': point['y'], 'button': 'left', 'clickCount': 1})
            assert target.wait_for(f"document.querySelector('.topbar__title')?.textContent === {json.dumps(label)}"), label
            assert target.eval(f"document.querySelector('.nav-item[aria-current=page] .nav-item__label')?.textContent === {json.dumps(label)}"), label
        print('PASS: real WebView2 mouse input navigates to another view and restores the original view; current navigation state follows it.')
        if args.screenshot:
            args.screenshot.write_bytes(base64.b64decode(target.call('Page.captureScreenshot', {'format': 'png'})['data']))
            print(f'Saved live WebView screenshot: {args.screenshot}')
    finally:
        target.eval('document.querySelector("#topbar-layout-check")?.remove()')
        target.close()


if __name__ == '__main__':
    main()
