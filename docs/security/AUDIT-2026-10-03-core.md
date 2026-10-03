<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->

# Security audit 2026-10-03: crypto, protocol, core, wasm adapter

## Status after the fixes

| ID | Severity | Status (2026-10-03) |
|---|---|---|
| F-01 | high | **Open**: needs an invite/answer format change (a hash of the answerer's ephemeral key in the answer code); the owner decides first |
| F-02 | high | **Fixed**: `Privacy::keeps_remote` filters the peer's candidates (LAN only: mDNS and private/link-local hosts; Drop IPv6: no v6) in invites, answers and in-band restarts; a direct invite opened from a link asks before connecting. Test: `crates/core/tests/security_regressions.rs` |
| F-03 | medium | Open |
| F-04 | medium | **Fixed**: `card::valid_nick` (no control, invisible, direction or check-mark characters) for HELLO, own nickname and contacts; older saved names that break it are dropped, not the key file. Tests: `security_regressions.rs`, `card.rs` |
| F-05 | medium | Open |
| F-06 – F-10 | low/info | Open |


## Scope and method

**Slice audited (one of three parallel audits).** `crates/crypto`, `crates/proto`, `crates/core`,
and `crates/wasm` (`lib.rs`, `rtc.rs`, `room.rs`, `qr.rs`, plus the frame and stream parsers in
`tor.rs`). Also the parsers of invites, answers, resume codes, contact cards, `#…` fragments, SDP,
key files and identity-transfer blobs, and the way `app/app.js` passes untrusted strings into the
wasm API and renders what comes back. Tor and Snowflake internals, channels, the vault, the
service worker and CSP belong to the sibling reports.

**Read first.** docs/P2P-CHAT.md §2, §5, §7, §9, §10, §11, §12, §16, §19–§21, §29;
docs/CONTACTS-UX.md §3.2.

**Ran.**
- `cargo test -p ephem-proto -p ephem-crypto -p ephem-core --offline`: all pass.
- A scratch crate outside the repo, with path dependencies on the three crates:
  `/tmp/claude-0/-home-user-p2p-chat/d0942418-921d-59db-bbeb-333e40bf79dc/scratchpad/audit-core/`.
  It has 7 proof tests: `tests/audit.rs` (5), `tests/sas_mitm.rs` (1), `tests/qrfuzz.rs` (1).
  All of them pass, which confirms the findings.
- A mutation fuzz of the QR decoder (rqrr 0.9.3) on corrupted QR images: 3 000 iterations in a
  release build, no panic.

**Could not run.**
- A real browser. The browser half of F-02 (that ICE connectivity checks go to the remote
  candidates as soon as the descriptions are set) follows from RFC 8445 behaviour and was not
  observed in Chrome here.
- `cargo audit` and `cargo deny`: not installed.
- `cargo fuzz`: no nightly toolchain.
- The E2E suites under `checks/`: not needed for these findings.

## Summary table

| ID | Severity | Area | Title | Status |
|---|---|---|---|---|
| F-01 | **high** | crypto / SAS | The 6-digit SAS can be matched by an out-of-band MITM who grinds the ephemeral of the handshake it answers (no commitment) | confirmed (test) |
| F-02 | **high** | core / rtc | Remote candidates ignore the local privacy mode: LAN-only and "Drop IPv6" still send ICE checks to the peer's public and IPv6 addresses. Opening an `#i=` link leaks the IP before any answer is sent | confirmed (core test) + browser behaviour by trace |
| F-03 | medium | core / wasm / app | A RESUME_INVITE forged from public values drops a *connected* chat's path and redirects it. It is auto-applied through the tab hand-off | confirmed (test) |
| F-04 | medium | core / wasm / app | HELLO nicknames accept `\t`/`\n` and are written unescaped into tab/line-separated API rows: forged room-member rows (name, handle, role, SAS) and forged contact rows | confirmed (test + trace) |
| F-05 | medium | crypto | The contact fingerprint is 12 digits (~40 bits) with no key stretching: a second preimage against a known pair is feasible offline | confirmed by analysis (not ground) |
| F-06 | low | crypto | A key file with non-default Argon2 parameters opens, but its re-save writes the default parameters with the old key, so the file never opens again. `open` also accepts up to 256 MiB × t16 × p4 | confirmed (test) |
| F-07 | low | core | IDENTITY_CHUNK has no `idx < total` check: chunks past the end are appended (`xfer` grows past `MAX_IDENTITY`) | confirmed (test) |
| F-08 | low | wasm / app | `resave_identity` returns an empty blob when the body exceeds 64 KiB (`set_section` allows 65 535 B). `persist()` and `downloadBackup()` then do nothing, silently | confirmed by trace |
| F-09 | low | crypto | A contact card can attach an attacker's onion key to an existing contact that has none | confirmed by trace |
| F-10 | info | core | No handshake timeout in `Connecting`, which makes F-01 easier | confirmed by trace |

Counts: critical 0, high 2, medium 3, low 4, info 1.

## Findings

### F-01 (high): the SAS can be matched by a grinding MITM

**Description.** `Sas::from_handshake_hash` (`crates/crypto/src/sas.rs:22-30`) derives the
6 digits (`% 1_000_000`, about 20 bits) and the 4 emoji (32 bits) from the Noise handshake hash.
Nothing commits either side to its ephemeral key before it sees the other side's. Consider an
out-of-band MITM, Mallory, who swapped the codes (the exact attacker §21 assigns to the SAS). She
runs two handshakes:

- Handshake B, Mallory → Bob: Mallory is the KK initiator and Bob answers last, so `h_B` and
  Bob's SAS are fixed once it completes.
- Handshake A, Alice → Mallory: Mallory is the responder. She receives Alice's `e` and only then
  picks her own `e` for message 2 (`e, ee, se`).

So Mallory can try ephemerals until `SAS_A.digits == SAS_B.digits`. That takes about 10⁶ tries.
The Tor IK flow has the same shape: the swapped invite makes Mallory the IK responder.

Nothing in `Session` times out a path in `Connecting` (`session.rs` `tick` returns for that
state, F-10), so Alice simply waits.

**Evidence.**
- `crates/crypto/src/sas.rs:28` (20-bit digits).
- `crates/crypto/src/noise.rs:121-130` (the hash is taken after message 2, which the responder
  controls).
- `app/index.html:438`: "Compare it with your peer on another channel, for example a call". Over
  a call, people read the digits.

**Reproduction.** Run `tests/sas_mitm.rs` (release build, 4 threads). Real `ephem_crypto`
handshakes run for Alice and Bob, and snow with a fixed ephemeral runs for Mallory. Result:
`tries 2416537 in 332s: Alice sees "691 132", Bob sees "691 132"`. The emoji differ. This grinder
is unoptimised: it rebuilds the whole snow state on every try. A grinder that precomputes the
message-1 read and adds the base point incrementally is at least 10× faster, and a GPU brings it
to seconds.

**Impact.** A full plaintext MITM of a 1:1 chat, a room-owner link, an identity transfer (§7.6:
the encrypted key file goes to Mallory, who can then guess the passphrase offline) or a Tor chat.
The users compared the safety code and saw it match.

**Fix proposal.**
1. Commit before revealing. The answer code already passes out of band and is bound into the
   prologue. Add `H(e_resp)` to it (the responder of the path is the answerer of KK; for IK, put
   it in the TOR_INVITE for the host), and check it in `Handshake::read` before message 2 is
   accepted.
2. Alternatively, keep the wire and make the SAS long enough that real-time grinding is out of
   reach. Show digits and emoji as one 52-bit code that must both be read, and add a handshake
   deadline in `Session::tick` for `Connecting` (for example 30 s).

Option 1 is the standard fix (ZRTP-style hash commitment) and costs 32 bytes per answer code.

### F-02 (high): remote candidates bypass LAN-only and Drop IPv6; opening an invite leaks the IP

**Description.** The privacy mode filters only the *local* candidates written into our code
(`Privacy::keeps`, `session.rs:101`; `build_code`). Remote candidates from the peer's code are
stored as received:

- `take_invite` copies them (`session.rs:431`);
- `apply_answer` copies them (`session.rs:841`);
- `render_signal` copies those of an in-band restart;
- `remote_sdp` (`session.rs:741`) then renders all of them.

`rtc::negotiate` applies the result with `set_remote_description` (`crates/wasm/src/rtc.rs:400`)
and then `set_local_description`. From that moment the answerer's ICE agent sends STUN binding
requests from its own sockets to every remote candidate. That includes public srflx and raw
IPv4/IPv6 candidates, whatever our mode is.

Three consequences:

1. **LAN-only** (§29.1: "Hides your IP from the peer ✓", "the peer sees only a LAN address"): a
   peer whose code carries a public srflx candidate receives checks from our NAT's public address.
2. **Drop IPv6** (the §29.2 fix for VPNs that cover only IPv4): a peer that offers an IPv6
   candidate receives checks from our global IPv6 address, which bypasses the VPN.
3. **Before consent.** `main()` applies `#i=` fragments as soon as the page loads
   (`app/app.js:2277`); a 1:1 invite gets no confirmation. Checks start while the user is still
   looking at the answer code. The inviter learns the opener's IP address even when no answer is
   ever sent back.

**Reproduction.** Run `tests/audit.rs::remote_candidates_ignore_local_privacy_mode`. Bob answers
with `Privacy::LanOnly` and `drop_ipv6 = true`. His remote SDP still contains
`198.51.100.7 40000 typ srflx`, `2001:db8::7 40001 typ srflx` and `203.0.113.5 40002 typ host`.

**Impact.** IP-address disclosure contrary to a documented guarantee, with a single user action
(opening a link). A public "invite" link becomes an IP grabber.

**Fix proposal.**
- In `Session`, filter remote candidates with the same mode when a code or signal is taken in:
  - add `fn keeps_remote(self, c, drop_ipv6)`;
  - in LAN-only, keep only `HostMdns` (and, if wanted, RFC 1918 / ULA host addresses);
  - with `drop_ipv6`, drop every v6 candidate.
  - Apply it in `take_invite`, `apply_answer` and `render_signal`. This is a fixed-size filter
    into `IceParams` and does not allocate.
- For point 3, call `createAnswer` on a remote description *without* candidates. Add the
  candidates (`addIceCandidate`) only after the user acts on the answer (copy, share or show the
  QR), and say in the UI that opening an invite reveals the IP address to its creator.

### F-03 (medium): a forged RESUME_INVITE hijacks a connected chat

**Description.** `accept_resume` (`session.rs:882-897`) checks three things: the kind, that the
chat ever connected and is not closed, that `room_id` matches, and that `static_pk` equals the
peer's key. All of these are public to anyone who saw the original invite. It does **not**
require the chat to be `Suspended`: it calls `drop_path()` on a `Connected` chat and points the
new path at the forger's ICE parameters.

In the app, a `#r=` link opened in a new tab is forwarded over the `p2pchat-codes`
BroadcastChannel. The tab holding the chat applies it when `code_fits` returns true, with no
prompt (`app/app.js:1782-1789`, `2280-2285`).

What happens next: the forger cannot complete Noise KK, so `on_handshake` fails and `fail()`
moves the chat to `Closed` (keys wiped, the pending ring lost). Or ICE never completes and the
chat sits in `Suspended`. Either way, the opener's IP goes to the forger's candidates (F-02).

**Reproduction.** Run `tests/audit.rs::forged_resume_invite_hijacks_live_path`. A connected Alice
accepts the forged code, her state becomes `Gathering`, and her remote SDP names
`198.51.100.66`.

**Impact.** A remote kill of a live chat and an IP leak, triggered by one link click.

**Fix proposal.**
- Accept a resume invite only in `Suspended`, or while `degraded`.
- Better, authenticate resume codes: exchange a 16-byte `resume_secret` inside the Noise channel
  (in HELLO), and require `invite_id = HMAC(resume_secret, nonce)`, or a tag over the code, in
  `accept_resume`.

### F-04 (medium): nickname row injection (room member list, contacts)

**Description.** The HELLO nickname is checked only for length and UTF-8
(`session.rs:1096`). The card path, by contrast, rejects control characters (`card::valid_nick`).
The adapter then writes nicknames unescaped into rows separated by tabs and new lines:

- `room::members` (`crates/wasm/src/room.rs:680`), stored from `Event::Hello` in
  `lib.rs` `on_event`;
- `App::contacts` (`lib.rs:831`). The 1:1 peer's HELLO nickname becomes the *local* contact
  nickname through `save_contact(c.peerNick)` (`app/app.js` `b-save-contact`, `answerCardRequest`
  and `fillContactNick`), although §7.5 says that name is "chosen by the user, not by the peer".

`app.js` splits on `\n` and `\t` (`977`, `1286`).

**Exploit.**
1. Room: a member sends the nickname `"\n3\t0\tanon_<owner handle>\tme\tBoss"` (24 B).
   `renderRoom` then gets an extra row. That row overwrites `c.names` for index 3 (for example
   the attacker's own index), so its messages show as `Boss (anon_<owner handle>)`. It can also
   fake the role, link status and the SAS shown to the owner.
2. Contacts: once saved, the nickname adds a row with any `hex`, `flags=1` (✔ verified) and any
   handle to the contact list.

DOM insertion uses `textContent`, so there is no XSS.

**Reproduction.** Run `tests/audit.rs::hello_nick_with_row_separators`. The core accepts the
nickname and the chat stays connected.

**Impact.** Integrity of the UI's identity anchors. A member can impersonate the owner or another
member inside a room. A peer can plant a fake "verified" row among the contacts.

(Handle collisions, 24 bits, are documented as "not authentication", but F-04 also forges role,
status and SAS, and it needs no grinding.)

**Fix proposal.**
- Apply `card::valid_nick` (no `char::is_control`) to HELLO nicknames in `on_transport`, to
  `Settings::set_nick` and to `Contact::set_nick`. Also reject the bidi and format characters
  U+200B–U+200F, U+202A–U+202E and U+2066–U+2069; that also closes the
  `impersonated()` bypass by a zero-width suffix.
- Do not save the peer's nickname as the local contact name without the user's confirmation.
- Longer term, return structured data (JSON via `serde`-free escaping, or several getters)
  instead of `\t`/`\n` rows.

### F-05 (medium): the contact fingerprint is short enough to grind

**Description.** `PeerId::fingerprint` (`identity.rs:31-38`) is
`BLAKE2s("ephem-contact-fp-v1" ‖ min ‖ max) mod 10¹²`, about 39.9 bits, with no stretching. It
exists to catch a key swapped through a card or a link (CONTACTS-UX §3.2, "verify without a
chat").

The attack: Mallory knows the victim's key and the real Alice's key (both are in shareable
cards). She searches for a key `K` with `fp(victim, K) = fp(victim, Alice)`. Using incremental
keys (`P + 8G`, which keeps the scalar clamped) and BLAKE2s, that is about 10¹² tries: around
30 CPU core-hours, or less than an hour on a GPU. She then plants `K` as "Alice" (a card with
Alice's nickname). When the victim and Alice compare the fingerprints in person, the 12 digits
match.

**Impact.** In-person verification of a swapped contact is defeated, and a later Tor contact dial
then reaches Mallory, verified and without a SAS prompt.

**Fix proposal.**
- Show at least 80 bits (for example 24 digits in 6 groups, or 12 digits plus emoji), and/or
  stretch the hash (Signal-style iterated hashing over both keys).
- Bump the domain tag to `-v2`.

### F-06 (low): the key-file re-save breaks files with non-default KDF parameters

**Description.** `keyfile::open` accepts `m ∈ 8..=256 MiB`, `t ∈ 1..=16`, `p ∈ 1..=4`
(`keyfile.rs:117`) and returns the key derived with *those* parameters (`:132`). `keyfile::seal`
always writes `M_MIB`/`T_COST`/`P_COST` (`:78-80`) next to that key.

**Reproduction.** Run `tests/audit.rs::resave_with_non_default_params_bricks_file`: a file with
m=8 and t=1 opens; after a re-save the header says 19/4/1 and the right passphrase fails.

Today no writer produces other parameters, so this is latent. It bites as soon as the defaults
change (§7.3 already mentions a t=2 spike), and the result is silent identity loss on the next
contacts change, because `persist()` overwrites the remembered slot.

Separately, a hostile blob, from a key file the user loads or an identity transfer (§7.6), can
demand 256 MiB × t16 × p4 of Argon2 in the tab. That is a bounded DoS of one's own tab.

**Fix.** Keep `(m, t, p)` in `Opened` and `Saved` and have `seal` write them. Better, re-derive
with the defaults on the first save after a parameter change. Lower the accepted maximum to what
a phone can do (for example m ≤ 64 MiB, p = 1).

### F-07 (low): identity chunks are accepted past `total`

**Description.** The IDENTITY_CHUNK check (`session.rs:1243-1251`) never tests `idx < total`.
After the last chunk, `xfer_next == total`, so a chunk with `idx == total` and the same `total`
passes and is appended. This repeats (the `u16` index wraps in release).

**Reproduction.** Run `tests/audit.rs::identity_chunks_past_total`: 40 chunks with `total = 1`
are all accepted (480 KiB against `MAX_IDENTITY = 72 KiB`), and the event fires once.

In the app the receiver closes the link in a microtask after IDENTITY_RECEIVED, so the effect is
bounded to one frame. That is why this is low. It is still an unbounded buffer in the core, which
§22 forbids.

**Fix.** Add `idx >= total` to the reject condition, and stop accepting chunks once
`xfer_next == xfer_total`.

### F-08 (low): a key-file re-save fails silently above 64 KiB

**Description.** `set_section` accepts a value of up to 65 535 B (`contacts.rs` `set_section`),
and the CONTACTS TLV can reach about 38 KB. `keyfile::seal` refuses a body over 64 KiB, and
`resave_identity` maps the error to an empty `Vec` (`lib.rs:746`). Then:

- `persist()` and `downloadBackup()` return without a message (`app/app.js:1432`, `1634`);
- the follow list or contact change is never saved;
- the "backup out of date" flag is not raised;
- an identity transfer fails with `E_KEYFILE_INVALID`.

This is user-driven (many followed channels with long titles), not attacker-driven.

**Fix.** Emit `ERROR(E_KEYFILE_INVALID)` from `resave_identity`. Bound each `set_section` call
against the space left in the body (`MAX_BODY - 33 - nick - tlv.len()`).

### F-09 (low): a card attaches an onion key to an existing contact

**Description.** `Contacts::add_from_card` (`contacts.rs`, "existing contact keeps its state")
sets `onion_pk` on an existing contact that has none. So a card carrying *Alice's* `peer_id` and
*Mallory's* onion key makes "Connect" to Alice dial Mallory's onion.

The IK handshake to Alice's static key fails, so there is no impersonation. Mallory does learn
when the victim tries to reach Alice (presence and the relationship), and the contact stays
unreachable.

**Fix.** Do not change an existing contact from a card, or ask the user first. Learn onion keys
only inside an authenticated Tor chat (`set_onion` after IK).

### F-10 (info): no deadline for the handshake

`Session::tick` returns for `Connecting`, so a peer that holds back Noise message 2 keeps the path
open for as long as ICE stays up. Add a 30 s handshake deadline; it also makes F-01 harder.

## Attack-surface inventory (this slice)

| Entry point | Parser / first use | Notes |
|---|---|---|
| `#i= #a= #r= #q= #t= #k=` fragments, paste, BroadcastChannel hand-off, QR | `extract_payload` → `b64url::decode` (into a `MAX_CODE_LEN` stack buffer) → `Code::decode` / `Card::decode` | Strict: exact lengths, reserved bits, no trailing bytes. b64url accepts non-canonical trailing bits (harmless: the decoded bytes are what the prologue binds). F-02, F-03 |
| Camera frames | `App::scan` → `scan_rgba` → rqrr | Fuzzed, no panic. `width*height*4` is a `u32` from JS (guarded by `videoWidth`) |
| SDP (local, from the browser) | `sdp::parse_local`, `parse_candidate`, `parse_fp` | Only UDP component 1 host/srflx; fails closed |
| Remote SDP (rendered by us) | `render_remote` into 2 KiB | Fixed template; ICE characters only; F-02 |
| DataChannel / Tor frames | `on_frame` (≤ 16 KiB) → `Header::read` → `Transport::open` (strict nonce) → `Records` | Every record checks its length; unknown non-ignorable record types close the link |
| HELLO, CHAT, EDIT, DELETE, REACT, ACK, READ, PING, TYPING, SIGNAL_*, ROOM_*, IDENTITY_* | `on_transport` | F-04, F-07; observer and owner rules enforced |
| Room state, sealed signals | `RoomState::verify`, `seal::open`, `signal_decode`, `room::on_state` / `on_signal` | Signature, owner binding, room id, version rollback check, sealed code `static_pk` equals the state key |
| Tor streams | `tor::incoming` / `reader` (u16 length ≤ 16 KiB, `RX_CAP`) → `tor_accept` (IK, invite_id or card secret, `allow`) | Bounded; unknown dialers dropped unanswered |
| Key files (file picker, text, IndexedDB, identity transfer) | `keyfile::open` → `Contacts::from_tlv` / `section` | AAD covers the whole header; F-06, F-08 |
| JS → wasm strings | `apply_code`, `add_card`, `card_nick`, `set_stun`, `set_section`, `set_nick`, `rename_contact`, `contact_connect` (hex) | All validated in Rust. `pass` buffers are wiped. Text buffer lengths are clamped to `MAX_TEXT` |
| wasm → JS strings | `contacts()`, `room_members()`, `peer_contact()`, `room_info()`, `diag()`, `bridges_check()` (JSON of static strings and hex) | F-04 (no escaping in the tab/line rows); the DOM uses `textContent` |

## What is solid

These are the controls tried and not broken.

- **Transport cipher.** The nonce must equal the header `seq` and be exactly `rx_n`; replay,
  reordering and tampering of the header (which is the AAD) fail. Rekey follows Noise exactly.
  `tx_n == u64::MAX` is guarded. Keys are zeroized on drop and on close.
- **Noise binding.** KK binds both full codes as prologue, and IK binds the Tor invite. Changing
  one byte of a code fails the handshake (the existing test, re-checked).
- **Strict decoders.** Invite, answer and card decoders reject truncation, trailing bytes, unknown
  kinds, reserved flags, out-of-range credential lengths, port 0, relay and TCP candidates, and
  duplicate candidates. No panics on the paths read; release `panic = "abort"` was considered for
  every `expect` in non-test code (all are infallible).
- **Room authority.** Ed25519 `verify_strict` over a domain-separated state. Indices must be
  unique and sorted. The owner must be at index 0 and match the link's static key and HELLO key.
  The room id is checked by the adapter. Versions only increase. Observers are dropped at every
  receiver. Only the owner's link can carry ROOM_STATE. Sealed signals are bound to
  `room ‖ from ‖ to`, and the relayed code's static key must equal the signed state's key.
- **Identity transfer gating.** Only on a TRANSFER link, receiver side only, after its own SAS
  confirmation. The UI asks before answering a TRANSFER invite. Only a saved identity is sent.
- **Key file.** XChaCha20-Poly1305 with every outer byte as AAD. A wrong passphrase and a damaged
  file give the same error. Contacts TLV duplicates and over-capacity are rejected.
- **Re-entrancy.** Event handlers in `app.js` defer every call back into `App` (`later` /
  `queueMicrotask`); the synchronous calls found are all user-triggered.
- **No `unsafe`** in this slice. No logging of secrets.

## Recommended next audits

1. After F-01 is fixed: a formal check (Tamarin or ProVerif) of KK/IK with the code commitment,
   including the T3 resume and the room T2 paths.
2. A browser-level test (Playwright in `checks/`) that asserts no STUN packets reach a
   non-mDNS remote candidate in LAN-only or with Drop IPv6 (regression test for F-02).
3. Coverage-guided fuzzing (`cargo fuzz`) of `Code::decode`, `Card::decode`, `on_transport`
   record sequences, `RoomState::verify` and `keyfile::open` + `Contacts::from_tlv`.
4. A review of UI identity anchors: handles are 24 bits (cheap collisions). Consider showing
   the contact fingerprint, or a longer handle, wherever a name has to be trusted.
