// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Contact cards in direct mode (docs/P2P-CHAT.md §7.5), in real Chromium:
//
//   Alice (saved identity) shows her card (#k= link, 30 days) → Bob (saved identity) opens it
//   and adds "Alice" as an unverified contact → a temporary identity cannot → they chat by
//   invite and answer (a card does not connect in direct mode) → Bob sees his contact's name,
//   the SAS is still prompted, then ✔ → Alice resets her card (new secret, the link changes).
//
// Usage: node checks/e2e_cards.mjs   (build first with ./build.sh)
import { check, contactRow, toAdd, toChats, connect, finish, launch, PASS, problems, serve, toSettings, watch, openCode } from './e2e_lib.mjs';

const srv = await serve();
const base = `http://127.0.0.1:${srv.address().port}/app/`;
const browsers = [];

/** A page whose dialogs are answered from `answers` (prompt text, true = OK) in order. */
async function open(who) {
  const b = await launch();
  browsers.push(b);
  const p = await (await b.newContext({ acceptDownloads: true })).newPage();
  watch(p, who);
  p.answers = [];
  p.on('dialog', (d) => {
    const a = p.answers.shift();
    if (a === false) d.dismiss();
    else d.accept(typeof a === 'string' ? a : undefined);
  });
  await p.goto(base);
  await p.waitForSelector('#v-start:not([hidden])');
  return p;
}

async function saveIdentity(p, label, nick) {
  await toSettings(p);
  await p.fill('#i-nick', nick);
  await p.dispatchEvent('#i-nick', 'change');
  await p.click('#b-id-save');
  await p.fill('#i-label', label);
  await p.fill('#i-pass', PASS);
  await p.fill('#i-pass2', PASS);
  await Promise.all([p.waitForEvent('download'), p.click('#b-id-do-save')]);
}

let a, b, t;
try {
  [a, b, t] = await Promise.all([open('alice'), open('bob'), open('temp')]);
  await saveIdentity(a, 'A', 'Alice');
  await saveIdentity(b, 'B', 'Bob');
  await toAdd(a);
  await toAdd(t);
  check('the contact card is shared from Chats → New → Add a contact, only for a saved identity', await a.isVisible('#card') && await t.isHidden('#card') && await t.isVisible('#card-none'));

  await a.waitForFunction(() => document.querySelector('#card .link')?.value.includes('#k='));
  const card = await a.inputValue('#card .link');
  check('Alice shows her card (QR + #k= link, 30 days)', /works until/.test(await a.textContent('#card-expiry')) && (await a.$('#card .qr svg')) !== null, card.length + ' chars');

  // A temporary identity has no contacts: ⌗ Code opens the Add pane, adding is refused.
  await openCode(t);
  await t.fill('#t-code', card);
  await t.click('#b-apply');
  await t.waitForSelector('#v-add:not([hidden])');
  await t.click('#b-add-card');
  await t.waitForSelector('#error:not([hidden])');
  check('a temporary identity cannot add from a card (and the list says why)', /saved identity/.test(await t.textContent('#error')) && (await toChats(t), /kept in a saved identity/.test(await t.textContent('#contacts-note'))));

  // Bob: Add a contact refuses what is not a card; a card suggests the name (no dialog).
  await toAdd(b);
  await b.fill('#t-card', 'https://example.org/app/#i=AAAA');
  await b.click('#b-add-card');
  await b.waitForSelector('#error:not([hidden])');
  check('Add a contact refuses what is not a card', /not a contact card/.test(await b.textContent('#error')));
  await b.fill('#t-card', card);
  await b.dispatchEvent('#t-card', 'input');
  check('the card suggests its owner\'s nickname as the name', (await b.inputValue('#i-card-name')) === 'Alice');
  await b.fill('#i-card-name', 'Alice from the card');
  await b.click('#b-add-card');
  const row = await contactRow(b, 'Alice from the card');
  await row.waitFor({ timeout: 10_000 });
  check('Bob adds Alice from her card: a contact row in Chats, unverified, "Invite" in direct mode', !/✔/.test(await row.textContent()) && /Invite/.test(await row.textContent()), (await row.textContent()).trim());

  // The person pane: rename inline, fingerprint, remove with undo.
  await row.click();
  await b.waitForSelector('#v-person:not([hidden])');
  const fpB = await b.textContent('#p-fp');
  await b.fill('#p-name', 'Alice A.');
  await b.dispatchEvent('#p-name', 'change');
  await (await contactRow(b, 'Alice A.')).waitFor({ timeout: 5_000 });
  check('person pane: renamed inline, no dialog; a 12-digit fingerprint', /^\d{4} \d{4} \d{4}$/.test(fpB), fpB);
  await (await contactRow(b, 'Alice A.')).click();
  await b.click('#b-p-remove');
  await b.waitForSelector('#toast:not([hidden])');
  const gone = await b.locator('#contacts li', { hasText: 'Alice A.' }).count() === 0;
  await b.click('#b-undo');
  await (await contactRow(b, 'Alice A.')).waitFor({ timeout: 5_000 });
  check('remove → undo brings the contact back', gone);

  // Direct mode: a card pins the key and name; a chat still needs invite and answer.
  await connect(b, a);
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: 20_000 })));
  await b.waitForFunction(() => /Alice A\./.test(document.querySelector('#peer')?.textContent), null, { timeout: 10_000 });
  check('Bob sees his contact\'s name; the SAS is still asked', await b.isVisible('#sas') && !(await b.textContent('#peer')).includes('✔'));
  check('one row per person: the open chat replaces the contact row', await b.locator('#contacts li', { hasText: 'Alice A.' }).count() === 0 && /Alice A\./.test(await b.textContent('#chats')));
  await b.click('#b-sas-ok');
  await b.waitForFunction(() => document.querySelector('#peer')?.textContent.includes('✔'), null, { timeout: 5_000 });
  check('after the SAS the contact is verified', true, await b.textContent('#peer'));
  // Alice adds Bob from the chat ("Add to contacts": his own name, no dialog); both see the same fingerprint.
  await a.click('#b-save-contact');
  await a.waitForFunction(() => document.querySelector('#b-save-contact').hidden);
  a.answers.push(true);
  await a.click('#b-leave');
  await a.click('#b-again');
  const rowA = await contactRow(a, 'Bob');
  await rowA.waitFor({ timeout: 10_000 });
  await rowA.click();
  const fpA = await a.textContent('#p-fp');
  check('"Add to contacts" in a chat saves their own name; the fingerprint is the same on both sides', fpA === fpB, `${fpA} / ${fpB}`);
  await a.click('#b-p-verify');
  await a.waitForFunction(() => /verified ✔/.test(document.querySelector('#p-verified').textContent));
  check('verified from the fingerprint, without a chat', /Bob ✔/.test(await a.textContent('#contacts')));

  a.answers.push(true); // confirm the reset
  await toSettings(a);
  await a.click('#b-card-reset');
  await toAdd(a);
  await a.waitForFunction((old) => document.querySelector('#card .link')?.value !== old, card);
  check('reset (Settings): a new card (the old secret is gone)', (await a.inputValue('#card .link')) !== card && /Backup out of date/.test(await a.textContent('#backup-stale')));
  check('no page errors or CSP violations', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('cards flow', false, e.message.split('\n')[0]);
  console.log(problems.join('\n'));
  for (const [p, w] of [[a, 'a'], [b, 'b']]) console.log(w, await p?.evaluate(() => ({ views: [...document.querySelectorAll('.view:not([hidden])')].map((v) => v.id).join(), err: document.querySelector('#error:not([hidden])')?.textContent, status: document.querySelector('#status')?.textContent })).catch((x) => x.message));
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
