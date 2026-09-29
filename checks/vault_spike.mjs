// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Spikes V-P1 and V-P2 (docs/P2P-CHAT.md §D.11.7), on the real network. Run on any machine that
// reaches delegated-ipfs.dev (the app goes there through a Tor exit; the service sees the same
// request either way, and CORS does not apply: the app's request is not a browser fetch).
//
//   V-P1: a record shaped like the largest vault (IPNS V2-only, value = an inline identity CID
//         of 5 672 bytes of ciphertext-like data, ≈ 9.2 KiB) is accepted by the routing API's
//         PUT and comes back byte-identical from its GET and from trustless-gateway.link.
//   V-P2: then it is resolved every CHECK_MIN minutes (default 60) for HOURS hours (default 0 =
//         V-P1 only), without republishing: when does the DHT forget it?
//
//   node checks/vault_spike.mjs                V-P1 only
//   HOURS=72 node checks/vault_spike.mjs       V-P1, then V-P2 for 72 h (leave it running)
import * as crypto from 'node:crypto';
import * as ipns from 'ipns';
import * as keys from '@libp2p/crypto/keys';
import * as pid from '@libp2p/peer-id';
import * as b36 from 'multiformats/bases/base36';
import * as cidlib from 'multiformats/cid';
import * as identity from 'multiformats/hashes/identity';

const ROUTING = 'https://delegated-ipfs.dev';
const GATEWAY = 'https://trustless-gateway.link';
const CT = 'application/vnd.ipfs.ipns-record';
const HOURS = Number(process.env.HOURS || 0);
const CHECK_MIN = Number(process.env.CHECK_MIN || 60);
// The vault's largest padded plaintext (vault::MAX_PLAIN) + nonce (24) + tag (16).
const SEALED = 5632 + 24 + 16;

const priv = await keys.generateKeyPair('Ed25519');
const name = pid.peerIdFromPrivateKey(priv).toCID().toString(b36.base36);
const inline = cidlib.CID.createV1(0x55, identity.identity.digest(crypto.randomBytes(SEALED)));
const record = ipns.marshalIPNSRecord(await ipns.createIPNSRecord(priv, `/ipfs/${inline.toString()}`, 1n, 30 * 24 * 3600 * 1000, { v1Compatible: false }));
const stamp = () => new Date().toISOString().slice(0, 16).replace('T', ' ');
console.log(`${stamp()}  vault-shaped record: ${record.length} bytes, name ${name}`);

async function get(url) {
  try {
    const r = await fetch(url, { headers: { Accept: CT }, signal: AbortSignal.timeout(60_000) });
    const b = Buffer.from(await r.arrayBuffer());
    return { status: r.status, same: b.equals(Buffer.from(record)), bytes: b.length };
  } catch (e) {
    return { error: e.message };
  }
}
const show = (r) => r.error ? `error ${r.error}` : `${r.status}, ${r.bytes} bytes, ${r.same ? 'byte-identical' : 'DIFFERENT'}`;

const put = await fetch(`${ROUTING}/routing/v1/ipns/${name}`, { method: 'PUT', headers: { 'Content-Type': CT }, body: record, signal: AbortSignal.timeout(60_000) }).catch((e) => ({ ok: false, status: e.message, text: async () => '' }));
console.log(`${stamp()}  V-P1 PUT ${ROUTING}: ${put.status} ${put.ok ? 'accepted' : `REFUSED ${(await put.text()).slice(0, 200)}`}`);
if (!put.ok) process.exit(1);
let ok = false;
for (let i = 0; i < 4 && !ok; i++) {
  if (i) await new Promise((z) => setTimeout(z, 15_000));
  const a = await get(`${ROUTING}/routing/v1/ipns/${name}`);
  const b = await get(`${GATEWAY}/ipns/${name}?format=ipns-record`);
  console.log(`${stamp()}  V-P1 GET routing API: ${show(a)} | gateway: ${show(b)}`);
  ok = a.same && b.same;
}
console.log(`${stamp()}  V-P1 ${ok ? 'PASS' : 'FAIL'}`);
if (!HOURS) process.exit(ok ? 0 : 1);

// V-P2: never republished from here on.
const t0 = Date.now();
while (Date.now() - t0 < HOURS * 3600_000) {
  await new Promise((z) => setTimeout(z, CHECK_MIN * 60_000));
  const h = ((Date.now() - t0) / 3600_000).toFixed(1);
  console.log(`${stamp()}  V-P2 +${h} h: routing API ${show(await get(`${ROUTING}/routing/v1/ipns/${name}`))} | gateway ${show(await get(`${GATEWAY}/ipns/${name}?format=ipns-record`))}`);
}
