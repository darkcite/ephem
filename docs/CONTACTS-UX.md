<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
# Contacts: one coherent place in the UI (proposal, not built)

**Status:** proposal, 2026-09-29. It rearranges what exists (§7 identity, §28.7 Tor contacts,
contact cards, Appendix F tabs); the protocol and the key file do not change. Decisions for the
owner are in §8.

## 1. Where contacts live today (and why it feels scattered)

| Task | Where it is now | Problem |
|---|---|---|
| See my contacts | Settings → Contacts card; in Tor mode also at the bottom of the Chats list (only those with an onion key) | Two lists that differ; direct mode shows none in Chats |
| Talk to a contact | Chats → a contact row → Connect (Tor only); direct mode: no action at all | The main reason to have a contact is hidden in Settings or missing |
| Add a contact | Settings → Contacts → paste a card; the ⌗ Code sheet; "＋ contact" in a chat header; "Accept and add" when a card holder dials in | Four entry points with four wordings |
| Share my card | Settings → Your identity → "My contact card"; "Reset my contact card" beside it | Sharing is a daily action buried in identity management |
| Rename, remove | `prompt()` / `confirm()` browser dialogs | Look foreign, break the PWA feel, no undo |
| Is this contact verified? | A ✔ after the name | No way to see or compare the safety code outside a live chat |
| A temporary identity | The Contacts card is simply hidden | Nothing says why, or how to get contacts |

## 2. Principles

1. **People are a destination, settings are not.** Everything you do *with* a person starts from
   the Chats tab; Settings keeps only what configures *you*.
2. **One row, one person.** An open chat and a contact are the same person: one row with its
   state (chatting, reachable through Tor, not connected), never two rows.
3. **One way to add, reachable from everywhere.** "Add" always means the same sheet: scan or
   paste a card, or share mine. The ⌗ Code button already reaches it.
4. **The primary action is on the row; the rest is one tap away** (the person pane).
5. **No browser dialogs:** inline editing, and undo instead of "are you sure?" (except for
   resetting my card, which cannot be undone).
6. **Honest state:** say what a contact can do in this mode (Tor: connect directly; direct mode:
   a new chat still needs an invite, the contact only pins the key and name).

## 3. The arrangement

### 3.1 Chats tab: people first

```
┌ Chats ───────────────────────── [＋] ┐   [＋] opens: New chat · Add contact · New room
│ 🔍 Search people and chats            │   (search appears from 8 rows)
│ ● Bob            typing…         2   │   open chats, most recent first
│ ● Room: ops      Carol: ok           │
│ ─ Contacts ─────────────────────────  │
│ ○ Alice ✔        Connect through Tor │   Tor: tap = connect (as today)
│ ○ Dave           Invite              │   direct: tap = person pane, "Invite" preselected
│ ─────────────────────────────────────│
│ Your card: [Share] [QR]              │   when there are fewer than 3 contacts
└──────────────────────────────────────┘
```

- A contact with an open chat appears once, in the chats part (its row gains the ✔ and name).
- The contact section shows in **both modes**; in direct mode the row action is "Invite" (a
  normal invite, the chat then recognises the pinned key: name, ✔, and a warning if the key
  changed).
- Empty state (no chats, no contacts): the centred "＋ New chat" button (as now) plus "Share
  your card" and "Add a contact".
- Temporary identity: the contact section reads "Contacts are kept in your saved identity.
  [Save identity]" instead of disappearing.

### 3.2 The person pane (tap and hold, or the ⓘ on a row)

```
  (A)  Alice ✔                    rename ✎ (inline)
       anon_5b4405 · added 12 Sep
  [ Connect through Tor ]  or  [ Invite ]      primary action for the mode
  Key fingerprint  4417 0923 8812  [Compare in person]   verified on 14 Sep / not yet
  Reachable through Tor: yes (has an onion key)  ·  Last chat: yesterday
  [ Remove contact ]  → the row disappears, "Removed · Undo" for 8 s
```

- The chat's safety code comes from each chat's Noise handshake hash (`Sas::from_handshake_hash`),
  so it exists only during a chat. The pane shows instead a **contact fingerprint**: a short
  code from both static public keys (sorted, hashed, domain-separated), the same on both phones
  and stable across chats. Comparing it in person and marking it sets the same verified flag as a
  chat's safety code. Both remain valid ways to verify.
- Remove is undoable (the key file is re-saved only after the undo window).

### 3.3 One "Add contact" sheet (the ⌗ Code sheet, contact tab)

- Two halves: **Add someone** (scan QR / paste their card) and **Share mine** (QR, copy link,
  system share on phones).
- Pasting any code still works as today (invite, answer, channel, bridges); the sheet only opens
  on the right half when started from "Add contact".
- After adding: the new person row is highlighted in Chats; in Tor mode a "Connect now" toast.

### 3.4 Settings keeps only "me"

- Your identity: save, sign in, backup, **Reset my contact card** (danger zone).
- The Contacts card and the "My contact card" button move out (to §3.1–3.3); Settings → Contacts
  becomes a one-line link "Manage contacts in Chats" for people who look there first, then is
  removed in the release after.

### 3.5 In a chat

- The header's "＋ contact" stays (it is contextual) but says "Add Bob to contacts" and, once
  added, turns into the ✔/unverified pill that opens the person pane.
- An incoming card-dial ("Alice wants to connect: she has your card") keeps its accept/decline
  prompt; accepting adds her and opens the chat, as now.

## 4. States and wording

| State | Row subtitle | Primary action |
|---|---|---|
| Open chat, connected | last message / typing… | open chat |
| Contact, Tor, has onion | "Connect through Tor" | connect |
| Contact, Tor, no onion key (added in direct mode) | "Invite needed (no Tor address)" | invite |
| Contact, direct mode | "Invite to chat" | invite |
| Connecting | "Connecting through Tor… (10–60 s)" | cancel |
| Key changed (a chat with a pinned key but a different key) | "⚠ key changed" in red | person pane, with the explanation |

## 5. Phones

- The contact section sits in the same scrolling list; the ＋ floating button (bottom centre)
  opens the add menu.
- Person pane = full-screen pane with Back (like a chat), the tab bar stays.
- Share mine uses `navigator.share` where available.

## 6. What changes in code (small, UI only)

| Area | Change |
|---|---|
| `app/index.html` | Chats list: search, contact section, empty state; person pane view (`v-person`); Code sheet halves; Settings: remove the Contacts card and "My contact card" |
| `app/app.js` | `renderContacts()` builds one merged list (chats ⋃ contacts by key); person pane; inline rename; undo remove; invite-from-contact (direct) |
| `crates/crypto`, `crates/wasm` | `contact_fingerprint(hex)`: `BLAKE2s("ephem-contact-fp-v1" ‖ min(pk_a, pk_b) ‖ max(pk_a, pk_b))` shown as 12 digits (three groups), with a unit test that both sides compute the same code |
| Tests | `checks/e2e_cards.mjs` and `tor-lab/e2e_tor_cards.mjs` follow the new places; new `checks/e2e_contacts_ux.mjs`: merged row, rename, undo remove, phone layout |

## 7. Phases

| ID | Scope | Done when |
|---|---|---|
| CU-1 | Chats: merged people list (chats + contacts, both modes), states of §4, temporary-identity hint | E2E: a contact with an open chat shows once; direct-mode contact shows "Invite" |
| CU-2 | Person pane: inline rename, remove with undo, safety code outside a chat, verify | E2E: rename without `prompt()`, remove → undo restores, ✔ set from the pane |
| CU-3 | One Add sheet (scan/paste/share mine), ＋ menu, Settings trimmed | E2E: add from a card via ＋, share mine via the sheet; Settings has no contacts list |
| CU-4 | Search (from 8 rows), phone polish, key-changed warning | E2E at 390×844; a key change shows the warning |

## 8. Decisions for the owner

| # | Question | Recommendation |
|---|---|---|
| C1 | Contacts inside the Chats tab, or a 5th "People" tab? | **Inside Chats** (4 tabs fit a phone; one list per person) |
| C2 | Direct-mode contacts: show them, with "Invite"? | **Yes** (they pin key and name; the invite is still needed) |
| C3 | A contact fingerprint (from both static keys) in the person pane, to verify without a chat? | **Yes** (the chat safety code is per-session and cannot be shown outside a chat) |
| C4 | Remove: undo toast instead of a confirmation? | **Yes**, 8 s |
| C5 | Share my card: always visible in Chats while < 3 contacts? | **Yes**, then only in the ＋ menu |
