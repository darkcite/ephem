// Contact cards in direct mode (docs/P2P-CHAT.md §7.5), in real Chromium:
//
//   Alice (saved identity) shows her card (#k= link, 30 days) → Bob (saved identity) opens it
//   and adds "Alice" as an unverified contact → a temporary identity cannot → they chat by
//   invite and answer (a card does not connect in direct mode) → Bob sees his contact's name,
//   the SAS is still prompted, then ✔ → Alice resets her card (new secret, the link changes).
//
// Usage: node checks/e2e_cards.mjs   (build first with ./build.sh)
import { PASS, check, connect, finish, launch, problems, serve, watch } from './e2e_lib.mjs';

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
  await p.fill('#i-nick', nick);
  await p.dispatchEvent('#i-nick', 'change');
  await p.click('#b-id-save');
  await p.fill('#i-label', label);
  await p.fill('#i-pass', PASS);
  await p.fill('#i-pass2', PASS);
  await Promise.all([p.waitForEvent('download'), p.click('#b-id-do-save')]);
}

try {
  const [a, b, t] = await Promise.all([open('alice'), open('bob'), open('temp')]);
  await saveIdentity(a, 'A', 'Alice');
  await saveIdentity(b, 'B', 'Bob');
  check('"My contact card" only for a saved identity', await a.isVisible('#b-card') && await t.isHidden('#b-card'));

  await a.click('#b-card');
  await a.waitForFunction(() => document.querySelector('#card .link')?.value.includes('#k='));
  const card = await a.inputValue('#card .link');
  check('Alice shows her card (QR + #k= link, 30 days)', /works until/.test(await a.textContent('#card-expiry')) && (await a.$('#card .qr svg')) !== null, card.length + ' chars');

  // A temporary identity has no contacts.
  await t.fill('#t-code', card);
  await t.click('#b-apply');
  await t.waitForSelector('#error:not([hidden])');
  check('a temporary identity cannot add from a card', /saved identity/.test(await t.textContent('#error')));

  // Bob uses "Add from a contact card" in his contacts: an invite there is refused with a hint,
  // the card asks for a name (suggested: Alice's nickname).
  await b.fill('#t-card', 'https://example.org/app/#i=AAAA');
  await b.click('#b-add-card');
  await b.waitForSelector('#error:not([hidden])');
  check('the contacts card field refuses what is not a card', /not a contact card/.test(await b.textContent('#error')));
  b.answers.push('Alice from the card');
  await b.fill('#t-card', card);
  await b.click('#b-add-card');
  await b.waitForFunction(() => /Alice from the card/.test(document.querySelector('#contacts')?.textContent), null, { timeout: 10_000 });
  const row = await b.textContent('#contacts');
  check('Bob adds Alice from her card in Contacts: an unverified contact', !/✔/.test(row) && !/Connect/.test(row), row.trim());

  // Direct mode: a card pins the key and name; a chat still needs invite and answer.
  await connect(b, a);
  await Promise.all([a, b].map((p) => p.waitForSelector('#v-chat:not([hidden])', { timeout: 20_000 })));
  await b.waitForFunction(() => /Alice from the card/.test(document.querySelector('#peer')?.textContent), null, { timeout: 10_000 });
  check('Bob sees his contact\'s name; the SAS is still asked', await b.isVisible('#sas') && !(await b.textContent('#peer')).includes('✔'));
  await b.click('#b-sas-ok');
  await b.waitForFunction(() => document.querySelector('#peer')?.textContent.includes('✔'), null, { timeout: 5_000 });
  check('after the SAS the contact is verified', true, await b.textContent('#peer'));
  a.answers.push(true);
  await a.click('#b-leave');

  a.answers.push(true); // confirm the reset
  await a.click('#b-again');
  await a.click('#b-card');
  if (await a.isHidden('#card')) await a.click('#b-card');
  await a.click('#b-card-reset');
  await a.waitForFunction((old) => document.querySelector('#card .link')?.value !== old, card);
  check('reset: a new card (the old secret is gone)', (await a.inputValue('#card .link')) !== card && /Backup out of date/.test(await a.textContent('#backup-stale')));
  check('no page errors or CSP violations', problems.length === 0, problems.join(' | '));
} catch (e) {
  check('cards flow', false, e.message.split('\n')[0]);
} finally {
  for (const b of browsers) await b.close();
  srv.close();
}
finish();
