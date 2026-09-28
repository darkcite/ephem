// Remembered identities (§7.2, §7.3): up to 8 slots in IndexedDB, each holding the ENCRYPTED key
// file exactly as Rust produced it, plus its plaintext label and display handle for the sign-in
// list. The passphrase is never stored. Safari may clear this storage after 7 days without a
// visit in a Safari tab (not for an installed app): the downloaded key file is the backup.

export const MAX_SLOTS = 8;
const DB = 'ephem';
const STORE = 'slots';

function open() {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE, { keyPath: 'id' });
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function tx(mode, fn) {
  const db = await open();
  try {
    return await new Promise((resolve, reject) => {
      const t = db.transaction(STORE, mode);
      const req = fn(t.objectStore(STORE));
      t.oncomplete = () => resolve(req?.result);
      t.onerror = () => reject(t.error);
    });
  } finally {
    db.close();
  }
}

/** All slots, oldest first: `{ id, label, handle, blob, savedAt, stale }`. */
export async function list() {
  try {
    const all = (await tx('readonly', (s) => s.getAll())) || [];
    return all.sort((a, b) => a.savedAt - b.savedAt);
  } catch {
    return []; // private mode or storage blocked: the app still works with key files
  }
}

/**
 * Stores or updates a slot. `id` is the identity's lock name, so one identity has one slot.
 * `stale` = the downloaded backup no longer matches (contacts or nickname changed since).
 * Returns false when all 8 slots are taken by other identities.
 */
export async function put(slot) {
  const all = await list();
  if (!all.some((s) => s.id === slot.id) && all.length >= MAX_SLOTS) return false;
  await tx('readwrite', (s) => s.put({ ...slot, savedAt: slot.savedAt || Date.now() }));
  return true;
}

export async function get(id) {
  return (await list()).find((s) => s.id === id) || null;
}

export async function remove(id) {
  await tx('readwrite', (s) => s.delete(id));
}
