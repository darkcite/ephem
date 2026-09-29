// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// End-to-end test of the MVP-2 features in real Chromium (docs/P2P-CHAT.md §23.1):
//
//   remembered identities (IndexedDB slot, sign in from the list after a reload) · nicknames ·
//   contacts (save, verified by SAS, SAS skipped next time, backup-out-of-date notice) ·
//   reactions · full diagnostics · in-band ICE restart (T1) with the chat continuing ·
//   identity transfer to a new device (SAS on both, encrypted key file incl. contacts).
//
// Usage: node checks/e2e_mvp2.mjs   (build first with ./build.sh; E2E_BROWSER=chrome for installed Chrome)
import { PASS, check, connect, finish, launch, msgWith, problems, serve, watch } from './e2e_lib.mjs';

const srv = await serve();
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await launch();
const answer = (page, value) => page.once('dialog', (d) => (value === false ? d.dismiss() : value === true ? d.accept() : d.accept(value)));

try {
  const ctxA = await browser.newContext({ acceptDownloads: true });
  const ctxB = await browser.newContext();
  const a = await ctxA.newPage(); watch(a, 'alice');
  const b = await ctxB.newPage(); watch(b, 'bob');
  for (const p of [a, b]) {
    await p.goto(`${base}/app/`);
    await p.waitForSelector('#v-start:not([hidden])');
  }

  // ---- saved identity remembered on this device (§7.2, §7.3) ----
  await a.fill('#i-nick', 'Alice');
  await a.dispatchEvent('#i-nick', 'change');
  await a.click('#b-id-save');
  await a.fill('#i-label', 'Laptop');
  await a.fill('#i-pass', PASS);
  await a.fill('#i-pass2', PASS);
  await Promise.all([a.waitForEvent('download'), a.click('#b-id-do-save')]);
  const handleA = await a.textContent('#me');
  await a.waitForSelector('#slots li');
  check('identity remembered in a slot', /Laptop/.test(await a.textContent('#slots')) && /in use/.test(await a.textContent('#slots')));
  await a.reload();
  await a.waitForSelector('#v-start:not([hidden])');
  await a.waitForSelector('#slots li button:text("Sign in")');
  await a.click('#slots li button:text("Sign in")');
  await a.fill('#slots li input[type=password]', PASS);
  await a.click('#slots li button:text("Sign in")');
  await a.waitForFunction((h) => document.querySelector('#me')?.textContent === h, handleA, { timeout: 10000 });
  check('after a reload, sign in from the remembered list', (await a.inputValue('#i-nick')) === 'Alice', 'nickname kept in the key file');

  await b.fill('#i-nick', 'Bob');
  await b.dispatchEvent('#i-nick', 'change');

  // ---- chat with nicknames, contact, reactions ----
  await connect(a, b);
  await Promise.all([a.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 }), b.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 })]);
  await b.waitForFunction(() => document.querySelector('#peer')?.textContent.includes('“Alice”'));
  check('peer nickname shown, marked as self-chosen', (await b.textContent('#peer')) === `${handleA} “Alice”`);
  check('"+ contact" only for saved identities', (await a.isVisible('#b-save-contact')) && !(await b.isVisible('#b-save-contact')));
  await a.click('#b-sas-ok');
  answer(a, 'Bobby');
  await a.click('#b-save-contact');
  await a.waitForFunction(() => document.querySelector('#peer')?.textContent === 'Bobby ✔');
  check('contact saved, verified by the SAS', (await a.textContent('#verified')) === 'verified contact');

  await a.fill('#t-msg', 'react to this');
  await a.press('#t-msg', 'Enter');
  await msgWith(b, 'them', 'react to this').click();
  await b.click('#log .acts button:text("React")');
  await b.click('#log .picker button:text("👍")');
  await a.waitForFunction(() => document.querySelector('#log li.me .reacts')?.textContent === '👍 peer', null, { timeout: 5000 });
  check('reaction reaches the sender', (await b.textContent('#log li.them .reacts')) === '👍 you');

  // ---- diagnostics and in-band ICE restart (§13 T1, §18) ----
  await a.click('#b-info');
  await a.waitForFunction(() => /Noise KK/.test(document.querySelector('#diag-core')?.textContent), null, { timeout: 5000 });
  check('diagnostics show crypto and connection state', /ice connected/.test(await a.textContent('#diag-core')), (await a.textContent('#diag-core')).replace(/\n/g, ' | '));
  await a.click('#b-restart');
  await a.waitForFunction(() => /ICE restarts 1/i.test(document.querySelector('#diag-core')?.textContent), null, { timeout: 8000 });
  await a.waitForTimeout(4000); // re-offer, gathering, re-answer
  await b.fill('#t-msg', 'after the ICE restart');
  await b.press('#t-msg', 'Enter');
  await msgWith(a, 'them', 'after the ICE restart').waitFor({ timeout: 8000 });
  check('in-band ICE restart keeps the chat', /ice (connected|completed)/.test(await a.textContent('#diag-core')), (await a.textContent('#diag-core')).split('\n')[1]);

  await b.click('#b-leave');
  await a.waitForSelector('#v-note:not([hidden])', { timeout: 10000 });
  await a.click('#b-again');
  await b.click('#b-again');
  await a.waitForSelector('#backup-stale:not([hidden])');
  check('backup marked out of date after the contact change', /Backup out of date/.test(await a.textContent('#backup-stale')));
  check('contacts list shows the verified contact', /Bobby\s*✔/.test(await a.textContent('#contacts')));

  // A verified contact skips the SAS prompt (§10.4).
  await connect(a, b);
  await a.waitForSelector('#v-chat:not([hidden])', { timeout: 20000 });
  await a.waitForFunction(() => document.querySelector('#verified')?.textContent === 'verified contact', null, { timeout: 5000 });
  check('verified contact: no SAS prompt', await a.isHidden('#sas'));
  await a.click('#b-leave');
  await b.waitForSelector('#v-note:not([hidden])', { timeout: 10000 });
  await a.click('#b-again');

  // ---- identity transfer to a new device (§7.6) ----
  const ctxN = await browser.newContext();
  const n = await ctxN.newPage(); watch(n, 'new-device');
  await n.goto(`${base}/app/`);
  await n.waitForSelector('#v-start:not([hidden])');
  answer(n, true);
  await n.click('#b-id-receive');
  await n.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#i='), null, { timeout: 15000 });
  check('new device shows a transfer invite', (await n.textContent('#code-title')) === 'Receive an identity');
  await a.fill('#t-code', await n.inputValue('#v-code .link'));
  answer(a, true);
  await a.click('#b-apply');
  await a.waitForFunction(() => document.querySelector('#v-code .link')?.value.includes('#a='), null, { timeout: 15000 });
  await n.fill('#t-answer', await a.inputValue('#v-code .link'));
  await n.click('#b-answer');
  await Promise.all([n.waitForSelector('#v-transfer:not([hidden])', { timeout: 20000 }), a.waitForSelector('#v-transfer:not([hidden])', { timeout: 20000 })]);
  check('both devices show the transfer SAS', (await n.textContent('#xfer-digits')) === (await a.textContent('#xfer-digits')));
  await a.click('#b-xfer-ok');
  await n.click('#b-xfer-ok');
  await n.waitForSelector('#xfer-unlock:not([hidden])', { timeout: 10000 });
  await a.waitForSelector('#xfer-sent:not([hidden])', { timeout: 10000 });
  check('encrypted key file sent after both confirmed', true);
  await n.fill('#i-xfer-pass', PASS);
  answer(n, false); // no backup download now
  await n.click('#b-xfer-unlock');
  await n.waitForFunction((h) => document.querySelector('#me')?.textContent === h, handleA, { timeout: 10000 });
  await n.waitForSelector('#v-start:not([hidden])');
  check('new device signed in as the same identity', /Laptop/.test(await n.textContent('#id-desc')));
  check('contacts moved with the identity', /Bobby\s*✔/.test(await n.textContent('#contacts')) && (await n.inputValue('#i-nick')) === 'Alice');
  await a.click('#b-xfer-keep');
  await a.waitForSelector('#v-start:not([hidden])');
  check('old device keeps the identity by default', (await a.textContent('#me')) === handleA);

  check('no CSP violations or page errors', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('mvp2 flow', false, e.message.split('\n')[0]);
  if (problems.length) console.log(problems.join('\n'));
} finally {
  await browser.close();
  srv.close();
}
finish();
