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

mod boards;
mod https;
mod json;

pub use boards::{BoardApp, Draft};

use ephem_channel::car;
use ephem_channel::cbor::Value;
use ephem_channel::channel::{self, Base, Channel, View, RECORD_VALIDITY_S};
use ephem_channel::vault::{self, Entry, Lease, Vault};
use ephem_channel::gateway::{self, Hosted};
use ephem_channel::page::Served;
use ephem_channel::{Cid, ipns};
use ephem_crypto::{Identity, keyfile};
use ephem_tor::web::{DataStream, Service, Snowflake, Tor, TorSlot, list, onion_address, sleep_ms, tor_log};
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
/// After the first valid answer of a read, how long the other sources may take to offer a newer
/// version (M-3).
const NEWEST_GRACE_MS: u32 = 4_000;
/// The owner re-signs the record when it is older than this (§D.5.3).
const RESIGN_AFTER_S: u64 = 7 * 24 * 3600;
/// A probe's try (each of its two) that takes longer counts as no answer.
const PROBE_MS: u32 = 45_000;

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
    /// Signed in here (`sign_in`, a separate identity for channels): the chat app's identity
    /// (`bind`) no longer replaces it.
    separate: bool,
    tor: TorSlot,
    /// The owner's open channels.
    own: Vec<Own>,
    /// Owned channels online, by name (a reopened channel keeps its service and cell).
    served: Vec<Online>,
    mirrors: Vec<Mirror>,
    /// Onion services launched so far (each needs its own arti nickname).
    launched: u32,
    /// The newest vault seen or published (§D.11) and its record sequence. `vault_seq` may also
    /// be a floor the page remembered from earlier visits (`vault_floor`); `vault_known` says
    /// whether `vault` holds what that sequence names (only then may this tab publish over it).
    vault: Vault,
    vault_seq: u64,
    vault_known: bool,
    /// A vault written by a newer app was seen (BW-3): this tab never publishes over it, even
    /// after a later 404 from the routing service.
    vault_newer: bool,
    /// Boards hosted here (docs/BOARDS.md).
    boards: boards::Boards,
    /// Bumped at every identity change (BW-8): work started under another identity (a takeover,
    /// a mirror) checks it after each wait and drops its result.
    pub(crate) generation: u64,
}

/// An owned channel's onion service and what it serves.
struct Online {
    name: Cid,
    onion: String,
    hosted: Rc<RefCell<Hosted>>,
    _svc: Rc<Service>,
}

impl State {
    /// Another identity takes over this tab's channels (security audit M-1): nothing of the
    /// previous one may stay online here or leak into the new one's vault. Its channel and
    /// mirror onions go down; the vault and its sequence are forgotten.
    fn forget_identity(&mut self) {
        self.own.clear();
        self.served.clear();
        self.mirrors.clear();
        self.vault = Vault::default();
        self.vault_seq = 0;
        self.vault_known = false;
        self.vault_newer = false;
        self.boards.hosted.clear();
        // Board mirrors and the deletions read here go too (BW-9, as channel mirrors, M-1).
        self.boards.forget();
        self.generation += 1;
    }

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
        st.tor = tor;
        if st.separate {
            return;
        }
        let same = match (&st.id, &id) {
            (Some(a), Some(b)) => a.peer_id() == b.peer_id(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            st.forget_identity();
        }
        st.id = id;
        st.label = label.to_owned();
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
        let id = Identity::from_seed(&o.seed);
        if st.id.as_ref().is_none_or(|a| a.peer_id() != id.peer_id()) {
            st.forget_identity();
        } else {
            st.own.clear();
        }
        st.id = Some(id);
        st.label = String::from_utf8_lossy(&o.label).into_owned();
        st.separate = true;
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

    /// The onion address of channel `index` (`<56 chars>.onion`), derived like its keys: the
    /// same on every device of the identity, so a device can read what another one serves.
    pub fn channel_onion(&self, index: u32) -> Result<String, JsValue> {
        let st = self.st.borrow();
        let (mut sign, mut onion) = st.id.as_ref().ok_or_else(|| err("sign in first"))?.channel_seeds(index);
        sign.fill(0);
        let pk = ed25519_dalek::SigningKey::from_bytes(&onion).verifying_key().to_bytes();
        onion.fill(0);
        Ok(onion_address(&pk))
    }

    // ---- Tor ----

    /// As the chat's Tor mode (`tor.html`): Snowflake bridge lines (Appendix F.2), NAT hint, lab
    /// network (empty = real Tor), directory snapshot for a warm start.
    pub fn tor_start(&self, bridges: &str, nat: &str, network_toml: &str, cache: &[u8]) -> Result<js_sys::Promise, JsValue> {
        let b = ephem_tor::bridge::parse(bridges);
        if !b.usable() {
            return Err(err("no usable snowflake bridge line"));
        }
        let sf = Snowflake { brokers: b.brokers, fingerprints: b.fingerprints, routes: b.routes, ice: b.ice, nat: if nat.is_empty() { "unknown".into() } else { nat.into() } };
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
                let hosted = st.cell(&ch.name(), Hosted::new(ch.name(), root, record.to_vec(), blocks, Served::Owner));
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
        let hosted = Hosted::owned(root.clone(), record.clone(), blocks, &ch.view(&root, now));
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
            missing: o.ch.base.as_ref().map_or(0, |b| b.count),
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
        wasm_bindgen_futures::spawn_local(serve_loop(Rc::downgrade(&svc), hosted.clone()));
        self.st.borrow_mut().served.push(Online { name, onion: onion.clone(), hosted, _svc: svc });
        Ok(onion)
    }

    /// Whether readers can reach channel `index`'s onion yet (see `Service::reach`): empty if it
    /// is not served.
    pub fn reach(&self, index: u32) -> String {
        let st = self.st.borrow();
        let Some(o) = st.own.iter().find(|o| o.index == index) else { return String::new() };
        let name = o.hosted.borrow().name.clone();
        st.served.iter().find(|s| s.name == name).map_or_else(String::new, |s| s._svc.reach().to_owned())
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
            let status = with_timeout(FETCH_TIMEOUT_MS, https::request(&tor, "PUT", &host, &path, gateway::CT_RECORD, &record, &root)).await.map_err(err)?.0;
            if (200..300).contains(&status) {
                Ok(JsValue::from(status))
            } else {
                Err(err(format!("the routing service answered {status}")))
            }
        }))
    }

    // ---- the vault: one identity on several devices (§D.11) ----

    /// The vault's IPNS name (`k51…`) of the signed-in identity.
    pub fn vault_name(&self) -> Result<String, JsValue> {
        let (mut sign, mut key) = self.vault_seeds()?;
        key.fill(0);
        let n = vault::name(&sign).to_text();
        sign.fill(0);
        Ok(n)
    }

    fn vault_seeds(&self) -> Result<([u8; 32], [u8; 32]), JsValue> {
        Ok(self.st.borrow().id.as_ref().ok_or_else(|| err("sign in first"))?.vault_seeds())
    }

    /// Fetches the vault record through a Tor exit (`GET https://<host>/routing/v1/ipns/<vault
    /// name>`), verifies and opens it. Resolves to the vault as JSON ([`vault_json`]), or to ""
    /// when there is none (404: never published, or forgotten by the DHT). A record older than
    /// one seen before is ignored (the newer one stays); an equal one replaces ours.
    pub fn vault_fetch(&self, host: &str, extra_root: &[u8]) -> Result<js_sys::Promise, JsValue> {
        let tor = self.tor()?;
        let name = vault::name(&{
            let (sign, mut key) = self.vault_seeds()?;
            key.fill(0);
            sign
        });
        let (host, root, st) = (host.to_owned(), extra_root.to_vec(), self.st.clone());
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            // A fresh URL each time: the service caches answers by URL (a takeover would stay
            // unseen for minutes, §D.11.6).
            let mut nonce = [0u8; 8];
            ephem_crypto::random(&mut nonce);
            let path = format!("/routing/v1/ipns/{}?fresh={:016x}", name.to_text(), u64::from_le_bytes(nonce));
            let (status, body) = with_timeout(FETCH_TIMEOUT_MS, https::request(&tor, "GET", &host, &path, gateway::CT_RECORD, &[], &root)).await.map_err(err)?;
            // No vault yet: 404, or (delegated-ipfs.dev, 2026-09-29) 200 with a text body
            // "delegate error: routing: not found".
            let missing = std::str::from_utf8(&body).is_ok_and(|t| t.contains("not found"));
            if status == 404 || (status == 200 && missing) {
                // The record expired from the DHT (13–86 h without a republish, V-P2) or never
                // existed: this tab may publish from what it holds. Not over a vault a newer
                // app wrote (BW-3): that one may only be missing for now.
                let mut st = st.borrow_mut();
                if st.vault_newer {
                    return Err(err("E_VAULT_NEWER: your other device runs a newer Ephem; update the app on this device to manage your channels and boards here"));
                }
                st.vault_known = true;
                return Ok(JsValue::from_str(""));
            }
            if status != 200 {
                return Err(err(format!("the routing service answered {status}")));
            }
            let (_, mut key) = st.borrow().id.as_ref().ok_or_else(|| err("sign in first"))?.vault_seeds();
            let opened = vault::open(&name, &key, &body, now_s());
            key.fill(0);
            if let Err(vault::VaultError::Newer(seq)) = opened {
                // Written by a newer app: never overwrite it from here (docs/BOARDS.md G.13.1).
                let mut st = st.borrow_mut();
                st.vault_seq = st.vault_seq.max(seq);
                st.vault_known = false;
                st.vault_newer = true;
                return Err(err("E_VAULT_NEWER: your other device runs a newer Ephem; update the app on this device to manage your channels and boards here"));
            }
            let (v, seq) = opened.map_err(|e| err(format!("vault: {e:?}")))?;
            let mut st = st.borrow_mut();
            // On a tie the network's record wins: two devices can publish the same sequence at
            // once (a renewal racing a takeover), and the routing service keeps only one.
            if seq >= st.vault_seq {
                st.vault = v;
                st.vault_seq = seq;
                st.vault_known = true;
            } else if !st.vault_known {
                // Older than this device has seen (a stale or replayed record, M-2): neither adopt
                // it nor publish over the newer one we cannot read.
                return Err(err(format!("the routing service returned an older list of your channels (version {seq}; this device has seen {})", st.vault_seq)));
            }
            Ok(JsValue::from_str(&vault_json(&st.vault, st.vault_seq)))
        }))
    }

    /// The highest vault sequence this device saw on an earlier visit (the page keeps it): an
    /// older vault record is refused from now on (§D.11.5, security audit M-2).
    pub fn vault_floor(&self, seq: f64) {
        let mut st = self.st.borrow_mut();
        if seq as u64 > st.vault_seq {
            st.vault_seq = seq as u64;
            st.vault_known = false;
        }
    }

    /// The vault sequence this tab saw or published (0: none).
    pub fn vault_seq(&self) -> f64 {
        self.st.borrow().vault_seq as f64
    }

    /// The vault as last fetched or published (JSON), "" before either.
    pub fn vault(&self) -> String {
        let st = self.st.borrow();
        if st.vault_seq == 0 { String::new() } else { vault_json(&st.vault, st.vault_seq) }
    }

    /// Publishes the vault (the open channels, plus the channels of the last vault that are not
    /// open here) with the writer lease `{device (32 hex), until}` through a Tor exit (`PUT`, as
    /// `publish_ipfs`). Resolves to the new sequence.
    pub fn vault_publish(&self, host: &str, extra_root: &[u8], device: &str, until: f64) -> Result<js_sys::Promise, JsValue> {
        publish_vault(&self.st, host, extra_root, device, until, true)
    }

    /// Continues channel `index` from the vault, without its blocks (§D.11.3 step 4): the
    /// manifest is re-signed, new posts go on top of the older chain, which joins when a host
    /// holding it is found (`backfill`).
    pub fn resume(&self, index: u32) -> Result<(), JsValue> {
        let ch = {
            let st = self.st.borrow();
            let e = st.vault.entries.iter().find(|e| e.index == index).ok_or_else(|| err("that channel is not in the vault"))?;
            let (mut sign, mut onion) = st.id.as_ref().ok_or_else(|| err("sign in first"))?.channel_seeds(index);
            onion.fill(0);
            let base = e.head.clone().map(|head| Base { head, count: e.count });
            let ch = Channel::resume(&sign, &e.title, &e.about, e.created, e.mirrors.clone(), base, e.record_seq);
            sign.fill(0);
            ch.map_err(|e| err(format!("{e:?}")))?
        };
        self.set_own(index, ch, now_s());
        Ok(())
    }

    /// Joins the older posts of channel `index` from a CAR (fetched from a host that holds
    /// them, or a backup); still-missing ones stay missing. Resolves the view's `missing`.
    pub fn backfill(&self, index: u32, car_bytes: &[u8]) -> Result<f64, JsValue> {
        let (_, blocks) = car::read(car_bytes).ok_or_else(|| err("not a valid CAR file"))?;
        self.change(index, |c| c.backfill(&blocks))?;
        let st = self.st.borrow();
        Ok(st.own.iter().find(|o| o.index == index).and_then(|o| o.ch.base.as_ref()).map_or(0.0, |b| b.count as f64))
    }

    /// Stops writing here: the open channels close and their onions go down (another device
    /// holds the lease, §D.11.3 step 5). The store keeps them for reading.
    pub fn release(&self) {
        let mut st = self.st.borrow_mut();
        st.own.clear();
        st.served.clear();
    }

    /// The page signed out of channels or switched identity (also done by `bind` and
    /// `sign_in` when the identity changes, M-1).
    pub fn forget(&self) {
        self.st.borrow_mut().forget_identity();
    }

    // ---- readers and mirrors ----

    /// Whether the onion service at `onion` (a followed channel's or board's owner) accepts a
    /// stream, i.e. its host is online. Nothing is sent: the stream is dropped once open. The
    /// second try runs on a fresh circuit (a host that restarted has new introduction points).
    /// Resolves to a bool; only a malformed address rejects.
    pub fn probe(&self, onion: &str) -> Result<js_sys::Promise, JsValue> {
        if !ephem_tor::web::is_onion(onion) {
            return Err(err("not an onion address"));
        }
        let tor = self.tor()?;
        let onion = onion.to_owned();
        Ok(wasm_bindgen_futures::future_to_promise(async move {
            for fresh in [false, true] {
                if with_timeout(PROBE_MS, tor.connect(&onion, PORT, fresh)).await.is_ok() {
                    return Ok(JsValue::TRUE);
                }
            }
            Ok(JsValue::FALSE)
        }))
    }

    /// Reads channel `name` over Tor from the first onion (comma-separated: owner, mirrors) that
    /// serves a valid state; `min_seq` is the reader's high-water mark (§D.8). Resolves to the
    /// JSON view plus the raw record and CAR (for a mirror or a local copy).
    pub fn read(&self, name: &str, onions: &str, min_seq: f64) -> Result<js_sys::Promise, JsValue> {
        let name = Cid::parse(name).filter(|c| c.ed25519_key().is_some()).ok_or_else(|| err("not a channel name"))?;
        let tor = self.tor()?;
        // Well-formed onions only, each once, at most the owner's plus a full mirror list (L-1).
        let mut onions_ok: Vec<String> = Vec::with_capacity(1 + ephem_channel::channel::MAX_MIRRORS);
        for o in list(onions) {
            if ephem_tor::web::is_onion(&o) && !onions_ok.contains(&o) && onions_ok.len() <= ephem_channel::channel::MAX_MIRRORS {
                onions_ok.push(o);
            }
        }
        let onions = onions_ok;
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
                let mut tries: futures::stream::FuturesUnordered<_> = onions
                    .iter()
                    .map(|onion| {
                        let (tor, name) = (tor.clone(), name.clone());
                        Box::pin(async move { fetch_channel(&tor, &name, onion, min_seq as u64, round > 0).await.map_err(|e| format!("{onion}: {e}")) })
                    })
                    .collect();
                // The newest valid state wins, not the first (security audit M-3: a stale or
                // malicious mirror that answers fast would hold readers on an old version). After
                // the first valid answer the others get NEWEST_GRACE_MS to beat it.
                let mut best: Option<Reading> = None;
                // Started at the first valid answer.
                let mut grace = None;
                loop {
                    let next = futures::StreamExt::next(&mut tries);
                    let r = match grace.as_mut() {
                        Some(g) => match futures::future::select(next, g).await {
                            futures::future::Either::Left((r, _)) => r,
                            futures::future::Either::Right(_) => break,
                        },
                        None => next.await,
                    };
                    match r {
                        None => break,
                        Some(Ok(r)) => {
                            if best.as_ref().is_none_or(|b| r.seq > b.seq) {
                                best = Some(r);
                            }
                            if grace.is_none() {
                                grace = Some(Box::pin(sleep_ms(NEWEST_GRACE_MS)));
                            }
                        }
                        Some(Err(e)) => {
                            tracing::info!("channel: {e}");
                            last = e;
                        }
                    }
                }
                if let Some(r) = best {
                    return Ok(r.into());
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
        let hosted = Hosted::new(name.clone(), root, reading.record.clone(), blocks, Served::Mirror);
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
        wasm_bindgen_futures::spawn_local(serve_loop(Rc::downgrade(&svc), hosted.clone()));
        self.st.borrow_mut().mirrors.push(Mirror { name, onion: onion.clone(), hosted, seq: std::cell::Cell::new(reading.seq), _svc: svc });
        Ok(onion)
    }
}

/// Publishes the vault (§D.11, docs/BOARDS.md G.13) through a Tor exit. `channels`: this device
/// writes the channels (their lease and entries are refreshed); otherwise they stay as last read
/// and only the boards hosted here are (each board has its own lease). Boards hosted here get the
/// lease `{device, until}`; other boards stay as last read. Resolves to the new sequence.
fn publish_vault(st_rc: &Rc<RefCell<State>>, host: &str, extra_root: &[u8], device: &str, until: f64, channels: bool) -> Result<js_sys::Promise, JsValue> {
    let tor = st_rc.borrow().tor.borrow().clone().ok_or_else(|| err("Tor is not started"))?;
    let dev = unhex16(device).ok_or_else(|| err("device id: 32 hex digits"))?;
    let (v, seq, record, name) = {
        let st = st_rc.borrow();
        if st.vault_newer || ((st.vault_seq > 0 || !channels) && !st.vault_known) {
            return Err(err("the list of your channels could not be read yet; not overwriting it"));
        }
        let (lease, entries) = if channels {
            let mut entries: Vec<Entry> = st.own.iter().map(entry_of).collect();
            for e in &st.vault.entries {
                if !entries.iter().any(|x| x.index == e.index) {
                    entries.push(e.clone());
                }
            }
            entries.sort_by_key(|e| e.index);
            entries.truncate(vault::MAX_ENTRIES);
            (Lease { device: dev, until: until as u64 }, entries)
        } else {
            (st.vault.lease.clone(), st.vault.entries.clone())
        };
        let mut boards: Vec<vault::BoardEntry> = st.boards.vault_entries(Lease { device: dev, until: until as u64 }, now_s());
        for b in &st.vault.boards {
            if !boards.iter().any(|x| x.index == b.index) {
                boards.push(b.clone());
            }
        }
        boards.sort_by_key(|b| b.index);
        let v = Vault { lease, entries, boards };
        let seq = st.vault_seq + 1;
        let (mut sign, mut key) = st.id.as_ref().ok_or_else(|| err("sign in first"))?.vault_seeds();
        let mut nonce = [0u8; 24];
        ephem_crypto::random(&mut nonce);
        let record = vault::seal(&sign, &key, &v, seq, now_s(), &nonce);
        let name = vault::name(&sign);
        sign.fill(0);
        key.fill(0);
        (v, seq, record.map_err(|e| err(format!("vault: {e:?}")))?, name)
    };
    let (host, root, st) = (host.to_owned(), extra_root.to_vec(), st_rc.clone());
    Ok(wasm_bindgen_futures::future_to_promise(async move {
        let path = format!("/routing/v1/ipns/{}", name.to_text());
        let status = with_timeout(FETCH_TIMEOUT_MS, https::request(&tor, "PUT", &host, &path, gateway::CT_RECORD, &record, &root)).await.map_err(err)?.0;
        if !(200..300).contains(&status) {
            return Err(err(format!("the routing service answered {status}")));
        }
        let mut st = st.borrow_mut();
        if seq > st.vault_seq {
            st.vault = v;
            st.vault_seq = seq;
            st.vault_known = true;
        }
        Ok(JsValue::from(seq as f64))
    }))
}

/// An open channel as the vault lists it.
fn entry_of(o: &Own) -> Entry {
    let h = o.hosted.borrow();
    let head = h.dag(&h.root).and_then(|b| b.into_iter().next()).and_then(|(_, root)| Value::decode(&root)).and_then(|r| r.get("head").and_then(Value::link).cloned());
    let m = &o.ch.manifest;
    Entry { index: o.index, title: m.title.clone(), about: m.about.clone(), created: m.created, mirrors: m.mirrors.clone(), head, count: o.ch.count(), last_seq: o.ch.count(), record_seq: o.ch.revision }
}

fn unhex16(s: &str) -> Option<[u8; 16]> {
    let mut out = [0u8; 16];
    if s.len() != 32 {
        return None;
    }
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// `{"seq","device","until","channels":[{"index","title","count","missing"…}],"boards":[{"index",
/// "title","next_no","seq","updated","device","until","mirrors"}…]}`.
fn vault_json(v: &Vault, seq: u64) -> String {
    use std::fmt::Write;
    let mut o = String::with_capacity(128 + v.entries.len() * 128);
    let _ = write!(o, "{{\"seq\":{seq},\"device\":\"");
    for b in v.lease.device {
        let _ = write!(o, "{b:02x}");
    }
    let _ = write!(o, "\",\"until\":{},\"channels\":[", v.lease.until);
    for (i, e) in v.entries.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"index\":{},\"count\":{},\"record_seq\":{},\"title\":", e.index, e.count, e.record_seq);
        json::string(&mut o, &e.title);
        o.push_str(",\"mirrors\":[");
        for (j, m) in e.mirrors.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            json::string(&mut o, m);
        }
        o.push_str("]}");
    }
    o.push_str("],\"boards\":[");
    for (i, b) in v.boards.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        let _ = write!(o, "{{\"index\":{},\"next_no\":{},\"seq\":{},\"updated\":{},\"until\":{},\"device\":\"", b.index, b.next_no, b.seq, b.updated, b.lease.until);
        for x in b.lease.device {
            let _ = write!(o, "{x:02x}");
        }
        o.push_str("\",\"title\":");
        json::string(&mut o, &b.title);
        o.push_str(",\"root\":");
        json::string(&mut o, &b.root.as_ref().map(Cid::to_text).unwrap_or_default());
        o.push_str(",\"mirrors\":[");
        for (j, m) in b.mirrors.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            json::string(&mut o, m);
        }
        o.push_str("]}");
    }
    o.push_str("]}");
    o
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
    // Re-written with the record's root and only the blocks it links to (security audit M-4):
    // what the page stores, mirrors and serves is exactly the verified channel.
    let car = car::write(std::slice::from_ref(&v.root), &gateway::reachable(&v.root, blocks));
    Ok(Reading { name: name.to_text(), json: json::view(&v), record: record.to_vec(), car, seq: v.record.sequence })
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
        // The record's circuit (a retry round's is fresh: the cached one may lead to a host
        // instance that is gone).
        let car_bytes = http_get(tor, onion, &format!("/ipfs/{root}?format=car"), fresh).await?;
        verified(name, &record, &car_bytes, min_seq)
    })
    .await
}

/// Requests one hosted onion answers at once; more streams are closed at once (security audit
/// H-2: one task per stream, unbounded, with responses of the whole channel).
const MAX_SERVING: u32 = 16;
/// The whole of one request, write included: a client that stops reading cannot hold a response.
const SERVE_TIMEOUT_MS: u32 = 60_000;

/// Serves until the service is dropped (its owner stopped writing here, §D.11.3 step 5).
async fn serve_loop(svc: std::rc::Weak<Service>, hosted: Rc<RefCell<Hosted>>) {
    let serving = Rc::new(std::cell::Cell::new(0u32));
    while let Some(s) = svc.upgrade() {
        match s.try_accept() {
            Some(stream) if serving.get() < MAX_SERVING => {
                serving.set(serving.get() + 1);
                let (hosted, serving) = (hosted.clone(), serving.clone());
                wasm_bindgen_futures::spawn_local(async move {
                    if with_timeout(SERVE_TIMEOUT_MS, serve_one(stream, hosted)).await.is_err() {
                        tracing::info!("channel: a request took too long; dropped");
                    }
                    serving.set(serving.get() - 1);
                });
            }
            Some(stream) => drop(stream), // busy: the stream closes at once
            None => {
                drop(s);
                sleep_ms(50).await;
            }
        }
    }
}

/// One request per stream (`Connection: close`).
async fn serve_one(s: DataStream, hosted: Rc<RefCell<Hosted>>) -> Result<(), String> {
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
            return Ok(());
        }
    };
    // The borrow ends here: the response is shared, so the channel may change while it is written.
    let resp = gateway::respond(&head, &hosted.borrow());
    tracing::debug!("channel: served {} bytes", resp.len());
    let _ = w.write_all(&resp).await;
    let _ = w.flush().await;
    let _ = w.close().await;
    Ok(())
}
