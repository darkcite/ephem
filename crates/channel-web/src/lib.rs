// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! Public channels in the browser (§27, Appendix D), over the embedded Tor client. Part of the
//! Tor build of the app (Appendix F.3.3): in Tor mode it shares the chats' Tor client and
//! identity (`App::bind_channels`); the direct page loads that build only for its channel tabs,
//! with a Tor client of its own (`tor_start`) and a sign-in of its own (`sign_in`).
//!
//! - **Owner** (a saved identity; desktop): channel `index` gets its keys from the identity
//!   seed (derived, so unlinkable in public, §D.3); create, post, delete, sign the mirror list;
//!   every change rebuilds the blocks and signs the next IPNS record. Several channels can be
//!   open and online at once. The page stores each CAR and record (OPFS).
//! - **Gateway**: the channel is served on its own onion address as the read-only trustless
//!   gateway subset ([`ephem_channel::gateway`]); mirrors are served the same way.
//! - **Reader**: fetches the record and the CAR over Tor from an owner or mirror onion, or is
//!   handed them (a public gateway, fetched by the page), and verifies everything in Rust.
//!
//! Setup/UI path throughout (posting and reading are human-paced); the copies are of public
//! data: a response is built in full, then written to the stream.
// Browser-only (it drives the embedded Tor client's page runtime); empty on native targets,
// so `cargo test --workspace` builds.
#![cfg(all(target_arch = "wasm32", target_os = "unknown"))]

mod https;
mod json;

use ephem_channel::car;
use ephem_channel::channel::{self, Channel, View, RECORD_VALIDITY_S};
use ephem_channel::gateway::{self, Hosted};
use ephem_channel::{Cid, ipns};
use ephem_crypto::{Identity, keyfile};
use ephem_tor::web::{DataStream, Service, Snowflake, Tor, TorSlot, list, sleep_ms, tor_log};
use futures::{AsyncReadExt, AsyncWriteExt};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

/// The HTTP port of channel onions.
const PORT: u16 = 80;
/// A request head or a whole reader fetch that takes longer fails.
const REQUEST_TIMEOUT_MS: u32 = 30_000;
const FETCH_TIMEOUT_MS: u32 = 90_000;
/// Rounds over the given addresses before a read fails.
const READ_ROUNDS: u32 = 4;
/// The owner re-signs the record when it is older than this (§D.5.3).
const RESIGN_AFTER_S: u64 = 7 * 24 * 3600;

fn now_s() -> u64 {
    (js_sys::Date::now() / 1000.0) as u64
}

fn err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}

struct Own {
    index: u32,
    ch: Channel,
    record: Vec<u8>,
    hosted: Rc<RefCell<Hosted>>,
}

#[derive(Default)]
struct State {
    id: Option<Identity>,
    label: String,
    tor: TorSlot,
    /// The owner's open channels.
    own: Vec<Own>,
    /// Owned channels online, by name (a reopened channel keeps its service and cell).
    served: Vec<Served>,
    mirrors: Vec<Mirror>,
    /// Onion services launched so far (each needs its own arti nickname).
    launched: u32,
}

/// An owned channel's onion service and what it serves.
struct Served {
    name: Cid,
    onion: String,
    hosted: Rc<RefCell<Hosted>>,
    _svc: Rc<Service>,
}

impl State {
    /// The cell serving channel `name` (updated in place if it is online), holding `h`.
    fn cell(&self, name: &Cid, h: Hosted) -> Rc<RefCell<Hosted>> {
        match self.served.iter().find(|s| s.name == *name) {
            Some(s) => {
                *s.hosted.borrow_mut() = h;
                s.hosted.clone()
            }
            None => Rc::new(RefCell::new(h)),
        }
    }
}

#[wasm_bindgen]
pub struct ChannelApp {
    st: Rc<RefCell<State>>,
}

/// Rust side only (the chat app's `bind_channels`).
impl ChannelApp {
    /// The identity channels are owned with (Rust side: the chat app hands over its own) and
    /// the Tor client they use. A new identity closes the open channels.
    pub fn bind(&self, id: Option<Identity>, label: &str, tor: TorSlot) {
        let mut st = self.st.borrow_mut();
        let same = match (&st.id, &id) {
            (Some(a), Some(b)) => a.peer_id() == b.peer_id(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            st.own.clear();
        }
        st.id = id;
        st.label = label.to_owned();
        st.tor = tor;
    }
}

impl Default for ChannelApp {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl ChannelApp {
    #[wasm_bindgen(constructor)]
    pub fn new() -> ChannelApp {
        ChannelApp { st: Rc::default() }
    }

    fn tor(&self) -> Result<Rc<Tor>, JsValue> {
        self.st.borrow().tor.borrow().clone().ok_or_else(|| err("Tor is not started"))
    }

    // ---- identity (owners only; readers need none) ----

    /// Opens a key file (the passphrase buffer is wiped). Only the seed is kept: channel keys
    /// are derived from it and nothing of the chat identity is used (§D.3).
    pub fn sign_in(&self, blob: &[u8], pass: &mut [u8]) -> Result<(), JsValue> {
        let r = keyfile::open(blob, pass);
        pass.fill(0);
        let o = r.map_err(|e| err(e.name()))?;
        let mut st = self.st.borrow_mut();
        st.id = Some(Identity::from_seed(&o.seed));
        st.label = String::from_utf8_lossy(&o.label).into_owned();
        st.own.clear();
        Ok(())
    }

    pub fn label(&self) -> String {
        self.st.borrow().label.clone()
    }

    /// The IPNS name of channel `index` of the signed-in identity (`k51…`).
    pub fn channel_name(&self, index: u32) -> Result<String, JsValue> {
        let st = self.st.borrow();
        let (mut sign, mut onion) = st.id.as_ref().ok_or_else(|| err("sign in first"))?.channel_seeds(index);
        let name = Channel::new(&sign, "", "", 0).map(|c| c.name().to_text());
        sign.fill(0);
        onion.fill(0);
        name.map_err(|_| err("channel key"))
    }

    // ---- Tor ----

    /// As the chat's Tor mode (`tor.html`): Snowflake bridge lines (Appendix F.2), NAT hint, lab
    /// network (empty = real Tor), directory snapshot for a warm start.
    pub fn tor_start(&self, bridges: &str, nat: &str, network_toml: &str, cache: &[u8]) -> Result<js_sys::Promise, JsValue> {
        let b = ephem_tor::bridge::parse(bridges);
        if !b.usable() {
            return Err(err("no usable snowflake bridge line"));
        }
        let sf = Snowflake { brokers: b.brokers, fingerprints: b.fingerprints, ice: b.ice, nat: if nat.is_empty() { "unknown".into() } else { nat.into() } };
        let tor = Rc::new(Tor::new(sf, network_toml, cache).map_err(err)?);
        *self.st.borrow().tor.borrow_mut() = Some(tor.clone());
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            tor.bootstrap().await.map_err(err)?;
            Ok(JsValue::UNDEFINED)
        }))
    }

    pub fn tor_status(&self) -> String {
        self.tor().map_or_else(|_| String::new(), |t| t.status())
    }

    pub fn tor_cache(&self) -> Vec<u8> {
        self.tor().ok().and_then(|t| t.cache()).unwrap_or_default()
    }

    pub fn tor_log(&self, level: &str) {
        tor_log(level);
    }

    // ---- owner ----

    /// A new channel `index` (§D.3: use a separate identity for channels). Replaces nothing
    /// stored: the page refuses when this channel already exists in its store.
    pub fn create(&self, index: u32, title: &str, about: &str) -> Result<(), JsValue> {
        let now = now_s();
        let ch = {
            let st = self.st.borrow();
            let (mut sign, mut onion) = st.id.as_ref().ok_or_else(|| err("sign in first"))?.channel_seeds(index);
            onion.fill(0);
            let ch = Channel::new(&sign, title, about, now);
            sign.fill(0);
            ch.map_err(|e| err(format!("{e:?}")))?
        };
        self.set_own(index, ch, now);
        Ok(())
    }

    /// Reopens channel `index` from its stored CAR and record (the page's store, or an
    /// imported backup). A record older than 7 days is re-signed (§D.5.3).
    pub fn open(&self, index: u32, car_bytes: &[u8], record: &[u8]) -> Result<(), JsValue> {
        let now = now_s();
        let (roots, blocks) = car::read(car_bytes).ok_or_else(|| err("not a valid CAR file"))?;
        let ch = {
            let st = self.st.borrow();
            let (mut sign, mut onion) = st.id.as_ref().ok_or_else(|| err("sign in first"))?.channel_seeds(index);
            onion.fill(0);
            let name = Channel::new(&sign, "", "", 0).map_err(|_| err("channel key"))?.name();
            // Our own last record: its sequence continues, its root must be the CAR's.
            let rec = ipns::verify(&name, record, 0).map_err(|e| err(format!("record: {e:?}")))?;
            let root = rec.value.strip_prefix("/ipfs/").and_then(Cid::parse).filter(|r| roots.contains(r)).ok_or_else(|| err("the record does not match the CAR"))?;
            let ch = Channel::load(&sign, &root, &blocks, rec.sequence);
            sign.fill(0);
            let ch = ch.map_err(|e| err(format!("{e:?}")))?;
            if rec.validity >= now + RECORD_VALIDITY_S - RESIGN_AFTER_S {
                // Fresh enough: keep serving the stored record and blocks as they are.
                drop(st);
                let mut st = self.st.borrow_mut();
                let hosted = st.cell(&ch.name(), Hosted::new(ch.name(), root, record.to_vec(), blocks));
                st.own.retain(|o| o.index != index);
                st.own.push(Own { index, ch, record: record.to_vec(), hosted });
                return Ok(());
            }
            ch
        };
        self.set_own(index, ch, now);
        Ok(())
    }

    /// Rebuilds the blocks and signs the next record (after every change).
    fn set_own(&self, index: u32, mut ch: Channel, now: u64) {
        let (root, blocks) = ch.build(now);
        let record = ch.record(&root, now);
        let mut st = self.st.borrow_mut();
        let hosted = Hosted::new(ch.name(), root, record.clone(), blocks);
        // The onion serving the channel keeps the same `Hosted` cell: update it in place.
        match st.own.iter_mut().find(|o| o.index == index) {
            Some(o) => {
                *o.hosted.borrow_mut() = hosted;
                o.ch = ch;
                o.record = record;
            }
            None => {
                let hosted = st.cell(&ch.name(), hosted);
                st.own.push(Own { index, ch, record, hosted });
            }
        }
    }

    fn change(&self, index: u32, f: impl FnOnce(&mut Channel) -> Result<(), channel::ChannelError>) -> Result<(), JsValue> {
        let ch = {
            let st = self.st.borrow();
            let own = st.own.iter().find(|o| o.index == index).ok_or_else(|| err("that channel is not open"))?;
            // Work on a copy: a refused change leaves the channel as it was.
            let mut copy = own.ch.clone();
            f(&mut copy).map_err(|e| err(format!("{e:?}")))?;
            copy
        };
        self.set_own(index, ch, now_s());
        Ok(())
    }

    /// Indices of the open channels.
    pub fn open_channels(&self) -> Vec<u32> {
        self.st.borrow().own.iter().map(|o| o.index).collect()
    }

    /// Publishes a post (≤ 4 KiB) in channel `index`; `reply` = the `seq` it answers, or 0.
    pub fn post(&self, index: u32, body: &str, reply: u32) -> Result<(), JsValue> {
        let now = now_s();
        self.change(index, |c| c.post(body, u64::from(reply), now).map(|_| ()))
    }

    pub fn delete(&self, index: u32, seq: u32) -> Result<(), JsValue> {
        self.change(index, |c| c.delete(u64::from(seq)))
    }

    /// Signs the mirror list (comma-separated onion addresses) into the manifest (§D.7.1).
    pub fn set_mirrors(&self, index: u32, csv: &str) -> Result<(), JsValue> {
        let list = list(csv);
        self.change(index, |c| c.set_mirrors(list))
    }

    /// Channel `index` as JSON (see [`json::view`]); empty if it is not open.
    pub fn view(&self, index: u32) -> String {
        let st = self.st.borrow();
        let Some(o) = st.own.iter().find(|o| o.index == index) else { return String::new() };
        let h = o.hosted.borrow();
        let rec = ipns::verify(&h.name, &h.record, 0).ok();
        json::view(&View {
            name: h.name.clone(),
            root: h.root.clone(),
            record: rec.unwrap_or(ipns::Record { value: String::new(), sequence: 0, validity: 0, ttl_ns: 0 }),
            manifest: o.ch.manifest.clone(),
            posts: o.ch.posts.clone(),
            updated: now_s(),
        })
    }

    /// Channel `index`'s whole CAR (store, export, Kubo import).
    pub fn car(&self, index: u32) -> Vec<u8> {
        self.st.borrow().own.iter().find(|o| o.index == index).map(|o| o.hosted.borrow().car()).unwrap_or_default()
    }

    /// Channel `index`'s current signed record.
    pub fn record(&self, index: u32) -> Vec<u8> {
        self.st.borrow().own.iter().find(|o| o.index == index).map(|o| o.record.clone()).unwrap_or_default()
    }

    /// Serves channel `index` on its own onion address (§D.2); returns `<56 chars>.onion`. A
    /// channel already online keeps its address (and serves its latest version).
    pub fn serve(&self, index: u32) -> Result<String, JsValue> {
        let tor = self.tor()?;
        let (name, hosted) = {
            let st = self.st.borrow();
            let own = st.own.iter().find(|o| o.index == index).ok_or_else(|| err("that channel is not open"))?;
            let name = own.hosted.borrow().name.clone();
            if let Some(s) = st.served.iter().find(|s| s.name == name) {
                return Ok(s.onion.clone());
            }
            (name, own.hosted.clone())
        };
        let mut onion_seed = {
            let st = self.st.borrow();
            let (mut sign, onion) = st.id.as_ref().ok_or_else(|| err("sign in first"))?.channel_seeds(index);
            sign.fill(0);
            onion
        };
        let nick = {
            let mut st = self.st.borrow_mut();
            st.launched += 1;
            format!("channel{}", st.launched)
        };
        let svc = tor.launch(&nick, &onion_seed);
        onion_seed.fill(0);
        let svc = Rc::new(svc.map_err(err)?);
        let onion = svc.onion().to_owned();
        wasm_bindgen_futures::spawn_local(serve_loop(svc.clone(), hosted.clone()));
        self.st.borrow_mut().served.push(Served { name, onion: onion.clone(), hosted, _svc: svc });
        Ok(onion)
    }

    /// Publishes channel `index`'s record to the IPFS routing network (§D.5.2, optional):
    /// `PUT https://delegated-ipfs.dev/routing/v1/ipns/<name>` **through a Tor exit**, so the
    /// owner stays hidden. It only matters if some IPFS node holds the content (a follower's
    /// Kubo mirror). `host`/`extra_root`: the lab's stand-in; the page passes the real host.
    pub fn publish_ipfs(&self, index: u32, host: &str, extra_root: &[u8]) -> Result<js_sys::Promise, JsValue> {
        let tor = self.tor()?;
        let (name, record) = {
            let st = self.st.borrow();
            let own = st.own.iter().find(|o| o.index == index).ok_or_else(|| err("that channel is not open"))?;
            (own.hosted.borrow().name.to_text(), own.record.clone())
        };
        let (host, root) = (host.to_owned(), extra_root.to_vec());
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let path = format!("/routing/v1/ipns/{name}");
            let status = with_timeout(FETCH_TIMEOUT_MS, https::put(&tor, &host, &path, gateway::CT_RECORD, &record, &root)).await.map_err(err)?;
            if (200..300).contains(&status) {
                Ok(JsValue::from(status))
            } else {
                Err(err(format!("the routing service answered {status}")))
            }
        }))
    }

    // ---- readers and mirrors ----

    /// Reads channel `name` over Tor from the first onion (comma-separated: owner, mirrors) that
    /// serves a valid state; `min_seq` is the reader's high-water mark (§D.8). Resolves to the
    /// JSON view plus the raw record and CAR (for a mirror or a local copy).
    pub fn read(&self, name: &str, onions: &str, min_seq: f64) -> Result<js_sys::Promise, JsValue> {
        let name = Cid::parse(name).filter(|c| c.ed25519_key().is_some()).ok_or_else(|| err("not a channel name"))?;
        let tor = self.tor()?;
        let onions = list(onions);
        if onions.is_empty() {
            return Err(err("no onion address to read from"));
        }
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            let mut last = String::from("no onion address");
            // Each round tries every address; later rounds on fresh circuits and descriptors (a
            // host that just came online, or one whose descriptor we cached stale).
            // All addresses at once (the owner may be offline and a mirror up): the first to
            // deliver a valid state wins.
            for round in 0..READ_ROUNDS {
                let tries = onions.iter().map(|onion| {
                    let (tor, name) = (tor.clone(), name.clone());
                    Box::pin(async move { fetch_channel(&tor, &name, onion, min_seq as u64, round > 0).await.map_err(|e| format!("{onion}: {e}")) })
                });
                match futures::future::select_ok(tries).await {
                    Ok((r, _)) => return Ok(r.into()),
                    Err(e) => {
                        tracing::info!("channel: {e}");
                        last = e;
                    }
                }
                sleep_ms(2_000 << round).await;
            }
            Err(err(last))
        }))
    }

    /// The root CID (`bafy…`) a record names, after verifying it for `name` (a public
    /// gateway read: the page fetches the record, then the CAR of this root).
    pub fn record_root(&self, name: &str, record: &[u8]) -> Result<String, JsValue> {
        let name = Cid::parse(name).ok_or_else(|| err("not a channel name"))?;
        let rec = ipns::verify(&name, record, now_s()).map_err(|e| err(format!("record: {e:?}")))?;
        rec.value.strip_prefix("/ipfs/").filter(|r| Cid::parse(r).is_some()).map(str::to_owned).ok_or_else(|| err("record value"))
    }

    /// Verifies a record and a CAR fetched by the page (a public gateway, an imported file).
    pub fn verify(&self, name: &str, record: &[u8], car_bytes: &[u8], min_seq: f64) -> Result<Reading, JsValue> {
        let name = Cid::parse(name).ok_or_else(|| err("not a channel name"))?;
        verified(&name, record, car_bytes, min_seq as u64).map_err(err)
    }

    /// Mirrors a verified channel (§D.7.1) on this tab's onion for it. `seed` (32 bytes, kept
    /// by the page per channel) makes the mirror's address stable across visits, so the owner
    /// can sign it into the mirror list. Mirroring the same channel again (a newer version)
    /// updates what is served, on the same address. Returns `<56 chars>.onion`.
    pub fn mirror(&self, reading: &Reading, seed: &[u8]) -> Result<String, JsValue> {
        let (roots, blocks) = car::read(&reading.car).ok_or_else(|| err("CAR"))?;
        let name = Cid::parse(&reading.name).ok_or_else(|| err("name"))?;
        let root = roots.into_iter().next().ok_or_else(|| err("CAR has no root"))?;
        let hosted = Hosted::new(name.clone(), root, reading.record.clone(), blocks);
        {
            let st = self.st.borrow();
            if let Some(m) = st.mirrors.iter().find(|m| m.name == name) {
                // Only ever forward: a mirror never serves an older version than it has.
                if reading.seq > m.seq.get() {
                    *m.hosted.borrow_mut() = hosted;
                    m.seq.set(reading.seq);
                }
                return Ok(m.onion.clone());
            }
        }
        let tor = self.tor()?;
        let seed: [u8; 32] = seed.try_into().map_err(|_| err("mirror seed must be 32 bytes"))?;
        let nick = {
            let mut st = self.st.borrow_mut();
            st.launched += 1;
            format!("mirror{}", st.launched)
        };
        let svc = Rc::new(tor.launch(&nick, &seed).map_err(err)?);
        let onion = svc.onion().to_owned();
        let hosted = Rc::new(RefCell::new(hosted));
        wasm_bindgen_futures::spawn_local(serve_loop(svc.clone(), hosted.clone()));
        self.st.borrow_mut().mirrors.push(Mirror { name, onion: onion.clone(), hosted, seq: std::cell::Cell::new(reading.seq), _svc: svc });
        Ok(onion)
    }
}

/// A channel this tab mirrors.
struct Mirror {
    name: Cid,
    onion: String,
    hosted: Rc<RefCell<Hosted>>,
    seq: std::cell::Cell<u64>,
    _svc: Rc<Service>,
}

/// A verified channel as handed to the page: the JSON view, and the bytes it came from.
#[wasm_bindgen]
pub struct Reading {
    name: String,
    json: String,
    record: Vec<u8>,
    car: Vec<u8>,
    seq: u64,
}

#[wasm_bindgen]
impl Reading {
    #[wasm_bindgen(getter)]
    pub fn json(&self) -> String {
        self.json.clone()
    }

    #[wasm_bindgen(getter)]
    pub fn sequence(&self) -> f64 {
        self.seq as f64
    }

    pub fn record(&self) -> Vec<u8> {
        self.record.clone()
    }

    pub fn car(&self) -> Vec<u8> {
        self.car.clone()
    }
}

fn verified(name: &Cid, record: &[u8], car_bytes: &[u8], min_seq: u64) -> Result<Reading, String> {
    let (_, blocks) = car::read(car_bytes).ok_or("the CAR is invalid or a block does not match its CID")?;
    let v = channel::verify(name, record, &blocks, now_s(), min_seq).map_err(|e| format!("{e:?}"))?;
    Ok(Reading { name: name.to_text(), json: json::view(&v), record: record.to_vec(), car: car_bytes.to_vec(), seq: v.record.sequence })
}

async fn with_timeout<T>(ms: u32, f: impl std::future::Future<Output = Result<T, String>>) -> Result<T, String> {
    let t = sleep_ms(ms);
    futures::pin_mut!(f, t);
    match futures::future::select(f, t).await {
        futures::future::Either::Left((r, _)) => r,
        futures::future::Either::Right(_) => Err("timed out".into()),
    }
}

/// One GET over a new Tor stream to `onion`; the body of a 200 response.
async fn http_get(tor: &Tor, onion: &str, path: &str, fresh: bool) -> Result<Vec<u8>, String> {
    let s = tor.connect(onion, PORT, fresh).await?;
    let (mut r, mut w) = s.split();
    w.write_all(&gateway::get(onion, path)).await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;
    let mut resp = Vec::new();
    let mut buf = vec![0u8; 16 * 1024];
    // Until the response is complete; an end of stream after that is not an error.
    while !gateway::complete(&resp) {
        let n = match r.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if gateway::complete(&resp) => {
                tracing::debug!("channel: stream ended after the response: {e}");
                break;
            }
            Err(e) => return Err(e.to_string()),
        };
        if resp.len() + n > gateway::MAX_RESPONSE + gateway::MAX_HEAD {
            return Err("response too large".into());
        }
        resp.extend_from_slice(&buf[..n]);
    }
    gateway::parse_response(&resp).map(<[u8]>::to_vec).map_err(|e| format!("{e:?}"))
}

async fn fetch_channel(tor: &Tor, name: &Cid, onion: &str, min_seq: u64, fresh: bool) -> Result<Reading, String> {
    with_timeout(FETCH_TIMEOUT_MS, async {
        let record = http_get(tor, onion, &format!("/ipns/{}?format=ipns-record", name.to_text()), fresh).await?;
        let rec = ipns::verify(name, &record, now_s()).map_err(|e| format!("record: {e:?}"))?;
        if rec.sequence < min_seq {
            return Err(format!("older than what we saw (sequence {} < {min_seq})", rec.sequence));
        }
        let root = rec.value.strip_prefix("/ipfs/").ok_or("record value")?;
        let car_bytes = http_get(tor, onion, &format!("/ipfs/{root}?format=car"), false).await?;
        verified(name, &record, &car_bytes, min_seq)
    })
    .await
}

async fn serve_loop(svc: Rc<Service>, hosted: Rc<RefCell<Hosted>>) {
    loop {
        let s = svc.accept().await;
        wasm_bindgen_futures::spawn_local(serve_one(s, hosted.clone()));
    }
}

/// One request per stream (`Connection: close`).
async fn serve_one(s: DataStream, hosted: Rc<RefCell<Hosted>>) {
    let (mut r, mut w) = s.split();
    let head = with_timeout(REQUEST_TIMEOUT_MS, async {
        let mut head = Vec::with_capacity(512);
        let mut buf = [0u8; 1024];
        while !gateway::head_complete(&head) && head.len() <= gateway::MAX_HEAD {
            let n = r.read(&mut buf).await.map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("closed".to_owned());
            }
            head.extend_from_slice(&buf[..n]);
        }
        Ok(head)
    })
    .await;
    let head = match head {
        Ok(h) => h,
        Err(e) => {
            tracing::info!("channel: a request was not read: {e}");
            return;
        }
    };
    let resp = gateway::respond(&head, &hosted.borrow());
    tracing::debug!("channel: served {} bytes", resp.len());
    let _ = w.write_all(&resp).await;
    let _ = w.flush().await;
    let _ = w.close().await;
}
