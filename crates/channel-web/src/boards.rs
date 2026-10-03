// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Boards in the browser (docs/BOARDS.md, BD-3): the owner's host, posters and readers, over the
//! tab's Tor client. `BoardApp` shares [`ChannelApp`]'s state: the same identity (board keys
//! are derived from it, G.4), the same Tor client and onion nicknames; a new identity closes
//! the boards with the channels (M-1). Hosted in the main app tab (R6).
//!
//! - **Host:** [`ephem_board::host::Host`] behind the board's own onion. Each stream gets one of
//!   [`SLOTS`] preallocated buffers (head and submit body; busy = closed at once); a publish
//!   loop signs at most once a second and tells the page (`set_listener`), which moves the
//!   [`Delta`](ephem_board::host::Delta) into its block store (app/store-worker.js).
//! - **Poster:** `draft` fetches `/pow` and draws a fresh poster key; the page's Workers solve
//!   (app/pow-worker.js); `post` signs and submits on a new Tor isolation group (A-M9); `resend`
//!   repeats the same bytes (the host answers a retry with the original number, B-m9).
//! - **Reader:** the index (record and blocks in one response), then the threads asked for,
//!   each checked by CID and signature (`ephem_board::verify`).
//!
//! Copies (G.14.2): stream bytes are read into the slot (one copy); blocks cross to JS once,
//! for the store (`delta`), and once back at a reopen (`open`).

use crate::{ChannelApp, State, err, json, now_s, with_timeout, PORT};
use ephem_board::board::{Board, Entry};
use ephem_board::gateway::{self, Answer, Route, SHORT};
use ephem_board::host::Host;
use ephem_board::pipeline::{Efforts, Intake, Next, PowInfo, Refusal, submission};
use ephem_board::post::Signed;
use ephem_board::submit::{HEADER_LEN, MAX_SUBMIT, kind};
use ephem_board::verify::{self, View};
use ephem_channel::car::Block;
use ephem_channel::cid::Cid;
use ephem_channel::ipns;
use ephem_tor::web::{DataStream, Service, Tor, is_onion, list, sleep_ms};
use futures::{AsyncReadExt, AsyncWriteExt};
use std::cell::RefCell;
use std::fmt::Write as _;
use std::rc::{Rc, Weak};
use wasm_bindgen::prelude::*;

/// Boards per identity (G.4).
pub const MAX_BOARDS: u32 = 4;
/// Streams served at once per board; each holds one slot.
const SLOTS: usize = 16;
const SLOT: usize = gateway::MAX_HEAD + MAX_SUBMIT;
/// A submit must arrive whole within this (G.6.2 step 1); a read request head likewise.
const READ_MS: u32 = 10_000;
/// A submitter waits this long for the publish that answers it.
const ANSWER_MS: u32 = 20_000;
/// The whole of one request, write included.
const SERVE_MS: u32 = 60_000;
const FETCH_MS: u32 = 90_000;
const READ_ROUNDS: u32 = 4;
/// The publish loop's period; `Host::due` decides.
const TICK_MS: u32 = 250;

fn now_ms() -> u64 {
    js_sys::Date::now() as u64
}

/// A board this tab hosts.
pub(crate) struct Hosted {
    index: u32,
    host: Rc<RefCell<Host>>,
    onion: String,
    svc: Option<Rc<Service>>,
}

#[derive(Default)]
pub(crate) struct Boards {
    pub(crate) hosted: Vec<Hosted>,
    listener: Option<js_sys::Function>,
}

#[wasm_bindgen]
pub struct BoardApp {
    st: Rc<RefCell<State>>,
}

#[wasm_bindgen]
impl BoardApp {
    /// Boards of the page's channels engine (same identity and Tor client).
    #[wasm_bindgen(constructor)]
    pub fn new(ch: &ChannelApp) -> BoardApp {
        BoardApp { st: ch.st.clone() }
    }

    fn tor(&self) -> Result<Rc<Tor>, JsValue> {
        self.st.borrow().tor.borrow().clone().ok_or_else(|| err("Tor is not started"))
    }

    fn seeds(&self, index: u32) -> Result<[[u8; 32]; 3], JsValue> {
        if index >= MAX_BOARDS {
            return Err(err("at most 4 boards per identity"));
        }
        Ok(self.st.borrow().id.as_ref().ok_or_else(|| err("sign in first"))?.board_seeds(index))
    }

    fn host(&self, index: u32) -> Result<Rc<RefCell<Host>>, JsValue> {
        self.st.borrow().boards.hosted.iter().find(|b| b.index == index).map(|b| b.host.clone()).ok_or_else(|| err("that board is not open"))
    }

    // ---- owner ----

    /// Board `index`'s IPNS name (`k51…`), the same on every device of the identity.
    pub fn name(&self, index: u32) -> Result<String, JsValue> {
        let mut s = self.seeds(index)?;
        let pk = ed25519_dalek::SigningKey::from_bytes(&s[0]).verifying_key().to_bytes();
        s.iter_mut().for_each(|x| x.fill(0));
        Ok(Cid::ipns_name(&pk).to_text())
    }

    /// A new board `index` (the page refuses when its store already holds one).
    pub fn create(&self, index: u32, title: &str, about: &str, rules: &str) -> Result<(), JsValue> {
        let mut s = self.seeds(index)?;
        let board = Board::new(&s[0], title, about, rules, now_s());
        let r = board.map(|b| self.start(index, b, &s[2]));
        s.iter_mut().for_each(|x| x.fill(0));
        r.map_err(|e| err(format!("{e:?}")))
    }

    /// Reopens board `index` from its store: the last record and its blocks (`[[cid, bytes]…]`).
    pub fn open(&self, index: u32, record: &[u8], blocks: js_sys::Array) -> Result<(), JsValue> {
        let mut held: Vec<Block> = Vec::with_capacity(blocks.length() as usize);
        for pair in blocks.iter() {
            let pair = js_sys::Array::from(&pair);
            let cid = pair.get(0).as_string().and_then(|c| Cid::parse(&c)).ok_or_else(|| err("store: a block name"))?;
            let bytes = js_sys::Uint8Array::new(&pair.get(1)).to_vec();
            if !cid.verifies(&bytes) {
                return Err(err("store: a block does not match its name"));
            }
            held.push((cid, bytes));
        }
        let mut s = self.seeds(index)?;
        let r = (|| {
            let key = ed25519_dalek::SigningKey::from_bytes(&s[0]);
            let name = Cid::ipns_name(&key.verifying_key().to_bytes());
            let rec = ipns::verify(&name, record, 0).map_err(|e| format!("record: {e:?}"))?;
            let root = rec.value.strip_prefix("/ipfs/").and_then(Cid::parse).ok_or("record value")?;
            let mut view = verify::read(&key.verifying_key(), &name, &root, &held, &[]).map_err(|e| format!("{e:?}"))?;
            view.sequence = rec.sequence;
            let board = Board::load(&s[0], view, held).map_err(|e| format!("{e:?}"))?;
            self.start(index, board, &s[2]);
            Ok::<(), String>(())
        })();
        s.iter_mut().for_each(|x| x.fill(0));
        r.map_err(err)
    }

    fn start(&self, index: u32, board: Board, pow_secret: &[u8; 32]) {
        let intake = Intake::new(board.name(), *pow_secret, Efforts::DEFAULT, now_s());
        let host = Rc::new(RefCell::new(Host::new(board, intake, now_ms())));
        let mut st = self.st.borrow_mut();
        st.boards.hosted.retain(|b| b.index != index);
        st.boards.hosted.push(Hosted { index, host: host.clone(), onion: String::new(), svc: None });
        drop(st);
        wasm_bindgen_futures::spawn_local(publish_loop(Rc::downgrade(&host), Rc::downgrade(&self.st), index));
    }

    /// Indices of the boards open here.
    pub fn open_boards(&self) -> Vec<u32> {
        self.st.borrow().boards.hosted.iter().map(|b| b.index).collect()
    }

    /// Serves board `index` on its own onion (G.4: unlinked from the chat onion and channels);
    /// returns `<56 chars>.onion`.
    pub fn serve(&self, index: u32) -> Result<String, JsValue> {
        let tor = self.tor()?;
        let host = self.host(index)?;
        if let Some(b) = self.st.borrow().boards.hosted.iter().find(|b| b.index == index && b.svc.is_some()) {
            return Ok(b.onion.clone());
        }
        let mut s = self.seeds(index)?;
        let nick = {
            let mut st = self.st.borrow_mut();
            st.launched += 1;
            format!("board{}", st.launched)
        };
        let svc = tor.launch(&nick, &s[1]);
        s.iter_mut().for_each(|x| x.fill(0));
        let svc = Rc::new(svc.map_err(err)?);
        let onion = svc.onion().to_owned();
        wasm_bindgen_futures::spawn_local(serve_loop(Rc::downgrade(&svc), Rc::downgrade(&host)));
        let mut st = self.st.borrow_mut();
        if let Some(b) = st.boards.hosted.iter_mut().find(|b| b.index == index) {
            (b.onion, b.svc) = (onion.clone(), Some(svc));
        }
        Ok(onion)
    }

    /// Whether readers can reach board `index`'s onion yet (as channels' `reach`).
    pub fn reach(&self, index: u32) -> String {
        self.st.borrow().boards.hosted.iter().find(|b| b.index == index).and_then(|b| b.svc.as_ref().map(|s| s.reach().to_owned())).unwrap_or_default()
    }

    /// The owner posts (capcode, no proof of work): a new thread (`thread` 0) or a reply.
    pub fn post(&self, index: u32, thread: f64, sub: &str, body: &str, sage: bool) -> Result<f64, JsValue> {
        let host = self.host(index)?;
        let no = host.borrow_mut().post_owner(thread as u64, sub, body, sage, now_s());
        no.map(|n| n as f64).map_err(|e| err(format!("{e:?}")))
    }

    /// The owner deletes post `no` (an OP takes its thread).
    pub fn delete(&self, index: u32, no: f64) -> Result<(), JsValue> {
        self.host(index)?.borrow_mut().delete(no as u64, now_s()).map_err(|e| err(format!("{e:?}")))
    }

    /// The owner's base efforts (G.8; the adaptive multiplier applies on top). At least 1.
    pub fn set_efforts(&self, index: u32, reply: u32, thread: u32) -> Result<(), JsValue> {
        self.host(index)?.borrow_mut().intake.base = Efforts { reply: reply.max(1), thread: thread.max(1) };
        Ok(())
    }

    /// Called with the board's index after each publish (the page then takes the `delta`).
    pub fn set_listener(&self, f: js_sys::Function) {
        self.st.borrow_mut().boards.listener = Some(f);
    }

    /// What the store must write and delete since the last call: `{record, added: [[cid,
    /// bytes]…], removed: [cid…]}` (each block copied into JS once).
    pub fn delta(&self, index: u32) -> Result<js_sys::Object, JsValue> {
        let d = self.host(index)?.borrow_mut().take_delta();
        let added = js_sys::Array::new_with_length(d.added.len() as u32);
        for (i, (c, b)) in d.added.iter().enumerate() {
            added.set(i as u32, js_sys::Array::of2(&JsValue::from_str(&c.to_text()), &js_sys::Uint8Array::from(b.as_slice())).into());
        }
        let removed: js_sys::Array = d.removed.iter().map(|c| JsValue::from_str(&c.to_text())).collect();
        let o = js_sys::Object::new();
        js_sys::Reflect::set(&o, &"record".into(), &js_sys::Uint8Array::from(d.record.as_slice()))?;
        js_sys::Reflect::set(&o, &"added".into(), &added)?;
        js_sys::Reflect::set(&o, &"removed".into(), &removed)?;
        Ok(o)
    }

    /// The owner's view of board `index`: `{name, onion, seq, threads, next_no, paused,
    /// threads_closed, closed_notice, effort_reply, effort_thread, blocks}`.
    pub fn status(&self, index: u32) -> String {
        let st = self.st.borrow();
        let Some(b) = st.boards.hosted.iter().find(|b| b.index == index) else { return String::new() };
        let mut h = b.host.borrow_mut();
        let info = h.intake.pow_info(now_s());
        let mut o = String::with_capacity(256);
        o.push_str("{\"name\":");
        json::string(&mut o, h.served.name_text());
        o.push_str(",\"onion\":");
        json::string(&mut o, &b.onion);
        let _ = write!(
            o,
            ",\"seq\":{},\"threads\":{},\"next_no\":{},\"paused\":{},\"threads_closed\":{},\"closed_notice\":{},\"effort_reply\":{},\"effort_thread\":{},\"blocks\":{}}}",
            h.board.seq,
            h.board.threads.len(),
            h.board.next_no,
            h.intake.paused,
            h.intake.threads_closed,
            h.intake.closed_notice,
            info.effort_reply,
            info.effort_thread,
            h.served.blocks().count()
        );
        o
    }

    /// Closes board `index` here (its onion goes down; the store keeps it).
    pub fn close(&self, index: u32) {
        self.st.borrow_mut().boards.hosted.retain(|b| b.index != index);
    }

    // ---- poster ----

    /// Opens a reply box (G.6.1 step 1): `GET /pow` from the board's onion, a fresh poster key.
    /// `thread` 0 = a new thread.
    pub fn draft(&self, name: &str, onion: &str, thread: f64) -> Result<js_sys::Promise, JsValue> {
        let tor = self.tor()?;
        let name = Cid::parse(name).filter(|c| c.ed25519_key().is_some()).ok_or_else(|| err("not a board name"))?;
        if !is_onion(onion) {
            return Err(err("not an onion address"));
        }
        let onion = onion.to_owned();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let resp = with_timeout(FETCH_MS, fetch(&tor, &onion, &ephem_channel::gateway::get(&onion, "/pow"), false, 1024)).await.map_err(err)?;
            let body = ephem_channel::gateway::parse_response(&resp).map_err(|e| err(format!("/pow: {e:?}")))?;
            let info = PowInfo::read(body.try_into().map_err(|_| err("/pow: wrong length"))?);
            let mut seed = [0u8; 32];
            ephem_crypto::random(&mut seed);
            let key = ed25519_dalek::SigningKey::from_bytes(&seed);
            seed.fill(0);
            Ok(Draft { name, onion, thread: thread as u64, info, key, sent: RefCell::new(Vec::new()) }.into())
        }))
    }

    /// Signs and submits the post solved for `draft` (`n`, `solution` from a Worker), on a new
    /// Tor isolation group. Resolves to `{no, seq}` as JSON; a refusal rejects with its reason.
    pub fn post_draft(&self, draft: &Draft, sub: &str, body: &str, sage: bool, n: &[u8], solution: &[u8]) -> Result<js_sys::Promise, JsValue> {
        let tor = self.tor()?;
        let n: [u8; 16] = n.try_into().map_err(|_| err("nonce: 16 bytes"))?;
        let solution = solution.try_into().map_err(|_| err("solution: 16 bytes"))?;
        let s = Signed { b: draft.name.to_text(), t: draft.thread, k: draft.key.verifying_key().to_bytes(), n, sub: sub.into(), body: body.into(), sage, e: draft.info.epoch };
        let bytes = submission(&s, &draft.key, draft.effort(), solution).map_err(|e| err(format!("{e:?}")))?;
        let req = gateway::submit_request(&draft.onion, &bytes);
        *draft.sent.borrow_mut() = req.clone();
        Ok(submit(tor, draft.onion.clone(), req))
    }

    /// Sends the same submit again (a dropped stream): the host answers with the original
    /// `{no, seq}` if it was published.
    pub fn resend(&self, draft: &Draft) -> Result<js_sys::Promise, JsValue> {
        let req = draft.sent.borrow().clone();
        if req.is_empty() {
            return Err(err("nothing was sent yet"));
        }
        Ok(submit(self.tor()?, draft.onion.clone(), req))
    }

    // ---- reader ----

    /// Reads board `name` from the first onion (comma-separated: owner, mirrors) that serves a
    /// valid state, with the `threads` asked for (numbers). Resolves to the JSON view
    /// ([`view_json`]).
    pub fn read(&self, name: &str, onions: &str, min_seq: f64, threads: Vec<f64>) -> Result<js_sys::Promise, JsValue> {
        let tor = self.tor()?;
        let name = Cid::parse(name).filter(|c| c.ed25519_key().is_some()).ok_or_else(|| err("not a board name"))?;
        let mut ok: Vec<String> = Vec::with_capacity(1 + ephem_board::limits::MIRRORS);
        for o in list(onions) {
            if is_onion(&o) && !ok.contains(&o) && ok.len() <= ephem_board::limits::MIRRORS {
                ok.push(o);
            }
        }
        if ok.is_empty() {
            return Err(err("no onion address to read from"));
        }
        let threads: Vec<u64> = threads.into_iter().map(|t| t as u64).collect();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let mut last = String::new();
            for round in 0..READ_ROUNDS {
                for onion in &ok {
                    match with_timeout(FETCH_MS, read_from(&tor, &name, onion, min_seq as u64, &threads, round > 0)).await {
                        Ok(v) => return Ok(JsValue::from_str(&view_json(&v))),
                        Err(e) => last = format!("{onion}: {e}"),
                    }
                }
                sleep_ms(2_000 << round).await;
            }
            Err(err(last))
        }))
    }
}

/// An open reply box: the board, its `/pow` answer and a fresh poster key (RAM only).
#[wasm_bindgen]
pub struct Draft {
    name: Cid,
    onion: String,
    thread: u64,
    info: PowInfo,
    key: ed25519_dalek::SigningKey,
    /// The submit as sent (for `resend`).
    sent: RefCell<Vec<u8>>,
}

#[wasm_bindgen]
impl Draft {
    fn effort(&self) -> u32 {
        if self.thread == 0 { self.info.effort_thread } else { self.info.effort_reply }
    }

    /// What a Worker solves: `{name, seed, thread, k, kind, effort}` (the page adds a random
    /// starting nonce per Worker).
    pub fn params(&self) -> Result<js_sys::Object, JsValue> {
        let o = js_sys::Object::new();
        let set = |k: &str, v: JsValue| js_sys::Reflect::set(&o, &k.into(), &v);
        set("name", js_sys::Uint8Array::from(self.name.to_bytes().as_slice()).into())?;
        set("seed", js_sys::Uint8Array::from(&self.info.seed[..]).into())?;
        set("thread", JsValue::from(self.thread as f64))?;
        set("k", js_sys::Uint8Array::from(&self.key.verifying_key().to_bytes()[..]).into())?;
        set("kind", JsValue::from(if self.thread == 0 { kind::THREAD } else { kind::REPLY }))?;
        set("effort", JsValue::from(self.effort()))?;
        Ok(o)
    }

    #[wasm_bindgen(getter)]
    pub fn effort_now(&self) -> u32 {
        self.effort()
    }

    #[wasm_bindgen(getter)]
    pub fn paused(&self) -> bool {
        self.info.paused
    }

    #[wasm_bindgen(getter)]
    pub fn threads_open(&self) -> bool {
        self.info.threads_open
    }
}

fn submit(tor: Rc<Tor>, onion: String, req: Vec<u8>) -> js_sys::Promise {
    wasm_bindgen_futures::future_to_promise(async move {
        // A new isolation group per submit (A-M9): posts do not share a circuit.
        let resp = with_timeout(FETCH_MS, fetch(&tor, &onion, &req, true, 1024)).await.map_err(err)?;
        match gateway::parse_answer(&resp) {
            Some(Answer::Posted { no, seq }) => Ok(JsValue::from_str(&format!("{{\"no\":{no},\"seq\":{seq}}}"))),
            Some(Answer::Refused { status, code }) => Err(err(format!("refused: {status} {}", refusal_name(code)))),
            None => Err(err("the board answered something else")),
        }
    })
}

/// The stable names of the submit refusals (G.6.3).
fn refusal_name(code: u16) -> &'static str {
    [Refusal::Pow, Refusal::Busy, Refusal::Refused, Refusal::Paused].into_iter().find(|r| r.code() == code).map_or("E_BOARD", |r| match r {
        Refusal::Pow => "E_BOARD_POW",
        Refusal::Busy => "E_BOARD_BUSY",
        Refusal::Refused => "E_BOARD_REFUSED",
        Refusal::Paused => "E_BOARD_PAUSED",
    })
}

/// One request on a new stream to `onion`; the whole response (≤ `cap` body bytes).
async fn fetch(tor: &Tor, onion: &str, req: &[u8], fresh: bool, cap: usize) -> Result<Vec<u8>, String> {
    let s = tor.connect(onion, PORT, fresh).await?;
    let (mut r, mut w) = s.split();
    w.write_all(req).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;
    let mut resp = Vec::with_capacity(4096);
    let mut buf = [0u8; 16 * 1024];
    while !ephem_channel::gateway::complete(&resp) {
        let n = match r.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) if ephem_channel::gateway::complete(&resp) => break,
            Err(e) => return Err(e.to_string()),
        };
        if resp.len() + n > cap + gateway::MAX_HEAD {
            return Err("response too large".into());
        }
        resp.extend_from_slice(&buf[..n]);
    }
    Ok(resp)
}

async fn read_from(tor: &Tor, name: &Cid, onion: &str, min_seq: u64, threads: &[u64], fresh: bool) -> Result<View, String> {
    let path = format!("/ipns/{}?format=ephem-board", name.to_text());
    let resp = fetch(tor, onion, &ephem_channel::gateway::get(onion, &path), fresh, 2 + gateway::MAX_RECORD + gateway::MAX_CAR).await?;
    let body = ephem_channel::gateway::parse_response(&resp).map_err(|e| format!("index: {e:?}"))?;
    let (record, _, mut blocks) = gateway::parse_index(body).ok_or("index: malformed")?;
    let index = verify::verify(name, &record, &blocks, now_ms(), min_seq, &[]).map_err(|e| format!("{e:?}"))?;
    if threads.is_empty() {
        return Ok(index);
    }
    for no in threads {
        let Some(c) = index.catalog.iter().find(|c| c.no == *no) else { continue }; // pruned or deleted
        let resp = fetch(tor, onion, &ephem_channel::gateway::get(onion, &format!("/ipfs/{}?format=car", c.thread.to_text())), false, gateway::MAX_CAR).await?;
        let body = ephem_channel::gateway::parse_response(&resp).map_err(|e| format!("thread {no}: {e:?}"))?;
        let (_, b) = ephem_channel::car::read(body).ok_or("thread: not a valid CAR")?;
        blocks.extend(b);
    }
    verify::verify(name, &record, &blocks, now_ms(), min_seq, &[]).map_err(|e| format!("{e:?}"))
}

/// `{"name","root","sequence","title","about","rules","next_no","updated","catalog":[{"no",
/// "sub","ex","r","bump","st","lk"}…],"threads":[{"no","sub","posts":[{"no","ts","sub","body",
/// "sage","cap","del"}…]}…],"archive":[{"no","sub","ex","pruned"}…]}` (`del`: 0, or who deleted).
pub fn view_json(v: &View) -> String {
    let mut o = String::with_capacity(512 + v.threads.iter().map(|t| t.entries.len() * 160).sum::<usize>() + v.catalog.len() * 200);
    o.push_str("{\"name\":");
    json::string(&mut o, &v.name.to_text());
    o.push_str(",\"root\":");
    json::string(&mut o, &v.root.to_text());
    let _ = write!(o, ",\"sequence\":{},\"next_no\":{},\"updated\":{}", v.sequence, v.next_no, v.updated);
    for (k, s) in [("title", &v.manifest.title), ("about", &v.manifest.about), ("rules", &v.manifest.rules)] {
        let _ = write!(o, ",\"{k}\":");
        json::string(&mut o, s);
    }
    o.push_str(",\"catalog\":[");
    for (i, c) in v.catalog.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"no\":{},\"r\":{},\"bump\":{},\"st\":{},\"lk\":{},\"sub\":", c.no, c.replies, c.bump, c.sticky, c.locked);
        json::string(&mut o, &c.sub);
        o.push_str(",\"ex\":");
        json::string(&mut o, &c.ex);
        o.push('}');
    }
    o.push_str("],\"threads\":[");
    for (i, t) in v.threads.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"no\":{},\"sub\":", t.no);
        json::string(&mut o, &t.sub);
        o.push_str(",\"posts\":[");
        for (j, e) in t.entries.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            match e {
                Entry::Post(p) => {
                    let _ = write!(o, "{{\"no\":{},\"ts\":{},\"sage\":{},\"cap\":{},\"del\":0,\"sub\":", p.no, p.ts, p.s.sage, p.cap);
                    json::string(&mut o, &p.s.sub);
                    o.push_str(",\"body\":");
                    json::string(&mut o, &p.s.body);
                    o.push('}');
                }
                Entry::Tomb { no, ts, del, .. } => {
                    let _ = write!(o, "{{\"no\":{no},\"ts\":{ts},\"sage\":false,\"cap\":0,\"del\":{del},\"sub\":\"\",\"body\":\"\"}}");
                }
            }
        }
        o.push_str("]}");
    }
    o.push_str("],\"archive\":[");
    for (i, a) in v.archive.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"no\":{},\"pruned\":{},\"sub\":", a.no, a.pruned);
        json::string(&mut o, &a.sub);
        o.push_str(",\"ex\":");
        json::string(&mut o, &a.ex);
        o.push('}');
    }
    o.push_str("]}");
    o
}

/// Publishes when due and tells the page; stops when the board is closed here.
async fn publish_loop(host: Weak<RefCell<Host>>, st: Weak<RefCell<State>>, index: u32) {
    let mut minute = now_s() / 60;
    loop {
        sleep_ms(TICK_MS).await;
        let Some(h) = host.upgrade() else { return };
        let now = now_ms();
        if now / 60_000 != minute {
            minute = now / 60_000;
            h.borrow_mut().intake.tick(now / 1000);
        }
        if !h.borrow().due(now) {
            continue;
        }
        h.borrow_mut().publish(now);
        drop(h);
        let f = st.upgrade().and_then(|s| s.borrow().boards.listener.clone());
        if let Some(f) = f {
            let _ = f.call1(&JsValue::NULL, &JsValue::from(index));
        }
    }
}

/// Accepts streams while the service lives, each with a slot from a fixed pool.
async fn serve_loop(svc: Weak<Service>, host: Weak<RefCell<Host>>) {
    let pool: Rc<RefCell<Vec<Box<[u8; SLOT]>>>> = Rc::new(RefCell::new((0..SLOTS).map(|_| Box::new([0u8; SLOT])).collect()));
    while let Some(s) = svc.upgrade() {
        let Some(stream) = s.try_accept() else {
            drop(s);
            sleep_ms(50).await;
            continue;
        };
        let Some(slot) = pool.borrow_mut().pop() else {
            drop(stream); // busy: closed at once (G.6.2 step 0)
            continue;
        };
        let (host, pool) = (host.clone(), pool.clone());
        wasm_bindgen_futures::spawn_local(async move {
            let mut slot = slot;
            if with_timeout(SERVE_MS, serve_one(stream, &host, &mut slot)).await.is_err() {
                tracing::info!("board: a request took too long; dropped");
            }
            pool.borrow_mut().push(slot);
        });
    }
}

async fn serve_one(s: DataStream, host: &Weak<RefCell<Host>>, slot: &mut [u8; SLOT]) -> Result<(), String> {
    let (mut r, mut w) = s.split();
    let mut short = [0u8; SHORT];
    // The head (and, for a submit, the body after it) into the slot.
    let mut have = 0usize;
    let head_end = with_timeout(READ_MS, async {
        loop {
            if let Some(p) = slot[..have].windows(4).position(|x| x == b"\r\n\r\n") {
                return Ok(p + 4);
            }
            if have >= gateway::MAX_HEAD {
                return Err("head too large".to_owned());
            }
            let n = r.read(&mut slot[have..gateway::MAX_HEAD]).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("closed".to_owned());
            }
            have += n;
        }
    })
    .await;
    let resp: Out = 'resp: {
        let head_end = match head_end {
            Ok(e) => e,
            Err(e) if e == "head too large" => break 'resp Out::Short(gateway::status(431, &mut short)),
            Err(_) => return Ok(()),
        };
        let Some(h) = host.upgrade() else { return Ok(()) };
        let route = gateway::route(&slot[..head_end], h.borrow().served.name_text());
        match route {
            Err(code) => Out::Short(gateway::status(code, &mut short)),
            Ok(Route::Pow) => {
                let info = h.borrow_mut().intake.pow_info(now_s());
                Out::Short(gateway::pow(&info, &mut short))
            }
            Ok(Route::Submit(len)) => {
                drop(h);
                Out::Short(match submit_one(&mut r, host, slot, head_end, have, len).await {
                    Ok(Ok((no, seq))) => gateway::answer(no, seq, &mut short),
                    Ok(Err(refusal)) => gateway::refusal(refusal, &mut short),
                    Err(code) => gateway::status(code, &mut short),
                })
            }
            Ok(route) => Out::Shared(h.borrow().served.respond(&route)),
        }
    };
    let bytes = match &resp {
        Out::Short(n) => &short[..*n],
        Out::Shared(b) => &b[..],
    };
    let _ = w.write_all(bytes).await;
    let _ = w.flush().await;
    let _ = w.close().await;
    Ok(())
}

/// A response: short ones from the stack buffer (refusals, answers, `/pow`: no allocation),
/// the rest shared with the served state.
enum Out {
    Short(usize),
    Shared(Rc<[u8]>),
}

async fn read_to<R: futures::AsyncRead + Unpin>(r: &mut R, slot: &mut [u8; SLOT], have: &mut usize, want: usize) -> Result<(), String> {
    while *have < want {
        let n = r.read(&mut slot[*have..want]).await.map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("closed".into());
        }
        *have += n;
    }
    Ok(())
}

/// G.6.2 steps 1–9 for one submit: the header (checked before any body byte is read), the
/// body, then the wait for the publish. `Err(status)`: the request itself was bad or slow.
async fn submit_one<R: futures::AsyncRead + Unpin>(r: &mut R, host: &Weak<RefCell<Host>>, slot: &mut [u8; SLOT], body_at: usize, mut have: usize, len: usize) -> Result<Result<(u64, u64), Refusal>, u16> {
    let end = body_at + len;
    if len < HEADER_LEN || have > end {
        return Err(400);
    }
    with_timeout(READ_MS, read_to(r, slot, &mut have, body_at + HEADER_LEN)).await.map_err(|_| 408u16)?;
    let head: &[u8; HEADER_LEN] = slot[body_at..body_at + HEADER_LEN].try_into().expect("188 bytes");
    let now = now_s();
    let h = host.upgrade().ok_or(503u16)?;
    let (hd, next) = match h.borrow_mut().submit_header(head, len, now) {
        Ok(x) => x,
        Err(refusal) => return Ok(Err(refusal)),
    };
    drop(h);
    let replay_slot = match next {
        Next::Done { no, seq } => return Ok(Ok((no, seq))),
        Next::ReadBody(s) => s,
    };
    with_timeout(READ_MS, read_to(r, slot, &mut have, end)).await.map_err(|_| 408u16)?;
    let h = host.upgrade().ok_or(503u16)?;
    let id = match h.borrow_mut().submit_body(&hd, replay_slot, &slot[body_at + HEADER_LEN..end], now_s()) {
        Ok(id) => id,
        Err(refusal) => return Ok(Err(refusal)),
    };
    drop(h);
    let mut waited = 0;
    while waited < ANSWER_MS {
        sleep_ms(100).await;
        waited += 100;
        let h = host.upgrade().ok_or(503u16)?;
        if let Some(o) = h.borrow_mut().answer(id) {
            return Ok(o);
        }
    }
    Ok(Err(Refusal::Busy))
}
