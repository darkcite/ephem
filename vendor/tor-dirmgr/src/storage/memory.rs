//! Ephem patch: an in-memory [`Store`] for `wasm32-unknown-unknown`.
//!
//! Browsers have no SQLite, and arti 0.46 opens `SqliteStore` unconditionally. This store keeps
//! the same data with the same selection and expiry rules as `sqlite.rs` (the queries are
//! quoted next to each method), in plain maps. Everything is public directory data; the Ephem
//! adapter snapshots it to IndexedDB for warm starts ([`cache_export`], [`cache_import`]).
//!
//! A tab runs one Tor client, so the maps are one per page (a static): the store handle that
//! arti owns and the snapshot functions see the same data.

use super::ExpirationConfig;
use crate::docmeta::{AuthCertMeta, ConsensusMeta};
use crate::storage::{InputString, Store};
use crate::Result;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Read;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tor_llcrypto::pk::rsa::RsaIdentity;
use tor_netdoc::doc::netstatus::Lifetime;
use time::OffsetDateTime;
use tor_netdoc::doc::authcert::AuthCertKeyIds;
use tor_netdoc::doc::microdesc::MdDigest;
use tor_netdoc::doc::netstatus::{ConsensusFlavor, ProtoStatuses};
#[cfg(feature = "routerdesc")]
use tor_netdoc::doc::routerdesc::RdDigest;
use web_time_compat::SystemTimeExt;

#[cfg(feature = "bridge-client")]
use super::{BridgeConfig, CachedBridgeDescriptor};

struct Consensus {
    meta: ConsensusMeta,
    flavor: ConsensusFlavor,
    pending: bool,
    text: String,
}

/// The directory cache of a browser tab.
#[derive(Default)]
struct Maps {
    consensuses: Vec<Consensus>,
    authcerts: HashMap<AuthCertKeyIds, (OffsetDateTime, String)>,
    microdescs: HashMap<MdDigest, (OffsetDateTime, String)>,
    #[cfg(feature = "routerdesc")]
    routerdescs: HashMap<RdDigest, (OffsetDateTime, String)>,
    #[cfg(feature = "bridge-client")]
    bridgedescs: HashMap<String, (CachedBridgeDescriptor, OffsetDateTime)>,
    protocols: Option<(SystemTime, ProtoStatuses)>,
}

static MAPS: Mutex<Option<Maps>> = Mutex::new(None);

/// Runs `f` on the page's maps (created empty on first use).
fn with<R>(f: impl FnOnce(&mut Maps) -> R) -> R {
    let mut g = MAPS.lock().expect("directory cache lock");
    f(g.get_or_insert_with(Maps::default))
}

/// Handle to the page's directory cache (what `open_store` returns on wasm32).
#[derive(Default)]
pub(crate) struct MemoryStore;

impl Maps {
    /// `ORDER BY valid_until DESC LIMIT 1` over the matching consensuses.
    fn latest(&self, flavor: ConsensusFlavor, pending: Option<bool>) -> Option<&Consensus> {
        self.consensuses
            .iter()
            .filter(|c| c.flavor == flavor && pending.is_none_or(|p| c.pending == p))
            .max_by_key(|c| c.meta.lifetime().valid_until())
    }
}

impl Store for MemoryStore {
    fn is_readonly(&self) -> bool {
        false
    }

    fn upgrade_to_readwrite(&mut self) -> Result<bool> {
        Ok(true)
    }

    fn expire_all(&mut self, expiration: &ExpirationConfig) -> Result<()> {
        with(|m| {
            let now: OffsetDateTime = SystemTime::get().into();
            // DROP_OLD_MICRODESCS / AUTHCERTS / CONSENSUSES / ROUTERDESCS / BRIDGEDESCS
            m.microdescs.retain(|_, (listed, _)| *listed >= now - expiration.microdescs);
            m.authcerts.retain(|_, (expires, _)| *expires >= now - expiration.authcerts);
            m.consensuses
                .retain(|c| OffsetDateTime::from(c.meta.lifetime().valid_until()) >= now - expiration.consensuses);
            #[cfg(feature = "routerdesc")]
            m.routerdescs.retain(|_, (published, _)| *published >= now - expiration.router_descs);
            #[cfg(feature = "bridge-client")]
            m.bridgedescs
                .retain(|_, (d, until)| !(now > *until || OffsetDateTime::from(d.fetched) > now));
            Ok(())
        })
    }

    fn latest_consensus(&self, flavor: ConsensusFlavor, pending: Option<bool>) -> Result<Option<InputString>> {
        with(|m| {
            Ok(m.latest(flavor, pending).map(|c| InputString::from(c.text.clone())))
        })
    }

    fn latest_consensus_meta(&self, flavor: ConsensusFlavor) -> Result<Option<ConsensusMeta>> {
        with(|m| {
            Ok(m.latest(flavor, Some(false)).map(|c| c.meta.clone()))
        })
    }

    #[cfg(test)]
    fn consensus_by_meta(&self, cmeta: &ConsensusMeta) -> Result<InputString> {
        self.consensus_by_sha3_digest_of_signed_part(cmeta.sha3_256_of_signed())?
            .map(|(t, _)| t)
            .ok_or(crate::Error::CacheCorruption("couldn't find a consensus we thought we had."))
    }

    fn consensus_by_sha3_digest_of_signed_part(&self, d: &[u8; 32]) -> Result<Option<(InputString, ConsensusMeta)>> {
        with(|m| {
            Ok(m.consensuses
                .iter()
                .find(|c| c.meta.sha3_256_of_signed() == d)
                .map(|c| (InputString::from(c.text.clone()), c.meta.clone())))
        })
    }

    fn store_consensus(&mut self, cmeta: &ConsensusMeta, flavor: ConsensusFlavor, pending: bool, contents: &str) -> Result<()> {
        with(|m| {
            // INSERT OR REPLACE keyed by the digest of the whole document.
            m.consensuses.retain(|c| c.meta.sha3_256_of_whole() != cmeta.sha3_256_of_whole());
            m.consensuses.push(Consensus { meta: cmeta.clone(), flavor, pending, text: contents.to_owned() });
            Ok(())
        })
    }

    fn mark_consensus_usable(&mut self, cmeta: &ConsensusMeta) -> Result<()> {
        with(|m| {
            for c in &mut m.consensuses {
                if c.meta.sha3_256_of_whole() == cmeta.sha3_256_of_whole() {
                    c.pending = false;
                }
            }
            Ok(())
        })
    }

    fn delete_consensus(&mut self, cmeta: &ConsensusMeta) -> Result<()> {
        with(|m| {
            m.consensuses.retain(|c| c.meta.sha3_256_of_whole() != cmeta.sha3_256_of_whole());
            Ok(())
        })
    }

    fn authcerts(&self, certs: &[AuthCertKeyIds]) -> Result<HashMap<AuthCertKeyIds, String>> {
        with(|m| {
            Ok(certs.iter().filter_map(|ids| m.authcerts.get(ids).map(|(_, t)| (*ids, t.clone()))).collect())
        })
    }

    fn store_authcerts(&mut self, certs: &[(AuthCertMeta, &str)]) -> Result<()> {
        with(|m| {
            for (meta, content) in certs {
                m.authcerts.insert(*meta.key_ids(), (meta.expires().into(), (*content).to_owned()));
            }
            Ok(())
        })
    }

    fn microdescs(&self, digests: &[MdDigest]) -> Result<HashMap<MdDigest, String>> {
        with(|m| {
            Ok(digests.iter().filter_map(|d| m.microdescs.get(d).map(|(_, t)| (*d, t.clone()))).collect())
        })
    }

    fn store_microdescs(&mut self, digests: &[(&str, &MdDigest)], when: SystemTime) -> Result<()> {
        with(|m| {
            let when: OffsetDateTime = when.into();
            for (content, d) in digests {
                m.microdescs.insert(**d, (when, (*content).to_owned()));
            }
            Ok(())
        })
    }

    fn update_microdescs_listed(&mut self, digests: &[MdDigest], when: SystemTime) -> Result<()> {
        with(|m| {
            let when: OffsetDateTime = when.into();
            for d in digests {
                if let Some((listed, _)) = m.microdescs.get_mut(d) {
                    *listed = (*listed).max(when);
                }
            }
            Ok(())
        })
    }

    #[cfg(feature = "routerdesc")]
    fn routerdescs(&self, digests: &[RdDigest]) -> Result<HashMap<RdDigest, String>> {
        with(|m| {
            Ok(digests.iter().filter_map(|d| m.routerdescs.get(d).map(|(_, t)| (*d, t.clone()))).collect())
        })
    }

    #[cfg(feature = "routerdesc")]
    fn store_routerdescs(&mut self, digests: &[(&str, SystemTime, &RdDigest)]) -> Result<()> {
        with(|m| {
            for (content, when, d) in digests {
                m.routerdescs.insert(**d, ((*when).into(), (*content).to_owned()));
            }
            Ok(())
        })
    }

    #[cfg(feature = "bridge-client")]
    fn lookup_bridgedesc(&self, bridge: &BridgeConfig) -> Result<Option<CachedBridgeDescriptor>> {
        with(|m| {
            Ok(m.bridgedescs.get(&bridge.to_string()).map(|(d, _)| d.clone()))
        })
    }

    #[cfg(feature = "bridge-client")]
    fn store_bridgedesc(&mut self, bridge: &BridgeConfig, entry: CachedBridgeDescriptor, until: SystemTime) -> Result<()> {
        with(|m| {
            m.bridgedescs.insert(bridge.to_string(), (entry, until.into()));
            Ok(())
        })
    }

    #[cfg(feature = "bridge-client")]
    fn delete_bridgedesc(&mut self, bridge: &BridgeConfig) -> Result<()> {
        with(|m| {
            m.bridgedescs.remove(&bridge.to_string());
            Ok(())
        })
    }

    fn update_protocol_recommendations(&mut self, valid_after: SystemTime, protocols: &ProtoStatuses) -> Result<()> {
        with(|m| {
            m.protocols = Some((valid_after, protocols.clone()));
            Ok(())
        })
    }

    fn cached_protocol_recommendations(&self) -> Result<Option<(SystemTime, ProtoStatuses)>> {
        with(|m| {
            Ok(m.protocols.clone())
        })
    }
}

// ---- Ephem: snapshots for warm starts (IndexedDB, kept by the page) ----
//
// A snapshot is gzip-compressed JSON. The real directory is tens of MB of text (mostly
// microdescriptors): export streams borrowed data straight into the compressor, so no second
// copy of the directory is made in wasm memory (which never shrinks).

/// Snapshot format version.
const SNAPSHOT_V: u8 = 2;

#[derive(Serialize, Deserialize)]
struct Snapshot<'a> {
    v: u8,
    #[serde(borrow)]
    consensuses: Vec<SnapConsensus<'a>>,
    #[serde(borrow)]
    authcerts: Vec<SnapCert<'a>>,
    #[serde(borrow)]
    microdescs: Vec<SnapMd<'a>>,
}

#[derive(Serialize, Deserialize)]
struct SnapConsensus<'a> {
    #[serde(borrow)]
    flavor: Cow<'a, str>,
    /// valid-after, fresh-until, valid-until (Unix seconds).
    lifetime: [u64; 3],
    /// SHA3-256 of the signed part and of the whole document (hex).
    signed: String,
    whole: String,
    #[serde(borrow)]
    text: Cow<'a, str>,
}

#[derive(Serialize, Deserialize)]
struct SnapCert<'a> {
    id: String,
    sk: String,
    expires: i64,
    #[serde(borrow)]
    text: Cow<'a, str>,
}

#[derive(Serialize, Deserialize)]
struct SnapMd<'a> {
    digest: String,
    listed: i64,
    #[serde(borrow)]
    text: Cow<'a, str>,
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn digest(h: &str) -> Option<[u8; 32]> {
    hex::decode(h).ok()?.try_into().ok()
}

/// The usable (not pending) consensuses, authority certificates and microdescriptors as a
/// gzip-compressed JSON snapshot; `None` before anything was downloaded. Public directory
/// data only.
pub fn cache_export() -> Option<Vec<u8>> {
    with(|m| {
        if m.consensuses.iter().all(|c| c.pending) {
            return None;
        }
        let snap = Snapshot {
            v: SNAPSHOT_V,
            consensuses: m
                .consensuses
                .iter()
                .filter(|c| !c.pending)
                .map(|c| {
                    let l = c.meta.lifetime();
                    SnapConsensus {
                        flavor: Cow::Borrowed(c.flavor.name()),
                        lifetime: [secs(l.valid_after()), secs(l.fresh_until()), secs(l.valid_until())],
                        signed: hex::encode(c.meta.sha3_256_of_signed()),
                        whole: hex::encode(c.meta.sha3_256_of_whole()),
                        text: Cow::Borrowed(&c.text),
                    }
                })
                .collect(),
            authcerts: m
                .authcerts
                .iter()
                .map(|(ids, (expires, text))| SnapCert {
                    id: hex::encode(ids.id_fingerprint.as_bytes()),
                    sk: hex::encode(ids.sk_fingerprint.as_bytes()),
                    expires: expires.unix_timestamp(),
                    text: Cow::Borrowed(text),
                })
                .collect(),
            microdescs: m
                .microdescs
                .iter()
                .map(|(d, (listed, text))| SnapMd { digest: hex::encode(d), listed: listed.unix_timestamp(), text: Cow::Borrowed(text) })
                .collect(),
        };
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        serde_json::to_writer(&mut gz, &snap).ok()?;
        gz.finish().ok()
    })
}

/// Seeds the cache from a snapshot of [`cache_export`], before the Tor client starts. Entries
/// are kept as they are: arti validates cached documents (signatures, lifetimes) on load, as it
/// does with its SQLite cache. Returns whether the snapshot was readable.
pub fn cache_import(gz: &[u8]) -> bool {
    let mut json = String::new();
    if GzDecoder::new(gz).read_to_string(&mut json).is_err() {
        return false;
    }
    let Ok(snap) = serde_json::from_str::<Snapshot<'_>>(&json) else { return false };
    if snap.v != SNAPSHOT_V {
        return false;
    }
    let time = |s: i64| OffsetDateTime::from_unix_timestamp(s).ok();
    with(|m| {
        for c in snap.consensuses {
            let at = |i: usize| UNIX_EPOCH + Duration::from_secs(c.lifetime[i]);
            let (Ok(flavor), Ok(lifetime), Some(signed), Some(whole)) = (
                ConsensusFlavor::from_opt_name(Some(&c.flavor)),
                Lifetime::new(at(0), at(1), at(2)),
                digest(&c.signed),
                digest(&c.whole),
            ) else {
                continue;
            };
            m.consensuses.retain(|x| x.meta.sha3_256_of_whole() != &whole);
            m.consensuses.push(Consensus { meta: ConsensusMeta::new(lifetime, signed, whole), flavor, pending: false, text: c.text.into_owned() });
        }
        for a in snap.authcerts {
            let ids = hex::decode(&a.id).ok().zip(hex::decode(&a.sk).ok()).and_then(|(id, sk)| {
                Some(AuthCertKeyIds { id_fingerprint: RsaIdentity::from_bytes(&id)?, sk_fingerprint: RsaIdentity::from_bytes(&sk)? })
            });
            if let (Some(ids), Some(expires)) = (ids, time(a.expires)) {
                m.authcerts.insert(ids, (expires, a.text.into_owned()));
            }
        }
        for d in snap.microdescs {
            if let (Some(digest), Some(listed)) = (digest(&d.digest), time(d.listed)) {
                m.microdescs.insert(digest, (listed, d.text.into_owned()));
            }
        }
    });
    true
}
