// Runs inside the browser. Simulates what the Rust core will do: extract the
// minimal fields from the local SDP, and rebuild a remote SDP from the template.
window.T = (() => {
  const st = { pc: null, dc: null, got: [] };

  function extract(sdp) {
    const g = (re) => (sdp.match(re) || [])[1];
    const cands = [...sdp.matchAll(/^a=candidate:(\S+) (\d) (\S+) (\d+) (\S+) (\d+) typ (\S+)/gm)]
      .filter((m) => m[2] === '1' && m[3].toLowerCase() === 'udp')
      .map((m) => ({ addr: m[5], port: +m[6], typ: m[7] }));
    return {
      ufrag: g(/^a=ice-ufrag:(\S+)/m),
      pwd: g(/^a=ice-pwd:(\S+)/m),
      fp: g(/^a=fingerprint:sha-256 (\S+)/m),
      setup: g(/^a=setup:(\S+)/m),
      mid: g(/^a=mid:(\S+)/m),
      sctpPort: g(/^a=sctp-port:(\d+)/m),
      maxMsg: g(/^a=max-message-size:(\d+)/m),
      cands,
      rawSdpBytes: sdp.length,
    };
  }

  // Appendix A template (SPEC). role = actpass for an offer, active for an answer.
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
      const pri = (typPref[c.typ] << 24) + ((65535 - i) << 8) + 255;
      const extra = c.typ === 'srflx' ? ' raddr 0.0.0.0 rport 0' : '';
      lines.push(`a=candidate:${c.typ}${i} 1 udp ${pri} ${c.addr} ${c.port} typ ${c.typ}${extra}`);
    });
    lines.push('a=end-of-candidates');
    return lines.join('\r\n') + '\r\n';
  }

  async function gather(pc, capMs) {
    if (pc.iceGatheringState === 'complete') return;
    await new Promise((r) => {
      const t = setTimeout(r, capMs);
      pc.addEventListener('icegatheringstatechange', () => {
        if (pc.iceGatheringState === 'complete') { clearTimeout(t); r(); }
      });
    });
  }

  function mkpc() {
    const pc = new RTCPeerConnection({ iceServers: [], bundlePolicy: 'max-bundle', rtcpMuxPolicy: 'require' });
    const dc = pc.createDataChannel('c', { negotiated: true, id: 0, ordered: true });
    dc.binaryType = 'arraybuffer';
    dc.onmessage = (e) => st.got.push(typeof e.data === 'string' ? e.data : new TextDecoder().decode(e.data));
    st.pc = pc; st.dc = dc;
    return pc;
  }

  return {
    async camera() {
      const s = await navigator.mediaDevices.getUserMedia({ video: true });
      return s;
    },
    async cameraThenStop() {
      const s = await navigator.mediaDevices.getUserMedia({ video: true });
      s.getTracks().forEach((t) => t.stop());
      return true;
    },
    async offer() {
      const pc = mkpc();
      await pc.setLocalDescription(await pc.createOffer());
      const t0 = performance.now();
      await gather(pc, 3000);
      const f = extract(pc.localDescription.sdp);
      f.gatherMs = Math.round(performance.now() - t0);
      f.sdp = pc.localDescription.sdp;
      return f;
    },
    async answer(offerFields) {
      const pc = mkpc();
      await pc.setRemoteDescription({ type: 'offer', sdp: rebuild(offerFields, 'actpass', '1234567890') });
      await pc.setLocalDescription(await pc.createAnswer());
      await gather(pc, 3000);
      const f = extract(pc.localDescription.sdp);
      f.sdp = pc.localDescription.sdp;
      return f;
    },
    async applyAnswer(answerFields) {
      await st.pc.setRemoteDescription({ type: 'answer', sdp: rebuild(answerFields, 'active', '987654321') });
    },
    async waitOpen(ms) {
      if (st.dc.readyState === 'open') return 'open';
      return await new Promise((r) => {
        const t = setTimeout(() => r('timeout:' + st.pc.connectionState + '/' + st.pc.iceConnectionState), ms);
        st.dc.onopen = () => { clearTimeout(t); r('open'); };
      });
    },
    send(msg) { st.dc.send(new TextEncoder().encode(msg)); },
    got() { return st.got.slice(); },
    async selectedPair() {
      const stats = await st.pc.getStats();
      let pair = null, local = {}, remote = {};
      stats.forEach((r) => { if (r.type === 'transport' && r.selectedCandidatePairId) pair = stats.get(r.selectedCandidatePairId); });
      if (!pair) return null;
      const l = stats.get(pair.localCandidateId), rm = stats.get(pair.remoteCandidateId);
      return { local: l && { type: l.candidateType, address: l.address, protocol: l.protocol },
               remote: rm && { type: rm.candidateType, address: rm.address, protocol: rm.protocol },
               rtt: pair.currentRoundTripTime };
    },
  };
})();
