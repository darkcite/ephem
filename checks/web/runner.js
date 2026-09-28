// Checkpoint runner for a real browser tab: laptop Safari (safari_checks.mjs),
// iPhone (iphone_checks.mjs over a tunnel), or GitHub Pages (static config.json).
// Config comes from ./config.json; see those scripts for the fields.
(async () => {
  const $ = (id) => document.getElementById(id);
  const cfg = await (await fetch('config.json', { cache: 'no-store' })).json();
  const res = [];
  const isIp = (a) => /^[0-9.]+$/.test(a) || a.includes(':');
  const log = (m) => { $('log').textContent += m + '\n'; $('log').scrollTop = 1e9; };
  const add = (id, name, status, details) => {
    res.push({ id, name, status, details });
    log(`[${status}] ${id} ${name}: ${typeof details === 'string' ? details : JSON.stringify(details)}`);
  };
  const send = async (final) => {
    if (!cfg.resultUrl) return;
    try { await fetch(cfg.resultUrl + (final ? '?final=1' : ''), { method: 'POST', body: JSON.stringify(res) }); }
    catch (e) { log('could not send results: ' + e.message); }
  };
  const mb = {
    put: (k, v) => fetch('mb/' + k, { method: 'POST', body: JSON.stringify(v) }),
    get: async (k) => { for (;;) { const r = await fetch('mb/' + k); if (r.status === 200) return r.json(); } },
  };

  async function cross() {
    // A: this browser offers, the Playwright peer answers.
    const f = await C.offer([]);
    await mb.put('offerA', f);
    await C.applyAnswer(await mb.get('answerA'));
    const stA = await C.waitOpen(20000);
    if (stA === 'open') C.send(`from ${cfg.label}`);
    await mb.put('doneA', { open: stA });
    // B: the Playwright peer offers, this browser answers.
    const ansB = await C.answer(await mb.get('offerB'));
    await mb.put('answerB', ansB);
    const stB = await C.waitOpen(20000);
    await new Promise((r) => setTimeout(r, 4000));
    await mb.put('doneB', { open: stB, got: C.got() });
    log(`cross-engine: offerer=${cfg.label} ${stA}; answerer=${cfg.label} ${stB}`);
  }

  async function run() {
    $('run').disabled = true;
    add('ENV', cfg.label, 'INFO', navigator.userAgent);
    try { const r = await C.selfPair(); add('S1', `${cfg.label} → ${cfg.label} (one tab)`, r.ok ? 'PASS' : 'FAIL', r); } catch (e) { add('S1', 'one tab', 'FAIL', String(e)); }
    if (cfg.cross) { try { await cross(); } catch (e) { add('S1', 'cross-engine', 'FAIL', String(e)); } }
    try {
      const f = await C.offer([]);
      add('S2', `${cfg.label} offer fields`, 'PASS', { ufrag: f.ufrag.length, pwd: f.pwd.length, setup: f.setup, mid: f.mid, sctpPort: f.sctpPort, maxMsg: f.maxMsg, cands: f.cands.map((c) => c.typ + ':' + (isIp(c.addr) ? 'ip' : 'mdns')), rawSdpBytes: f.rawSdpBytes });
      add('S4', `${cfg.label}: no permission`, 'INFO', 'host candidates: ' + ([...new Set(f.cands.filter((c) => c.typ === 'host').map((c) => (isIp(c.addr) ? 'RAW-IP' : 'mdns')))].join(',') || 'none'));
    } catch (e) { add('S2', 'offer', 'FAIL', String(e)); }
    if (cfg.net) {
      try { const r = await C.srflx(cfg.stun); add('S8', `${cfg.label} srflx via Google+Cloudflare`, r.srflx.length ? 'PASS' : 'FAIL', r); } catch (e) { add('S8', 'srflx', 'FAIL', String(e)); }
      for (const b of cfg.snowflake.brokers) {
        let r = null;
        for (let i = 1; i <= 3 && !(r && r.ok); i++) { r = await C.snowflake(b, cfg.snowflake.fp, cfg.snowflake.stun, 20000); r.attempt = i; }
        add('E2', `${cfg.label} via ${new URL(b).host}`, r.ok ? 'PASS' : 'FAIL', r);
      }
      for (const g of cfg.gateways) { const r = await C.gwCar(g, cfg.cid); add('C-P1', `${cfg.label} CAR from ${new URL(g).host}`, r.ok ? 'PASS' : 'FAIL', r); }
      if (cfg.ipnsName) {
        const put = await C.ipnsPut(cfg.delegated, cfg.ipnsName, cfg.ipnsRecord);
        add('C-P4', `${cfg.label} PUT IPNS record`, put.ok ? 'PASS' : 'FAIL', put);
        if (put.ok) {
          for (const g of cfg.gateways) {
            let r = null;
            for (let i = 0; i < 4 && !(r && r.ok); i++) { if (i) await new Promise((z) => setTimeout(z, 15000)); r = await C.ipnsGet(g, cfg.ipnsName, cfg.ipnsRecord); }
            add('C-P4', `${cfg.label} read back via ${new URL(g).host}`, r.ok ? 'PASS' : 'FAIL', r);
          }
        }
      }
    }
    log('Automatic checks finished.' + (cfg.interactive ? ' Now run S6 and S4 below, then send the results.' : ''));
    await send(!cfg.interactive);
    $('run').disabled = false;
  }

  // S6: start, leave the app, come back; the finish step runs automatically on return.
  let s6armed = false;
  async function s6() {
    $('s6').disabled = true;
    await C.bgStart();
    s6armed = true;
    log('S6 armed: switch to another app (e.g. Messages) for about 60 s, then come back to this tab.');
  }
  document.addEventListener('visibilitychange', async () => {
    if (!s6armed || document.visibilityState !== 'visible') return;
    s6armed = false;
    await new Promise((r) => setTimeout(r, 500));
    const r = await C.bgFinish();
    add('S6', `${cfg.label} pending offer after ${r.hiddenSeconds} s in background`, r.ok ? 'PASS' : 'FAIL', r);
    $('s6').disabled = false;
    await send(false);
  });

  async function s4() {
    try { const kinds = await C.cameraOffer(); add('S4', `${cfg.label}: after camera permission`, 'INFO', 'host candidates: ' + (kinds.join(',') || 'none')); }
    catch (e) { add('S4', `${cfg.label}: after camera permission`, 'FAIL', String(e)); }
    await send(false);
  }

  const text = () => res.map((r) => `[${r.status}] ${r.id} ${r.name}: ${typeof r.details === 'string' ? r.details : JSON.stringify(r.details)}`).join('\n');
  $('run').onclick = run;
  $('s6').onclick = s6;
  $('s4').onclick = s4;
  $('copy').onclick = async () => { await navigator.clipboard.writeText(text()); log('Copied.'); };
  $('share').onclick = () => navigator.share ? navigator.share({ title: 'p2p-chat checks', text: text() }) : log('Share not available; use Copy.');
  $('finish').onclick = async () => { await send(true); log(cfg.resultUrl ? 'Results sent to the laptop.' : 'No laptop connected: use Copy or Share.'); };
  for (const id of ['s6', 's4', 'finish']) $(id).hidden = !cfg.interactive;
  if (cfg.autorun) run();
})();
