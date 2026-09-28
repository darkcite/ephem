# Plan: What to reuse from qTox/Tox and Telegram under a zero-infrastructure design

| Field | Value |
|---|---|
| Status | **Adopted into SPEC v0.4** (§7.2–§7.6, §11.2, §11.3, §11.7, §14.2). The remaining choices are open questions QN in the chat of 2026-09-28 |
| Relates to | [`SPEC.md`](../spec/SPEC.md) v0.3, [`TOR-AND-IP-PRIVACY.md`](TOR-AND-IP-PRIVACY.md), [`PUBLIC-CHANNELS-IPFS.md`](PUBLIC-CHANNELS-IPFS.md) |
| Rule | A feature is adopted only if it works with **no application server**. It may use the peers' own devices and free, no-registration third-party services (STUN, public IPFS gateways, Tor) |
| Date | 2026-09-28 |

Each feature below gets one of four verdicts:

- **ADOPT**: fits as it is.
- **ADAPT**: fits, but with changes, which are stated.
- **LATER**: fits, but belongs to a later phase.
- **REJECT**: needs infrastructure, or contradicts a principle.

---

## 1. Lessons from Tox / qTox

Tox is the closest ancestor of this project: serverless, identity based on a public key, and end-to-end encrypted.

### 1.1 What Tox got right (adopt)

| Tox / qTox | Here | Verdict |
|---|---|---|
| **Identity is a key pair; the Tox ID is the public key** | Already: `PeerId` = X25519 public key (SPEC §7.1) | ADOPT (done) |
| **Encrypted profile file protected by a passphrase** (`.tox`) | Already: `.p2pkey` file with Argon2id and XChaCha20-Poly1305 (SPEC §7.3) | ADOPT (done) |
| **Friend list of verified keys** | New: an optional **contacts** list inside the encrypted key file, only for saved identities (§3.1) | ADAPT |
| **Groups with roles** (NGC: founder, moderator, user, observer) | Our owner model (SPEC §14.2). Add an **observer** (read-only member) role | ADAPT (MVP-3) |
| **Typing indicator, read receipts, nickname and status message** | Inner records over Noise (§3.3) | ADOPT |
| **File transfer that can resume** | LATER (a separate DataChannel with chunking; SPEC §23) | LATER |

### 1.2 What Tox struggled with (avoid)

| Tox problem | Root cause | Our position |
|---|---|---|
| Friend discovery needs a **UDP DHT and bootstrap nodes** | Browsers have no UDP sockets and cannot join a DHT | **REJECT** DHT discovery. The out-of-band exchange stays (SPEC §8). Reconnecting *known* contacts without a QR is possible only through Tor onion addresses (§3.2) |
| NAT traversal fails often, so Tox falls back to **TCP relay nodes** | A relay is infrastructure | **REJECT.** No relays (P8). The failure is shown explicitly, and Tor mode is offered instead |
| Battery drain on mobile from staying in the DHT | Always-on DHT participation | Avoided: no DHT |
| No offline messages | No mailbox | **Same by design** (SPEC §17.4). We adopt qTox's **"pending until the peer reconnects"** queue, in RAM, while the app is open (§3.4) |
| No multiple devices | The key lives on one device | **ADAPT:** move the identity to another device over a P2P link (§3.5) |
| Friend-request spam (Tox added a "nospam" value) | Public IDs let anyone send requests | Not applicable: nobody can contact you without an invite. Tor mode would bring the problem back, so onion contacts accept **only keys already in contacts** (§3.2) |

## 2. Lessons from Telegram

Telegram gets its UX from a central cloud. We take the **interaction patterns**, not the architecture.

| Telegram feature | Zero-infrastructure version | Verdict |
|---|---|---|
| **Secret chats**: end-to-end, tied to the device, **emoji key visualisation** | That is our whole model. The SAS already shows 4 emoji (SPEC §10.4) | ADOPT (done) |
| **Self-destruct timer** for messages | A per-message TTL in the CHAT record. Both UIs remove the message from RAM and the DOM when it expires. It cannot stop screenshots or copying, and the UI says so | ADOPT (MVP-1) |
| **Reply, edit, delete for everyone** | New inner records EDIT and DELETE that name `(sender, chat_seq)`. They only reach peers who are connected; nothing is stored anywhere | ADOPT (MVP-1) |
| **Reactions** | A REACT record: target, plus one emoji of 1–8 bytes | ADOPT (MVP-2) |
| **Typing indicator, "double ticks" read status** | TYPING and READ records (ACK already exists) | ADOPT (MVP-1) |
| **Markdown-style formatting, mentions** | Rendered on the receiving side from a small safe subset. No HTML is ever sent | ADOPT (MVP-2) |
| **Pinned message** | A PIN record from the owner (rooms) or either side (1:1), kept in RAM | ADOPT (MVP-3) |
| **Search** | In-RAM search over the current session only | ADOPT (MVP-2) |
| **Channels**: owner posts, subscribers read | Exactly our **public channels** plan on IPFS | ADAPT (separate plan) |
| **Channel comments / discussion groups** | They would break read-only. Replying to the owner privately is possible through a normal P2P invite | REJECT (comments), ADOPT (a "Message the owner" link that carries a normal invite, if the owner publishes one) |
| **Invite links with expiry and member limit** | Our invites have a TTL and are single-use (SPEC §8.6). Add a **batch of N invites** for rooms (one QR each, or a list of links) | ADAPT (MVP-3) |
| **QR login to link a device** | Identity transfer over P2P (§3.5) | ADAPT (MVP-2) |
| **Several accounts** | Several key files; switch at sign-in | ADOPT (MVP-2) |
| **Voice messages, stickers, media** | They need file transfer | LATER |
| **Link previews** | Fetching a preview reveals **the fetcher's IP address** to the linked site. Telegram's secret chats warn about exactly this. Off, and never automatic | REJECT (default); a per-link "Load preview" click, never automatic |
| **Cloud history, synced across devices** | Needs a server | REJECT (P7) |
| **Usernames and global search** | Needs a directory | REJECT |
| **View counters on channels** | Needs a server to count | REJECT |
| **Push notifications** | Needs a push server | REJECT. Local `Notification` API only while the app is open or in the foreground |
| **Bots and mini-apps** | Server-side code | REJECT |

## 3. Proposed new features (details)

### 3.1 Contacts (saved identities only)

- **Where they live:** the encrypted key file (SPEC §7.3) gets an optional list of up to 256 contacts. Each contact has:
  - `PeerId` (32 bytes);
  - a local nickname (≤ 32 bytes);
  - a `verified` flag, set when the SAS was compared;
  - an optional `onion_pk` (32 bytes, Tor mode);
  - `added_at`.
- The list is written to the file only when the user saves or exports. It is **never** written in plain text.
- **What contacts give you:**
  - "You connected to **Alice (verified)**" instead of an anonymous handle;
  - a warning when a known nickname comes with a **different** key (like Signal's safety-number change);
  - no SAS needed with a verified contact after an out-of-band exchange.
- **Privacy:** contacts record who you talk to. The feature is off for temporary identities, and the UI says what gets stored.

### 3.2 Reconnecting contacts without a QR (Tor mode only)

- With saved identities and Tor mode (TOR plan §4), each contact has a **stable onion address**.
- "Call Alice" dials her onion address directly: no QR, no link, working from any network. This is the thing Tox's DHT promised, done here with Tor's free, volunteer infrastructure instead of our own.
- An onion service accepts Noise handshakes **only from static keys in its contacts**. Everything else is dropped, which prevents spam.
- **Not possible over WebRTC**: without a signalling channel, two peers cannot find each other after both have changed networks (SPEC REVIEW R1 and R2).

### 3.3 New inner records (SPEC §11.2 additions)

| `rtype` | Record | Body |
|---|---|---|
| 0x08 | TYPING | `u8` state (0 = stopped, 1 = typing). At most one every 3 s |
| 0x09 | READ | `chat_seq u64` (cumulative) |
| 0x0A | EDIT | `target_seq u64`, new UTF-8 text |
| 0x0B | DELETE | `target_seq u64` |
| 0x0C | REACT | `target_sender u8`, `target_seq u64`, emoji `len u8` + 1–8 bytes |
| 0x0D | STATUS | `u8` presence, status text of up to 64 bytes |
| CHAT flag | `rflags.bit1` = has TTL | a `u32 ttl_s` prefix: the self-destruct timer |

All of these are best-effort, live only in RAM, and reach only connected peers.

### 3.4 Pending queue (qTox's "will send when online")

- A message typed while the peer is `SUSPENDED` goes into the existing resend ring (SPEC §11.3, 256 entries) and is marked **pending**.
- It is sent after recovery (T0–T3), or through Tor when the contact comes back.
- If the tab closes, the message is lost, and the UI says so ("pending messages exist only in this tab").

### 3.5 Moving an identity to another device (Telegram-style QR login, without a cloud)

1. The new device shows an invite QR.
2. The old device scans it, and the two connect P2P with the normal flow and a mandatory SAS.
3. The old device sends the **encrypted** key file (seed plus contacts) over Noise. The user types the passphrase on the new device.
4. There is no sync afterwards: each device is an independent copy. Using the same identity on two devices **at the same time** is discouraged, because two sessions would claim the same `PeerId` and `chat_seq` would conflict. The first version refuses a second concurrent session for the same `PeerId` in a room, with `E_DUPLICATE_SESSION`.

## 4. Where each feature lands

| Phase | Additions from this plan |
|---|---|
| MVP-1 | TYPING, READ, EDIT, DELETE, self-destruct TTL, pending queue |
| MVP-2 | Contacts (saved identities), identity transfer, several identities, REACT, formatting, in-RAM search, STATUS |
| MVP-3 | Observer role, PIN, batches of N invites |
| Tor track | Reconnecting contacts through onion addresses |
| Later | File transfer, voice messages, media |

## 5. What stays rejected, and why

| Rejected | Needs |
|---|---|
| Cloud history and sync, offline mailbox | A server (P7, §4.3) |
| DHT discovery (Tox style) | UDP sockets or a DHT inside the browser, plus bootstrap infrastructure |
| TCP relays or TURN | A relay (P8) |
| Usernames, global search, view counters | A directory or counter service |
| Push notifications, bots | A server |
| Automatic link previews | They reveal the IP address to third parties |

## 6. Decisions needed from the owner

| # | Question | Proposed default |
|---|---|---|
| F1 | Should a **contacts list** be stored in the encrypted key file (saved identities only)? | Yes, opt-in |
| F2 | Self-destruct timers, edit and delete in MVP-1? | Yes |
| F3 | Should typing and read receipts be on by default, or opt-in (they reveal activity)? | On in 1:1, off in rooms, with a toggle |
| F4 | Identity transfer between devices (§3.5) in MVP-2? | Yes |
| F5 | Link previews: never, or click-to-load with an IP warning? | Never in MVP; click-to-load later |
| F6 | Is the observer (read-only member) role wanted for rooms? | Yes (MVP-3) |
