// In-browser half of the checkpoint suite. Loaded by checks/browser_checks.mjs
// into Chromium, Firefox and WebKit. Every function returns plain JSON.
window.C = (() => {
  const st = { pc: null, dc: null, got: [] };

  // ---------- SDP minimal-field extraction and template rebuild (S1, S2) ----------
  function extract(sdp) {
    const g = (re) => (sdp.match(re) || [])[1];
    const cands = [...sdp.matchAll(/^a=candidate:(\S+) (\d) (\S+) (\d+) (\S+) (\d+) typ (\S+)/gm)]
      .filter((m) => m[2] === '1' && m[3].toLowerCase() === 'udp')
      .map((m) => ({ addr: m[5], port: +m[6], typ: m[7] }));
    return {
      ufrag: g(/^a=ice-ufrag:(\S+)/m), pwd: g(/^a=ice-pwd:(\S+)/m),
      fp: g(/^a=fingerprint:sha-256 (\S+)/m), setup: g(/^a=setup:(\S+)/m),
      mid: g(/^a=mid:(\S+)/m), sctpPort: g(/^a=sctp-port:(\d+)/m),
      maxMsg: g(/^a=max-message-size:(\d+)/m), cands, rawSdpBytes: sdp.length,
    };
  }

  // docs/P2P-CHAT.md Appendix A template.
  function rebuild(f, role, sessId) {
    const typPref = { host: 126, srflx: 100 };
    const lines = [
      'v=0', `o=- ${sessId} 2 IN IP4 127.0.0.1`, 's=-', 't=0 0', 'a=group:BUNDLE 0',
      'a=extmap-allow-mixed', 'a=msid-semantic: WMS',
      'm=application 9 UDP/DTLS/SCTP webrtc-datachannel', 'c=IN IP4 0.0.0.0',
      `a=ice-ufrag:${f.ufrag}`, `a=ice-pwd:${f.pwd}`, 'a=ice-options:trickle',
      `a=fingerprint:sha-256 ${f.fp}`, `a=setup:${role}`, 'a=mid:0',
      'a=sctp-port:5000', 'a=max-message-size:262144',
    ];
    f.cands.forEach((c, i) => {
      if (!(c.typ in typPref)) return;
      const pri = (typPref[c.typ] << 24) + ((65535 - i) << 8) + 255;
      const extra = c.typ === 'srflx' ? ' raddr 0.0.0.0 rport 0' : '';
      lines.push(`a=candidate:${c.typ}${i} 1 udp ${pri} ${c.addr} ${c.port} typ ${c.typ}${extra}`);
    });
    lines.push('a=end-of-candidates');
    return lines.join('\r\n') + '\r\n';
  }

  function gather(pc, capMs) {
    if (pc.iceGatheringState === 'complete') return Promise.resolve(true);
    return new Promise((r) => {
      const t = setTimeout(() => r(false), capMs);
      pc.addEventListener('icegatheringstatechange', () => {
        if (pc.iceGatheringState === 'complete') { clearTimeout(t); r(true); }
      });
    });
  }

  function mkpc(iceServers) {
    const pc = new RTCPeerConnection({ iceServers: iceServers || [], bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require' });
    const dc = pc.createDataChannel('c', { negotiated: true, id: 0, ordered: true });
    dc.binaryType = 'arraybuffer';
    dc.onmessage = (e) => st.got.push(typeof e.data === 'string' ? e.data : new TextDecoder().decode(e.data));
    st.pc = pc; st.dc = dc;
    return pc;
  }

  async function camera(keep) {
    const s = await navigator.mediaDevices.getUserMedia({ video: true });
    if (!keep) s.getTracks().forEach((t) => t.stop());
    return true;
  }

  async function offer(iceServers) {
    const pc = mkpc(iceServers);
    await pc.setLocalDescription(await pc.createOffer());
    const t0 = performance.now();
    const complete = await gather(pc, 5000);
    const f = extract(pc.localDescription.sdp);
    f.gatherMs = Math.round(performance.now() - t0); f.gatherComplete = complete;
    return f;
  }

  async function answer(offerFields) {
    const pc = mkpc([]);
    await pc.setRemoteDescription({ type: 'offer', sdp: rebuild(offerFields, 'actpass', '1234567890') });
    await pc.setLocalDescription(await pc.createAnswer());
    await gather(pc, 5000);
    return extract(pc.localDescription.sdp);
  }

  async function applyAnswer(answerFields) {
    await st.pc.setRemoteDescription({ type: 'answer', sdp: rebuild(answerFields, 'active', '987654321') });
  }

  function waitOpen(dc, pc, ms) {
    if (dc.readyState === 'open') return Promise.resolve('open');
    return new Promise((r) => {
      const t = setTimeout(() => r(`timeout (${pc.connectionState}/${pc.iceConnectionState})`), ms);
      dc.addEventListener('open', () => { clearTimeout(t); r('open'); });
    });
  }

  // S1 inside one tab (used for real Safari, which cannot run two automated instances):
  // two peer connections that exchange only the minimal fields through the template.
  async function selfPair() {
    const mk = () => {
      const pc = new RTCPeerConnection({ iceServers: [], bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require' });
      const dc = pc.createDataChannel('c', { negotiated: true, id: 0, ordered: true });
      return { pc, dc };
    };
    const a = mk(), b = mk();
    let got = null;
    b.dc.onmessage = (e) => { got = typeof e.data === 'string' ? e.data : new TextDecoder().decode(e.data); };
    await a.pc.setLocalDescription(await a.pc.createOffer());
    await gather(a.pc, 5000);
    const off = extract(a.pc.localDescription.sdp);
    await b.pc.setRemoteDescription({ type: 'offer', sdp: rebuild(off, 'actpass', '1234567890') });
    await b.pc.setLocalDescription(await b.pc.createAnswer());
    await gather(b.pc, 5000);
    const ans = extract(b.pc.localDescription.sdp);
    await a.pc.setRemoteDescription({ type: 'answer', sdp: rebuild(ans, 'active', '987654321') });
    const [oa, ob] = await Promise.all([waitOpen(a.dc, a.pc, 20000), waitOpen(b.dc, b.pc, 20000)]);
    if (oa === 'open') a.dc.send('p2p ✓');
    await new Promise((r) => setTimeout(r, 1000));
    a.pc.close(); b.pc.close();
    return { alice: oa, bob: ob, received: got, offerCands: off.cands.map((c) => c.typ), ok: got === 'p2p ✓' };
  }

  // ---------- S8 / TS3 / TS4: what the peer would see ----------
  async function srflx(iceServers) {
    const pc = new RTCPeerConnection({ iceServers });
    pc.createDataChannel('x');
    const cands = [];
    pc.onicecandidate = (e) => { if (e.candidate && e.candidate.candidate) cands.push(e.candidate.candidate); };
    await pc.setLocalDescription(await pc.createOffer());
    const complete = await gather(pc, 8000);
    pc.close();
    const parsed = cands.map((c) => { const m = c.match(/ (\S+) (\d+) typ (\S+)/); return m && { addr: m[1], port: +m[2], typ: m[3] }; }).filter(Boolean);
    return { complete, host: parsed.filter((c) => c.typ === 'host').map((c) => c.addr),
             srflx: [...new Set(parsed.filter((c) => c.typ === 'srflx').map((c) => c.addr))] };
  }

  // ---------- E2 / G2: live Snowflake rendezvous + DataChannel to a proxy ----------
  async function snowflake(brokerUrl, fingerprint, iceServers, timeoutMs) {
    const out = { broker: brokerUrl, steps: [] };
    const t0 = performance.now();
    const ms = () => Math.round(performance.now() - t0);
    const pc = new RTCPeerConnection({ iceServers });
    const dc = pc.createDataChannel('snowflake-check', { ordered: true });
    try {
      await pc.setLocalDescription(await pc.createOffer());
      await gather(pc, 10000);
      out.steps.push(`offer+gather ${ms()} ms`);
      const body = '1.0\n' + JSON.stringify({
        offer: JSON.stringify({ type: 'offer', sdp: pc.localDescription.sdp }), nat: 'unknown', fingerprint,
      });
      const ctl = new AbortController(); const to = setTimeout(() => ctl.abort(), 30000);
      const resp = await fetch(new URL('client', brokerUrl).toString(), { method: 'POST', body, signal: ctl.signal });
      clearTimeout(to);
      out.httpStatus = resp.status;
      out.acao = resp.headers.get('access-control-allow-origin');
      const txt = await resp.text();
      out.steps.push(`broker responded ${resp.status} after ${ms()} ms`);
      let j = null; try { j = JSON.parse(txt); } catch (_) { out.brokerBody = txt.slice(0, 200); }
      if (!j || !j.answer) { out.error = (j && j.error) || 'no answer'; return out; }
      await pc.setRemoteDescription(JSON.parse(j.answer));
      out.dataChannel = await waitOpen(dc, pc, timeoutMs);
      out.steps.push(`datachannel ${out.dataChannel} after ${ms()} ms`);
      out.ok = out.dataChannel === 'open';
    } catch (e) {
      out.error = `${e.name}: ${e.message}`;
    } finally {
      pc.close();
    }
    return out;
  }

  // ---------- C-P1 / C-P4: IPFS gateways and delegated IPNS publishing ----------
  async function gwCar(gw, cid) {
    try {
      const r = await fetch(`${gw}/ipfs/${cid}?format=car&dag-scope=entity`, { headers: { Accept: 'application/vnd.ipld.car' } });
      const b = new Uint8Array(await r.arrayBuffer());
      return { gw, status: r.status, type: r.headers.get('content-type'), bytes: b.length, ok: r.ok && /car/.test(r.headers.get('content-type') || '') };
    } catch (e) { return { gw, error: `${e.name}: ${e.message}` }; }
  }

  async function ipnsPut(base, name, recordB64) {
    try {
      const rec = Uint8Array.from(atob(recordB64), (c) => c.charCodeAt(0));
      const r = await fetch(`${base}/routing/v1/ipns/${name}`, { method: 'PUT', headers: { 'Content-Type': 'application/vnd.ipfs.ipns-record' }, body: rec });
      return { base, status: r.status, ok: r.ok, body: (await r.text()).slice(0, 160) };
    } catch (e) { return { base, error: `${e.name}: ${e.message}` }; }
  }

  async function ipnsGet(gw, name, recordB64) {
    try {
      const r = await fetch(`${gw}/ipns/${name}?format=ipns-record`, { headers: { Accept: 'application/vnd.ipfs.ipns-record' } });
      const b = new Uint8Array(await r.arrayBuffer());
      const want = atob(recordB64);
      const same = b.length === want.length && b.every((x, i) => x === want.charCodeAt(i));
      return { gw, status: r.status, bytes: b.length, sameRecord: same, ok: r.ok && same };
    } catch (e) { return { gw, error: `${e.name}: ${e.message}` }; }
  }

  // ---------- E8: timer drift in a hidden tab that holds an open DataChannel ----------
  let e8 = null;
  async function e8Start() {
    const a = new RTCPeerConnection(), b = new RTCPeerConnection();
    a.onicecandidate = (e) => e.candidate && b.addIceCandidate(e.candidate);
    b.onicecandidate = (e) => e.candidate && a.addIceCandidate(e.candidate);
    const dc = a.createDataChannel('keep');
    b.ondatachannel = (e) => { e.channel.onmessage = () => {}; };
    await a.setLocalDescription(await a.createOffer()); await b.setRemoteDescription(a.localDescription);
    await b.setLocalDescription(await b.createAnswer()); await a.setRemoteDescription(b.localDescription);
    await waitOpen(dc, a, 10000);
    e8 = { dc, maxGap: 0, last: performance.now(), hiddenSamples: 0, samples: 0 };
    const tick = () => {
      const now = performance.now();
      e8.maxGap = Math.max(e8.maxGap, now - e8.last); e8.last = now; e8.samples++;
      if (document.visibilityState === 'hidden') e8.hiddenSamples++;
      if (dc.readyState === 'open') dc.send('k');
      setTimeout(tick, 1000);
    };
    setTimeout(tick, 1000);
    return dc.readyState;
  }
  function e8Result() { return e8 && { maxGapMs: Math.round(e8.maxGap), samples: e8.samples, hiddenSamples: e8.hiddenSamples, visibility: document.visibilityState }; }

  return {
    ua: () => navigator.userAgent, camera, offer, answer, applyAnswer,
    waitOpen: (ms) => waitOpen(st.dc, st.pc, ms),
    send: (m) => st.dc.send(new TextEncoder().encode(m)), got: () => st.got.slice(),
    srflx, snowflake, gwCar, ipnsPut, ipnsGet, e8Start, e8Result, selfPair, extract,
  };
})();
