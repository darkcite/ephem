// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
// Small rendering helpers shared by the chat and channel lists (app.js, channels.js).

/** A row's avatar: the name's first letter on a colour from the name (an identicon, not an
 *  identity: the safety code is what proves who someone is). */
export function avatar(li, name) {
  const a = li.querySelector('.dot');
  let h = 0;
  for (const ch of name) h = (h * 31 + ch.codePointAt(0)) >>> 0;
  a.className = 'dot avatar';
  a.textContent = [...name.replace(/^anon_/, '')][0]?.toUpperCase() || '?';
  a.style.setProperty('--hue', String(h % 360));
}
