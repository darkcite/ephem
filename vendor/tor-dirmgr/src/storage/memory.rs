//! Ephem patch: an in-memory [`Store`] for `wasm32-unknown-unknown`.
//!
//! Browsers have no SQLite, and arti 0.46 opens `SqliteStore` unconditionally. This store keeps
//! the same data with the same selection and expiry rules as `sqlite.rs` (the queries are
//! quoted next to each method), in plain maps. Everything is public directory data; the Ephem
//! adapter snapshots it to IndexedDB for warm starts.

use super::ExpirationConfig;
use crate::docmeta::{AuthCertMeta, ConsensusMeta};
use crate::storage::{InputString, Store};
use crate::Result;
use std::collections::HashMap;
use std::time::SystemTime;
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
pub(crate) struct MemoryStore {
    consensuses: Vec<Consensus>,
    authcerts: HashMap<AuthCertKeyIds, (OffsetDateTime, String)>,
    microdescs: HashMap<MdDigest, (OffsetDateTime, String)>,
    #[cfg(feature = "routerdesc")]
    routerdescs: HashMap<RdDigest, (OffsetDateTime, String)>,
    #[cfg(feature = "bridge-client")]
    bridgedescs: HashMap<String, (CachedBridgeDescriptor, OffsetDateTime)>,
    protocols: Option<(SystemTime, ProtoStatuses)>,
}

impl MemoryStore {
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
        let now: OffsetDateTime = SystemTime::get().into();
        // DROP_OLD_MICRODESCS / AUTHCERTS / CONSENSUSES / ROUTERDESCS / BRIDGEDESCS
        self.microdescs.retain(|_, (listed, _)| *listed >= now - expiration.microdescs);
        self.authcerts.retain(|_, (expires, _)| *expires >= now - expiration.authcerts);
        self.consensuses
            .retain(|c| OffsetDateTime::from(c.meta.lifetime().valid_until()) >= now - expiration.consensuses);
        #[cfg(feature = "routerdesc")]
        self.routerdescs.retain(|_, (published, _)| *published >= now - expiration.router_descs);
        #[cfg(feature = "bridge-client")]
        self.bridgedescs
            .retain(|_, (d, until)| !(now > *until || OffsetDateTime::from(d.fetched) > now));
        Ok(())
    }

    fn latest_consensus(&self, flavor: ConsensusFlavor, pending: Option<bool>) -> Result<Option<InputString>> {
        Ok(self.latest(flavor, pending).map(|c| InputString::from(c.text.clone())))
    }

    fn latest_consensus_meta(&self, flavor: ConsensusFlavor) -> Result<Option<ConsensusMeta>> {
        Ok(self.latest(flavor, Some(false)).map(|c| c.meta.clone()))
    }

    #[cfg(test)]
    fn consensus_by_meta(&self, cmeta: &ConsensusMeta) -> Result<InputString> {
        self.consensus_by_sha3_digest_of_signed_part(cmeta.sha3_256_of_signed())?
            .map(|(t, _)| t)
            .ok_or(crate::Error::CacheCorruption("couldn't find a consensus we thought we had."))
    }

    fn consensus_by_sha3_digest_of_signed_part(&self, d: &[u8; 32]) -> Result<Option<(InputString, ConsensusMeta)>> {
        Ok(self
            .consensuses
            .iter()
            .find(|c| c.meta.sha3_256_of_signed() == d)
            .map(|c| (InputString::from(c.text.clone()), c.meta.clone())))
    }

    fn store_consensus(&mut self, cmeta: &ConsensusMeta, flavor: ConsensusFlavor, pending: bool, contents: &str) -> Result<()> {
        // INSERT OR REPLACE keyed by the digest of the whole document.
        self.consensuses.retain(|c| c.meta.sha3_256_of_whole() != cmeta.sha3_256_of_whole());
        self.consensuses.push(Consensus { meta: cmeta.clone(), flavor, pending, text: contents.to_owned() });
        Ok(())
    }

    fn mark_consensus_usable(&mut self, cmeta: &ConsensusMeta) -> Result<()> {
        for c in &mut self.consensuses {
            if c.meta.sha3_256_of_whole() == cmeta.sha3_256_of_whole() {
                c.pending = false;
            }
        }
        Ok(())
    }

    fn delete_consensus(&mut self, cmeta: &ConsensusMeta) -> Result<()> {
        self.consensuses.retain(|c| c.meta.sha3_256_of_whole() != cmeta.sha3_256_of_whole());
        Ok(())
    }

    fn authcerts(&self, certs: &[AuthCertKeyIds]) -> Result<HashMap<AuthCertKeyIds, String>> {
        Ok(certs.iter().filter_map(|ids| self.authcerts.get(ids).map(|(_, t)| (*ids, t.clone()))).collect())
    }

    fn store_authcerts(&mut self, certs: &[(AuthCertMeta, &str)]) -> Result<()> {
        for (meta, content) in certs {
            self.authcerts.insert(*meta.key_ids(), (meta.expires().into(), (*content).to_owned()));
        }
        Ok(())
    }

    fn microdescs(&self, digests: &[MdDigest]) -> Result<HashMap<MdDigest, String>> {
        Ok(digests.iter().filter_map(|d| self.microdescs.get(d).map(|(_, t)| (*d, t.clone()))).collect())
    }

    fn store_microdescs(&mut self, digests: &[(&str, &MdDigest)], when: SystemTime) -> Result<()> {
        let when: OffsetDateTime = when.into();
        for (content, d) in digests {
            self.microdescs.insert(**d, (when, (*content).to_owned()));
        }
        Ok(())
    }

    fn update_microdescs_listed(&mut self, digests: &[MdDigest], when: SystemTime) -> Result<()> {
        let when: OffsetDateTime = when.into();
        for d in digests {
            if let Some((listed, _)) = self.microdescs.get_mut(d) {
                *listed = (*listed).max(when);
            }
        }
        Ok(())
    }

    #[cfg(feature = "routerdesc")]
    fn routerdescs(&self, digests: &[RdDigest]) -> Result<HashMap<RdDigest, String>> {
        Ok(digests.iter().filter_map(|d| self.routerdescs.get(d).map(|(_, t)| (*d, t.clone()))).collect())
    }

    #[cfg(feature = "routerdesc")]
    fn store_routerdescs(&mut self, digests: &[(&str, SystemTime, &RdDigest)]) -> Result<()> {
        for (content, when, d) in digests {
            self.routerdescs.insert(**d, ((*when).into(), (*content).to_owned()));
        }
        Ok(())
    }

    #[cfg(feature = "bridge-client")]
    fn lookup_bridgedesc(&self, bridge: &BridgeConfig) -> Result<Option<CachedBridgeDescriptor>> {
        Ok(self.bridgedescs.get(&bridge.to_string()).map(|(d, _)| d.clone()))
    }

    #[cfg(feature = "bridge-client")]
    fn store_bridgedesc(&mut self, bridge: &BridgeConfig, entry: CachedBridgeDescriptor, until: SystemTime) -> Result<()> {
        self.bridgedescs.insert(bridge.to_string(), (entry, until.into()));
        Ok(())
    }

    #[cfg(feature = "bridge-client")]
    fn delete_bridgedesc(&mut self, bridge: &BridgeConfig) -> Result<()> {
        self.bridgedescs.remove(&bridge.to_string());
        Ok(())
    }

    fn update_protocol_recommendations(&mut self, valid_after: SystemTime, protocols: &ProtoStatuses) -> Result<()> {
        self.protocols = Some((valid_after, protocols.clone()));
        Ok(())
    }

    fn cached_protocol_recommendations(&self) -> Result<Option<(SystemTime, ProtoStatuses)>> {
        Ok(self.protocols.clone())
    }
}
