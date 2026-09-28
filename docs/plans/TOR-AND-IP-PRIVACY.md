# Plan: Tor transport and IP-address privacy

| Field | Value |
|---|---|
| Status | **Adopted into SPEC v0.4** (§28 Tor mode, §29 IP privacy). The remaining choices are open questions QN in the chat of 2026-09-28 |
| Relates to | [`SPEC.md`](../spec/SPEC.md) v0.3: P2, P8, §9, §13, §21 |
| Constraint | Zero application infrastructure. Anything third-party must be free and need no registration |
| Date | 2026-09-28 |

---

## 1. Two facts that shape everything

1. **Direct P2P and hiding your IP from the peer contradict each other.** A direct UDP path *is* the two IP addresses. Every way of hiding an IP puts something in the middle: a VPN, Tor, or a relay. "Direct and hidden" does not exist, and the app MUST NOT claim it.
2. **WebRTC and Tor are incompatible.** Tor carries only TCP streams, and WebRTC data runs over UDP. Tor Browser and Onion Browser also **turn WebRTC off completely** (`media.peerconnection.enabled = false`), so the current app cannot run there at all. Tor support therefore means a **second transport**, not "WebRTC through Tor".

## 2. Options for hiding your IP

"Hides from" means: *from the peer* / *from your ISP or a network observer* / *from the STUN operator*.

| # | Option | From the peer | From the ISP | From STUN | Cost / registration | Desktop | iOS PWA | Latency | Status |
|---|---|---|---|---|---|---|---|---|---|
| H0 | **Default: direct WebRTC** | ✗ | ✗ (contents are encrypted, the IPs are visible) | ✗ | free | ✓ | ✓ | best | SPEC v0.3 |
| H1 | **LAN-only mode** (no STUN, mDNS only) | only a LAN address, same network only | ✓ (traffic stays local) | ✓ (STUN is not used) | free | ✓ | ✓ | best | SPEC §9.4 |
| H2 | **The user's own VPN** with UDP support, full tunnel, set up outside the app | ✓ (the peer sees the VPN exit) | ✓ (the ISP sees that a VPN is used) | ✓ | depends on the VPN. **Cloudflare WARP** (1.1.1.1 app) is free and needs no account | ✓ | ✓ | +5–50 ms | Documented and checked in the app (§3) |
| H3 | **Tor mode, through a companion** (§4) | ✓ | ✓ (with bridges, even Tor use is hidden) | ✓ (no STUN) | free (volunteer network) | ✓ | ✗ | +300–1500 ms | Proposed |
| H4 | **Tor mode, fully in the browser** (§5) | ✓ | ✓ | ✓ | free | ✓? | ✗ (killed in the background) | +300–1500 ms | Research |
| H5 | **Forwarding by a trusted room member** (deferred) | from other members only, not from the forwarder | ✗ | ✗ | free | ✓ | ✓ | +1 hop | Deferred (SPEC Q6) |
| ✗ | TURN relay | would hide the IP | — | — | needs infrastructure | — | — | — | **Forbidden** (P8, §4.3) |
| ✗ | iCloud Private Relay | **does not** cover WebRTC UDP. It only covers Safari HTTP traffic | — | — | — | — | — | — | Not an option; the UI must say so |
| ✗ | Chrome's `disable_non_proxied_udp` policy | forces WebRTC through a proxy, which needs TURN-TCP (forbidden), so connections fail | — | — | — | — | — | — | Not an option |

**Recommendation.**

- **iOS:** H2 is the only practical way to hide your IP (WARP, or any VPN that carries UDP). Tor mode is not available on iOS.
- **Desktop:** H2 for low latency, or H3 for real anonymity.

## 3. Changes to the SPEC even without Tor (small, recommended for MVP-1)

1. **"What your peer sees" panel** in the diagnostics. It shows the local srflx address, which is exactly what the peer sees, with the note *"Your peer can see this IP address."* Users on a VPN can check at a glance that it shows the VPN's exit address, not their home address.
2. **Split-tunnel leak warning.** If both a srflx-v4 and a srflx-v6 candidate were gathered and they belong to different networks (for example v6 goes around a v4-only VPN), the app warns: *"IPv6 bypasses your VPN."* It also offers to drop IPv6 candidates for this session.
3. **Group disclosure.** Before joining a room: *"All N members will see your IP address."*
4. **Allowed-claims update (§21):** *"The app can use your VPN or Tor. It cannot hide your IP address from a peer on its own over a direct connection."*

## 4. Tor mode through a companion (proposed, desktop)

### 4.1 Shape

```
 PWA (browser)                         p2pchat-companion (native, Rust, open source, user-run)
 ┌───────────────────┐  ws://127.0.0.1  ┌─────────────────────────────────────────────┐
 │ core (sans-IO)    │ ◀──────────────▶ │ embedded **arti** (Tor Project's Rust Tor)  │
 │ Noise, rooms, UI  │  token-scoped,   │  - hosts an onion service for this identity │
 │ transport = Tor   │  origin-checked  │  - dials peers' .onion addresses            │
 └───────────────────┘                  │  - optional bridges: obfs4, Snowflake,      │
                                        │    WebTunnel (all free, built-in lists)     │
                                        └──────────────────────┬──────────────────────┘
                                                               │ Tor network (volunteer relays)
                                                               v
                                                      peer's onion service
```

- The **companion** is a single static binary (Linux, macOS, Windows), built from this repository and published as a GitHub Release. It embeds arti, so there is **no separate Tor install** and no configuration.
- It is not infrastructure. It runs on the user's own device, like Kubo in the public-channel plan.
- **The PWA stays the source of truth** for all protocol state and cryptography. The companion only moves opaque bytes: it cannot read Noise frames.
- **Local security:**
  - The WebSocket listener binds to `127.0.0.1` only.
  - It checks the `Origin` header against the app's origin.
  - It requires a 32-byte token, which the user pastes into the PWA once; the PWA stores it in the encrypted key file (SPEC §7.3).
  - The browser asks for Local Network Access permission the first time.

### 4.2 What Tor mode changes in the protocol

| Aspect | WebRTC mode (SPEC v0.3) | Tor mode |
|---|---|---|
| Reachability | ICE candidates (IP and port) | The peer's **onion address**: 32-byte Ed25519 key plus a port |
| Bootstrap | Always two-way (invite → answer) | **One-way is possible.** Bob dials Alice's onion from the invite, and no answer is needed. This is the "single QR" from v0.1 §16, which becomes real under Tor |
| Code | `INVITE` (§8.3) | New `kind = 5 TOR_INVITE`: `invite_id`, `room_id`, `static_pk`, `onion_pk` (32), `port` (u16), `expires_at`. About 104 B, which is a smaller QR |
| Handshake | Noise KK (both static keys known) | **Noise IK.** Bob knows Alice's static key from the invite, and Alice learns Bob's inside the handshake. The prologue is the invite bytes. The SAS is **always prompted**, because Alice has no out-of-band proof of Bob's key |
| Framing | DataChannel messages keep their boundaries | Byte stream: each frame is a `u16` length followed by the frame (§11.1), still ≤ 16 KiB |
| Reconnect / Wi-Fi change | Recovery ladder T0–T3 | **Trivial.** The onion address does not depend on the network, so the peer simply dials again. With saved identities, **contacts can reconnect at any time without a new QR** (see the qTox plan, §3.2) |
| Groups | Mesh of WebRTC links | Mesh of onion streams. Every member hosts an onion service. Allowed up to 16, but slower |
| UI indicator | `DIRECT P2P` | **`VIA TOR · IP hidden`** (never "direct") |
| Latency | 10–100 ms | 300–1500 ms per message (onion to onion is 6 hops). Fine for text; unusable for voice and video |

### 4.3 Principle amendments this needs (only if approved)

- **P2 / P8:** *"Tor mode is an explicit, user-selected anonymity transport. It routes through volunteer Tor relays by design. It is never an automatic fallback."*
- **No mixing:** while Tor mode is on, the app MUST NOT create any `RTCPeerConnection` and MUST NOT contact STUN, because either would leak the IP. There is **never a silent fallback** from Tor to direct: if Tor fails, the connection fails with `E_TOR_UNAVAILABLE` (a new code).
- **Onion address:** `onion_seed = HKDF(identity_seed, "p2pchat/onion")`. It is stable for saved identities and temporary for temporary identities.

### 4.4 Phases

| Phase | Scope |
|---|---|
| TOR-0 (spikes) | See §6 |
| TOR-1 | Companion: arti client plus onion-service hosting, the WebSocket bridge, token and Origin checks, built-in bridge lists; releases for 3 operating systems |
| TOR-2 | PWA: a `Transport` variant in `core` (WebRTC or Tor), `TOR_INVITE`, Noise IK, stream framing, the "VIA TOR" UI, and the no-mixing guard |
| TOR-3 | Contacts reconnect through stable onion addresses (depends on saved identities and the contacts feature in the qTox plan) |
| TOR-4 | Groups over Tor |

## 5. Fully in-browser Tor (research track, not proposed for delivery)

- **How it would work:** a Tor client compiled to WASM. arti is the natural choice, and a TypeScript client (Echalote) also exists as a community experiment. A browser cannot open TCP connections to Tor relays, so it would reach Tor through **WebSocket-capable bridges** (WebTunnel) or **Snowflake**. Both are free.
- **Blockers:**
  1. Hosting an onion service from a browser tab needs long-lived circuits to introduction points. It dies when the tab closes, and iOS suspends it within seconds.
  2. WASM support in arti is not an official target today *(spike TS1)*.
  3. The WASM binary grows by several MB.
  4. Snowflake depends on the Tor Project's broker (free, but a dependency).
- **What might be feasible later:** *client-only* Tor in the browser, dialling a peer that runs the companion. For example, an iPhone user connects anonymously to a desktop friend. Only the side that hosts the onion service needs the companion.

## 6. Spikes

| ID | Question |
|---|---|
| TS1 | Can current arti host an onion service reliably when embedded, and what is its onion-to-onion time to first message and RTT? |
| TS2 | Can the PWA reach `ws://127.0.0.1` in Chrome and Edge (Local Network Access prompt), Firefox, and desktop Safari? This is shared with spike P2 of the public-channel plan |
| TS3 | Does WebRTC work through Cloudflare WARP and 2–3 common VPNs on desktop and iOS, and is the srflx address the VPN's exit? |
| TS4 | How often does IPv6 bypass a v4-only VPN in practice (the §3.2 leak check)? |
| TS5 | Build size of an arti WASM client (research only) |

## 7. Security notes

- **Tor hides:** your IP from the peer and from network observers. With bridges, it also hides the fact that you use Tor.
- **Tor does not hide:**
  - what you type: anything identifying in messages or nicknames;
  - correlation of timing across a long session by a global observer;
  - linking of sessions when a **saved** identity is reused, because the onion address stays the same.
- **The companion is part of the trust base (P9):** reproducible builds, hashes published in the Release, and it cannot read message contents.
- **Local attackers:** another process on the same machine can connect to `127.0.0.1`. The token plus the Origin check stops this; without the token, the companion answers nothing.

## 8. Recommendation

1. **MVP-1:** add §3 (the "what your peer sees" panel, the leak warning, the disclosures). These are cheap, honest, and they make H2 (VPN or WARP) safe to recommend.
2. **After MVP-2:** build Tor mode through the companion (TOR-1 and TOR-2) as an opt-in desktop feature.
3. **Keep in-browser Tor as research.** Do not promise Tor on iOS.

## 9. Decisions needed from the owner

| # | Question | Proposed default |
|---|---|---|
| T1 | Is an **optional native companion** (desktop only) acceptable for Tor mode? | Yes |
| T2 | Is Tor-only on desktop acceptable, with no Tor on iOS (VPN or WARP recommended there)? | Yes |
| T3 | Should Tor mode be able to **skip the answer step** (a one-way invite)? The SAS is then always prompted | Yes |
| T4 | Should the built-in bridges (obfs4, Snowflake, WebTunnel) be on by default, or only on request? | On request ("Tor is blocked on my network") |
| T5 | Should groups over Tor be allowed? | Later (TOR-4) |
| T6 | Should §3 (the IP disclosure UI) go into MVP-1? | Yes |
