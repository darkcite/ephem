// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// E8 in your real browsers (macOS): opens the checks page in Chrome, Safari and Firefox
// (whichever are installed) in E8 mode and collects one result per browser.
// Usage: node e8_checks.mjs <out-dir>
import * as fs from 'node:fs';
import * as path from 'node:path';
import * as url from 'node:url';
import * as cp from 'node:child_process';
import * as mk from './make_web_config.mjs';
import * as sv from './serve.mjs';

const HERE = path.dirname(url.fileURLToPath(import.meta.url));
const OUT = process.argv[2] || path.join(HERE, 'out', 'e8');
fs.mkdirSync(OUT, { recursive: true });

const APPS = [['Google Chrome', 'mac-chrome'], ['Safari', 'mac-safari'], ['Firefox', 'mac-firefox']];
// E8_BROWSERS="Google Chrome,Firefox" limits which apps are opened.
const WANT = (process.env.E8_BROWSERS || '').split(',').map((x) => x.trim()).filter(Boolean);
const installed = process.platform === 'darwin'
  ? APPS.filter(([app]) => (!WANT.length || WANT.includes(app)) && cp.spawnSync('open', ['-Ra', app]).status === 0)
  : [];

const cfg = await mk.baseConfig({ label: 'desktop', net: false, autorun: false, interactive: true, resultUrl: 'result' });
delete cfg.ipnsName;
const got = new Map();
let allIn;
const allDone = new Promise((r) => { allIn = r; });
const { srv, target } = await sv.startServer(cfg, {
  onPartial: (res) => {
    for (const x of res) {
      if (x.id !== 'E8' || got.has(x.name)) continue;
      got.set(x.name, x);
      console.log(`[${x.status}] ${x.name}: max gap ${x.details.maxGapMs} ms over ${x.details.hiddenSeconds} s hidden`);
      if (installed.length && got.size >= installed.length) allIn();
    }
  },
});

if (!installed.length) {
  console.log(`Open ${target}?mode=e8&label=<browser-name> in each browser yourself.`);
} else {
  for (const [app, label] of installed) {
    cp.spawn('open', ['-a', app, `${target}?mode=e8&label=${label}`], { stdio: 'ignore' });
    await new Promise((r) => setTimeout(r, 1500));
  }
  console.log(`Opened the E8 page in: ${installed.map(([a]) => a).join(', ')}.`);
}
console.log('In EACH browser: switch to another tab (⌘T), wait at least 6 minutes, then come back to the E8 tab.');
console.log('Results appear here as each browser reports (Ctrl-C to stop early and keep what arrived).');

const finish = () => {
  srv.close();
  const results = [...got.values()];
  if (results.length) sv.writeReport(OUT, 'e8', results);
  else console.log('No E8 results received.');
  process.exit(results.length ? 0 : 1);
};
process.on('SIGINT', finish);
await Promise.race([allDone, new Promise((r) => setTimeout(r, 25 * 60 * 1000))]);
finish();
