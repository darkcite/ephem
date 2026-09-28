//! Ephem patch: `state_dir` for `wasm32-unknown-unknown`, in memory.
//!
//! The upstream module keeps each instance's state in a locked directory on disk
//! (`fs_mistrust::CheckedDir` + `fslock_guard`). A browser tab has no filesystem, and an Ephem
//! Tor session is ephemeral by design, so the same API is backed by a map that lives as long as
//! the `StateDirectory`: JSON values for [`StorageHandle`], nothing for raw subdirectories
//! (tor-hsservice's replay logs are ephemeral on wasm, see its patch), and a token instead of a
//! lock file (one tab, one process). Only the items tor-hsservice and arti-client use exist.

use crate::err::{Action, ErrorSource, Resource};
use crate::slug::TryIntoSlug;
use fs_mistrust::Mistrust;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::HashMap;
use std::fmt::{self, Display};
use std::marker::PhantomData;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub use crate::Error;

/// Result of state directory operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Stand-in for the lock file guard: a tab holds its instances for its whole life.
#[derive(Debug)]
pub struct LockFileGuard(());

/// Something holding an instance's "lock" (see [`LockFileGuard`]).
pub trait ContainsInstanceStateGuard {
    /// The guard.
    fn raw_lock_guard(&self) -> Arc<LockFileGuard>;
}

/// An instance of a facility that saves state (as upstream).
pub trait InstanceIdentity {
    /// The kind, e.g. `hss`.
    fn kind() -> &'static str;
    /// The identity, e.g. the onion service nickname.
    fn write_identity(&self, f: &mut fmt::Formatter) -> fmt::Result;
}

type Mem = Arc<Mutex<HashMap<String, String>>>;

/// The state of a tab's Tor instances.
#[derive(Debug, Clone)]
pub struct StateDirectory {
    mem: Mem,
}

impl StateDirectory {
    /// The path and trust settings are ignored: nothing is on disk.
    pub fn new(_state_dir: impl AsRef<Path>, _mistrust: &Mistrust) -> Result<Self> {
        Ok(Self { mem: Arc::new(Mutex::new(HashMap::new())) })
    }

    /// State of one instance (`kind/identity`).
    pub fn acquire_instance<I: InstanceIdentity>(&self, identity: &I) -> Result<InstanceStateHandle> {
        struct Id<'a, I>(&'a I);
        impl<I: InstanceIdentity> Display for Id<'_, I> {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                self.0.write_identity(f)
            }
        }
        let prefix = format!("{}/{}", I::kind(), Id(identity));
        Ok(InstanceStateHandle { mem: self.mem.clone(), prefix, guard: Arc::new(LockFileGuard(())) })
    }
}

/// One instance's state.
#[derive(Debug, Clone)]
pub struct InstanceStateHandle {
    mem: Mem,
    prefix: String,
    guard: Arc<LockFileGuard>,
}

impl ContainsInstanceStateGuard for InstanceStateHandle {
    fn raw_lock_guard(&self) -> Arc<LockFileGuard> {
        self.guard.clone()
    }
}

impl InstanceStateHandle {
    /// A JSON value of this instance.
    pub fn storage_handle<T>(&self, key: &(impl TryIntoSlug + ?Sized)) -> Result<StorageHandle<T>> {
        let key = key.try_into_slug()?;
        Ok(StorageHandle { mem: self.mem.clone(), key: format!("{}/{key}.json", self.prefix), guard: self.guard.clone(), marker: PhantomData })
    }

    /// A raw subdirectory: exists only as a name (nothing on wasm writes files).
    pub fn raw_subdir(&self, key: &(impl TryIntoSlug + ?Sized)) -> Result<InstanceRawSubdir> {
        let key = key.try_into_slug()?;
        Ok(InstanceRawSubdir { name: format!("{}/{key}", self.prefix), guard: self.guard.clone() })
    }

    /// Deletes everything of this instance.
    pub fn purge(self) -> Result<()> {
        let prefix = format!("{}/", self.prefix);
        self.mem.lock().expect("state lock").retain(|k, _| !k.starts_with(&prefix));
        Ok(())
    }
}

/// A JSON value kept for the life of the tab.
#[derive(Debug)]
pub struct StorageHandle<T> {
    mem: Mem,
    key: String,
    guard: Arc<LockFileGuard>,
    marker: PhantomData<fn(T) -> T>,
}

impl<T> ContainsInstanceStateGuard for StorageHandle<T> {
    fn raw_lock_guard(&self) -> Arc<LockFileGuard> {
        self.guard.clone()
    }
}

impl<T: Serialize + DeserializeOwned> StorageHandle<T> {
    fn err(&self, action: Action, e: impl Into<ErrorSource>) -> Error {
        Error::new(e, action, Resource::Temporary { key: self.key.clone() })
    }

    /// The stored value, if any.
    pub fn load(&self) -> Result<Option<T>> {
        let g = self.mem.lock().expect("state lock");
        g.get(&self.key).map(|s| serde_json::from_str(s).map_err(|e| self.err(Action::Loading, Arc::new(e)))).transpose()
    }

    /// Replaces the value.
    pub fn store(&mut self, v: &T) -> Result<()> {
        let s = serde_json::to_string(v).map_err(|e| self.err(Action::Storing, Arc::new(e)))?;
        self.mem.lock().expect("state lock").insert(self.key.clone(), s);
        Ok(())
    }

    /// Removes the value.
    pub fn delete(&mut self) -> Result<()> {
        self.mem.lock().expect("state lock").remove(&self.key);
        Ok(())
    }
}

/// A raw subdirectory's name (no files on wasm).
#[derive(Debug, Clone)]
pub struct InstanceRawSubdir {
    name: String,
    guard: Arc<LockFileGuard>,
}

impl InstanceRawSubdir {
    /// The virtual path (for messages; nothing is opened there on wasm).
    pub fn as_path(&self) -> &Path {
        Path::new(&self.name)
    }
}

impl ContainsInstanceStateGuard for InstanceRawSubdir {
    fn raw_lock_guard(&self) -> Arc<LockFileGuard> {
        self.guard.clone()
    }
}
