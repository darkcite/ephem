// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Phone keyboard layout (docs/P2P-CHAT.md Appendix F.3.1): with the on-screen keyboard open the
// frame is the visible area above it (header on top, composer right on the keyboard, no tab
// bar), and the page itself never scrolls. The keyboard is simulated by shrinking
// visualViewport.height, which is what iOS Safari does; Chromium has no on-screen keyboard.
import { check, connect, finish, launch, problems, serve, watch } from './e2e_lib.mjs';

const srv = await serve();
const base = `http://127.0.0.1:${srv.address().port}/app`;
const browser = await launch();
const KBD = 336;
try {
  const a = await (await browser.newContext()).newPage();
  const b = await (await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
  watch(a, 'alice');
  watch(b, 'bob');
  await Promise.all([a.goto(`${base}/`), b.goto(`${base}/`)]);
  await connect(a, b);
  await b.waitForSelector('#v-chat:not([hidden])', { timeout: 20_000 });
  check('iOS does not zoom into the composer (16px on touch screens)',
    await b.$eval('#t-msg', (t) => getComputedStyle(t).fontSize) === '16px');
  const layout = () => b.evaluate(() => {
    const r = (s) => document.querySelector(s).getBoundingClientRect();
    return { kbd: document.body.classList.contains('kbd'), tabs: getComputedStyle(document.querySelector('.tabs')).display,
      bar: r('.bar').top, composer: Math.round(r('.composer').bottom), body: Math.round(r('body').height), scroll: window.scrollY };
  });
  const before = await layout();
  check('keyboard closed: tab bar shown, composer above it', !before.kbd && before.tabs !== 'none' && before.body === 844, JSON.stringify(before));
  await b.focus('#t-msg');
  await b.evaluate((h) => {
    Object.defineProperty(visualViewport, 'height', { configurable: true, get: () => h });
    window.scrollTo(0, 200);
    visualViewport.dispatchEvent(new Event('resize'));
  }, 844 - KBD);
  await b.waitForTimeout(150);
  const open = await layout();
  check('keyboard open: frame fits above the keyboard, header on top, no tab bar, page not scrolled',
    open.kbd && open.tabs === 'none' && open.bar === 0 && open.body === 844 - KBD && open.composer === 844 - KBD && open.scroll === 0, JSON.stringify(open));
  await b.screenshot({ path: 'out/keyboard-open.png' });
  await b.evaluate(() => {
    delete visualViewport.height;
    document.activeElement.blur();
    visualViewport.dispatchEvent(new Event('resize'));
  });
  await b.waitForTimeout(150);
  const closed = await layout();
  check('keyboard closed again: full frame and tab bar back', !closed.kbd && closed.tabs !== 'none' && closed.body === 844, JSON.stringify(closed));
  check('no page errors or CSP violations', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('keyboard flow', false, e.message.split('\n')[0]);
} finally {
  await browser.close();
  srv.close();
}
finish();
