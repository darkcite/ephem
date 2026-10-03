// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
// Copyright 2026 Anton (darkcite)
//! BD-3 (docs/BOARDS.md G.16.3), natively: the board's onion as bytes in and bytes out. An owner
//! hosts, two posters solve and submit over the gateway's HTTP, a reader verifies what is
//! served, the store follows the deltas. The browser's loops (crates/channel-web) only move
//! these bytes over Tor streams.

use ed25519_dalek::SigningKey;
use ephem_board::board::Board;
use ephem_board::gateway::{self, Answer, Route, SHORT};
use ephem_board::host::Host;
use ephem_board::pipeline::{Efforts, Intake, Next, PowInfo, Refusal, submission};
use ephem_board::post::Signed;
use ephem_board::pow::{self, Challenge};
use ephem_board::submit::{HEADER_LEN, kind};
use ephem_board::verify::{self, View};
use ephem_channel::car;
use ephem_channel::cid::Cid;
use equix::SolverMemory;
use std::collections::HashMap;

const T0: u64 = 1_790_000_000;
const LOW: Efforts = Efforts { reply: 1, thread: 1 };

fn head_end(req: &[u8]) -> usize {
    req.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4
}

/// One request through the host, as the browser's serve loop runs it; a submit waits for the
/// next publish (`publish_at`).
fn serve(h: &mut Host, req: &[u8], now_ms: u64) -> Vec<u8> {
    let now = now_ms / 1000;
    let end = head_end(req);
    let route = match gateway::route(&req[..end], h.served.name_text()) {
        Ok(r) => r,
        Err(s) => return short(|o| gateway::status(s, o)),
    };
    match route {
        Route::Pow => {
            let info = h.intake.pow_info(now);
            short(|o| gateway::pow(&info, o))
        }
        Route::Submit(len) => {
            let body = &req[end..];
            assert_eq!(body.len(), len);
            let head: &[u8; HEADER_LEN] = body[..HEADER_LEN].try_into().unwrap();
            let (hd, slot) = match h.submit_header(head, len, now) {
                Ok((_, Next::Done { no, seq })) => return short(|o| gateway::answer(no, seq, o)),
                Ok((hd, Next::ReadBody(slot))) => (hd, slot),
                Err(r) => return short(|o| gateway::refusal(r, o)),
            };
            let id = match h.submit_body(&hd, slot, &body[HEADER_LEN..], now) {
                Ok(id) => id,
                Err(r) => return short(|o| gateway::refusal(r, o)),
            };
            // The next publish (at most one a second).
            let mut at = now_ms + 1_000;
            while !h.due(at) {
                at += 1_000;
            }
            h.publish(at);
            match h.answer(id).expect("answered at the publish") {
                Ok((no, seq)) => short(|o| gateway::answer(no, seq, o)),
                Err(r) => short(|o| gateway::refusal(r, o)),
            }
        }
        r => h.served.respond(&r).to_vec(),
    }
}

/// A short response (refusal, answer, `/pow`) from its fixed buffer.
fn short(f: impl FnOnce(&mut [u8; SHORT]) -> usize) -> Vec<u8> {
    let mut out = [0u8; SHORT];
    let n = f(&mut out);
    out[..n].to_vec()
}

fn get(path: &str) -> Vec<u8> {
    ephem_channel::gateway::get("board.onion", path)
}

fn body(resp: &[u8]) -> Vec<u8> {
    ephem_channel::gateway::parse_response(resp).unwrap().to_vec()
}

fn status(resp: &[u8]) -> u16 {
    std::str::from_utf8(&resp[9..12]).unwrap().parse().unwrap()
}

/// A poster: its key, and whether that key is a trip (stable on the board).
struct Poster(SigningKey, bool);

impl Poster {
    /// `GET /pow`, solve, sign, `POST /submit`: the request bytes (kept for a retry).
    fn submission(&self, h: &mut Host, now_ms: u64, t: u64, sub: &str, text: &str, sage: bool) -> Vec<u8> {
        let info = PowInfo::read(body(&serve(h, &get("/pow"), now_ms)).as_slice().try_into().unwrap());
        let name = h.served.name.clone();
        let effort = if t == 0 { info.effort_thread } else { info.effort_reply };
        let k = self.0.verifying_key().to_bytes();
        let mut n = [0u8; 16];
        n[8..].copy_from_slice(&ephem_board::submit::body_hash(text.as_bytes())[..8]);
        n[0] = sage as u8;
        let mut c = Challenge::new(&name.to_bytes(), &info.seed, if t == 0 { kind::THREAD } else { kind::REPLY }, t, &k, &n, effort).unwrap();
        let solution = pow::solve(&mut c, &mut n, effort, &mut SolverMemory::new(), 10_000).unwrap();
        let s = Signed { b: name.to_text(), t, k, n, sub: sub.into(), body: text.into(), sage, e: info.epoch, trip: self.1 };
        gateway::submit_request("board.onion", &submission(&s, &self.0, effort, solution).unwrap())
    }

    fn post(&self, h: &mut Host, now_ms: u64, t: u64, sub: &str, text: &str, sage: bool) -> Answer {
        let req = self.submission(h, now_ms, t, sub, text, sage);
        gateway::parse_answer(&serve(h, &req, now_ms)).unwrap()
    }
}

/// A reader: the index, then the threads it opens, all verified.
fn read(h: &mut Host, now_ms: u64, threads: &[u64]) -> View {
    let (record, root, mut blocks) = gateway::parse_index(&body(&serve(h, &get(&format!("/ipns/{}?format=ephem-board", h.served.name_text())), now_ms))).unwrap();
    let name = h.served.name.clone();
    let index = verify::verify(&name, &record, &blocks, now_ms, 0, &[]).unwrap();
    assert_eq!(index.root, root);
    for no in threads {
        let cid = &index.catalog.iter().find(|c| c.no == *no).unwrap().thread;
        let (_, b) = car::read(&body(&serve(h, &get(&format!("/ipfs/{}?format=car", cid.to_text())), now_ms))).unwrap();
        blocks.extend(b);
    }
    verify::verify(&name, &record, &blocks, now_ms, 0, &[]).unwrap()
}

/// The page's block store, kept by the deltas alone.
fn sync(h: &mut Host, store: &mut HashMap<Cid, Vec<u8>>) {
    let d = h.take_delta();
    for (c, b) in d.added {
        store.insert(c, b);
    }
    for c in d.removed {
        store.remove(&c);
    }
    let served: HashMap<Cid, Vec<u8>> = h.served.blocks().map(|(c, b)| (c.clone(), b.to_vec())).collect();
    assert_eq!(*store, served, "the store holds exactly what is served");
}

fn posted(a: Answer) -> (u64, u64) {
    match a {
        Answer::Posted { no, seq } => (no, seq),
        r => panic!("refused: {r:?}"),
    }
}

#[test]
fn owner_and_two_posters() {
    let board = Board::new(&[1; 32], "Lab board", "about", "be nice", T0).unwrap();
    let intake = Intake::new(board.name(), [2; 32], LOW, T0);
    let mut ms = T0 * 1000;
    let mut h = Host::new(board, intake, [9; 32], ms);
    let mut store = HashMap::new();
    sync(&mut h, &mut store);
    let (a, b) = (Poster(SigningKey::from_bytes(&[3; 32]), false), Poster(SigningKey::from_bytes(&[4; 32]), false));

    // Two threads (the thread budget: one per 2 minutes), then replies.
    let (t1, seq1) = posted(a.post(&mut h, ms, 0, "First", "op one", false));
    ms += 1_000;
    let early = b.post(&mut h, ms, 0, "Too soon", "op", false);
    assert_eq!(early, Answer::Refused { status: 503, code: Refusal::Busy.code() }, "the thread budget");
    ms += 121_000;
    let (t2, seq2) = posted(b.post(&mut h, ms, 0, "Second", "op two", false));
    assert!(seq2 > seq1 && t2 > t1);
    sync(&mut h, &mut store);
    ms += 5_000;
    posted(b.post(&mut h, ms, t1, "", "reply bumps one", false));
    ms += 5_000;
    let retry = a.submission(&mut h, ms, t2, "", "sage reply", true);
    let first = gateway::parse_answer(&serve(&mut h, &retry, ms)).unwrap();
    ms += 3_000;
    let again = gateway::parse_answer(&serve(&mut h, &retry, ms)).unwrap();
    assert_eq!(first, again, "a retried submit gets the original {{no, seq}}");
    let owner = h.post_owner(t2, "", "from the owner", false, ms / 1000).unwrap();
    ms += 1_000;
    assert!(h.due(ms));
    h.publish(ms);
    sync(&mut h, &mut store);

    let v = read(&mut h, ms, &[t1, t2]);
    assert_eq!(v.catalog.iter().map(|c| c.no).collect::<Vec<_>>(), vec![t2, t1], "the owner's reply bumped thread 2 last");
    let one = v.threads.iter().find(|t| t.no == t1).unwrap();
    let two = v.threads.iter().find(|t| t.no == t2).unwrap();
    assert_eq!(one.entries.len(), 2);
    assert_eq!(two.entries.len(), 3);
    assert!(matches!(&two.entries[2], ephem_board::board::Entry::Post(p) if p.no == owner && p.cap == ephem_board::board::cap::OWNER));
    let sage_bump = v.catalog.iter().find(|c| c.no == t2).unwrap().bump;
    assert_eq!(sage_bump, ms / 1000 - 1, "bumped by the owner, not by the sage reply");

    // What is not served: the whole board as one CAR.
    assert_eq!(status(&serve(&mut h, &get(&format!("/ipfs/{}?format=car", v.root.to_text())), ms)), 406);
    assert_eq!(status(&serve(&mut h, &get("/ipns/k51other?format=ephem-board"), ms)), 404);

    // Refusals on the front door.
    let mut bad = retry.clone();
    let e = head_end(&bad);
    bad[e + 120] ^= 1; // the solution
    assert_eq!(gateway::parse_answer(&serve(&mut h, &bad, ms)).unwrap(), Answer::Refused { status: 403, code: Refusal::Pow.code() });
    let wrong_type = String::from_utf8(retry[..head_end(&retry)].to_vec()).unwrap().replace(gateway::CT_SUBMIT, "text/plain");
    assert_eq!(status(&serve(&mut h, wrong_type.as_bytes(), ms)), 415);
    let too_long = format!("POST /submit HTTP/1.1\r\nContent-Type: {}\r\nContent-Length: 99999\r\n\r\n", gateway::CT_SUBMIT);
    assert_eq!(status(&serve(&mut h, too_long.as_bytes(), ms)), 413);
}

#[test]
fn a_full_board_prunes_its_oldest_thread() {
    let board = Board::new(&[1; 32], "Full", "", "", T0).unwrap();
    let intake = Intake::new(board.name(), [2; 32], LOW, T0);
    let mut ms = T0 * 1000;
    let mut h = Host::new(board, intake, [9; 32], ms);
    let mut store = HashMap::new();
    for i in 0..ephem_board::limits::THREADS {
        h.post_owner(0, &format!("thread {i}"), "op", false, ms / 1000 + i as u64).unwrap();
    }
    h.publish(ms);
    sync(&mut h, &mut store);
    ms += 3_600_000;
    let p = Poster(SigningKey::from_bytes(&[5; 32]), false);
    let (no, _) = posted(p.post(&mut h, ms, 0, "Newest", "pushes one out", false));
    sync(&mut h, &mut store);
    let v = read(&mut h, ms, &[no]);
    assert_eq!(v.catalog.len(), ephem_board::limits::THREADS);
    assert_eq!(v.catalog[0].no, no);
    assert!(!v.catalog.iter().any(|c| c.no == 1), "thread 1, bumped least recently, is pruned");
    assert_eq!(v.archive.len(), 1);
    assert_eq!(v.archive[0].no, 1);
    // The archived thread is still served as text.
    let arch = &v.archive[0].thread;
    assert_eq!(status(&serve(&mut h, &get(&format!("/ipfs/{}?format=car", arch.to_text())), ms)), 200);
}

/// Publishes once the undo window has passed.
fn settle(h: &mut Host, ms: &mut u64) {
    *ms += ephem_board::host::UNDO_MS + 1_000;
    assert!(h.due(*ms));
    h.publish(*ms);
}

#[test]
fn owner_moderation() {
    use ephem_board::board::Entry;
    use ephem_board::own::Switches;
    let board = Board::new(&[1; 32], "Moderated", "", "", T0).unwrap();
    let intake = Intake::new(board.name(), [2; 32], LOW, T0);
    let mut ms = T0 * 1000;
    let mut h = Host::new(board, intake, [9; 32], ms);
    let anon = Poster(SigningKey::from_bytes(&[3; 32]), false);
    let tripper = Poster(SigningKey::from_bytes(&[4; 32]), true);
    let (t, _) = posted(anon.post(&mut h, ms, 0, "Thread", "op", false));
    ms += 1_000;
    let (r1, _) = posted(tripper.post(&mut h, ms, t, "", "a trip speaks", false));
    ms += 61_000;
    let since = ms / 1000 - (ms / 1000) % 60;
    let (r2, _) = posted(anon.post(&mut h, ms, t, "", "spam one", false));
    let (r3, _) = posted(anon.post(&mut h, ms, t, "", "spam two", false));

    // A delete waits for its undo window; an undo keeps the post.
    h.delete(r1, ms).unwrap();
    assert_eq!(h.undo(r1), 1);
    // Mass delete: everything since a minute.
    assert_eq!(h.delete_since(since, ms), 2);
    settle(&mut h, &mut ms);
    let v = read(&mut h, ms, &[t]);
    let e = &v.threads[0].entries;
    assert!(matches!(e[1], Entry::Post(_)), "undone: still there");
    assert!(matches!(e[2], Entry::Tomb { no, .. } if no == r2) && matches!(e[3], Entry::Tomb { no, .. } if no == r3), "readers see tombstones");
    assert_eq!(v.dels.len(), 2);
    assert!(v.modlog.iter().filter(|m| m.act == "delete").count() == 2);

    // Ban the trip: refused from now on.
    h.ban(r1, "rude", ms / 1000).unwrap();
    ms += 2_000;
    assert_eq!(tripper.post(&mut h, ms, t, "", "again", false), Answer::Refused { status: 409, code: Refusal::Refused.code() });
    h.unban(&tripper.0.verifying_key().to_bytes());

    // Trips-only: an anonymous post is refused, a known trip passes, a new trip is refused.
    h.set_switches(Switches { trips_only: true, ..h.switches() });
    ms += 2_000;
    assert_eq!(anon.post(&mut h, ms, t, "", "anon under trips-only", false), Answer::Refused { status: 423, code: Refusal::Paused.code() });
    posted(tripper.post(&mut h, ms, t, "", "known trip", false));
    let newcomer = Poster(SigningKey::from_bytes(&[5; 32]), true);
    assert_eq!(newcomer.post(&mut h, ms, t, "", "new trip", false), Answer::Refused { status: 423, code: Refusal::Paused.code() });
    h.approve_trip(&newcomer.0.verifying_key().to_bytes(), true);
    posted(newcomer.post(&mut h, ms + 1_000, t, "", "approved trip", false));
    h.set_switches(Switches::default());

    // Pre-moderation: held (number 0), then approved by the owner.
    h.set_switches(Switches { premod: true, ..Switches::default() });
    ms += 2_000;
    let held = anon.post(&mut h, ms, t, "", "please approve", false);
    assert!(matches!(held, Answer::Posted { no: 0, .. }), "{held:?}");
    assert_eq!(h.own.held.len(), 1);
    let no = h.approve(0, ms / 1000).unwrap();
    ms += 2_000;
    h.publish(ms);
    let v = read(&mut h, ms, &[t]);
    assert!(v.threads[0].entries.iter().any(|e| matches!(e, Entry::Post(p) if p.no == no && p.s.body == "please approve")));

    // Lock and sticky, logged; a locked thread refuses replies.
    h.set_locked(t, true, ms / 1000).unwrap();
    h.set_switches(Switches::default());
    ms += 2_000;
    h.publish(ms);
    assert_eq!(anon.post(&mut h, ms, t, "", "into a locked thread", false), Answer::Refused { status: 409, code: Refusal::Refused.code() });
    let v = read(&mut h, ms, &[]);
    assert!(v.catalog[0].locked && v.modlog.iter().any(|m| m.act == "lock" && m.no == t));

    // Everything moderation needs comes back with the board (the encrypted own block).
    h.ban(no, "test", ms / 1000).unwrap();
    h.set_switches(Switches { trips_only: true, panic_trips: true, ..Switches::default() });
    ms += 2_000;
    h.publish(ms);
    let (record, blocks): (Vec<u8>, Vec<_>) = (h.served.record.clone(), h.served.blocks().map(|(c, b)| (c.clone(), b.to_vec())).collect());
    let key = ed25519_dalek::SigningKey::from_bytes(&[1; 32]);
    let name = h.served.name.clone();
    let rec = ephem_channel::ipns::verify(&name, &record, 0).unwrap();
    let root = Cid::parse(rec.value.strip_prefix("/ipfs/").unwrap()).unwrap();
    let mut view = verify::read(&key.verifying_key(), &name, &root, &blocks, &[]).unwrap();
    view.sequence = rec.sequence;
    let again = Host::new(Board::load(&[1; 32], view, blocks).unwrap(), Intake::new(&name, [2; 32], Efforts::DEFAULT, T0), [9; 32], ms + 1_000);
    assert_eq!(again.own.bans.len(), 1);
    assert!(again.switches().trips_only && again.switches().panic_trips);
    assert_eq!(again.intake.base, LOW, "efforts come back too");
    assert!(again.own.known.contains(&tripper.0.verifying_key().to_bytes()));
    let wrong = Host::new(Board::load(&[1; 32], verify::read(&key.verifying_key(), &name, &root, &h.served.blocks().map(|(c, b)| (c.clone(), b.to_vec())).collect::<Vec<_>>(), &[]).unwrap(), Vec::new()).unwrap(), Intake::new(&name, [2; 32], Efforts::DEFAULT, T0), [8; 32], ms + 1_000);
    assert!(wrong.own.bans.is_empty(), "another key opens nothing");
}

#[test]
fn a_stale_root_shows_later_deletions() {
    use ephem_board::board::Entry;
    let board = Board::new(&[1; 32], "Stale", "", "", T0).unwrap();
    let intake = Intake::new(board.name(), [2; 32], LOW, T0);
    let mut ms = T0 * 1000;
    let mut h = Host::new(board, intake, [9; 32], ms);
    let p = Poster(SigningKey::from_bytes(&[3; 32]), false);
    let (t, _) = posted(p.post(&mut h, ms, 0, "T", "op", false));
    ms += 1_000;
    let (r, _) = posted(p.post(&mut h, ms, t, "", "later deleted", false));
    // What a stale mirror keeps serving: this version.
    let old_record = h.served.record.clone();
    let old: Vec<_> = h.served.blocks().map(|(c, b)| (c.clone(), b.to_vec())).collect();
    h.delete(r, ms).unwrap();
    settle(&mut h, &mut ms);
    let fresh = read(&mut h, ms, &[t]);
    let known: Vec<[u8; 32]> = fresh.dels.iter().map(|(x, _)| *x).collect();
    let name = h.served.name.clone();
    let stale = verify::verify(&name, &old_record, &old, ms, 0, &[]).unwrap();
    assert!(matches!(stale.threads[0].entries[1], Entry::Post(_)), "without the newer list the old root still shows it");
    let stale = verify::verify(&name, &old_record, &old, ms, 0, &known).unwrap();
    assert!(matches!(stale.threads[0].entries[1], Entry::Tomb { .. }), "a reader who saw the newer deletion list sees a tombstone");
}

#[test]
fn plain_pages_for_tor_browser() {
    let board = Board::new(&[1; 32], "Pages <b>", "about & more", "no <script>", T0).unwrap();
    let intake = Intake::new(board.name(), [2; 32], LOW, T0);
    let mut ms = T0 * 1000;
    let mut h = Host::new(board, intake, [9; 32], ms);
    let p = Poster(SigningKey::from_bytes(&[3; 32]), true);
    let (t, _) = posted(p.post(&mut h, ms, 0, "Sub <i>", "op <script>alert(1)</script>", false));
    ms += 1_000;
    posted(p.post(&mut h, ms, t, "", ">greentext\nplain", false));
    let body = |r: Vec<u8>| String::from_utf8(r).unwrap();
    let cat = body(serve(&mut h, &get("/"), ms));
    assert!(cat.contains("Content-Security-Policy: default-src 'none'") && cat.contains("Referrer-Policy: no-referrer"));
    assert!(cat.contains("<title>Pages &lt;b&gt;</title>") && cat.contains("about &amp; more") && cat.contains("no &lt;script&gt;"));
    assert!(cat.contains(&format!("<a href=\"/t/{t}\">No. {t}</a> Sub &lt;i&gt;")) && cat.contains("1 replies"));
    assert!(cat.contains("board's own onion address") && !cat.contains("<script"));
    let th = body(serve(&mut h, &get(&format!("/t/{t}")), ms));
    assert!(th.contains("op &lt;script&gt;alert(1)&lt;/script&gt;") && !th.contains("<script"), "post text escaped");
    assert!(th.contains("<span class=\"gt\">&gt;greentext</span>") && th.contains("class=\"trip\">!"));
    assert_eq!(status(&serve(&mut h, &get("/t/999"), ms)), 404);
    assert_eq!(status(&serve(&mut h, &get("/?p=7"), ms)), 200, "a page past the end shows the last one");
}

#[test]
fn stale_mode_after_expiry() {
    let board = Board::new(&[1; 32], "Stale", "", "", T0).unwrap();
    let intake = Intake::new(board.name(), [2; 32], LOW, T0);
    let ms = T0 * 1000;
    let mut h = Host::new(board, intake, [9; 32], ms);
    let (record, blocks): (Vec<u8>, Vec<_>) = (h.served.record.clone(), h.served.blocks().map(|(c, b)| (c.clone(), b.to_vec())).collect());
    let name = h.served.name.clone();
    let later = ms + (ephem_board::limits::VALIDITY_S + 3600) * 1000;
    assert!(verify::verify(&name, &record, &blocks, later, 0, &[]).is_err(), "expired after 72 h");
    let v = verify::verify_stale(&name, &record, &blocks, later, 0, &[]).unwrap();
    assert!(v.stale, "shown as stale");
    assert!(!verify::verify_stale(&name, &record, &blocks, ms, 0, &[]).unwrap().stale, "fresh is not stale");
    let mut bad = record.clone();
    let n = bad.len() - 10;
    bad[n] ^= 1;
    assert!(verify::verify_stale(&name, &bad, &blocks, later, 0, &[]).is_err(), "a forged record is not let through as stale");
    let _ = &mut h;
}

#[test]
fn see_also_links() {
    let mut board = Board::new(&[1; 32], "Linked", "", "", T0).unwrap();
    let other = format!("k51qzi5uqu5dl{}@{}.onion", "a".repeat(10), "b".repeat(56));
    assert!(board.set_see_also(vec!["javascript:alert(1)".into()]).is_err(), "only <name>@<onion>");
    board.set_see_also(vec![other.clone()]).unwrap();
    let intake = Intake::new(board.name(), [2; 32], LOW, T0);
    let ms = T0 * 1000;
    let mut h = Host::new(board, intake, [9; 32], ms);
    let v = read(&mut h, ms, &[]);
    assert_eq!(v.manifest.see_also, vec![other], "signed into the manifest, verified by readers");
    let page = String::from_utf8(serve(&mut h, &get("/"), ms)).unwrap();
    assert!(page.contains(&format!("<a href=\"http://{}.onion/\">", "b".repeat(56))));
}
