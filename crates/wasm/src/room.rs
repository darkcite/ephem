//! Room layer of the adapter (§14): the owner-signed state, admissions, introductions and T2.
//!
//! Every member keeps one pairwise link per other member (full mesh, ≤ 15 links). The owner
//! admits whoever answers one of its GROUP invites, assigns the member index and role, and signs
//! the room state; members verify it with the owner's signing key from the owner link's HELLO.
//! Members introduce themselves to each other through the owner: the invite / answer codes of
//! a member-to-member link are sealed to the other member's static key (§14.4) and relayed on the
//! owner links as ROOM_SIGNAL, so the owner learns neither the candidates nor the keys of the
//! new link. Of every pair of members, the one with the greater `PeerId` offers (introductions
//! and T2 resumes alike), so both never offer at once.
//!
//! Setup path only (joins, removals, reconnects): allocations here are acceptable. Sending a
//! message costs one indirect call per link ([`fan_out`], [`each`]), not a per-frame cost.

use crate::{Inner, PENDING, Shared, emit, emit_err, ev, ids, now_ms, on_link, rtc};
use ephem_core::room::{MAX_MEMBERS, OWNER_IDX, RoomRole, RoomState, seal_context, signal_decode, signal_encode};
use ephem_core::{Event, Privacy, RoomLink, Session, Settings, State};
use ephem_crypto::{Identity, seal};
use ephem_proto::ErrorCode;
use ephem_proto::code::{Code, Kind, flags};
use ephem_proto::frame::rtype;

/// Inbox marker (not a record type): the peer's HELLO arrived on this link.
pub(crate) const HELLO: u8 = 0;
/// Validity of an introduction invite: it may wait for the user's "connect" confirmation.
const INTRO_TTL_S: u32 = 300;
/// Validity of a T2 resume invite (relayed at once); when it lapses the link is retried.
const T2_TTL_S: u32 = 60;
/// Between automatic T2 attempts of one link, and after an introduction found no direct path.
const RETRY_MS: u64 = 15_000;

pub(crate) type Sink<'a> = dyn FnMut(Event<'_>) + 'a;

pub(crate) struct Room {
    /// The owner's authoritative state, or the last one a member verified (`None`: not joined).
    state: Option<RoomState>,
    /// Our member index ([`PENDING`] until the first state).
    pub(crate) me: u8,
    pub(crate) owner: bool,
    pub(crate) role: RoomRole,
    /// Our last room message sequence number, the same on every link (§14.3).
    seq: u64,
    /// Owner: the room's self-destruct timer.
    ttl: u32,
    /// Member: the user agreed to connect directly to the other members (§29.2).
    confirmed: bool,
    /// Invites of members that arrived before the confirmation: (member, code).
    deferred: Vec<(u8, Vec<u8>)>,
    /// Per member: no introduction before this time (ms) after one failed.
    retry_at: [u64; MAX_MEMBERS],
    /// Per member (bit): the last introduction found no direct path.
    no_path: u16,
}

impl Room {
    pub(crate) fn owned(room_id: [u8; 16], id: &Identity) -> Self {
        Self { state: Some(RoomState::new(room_id, id)), me: OWNER_IDX, owner: true, role: RoomRole::Owner, confirmed: true, ..Self::blank() }
    }

    pub(crate) fn joining(observer: bool) -> Self {
        Self { role: if observer { RoomRole::Observer } else { RoomRole::Member }, ..Self::blank() }
    }

    fn blank() -> Self {
        Self {
            state: None,
            me: PENDING,
            owner: false,
            role: RoomRole::Member,
            seq: 0,
            ttl: 0,
            confirmed: false,
            deferred: Vec::new(),
            retry_at: [0; MAX_MEMBERS],
            no_path: 0,
        }
    }

    #[inline]
    fn version(&self) -> u32 {
        self.state.as_ref().map_or(0, |s| s.version)
    }
}

/// Room links carry no read receipts and no typing notices (§11.7).
fn room_settings(g: &Inner) -> Settings {
    Settings { read_receipts: false, typing: false, ..g.settings() }
}

#[inline]
fn now_s() -> u32 {
    (now_ms() / 1000) as u32
}

/// ROOM event (num = state version) so the UI re-reads the member list.
pub(crate) fn emit_state(g: &Inner) {
    emit(ev::ROOM, g.room.as_ref().map_or(0, |r| r.version()) as f64, &[]);
}

/// Aborts link `i` with `e` and removes it.
fn drop_link(g: &mut Inner, i: usize, e: ErrorCode) {
    on_link!(g, i, |s, k, _t| s.abort(e, &mut k));
    let l = g.links.remove(i);
    if let Some(r) = l.rtc {
        r.close();
    }
}

// ---- owner ----

/// Owner: a GROUP invite for one more member or observer. Earlier invites nobody answered are
/// withdrawn (the UI shows one code at a time).
pub(crate) fn invite(inner: &Shared, observer: bool, ttl_s: u32) -> Result<(u32, Privacy), ErrorCode> {
    let mut g = inner.borrow_mut();
    let room_id = match g.room.as_ref() {
        Some(Room { owner: true, state: Some(st), .. }) if st.members.len() < MAX_MEMBERS => st.room_id,
        Some(Room { owner: true, .. }) => return Err(ErrorCode::RoomFull),
        _ => return Err(ErrorCode::NotOwner),
    };
    while let Some(i) = g.links.iter().position(|l| l.member == PENDING && matches!(l.sess.state(), State::Gathering | State::AwaitingAnswer)) {
        g.close_link(i);
    }
    let (inv, _) = ids();
    let (privacy, drop_ipv6) = g.privacy();
    let mut s = Session::offerer(g.identity(), inv, room_id, now_s() + ttl_s.clamp(60, 1800), privacy, drop_ipv6, room_settings(&g));
    s.add_flags(flags::GROUP | if observer { flags::OBSERVER } else { 0 });
    Ok((g.add_link(PENDING, s, false), privacy))
}

/// Owner: the peer of a pending link sent HELLO (so its signing key is known): admit it with
/// the role its invite asked for, then send everyone the new state.
fn admit(g: &mut Inner, id: u32) {
    let Some(i) = g.find(id).filter(|i| g.links[*i].member == PENDING) else { return };
    let (peer, sign_pk, observer) = {
        let s = &g.links[i].sess;
        (s.remote(), s.peer_sign_pk(), s.code_flags() & flags::OBSERVER != 0)
    };
    // One link per identity: a second tab of a member is refused.
    if g.links.iter().any(|l| l.member != PENDING && l.sess.remote() == peer && l.sess.state() != State::Closed) {
        drop_link(g, i, ErrorCode::DuplicateSession);
        return;
    }
    let role = if observer { RoomRole::Observer } else { RoomRole::Member };
    let Some(r) = g.room.as_mut() else { return };
    let Some(st) = r.state.as_mut() else { return };
    let admitted = st.admit(peer, sign_pk, role);
    let (base, ttl) = (r.seq, r.ttl);
    let idx = match admitted {
        Ok(idx) => idx,
        Err(e) => {
            drop_link(g, i, e);
            return;
        }
    };
    let l = &mut g.links[i];
    l.member = idx;
    l.sess.set_room(RoomLink { me: OWNER_IDX, peer: idx, my_role: RoomRole::Owner, peer_role: role });
    l.sess.set_seq_base(base);
    sync(g);
    if ttl != 0 {
        // The newcomer learns the room's timer (a gap in our sequence on the other links).
        let r = g.room.as_mut().expect("room");
        r.seq += 1;
        let (at, now) = (r.seq, now_ms());
        let _ = on_link!(g, i, |s, k, _t| s.set_ttl(now, Some(at), ttl, &mut k));
    }
}

/// Owner: sends the current signed state to every connected member that has not got it yet.
fn sync(g: &mut Inner) {
    let (blob, version) = match g.room.as_ref() {
        Some(Room { owner: true, state: Some(st), .. }) => (st.sign(g.identity()), st.version),
        _ => return,
    };
    let now = now_ms();
    let mut sent = false;
    for i in 0..g.links.len() {
        let l = &g.links[i];
        if l.member == PENDING || l.synced >= version || l.sess.state() != State::Connected {
            continue;
        }
        if on_link!(g, i, |s, k, _t| s.send_room(now, rtype::ROOM_STATE, &blob, &mut k)).is_ok() {
            g.links[i].synced = version;
            sent = true;
        }
    }
    if sent {
        emit_state(g);
    }
}

/// Owner: removes member `idx` (it is sent the state without itself, then GOODBYE).
pub(crate) fn remove(inner: &Shared, idx: u8) -> Result<(), ErrorCode> {
    let mut g = inner.borrow_mut();
    remove_member(&mut g, idx)?;
    emit_state(&g);
    Ok(())
}

fn remove_member(g: &mut Inner, idx: u8) -> Result<(), ErrorCode> {
    let Inner { room, id, .. } = &mut *g;
    let Some(Room { owner: true, state: Some(st), .. }) = room.as_mut() else { return Err(ErrorCode::NotOwner) };
    st.remove(idx)?;
    let blob = st.sign(id);
    if let Some(i) = g.by_member(idx) {
        let now = now_ms();
        let _ = on_link!(g, i, |s, k, _t| s.send_room(now, rtype::ROOM_STATE, &blob, &mut k));
        g.close_link(i);
    }
    sync(g);
    Ok(())
}

/// Owner: a ROOM_SIGNAL from the member on link `i`, forwarded unread to its addressee.
fn forward(g: &mut Inner, i: usize, body: &[u8]) {
    let Some((from, to, _)) = signal_decode(body) else { return };
    if from != g.links[i].member || from == PENDING || to == OWNER_IDX {
        return;
    }
    if let Some(j) = g.by_member(to) {
        let now = now_ms();
        let _ = on_link!(g, j, |s, k, _t| s.send_room(now, rtype::ROOM_SIGNAL, body, &mut k));
    }
}

// ---- member ----

/// Member: the user agreed to connect directly to the other members (§29.2).
pub(crate) fn confirm(inner: &Shared) {
    let deferred = {
        let mut g = inner.borrow_mut();
        let Some(r) = g.room.as_mut().filter(|r| !r.owner) else { return };
        r.confirmed = true;
        core::mem::take(&mut r.deferred)
    };
    for (from, code) in deferred {
        accept_invite(inner, from, &code);
    }
    connect_members(inner);
}

/// Member: a new state from the owner (verified against the owner link's HELLO key).
fn on_state(inner: &Shared, o: usize, body: &[u8]) {
    let mut g = inner.borrow_mut();
    let (owner_peer, owner_pk, room_id) = {
        let s = &g.links[o].sess;
        (s.remote(), s.peer_sign_pk(), s.room_id())
    };
    let st = match RoomState::verify(body, &owner_peer, &owner_pk) {
        Ok(st) if st.room_id == room_id => st,
        Ok(_) => return closed(g, ErrorCode::InvalidRoom),
        Err(e) => return closed(g, e),
    };
    let Some(r) = g.room.as_ref() else { return };
    if st.version <= r.version() {
        return;
    }
    let first = r.state.is_none();
    let Some(me) = st.by_peer(&g.identity().peer_id()).copied() else {
        // Not (or no longer) in the room: removed by the owner.
        return closed(g, ErrorCode::NotPermitted);
    };
    if first {
        g.links[o].sess.set_room(RoomLink { me: me.idx, peer: OWNER_IDX, my_role: me.role, peer_role: RoomRole::Owner });
    }
    // Links to members who left (or whose index now names someone else) end.
    while let Some(i) = g.links.iter().position(|l| l.member != OWNER_IDX && st.member(l.member).is_none_or(|m| m.peer != l.sess.remote())) {
        g.close_link(i);
    }
    let r = g.room.as_mut().expect("room");
    r.me = me.idx;
    r.role = me.role;
    r.state = Some(st);
    emit_state(&g);
    drop(g);
    connect_members(inner);
}

/// The room ended for us (`e`): everything is closed.
fn closed(mut g: core::cell::RefMut<'_, Inner>, e: ErrorCode) {
    g.reset();
    drop(g);
    emit_err(ev::ROOM_CLOSED, e);
}

/// Member: opens the links we initiate (greater `PeerId`) to members we have no link to.
fn connect_members(inner: &Shared) {
    let mut started: Vec<(u32, Privacy)> = Vec::new();
    {
        let mut g = inner.borrow_mut();
        let Some(o) = g.by_member(OWNER_IDX).filter(|o| g.links[*o].sess.state() == State::Connected) else { return };
        let privacy = g.links[o].sess.privacy();
        let (_, drop_ipv6) = g.privacy();
        let settings = room_settings(&g);
        let now = now_ms();
        let Some(Room { state: Some(st), confirmed: true, me, role, seq, retry_at, .. }) = g.room.as_ref() else { return };
        let (me, role, seq, room_id) = (*me, *role, *seq, st.room_id);
        let mine = g.identity().peer_id();
        let targets: Vec<(u8, RoomRole)> = st
            .members
            .iter()
            .filter(|m| m.idx != OWNER_IDX && m.idx != me && m.peer < mine && now >= retry_at[m.idx as usize])
            .map(|m| (m.idx, m.role))
            .collect();
        for (idx, peer_role) in targets {
            if g.by_member(idx).is_some() {
                continue;
            }
            let (inv, _) = ids();
            let mut s = Session::offerer(g.identity(), inv, room_id, now_s() + INTRO_TTL_S, privacy, drop_ipv6, settings);
            s.set_room(RoomLink { me, peer: idx, my_role: role, peer_role });
            s.set_seq_base(seq);
            started.push((g.add_link(idx, s, true), privacy));
        }
    }
    for (id, privacy) in started {
        rtc::start(inner.clone(), id, privacy, rtc::Step::Offer);
    }
}

/// Member: sends the code of member link `id` sealed to that member, through the owner (§14.4).
pub(crate) fn relay_code(inner: &Shared, id: u32, code: &[u8]) -> Result<(), ErrorCode> {
    let mut g = inner.borrow_mut();
    let i = g.find(id).ok_or(ErrorCode::NotPermitted)?;
    let to = g.links[i].member;
    let Some(Room { state: Some(st), me, .. }) = g.room.as_ref() else { return Err(ErrorCode::InvalidRoom) };
    let peer = st.member(to).ok_or(ErrorCode::InvalidRoom)?.peer;
    let sealed = seal::seal(g.identity(), &peer, &seal_context(&st.room_id, *me, to), code);
    let body = signal_encode(*me, to, &sealed);
    let o = g.by_member(OWNER_IDX).ok_or(ErrorCode::PeerOffline)?;
    let now = now_ms();
    on_link!(g, o, |s, k, _t| s.send_room(now, rtype::ROOM_SIGNAL, &body, &mut k))
}

/// Member: a sealed code from member `from`, relayed by the owner.
fn on_signal(inner: &Shared, body: &[u8]) {
    let (from, code) = {
        let g = inner.borrow();
        let Some((from, to, boxed)) = signal_decode(body) else { return };
        let Some(Room { state: Some(st), me, .. }) = g.room.as_ref() else { return };
        let Some(m) = st.member(from).filter(|_| to == *me && from != *me && from != OWNER_IDX) else { return };
        let Some(code) = seal::open(g.identity(), &m.peer, &seal_context(&st.room_id, from, to), boxed) else { return };
        // The code must come from the key the signed state names for that member.
        match Code::decode(&code) {
            Ok(c) if c.static_pk == m.peer.0 => (from, code),
            _ => return,
        }
    };
    let Ok(c) = Code::decode(&code) else { return };
    match c.kind {
        Kind::Invite => {
            let mut g = inner.borrow_mut();
            let Some(r) = g.room.as_mut() else { return };
            if !r.confirmed {
                r.deferred.retain(|(f, _)| *f != from);
                r.deferred.push((from, code));
                return;
            }
            drop(g);
            accept_invite(inner, from, &code);
        }
        Kind::Answer | Kind::ResumeAnswer => {
            let id = {
                let mut g = inner.borrow_mut();
                let Some(i) = g.by_member(from).filter(|i| g.links[*i].via_owner && g.links[*i].sess.invite_id() == c.invite_id) else { return };
                let now = now_s();
                let crate::Inner { id, links, .. } = &mut *g;
                if links[i].sess.apply_answer(id, &code, now, false).is_err() {
                    return;
                }
                links[i].id
            };
            rtc::apply_answer(inner.clone(), id);
        }
        Kind::ResumeInvite => {
            let (id, privacy) = {
                let mut g = inner.borrow_mut();
                let Some(i) = g.by_member(from).filter(|i| g.links[*i].via_owner) else { return };
                let now = now_s();
                let crate::Inner { id, links, .. } = &mut *g;
                if links[i].sess.accept_resume(id, &code, now).is_err() {
                    return;
                }
                let privacy = links[i].sess.privacy();
                (g.new_path(i), privacy)
            };
            rtc::start(inner.clone(), id, privacy, rtc::Step::Answer);
        }
    }
}

/// Member: answers member `from`'s introduction (replacing an older link to it).
fn accept_invite(inner: &Shared, from: u8, code: &[u8]) {
    let (id, privacy) = {
        let mut g = inner.borrow_mut();
        let Some(Room { state: Some(st), me, role, seq, .. }) = g.room.as_ref() else { return };
        let Some(m) = st.member(from).copied() else { return };
        let (me, role, seq, room_id) = (*me, *role, *seq, st.room_id);
        let Some(o) = g.by_member(OWNER_IDX) else { return };
        let privacy = g.links[o].sess.privacy();
        let (_, drop_ipv6) = g.privacy();
        let mut s = match Session::answerer(g.identity(), code, now_s(), privacy, drop_ipv6, room_settings(&g), false) {
            Ok(s) if s.room_id() == room_id && s.code_flags() & flags::GROUP != 0 => s,
            _ => return,
        };
        s.set_room(RoomLink { me, peer: from, my_role: role, peer_role: m.role });
        s.set_seq_base(seq);
        if let Some(i) = g.by_member(from) {
            g.close_link(i);
        }
        let privacy = s.privacy();
        (g.add_link(from, s, true), privacy)
    };
    rtc::start(inner.clone(), id, privacy, rtc::Step::Answer);
}

/// Member leaving: tells the owner first (the GOODBYEs follow when the links close).
pub(crate) fn leave(inner: &Shared) {
    let mut g = inner.borrow_mut();
    if g.room.as_ref().is_some_and(|r| !r.owner)
        && let Some(o) = g.by_member(OWNER_IDX)
    {
        let now = now_ms();
        let _ = on_link!(g, o, |s, k, _t| s.send_room(now, rtype::ROOM_LEAVE, &[], &mut k));
    }
}

// ---- both ----

/// Handles the room records and HELLOs collected during the last core calls.
pub(crate) fn drain(inner: &Shared) {
    loop {
        let (id, rt, body, i, member, owner) = {
            let mut g = inner.borrow_mut();
            if g.inbox.is_empty() {
                return;
            }
            let (id, rt, body) = g.inbox.remove(0);
            let Some(owner) = g.room.as_ref().map(|r| r.owner) else {
                g.inbox.clear();
                return;
            };
            let Some(i) = g.find(id) else { continue };
            let member = g.links[i].member;
            (id, rt, body, i, member, owner)
        };
        match (rt, owner) {
            (HELLO, true) => admit(&mut inner.borrow_mut(), id),
            (rtype::ROOM_SIGNAL, true) => forward(&mut inner.borrow_mut(), i, &body),
            (rtype::ROOM_LEAVE, true) if member != PENDING => {
                let mut g = inner.borrow_mut();
                let _ = remove_member(&mut g, member);
                emit_state(&g);
            }
            (rtype::ROOM_STATE, false) if member == OWNER_IDX => on_state(inner, i, &body),
            (rtype::ROOM_SIGNAL, false) if member == OWNER_IDX => on_signal(inner, &body),
            (HELLO, false) => emit_state(&inner.borrow()),
            _ => {}
        }
    }
}

/// Every second: closed links are reaped, the owner re-sends the state to members that missed
/// it, members retry introductions and resume suspended member links (T2).
pub(crate) fn tick(inner: &Shared) {
    let mut resumed: Vec<(u32, Privacy)> = Vec::new();
    {
        let mut g = inner.borrow_mut();
        let Some(owner) = g.room.as_ref().map(|r| r.owner) else { return };
        let now = now_ms();
        let mut changed = false;
        while let Some(i) = g.links.iter().position(|l| l.sess.state() == State::Closed) {
            let (member, connected) = (g.links[i].member, g.links[i].sess.ever_connected());
            if let Some(r) = g.links.remove(i).rtc {
                r.close();
            }
            changed = true;
            if owner {
                if member != PENDING {
                    let _ = remove_member(&mut g, member);
                }
            } else if member == OWNER_IDX {
                return closed(g, ErrorCode::RoomDisposed);
            } else if let Some(r) = g.room.as_mut() {
                // An introduction that never connected: no direct path to that member (§14.4).
                let bit = 1u16 << member;
                r.no_path = if connected { r.no_path & !bit } else { r.no_path | bit };
                r.retry_at[member as usize] = now + RETRY_MS;
            }
        }
        if owner {
            sync(&mut g);
        } else {
            let owner_up = g.by_member(OWNER_IDX).is_some_and(|o| g.links[o].sess.state() == State::Connected);
            let mine = g.identity().peer_id();
            for i in 0..g.links.len() {
                let l = &g.links[i];
                if !owner_up || !l.via_owner || l.sess.state() != State::Suspended || l.sess.remote() > mine || now < l.t2_at + RETRY_MS {
                    continue;
                }
                let (inv, _) = ids();
                g.links[i].t2_at = now;
                if g.links[i].sess.resume_invite(inv, now_s() + T2_TTL_S).is_ok() {
                    let privacy = g.links[i].sess.privacy();
                    resumed.push((g.new_path(i), privacy));
                }
            }
        }
        if changed {
            emit_state(&g);
        }
    }
    for (id, privacy) in resumed {
        rtc::start(inner.clone(), id, privacy, rtc::Step::Offer);
    }
    connect_members(inner);
}

/// Sends one room message on every link with the same sequence number (§14.3); in a 1:1 chat on
/// the one link. Returns the sequence number. A room member alone (or with every link down)
/// still gets a number: the message is shown locally, and links that come back resend.
pub(crate) fn fan_out(inner: &Shared, mut f: impl FnMut(&mut Session, Option<u64>, u64, &[u8], &mut Sink<'_>) -> Result<u64, ErrorCode>) -> Result<u64, ErrorCode> {
    let mut g = inner.borrow_mut();
    let now = now_ms();
    let Some(r) = g.room.as_ref() else {
        if g.links.is_empty() {
            return Err(ErrorCode::PeerOffline);
        }
        return on_link!(g, 0, |s, k, text| f(s, None, now, &text[..], &mut k));
    };
    if r.me == PENDING || r.role == RoomRole::Observer {
        return Err(ErrorCode::NotPermitted);
    }
    let seq = r.seq + 1;
    let ttl = ttl(&g);
    let mut res = Ok(seq);
    let mut any = false;
    for i in 0..g.links.len() {
        let l = &mut g.links[i];
        if l.member == PENDING || !l.sess.ever_connected() || l.sess.state() == State::Closed {
            continue;
        }
        l.sess.apply_ttl(ttl);
        match on_link!(g, i, |s, k, text| f(s, Some(seq), now, &text[..], &mut k)) {
            Ok(_) => any = true,
            Err(e) => res = Err(e),
        }
    }
    if any || res.is_ok() {
        g.room.as_mut().expect("room").seq = seq;
        return Ok(seq);
    }
    res
}

/// Applies `f` on every link of the chat (an edit, delete or reaction reaches every member).
pub(crate) fn each(inner: &Shared, mut f: impl FnMut(&mut Session, u64, &[u8], &mut Sink<'_>) -> Result<(), ErrorCode>) -> Result<(), ErrorCode> {
    let mut g = inner.borrow_mut();
    let now = now_ms();
    if g.room.is_none() {
        if g.links.is_empty() {
            return Err(ErrorCode::PeerOffline);
        }
        return on_link!(g, 0, |s, k, text| f(s, now, &text[..], &mut k));
    }
    let mut res = Ok(());
    let mut any = false;
    for i in 0..g.links.len() {
        let s = &g.links[i].sess;
        if g.links[i].member == PENDING || !s.ever_connected() || s.state() == State::Closed {
            continue;
        }
        match on_link!(g, i, |s, k, text| f(s, now, &text[..], &mut k)) {
            Ok(()) => any = true,
            Err(e) => res = Err(e),
        }
    }
    if any { Ok(()) } else { res }
}

/// Owner: records the room timer after a successful [`fan_out`] of the setting.
pub(crate) fn set_ttl(g: &mut Inner, ttl_s: u32) {
    if let Some(r) = g.room.as_mut().filter(|r| r.owner) {
        r.ttl = ttl_s;
    }
}

/// The chat's self-destruct timer: the owner's setting, as the owner link reports it to members.
pub(crate) fn ttl(g: &Inner) -> u32 {
    match g.room.as_ref() {
        Some(r) if r.owner => r.ttl,
        Some(_) => g.by_member(OWNER_IDX).map_or(0, |o| g.links[o].sess.chat_ttl()),
        None => g.links.first().map_or(0, |l| l.sess.chat_ttl()),
    }
}

/// One line per member: `idx \t role \t handle \t link \t nick \t sas`; link is `me`,
/// `connected`, `connecting`, `suspended`, `no-path` or `none`.
pub(crate) fn members(g: &Inner) -> String {
    let Some(Room { state: Some(st), me, no_path, .. }) = g.room.as_ref() else { return String::new() };
    let mut out = String::new();
    for m in &st.members {
        let link = g.by_member(m.idx).map(|i| &g.links[i]);
        let status = match link.map(|l| l.sess.state()) {
            _ if m.idx == *me => "me",
            Some(State::Connected) => "connected",
            Some(State::Suspended) => "suspended",
            Some(State::Closed) | None if no_path & (1 << m.idx) != 0 => "no-path",
            Some(State::Closed) | None => "none",
            Some(_) => "connecting",
        };
        let nick = if m.idx == *me { g.settings().nick().to_vec() } else { link.map(|l| l.peer.nick.clone()).unwrap_or_default() };
        let sas = link.map_or(0, |l| l.peer.sas);
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            m.idx,
            m.role as u8,
            String::from_utf8_lossy(&m.peer.handle()),
            status,
            String::from_utf8_lossy(&nick),
            sas
        ));
    }
    out
}

/// `my_idx \t my_role \t owner(0/1) \t version \t confirmed(0/1)`, or empty outside a room.
pub(crate) fn info(g: &Inner) -> String {
    g.room
        .as_ref()
        .map(|r| format!("{}\t{}\t{}\t{}\t{}", r.me, r.role as u8, r.owner as u8, r.version(), r.confirmed as u8))
        .unwrap_or_default()
}
