// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Writes checks/web/config.json for the static (GitHub Pages) copy of the checks page.
// The IPNS test record is signed by a throwaway key and valid for one year; re-run to refresh.
import * as fs from 'node:fs';
import * as ipns from 'ipns';
import * as keys from '@libp2p/crypto/keys';
import * as pid from '@libp2p/peer-id';
import * as b36 from 'multiformats/bases/base36';

export const CID = 'bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi';
export async function baseConfig(extra = {}) {
  const priv = await keys.generateKeyPair('Ed25519');
  const rec = ipns.marshalIPNSRecord(await ipns.createIPNSRecord(priv, `/ipfs/${CID}`, 1n, 365 * 24 * 3600 * 1000));
  return {
    label: 'browser', net: true, autorun: false, interactive: true, cross: false, resultUrl: null,
    cid: CID,
    gateways: ['https://trustless-gateway.link'], // ipfs.io/dweb.link redirect here without CORS (C-P1)
    delegated: 'https://delegated-ipfs.dev',
    stun: [{ urls: ['stun:stun.l.google.com:19302', 'stun:stun.cloudflare.com:3478'] }],
    snowflake: {
      fp: '2B280B23E1107BB62ABFC40DDCC8824814F80A72',
      brokers: ['https://1098762253.rsc.cdn77.org/', 'https://snowflake-broker.torproject.net/'],
      stun: [{ urls: ['stun:stun.l.google.com:19302', 'stun:stun.antisip.com:3478', 'stun:stun.nextcloud.com:3478'] }],
    },
    ipnsName: pid.peerIdFromPrivateKey(priv).toCID().toString(b36.base36),
    ipnsRecord: Buffer.from(rec).toString('base64'),
    ...extra,
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const cfg = await baseConfig({ label: 'iphone' });
  fs.writeFileSync(new URL('./web/config.json', import.meta.url), JSON.stringify(cfg, null, 1) + '\n');
  console.log('wrote web/config.json', cfg.ipnsName);
}
