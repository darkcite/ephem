// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// CH-1 interop (docs/P2P-CHAT.md §23.1b C-1): what crates/channel writes is valid for the
// reference JavaScript IPFS libraries — the IPNS record validates for its name (`ipns`), the
// name and root parse as CIDs (`multiformats`), every CAR block hashes to its CID and decodes as
// dag-cbor (`cborg`, strict DAG-CBOR options), and the record points at the CAR's root.
//
// Usage: node checks/channel_interop.mjs   (runs the Rust example first)
import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { unmarshalIPNSRecord } from 'ipns';
import { validate } from 'ipns/validator';
import { publicKeyFromProtobuf } from '@libp2p/crypto/keys';
import { CID } from 'multiformats/cid';
import { base36 } from 'multiformats/bases/base36';
import { sha256 } from 'multiformats/hashes/sha2';
import { decode as cborDecode } from 'cborg';
import { ROOT, check, finish } from './e2e_lib.mjs';

const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'ephem-channel-'));
execFileSync('cargo', ['run', '-q', '-p', 'ephem-channel', '--example', 'vectors', '--', dir], { cwd: ROOT, stdio: 'inherit' });
const name = fs.readFileSync(`${dir}/name.txt`, 'utf8');
const root = fs.readFileSync(`${dir}/root.txt`, 'utf8');
const record = new Uint8Array(fs.readFileSync(`${dir}/record.bin`));
const car = new Uint8Array(fs.readFileSync(`${dir}/channel.car`));

const nameCid = CID.parse(name, base36);
check('IPNS name is a libp2p-key CIDv1 (k51…)', nameCid.version === 1 && nameCid.code === 0x72 && name.startsWith('k51'), name);
const pub = publicKeyFromProtobuf(nameCid.multihash.digest);
try {
  await validate(pub, record);
  check('IPNS record validates with the js-ipns reference validator', true);
} catch (e) {
  check('IPNS record validates with the js-ipns reference validator', false, e.message);
}
const rec = unmarshalIPNSRecord(record);
check('record value points at the root', rec.value === `/ipfs/${root}`, rec.value);
check('record is V1+V2 (signatureV1, signatureV2, data)', rec.signatureV1 !== undefined && rec.signatureV2 !== undefined && rec.data !== undefined);

// CAR v1 by hand: varint header length, dag-cbor header, then varint(len) CID bytes.
function varint(buf, pos) {
  let v = 0, shift = 0, b;
  do { b = buf[pos++]; v += (b & 0x7f) * 2 ** shift; shift += 7; } while (b & 0x80);
  return [v, pos];
}
const tags = []; tags[42] = (decode) => CID.decode(decode().subarray(1)); // cborg 5: a tag decoder gets the inner decode function
const opts = { tags, strict: true, rejectDuplicateMapKeys: true, allowIndefinite: false, allowUndefined: false, allowNaN: false, allowInfinity: false };
let [hl, pos] = varint(car, 0);
const header = cborDecode(car.subarray(pos, pos + hl), opts);
pos += hl;
check('CAR header: version 1, one root = the record\'s root', header.version === 1 && header.roots.length === 1 && header.roots[0].toString() === root);
let ok = true, blocks = 0;
while (pos < car.length) {
  const [len, p] = varint(car, pos);
  const section = car.subarray(p, p + len);
  const cid = CID.decode(section.subarray(0, 36));
  const data = section.subarray(cid.bytes.length);
  const want = CID.createV1(cid.code, await sha256.digest(data));
  ok &&= want.equals(cid);
  try { cborDecode(data, opts); } catch (e) { ok = false; console.log('  cbor:', e.message); }
  blocks++;
  pos = p + len;
}
check('every CAR block hashes to its CID and is strict DAG-CBOR', ok, `${blocks} blocks`);
fs.rmSync(dir, { recursive: true });
finish();
