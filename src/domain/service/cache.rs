//! Write-invalidated cache of derived answers (#415).
//!
//! `/releases` and `/environments` — the GUI context bar, fetched on every
//! navigation — parse every milestone or configuration document;
//! `/reports/summary` and milestone progress read and deserialize every run
//! document. All four are O(store) recomputation of a value that changes
//! only when a write changes it, so they shared one eviction story:
//!
//! * **Exactly stale on the local replica**: every domain write bumps the
//!   cache generation, and a cached value from an older generation never
//!   answers. The invalidation is deliberately coarser than necessary — a
//!   case update drops the run summaries too — because over-invalidating
//!   costs a recomputation while under-invalidating costs correctness.
//! * **Bounded stale across replicas**: a second replica's writes never bump
//!   this process's generation (flock serialises the writes, not the cache),
//!   so entries also age out after [`MAX_AGE`]. The value
//!   a context bar or summary reports is a few tens of seconds of eventual
//!   accuracy across replicas in exchange for turning the GUI's every-frame
//!   O(store) parse into a map hit; a caller that must see another replica's
//!   write immediately reads the document routes, which are never cached.

use std::{
    collections::HashMap,
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use serde_json::Value;

/// How long a derived value may answer even though nothing locally invalided
/// it — the cross-replica staleness bound above.
const MAX_AGE: Duration = Duration::from_secs(45);

/// A map of derived JSON values, valid for as long as the generation they
/// were computed under is current and they are younger than [`MAX_AGE`].
#[derive(Debug, Default)]
pub(crate) struct DerivationCache {
    generation: AtomicU64,
    entries: RwLock<HashMap<String, Entry>>,
}

#[derive(Debug)]
struct Entry {
    generation: u64,
    written: Instant,
    value: Value,
}

/// An injective encoding of a scope set into a cache-key segment.
///
/// Length-prefixed rather than joined, because a stored identifier may
/// legally contain the separator — two different sets must never collide on
/// one key. An empty scope and "no filter" are distinct too: `None` is the
/// trusted caller that sees everything.
pub(crate) fn scope_key(reachable: Option<&[String]>) -> String {
    match reachable {
        None => "*".to_owned(),
        Some(projects) => projects
            .iter()
            .map(|project| format!("{}:{}", project.len(), project))
            .collect(),
    }
}

/// The same encoding for one optional filter value.
pub(crate) fn optional_key(value: Option<&str>) -> String {
    value.map_or_else(|| "-".to_owned(), |v| format!("{}:{}", v.len(), v))
}

impl DerivationCache {
    /// The cached value for `key`, if one is stored under the current
    /// generation and within the age bound.
    pub(crate) fn get(&self, key: &str) -> Option<Value> {
        let generation = self.generation.load(Ordering::SeqCst);
        let entries = self.entries.read().ok()?;
        let entry = entries.get(key)?;
        (entry.generation == generation && entry.written.elapsed() < MAX_AGE)
            .then(|| entry.value.clone())
    }

    /// Remembers `value` under the current generation. Entries from earlier
    /// generations — dead on arrival for every reader — are dropped here,
    /// which is what keeps the map from growing across writes.
    pub(crate) fn put(&self, key: &str, value: &Value) {
        let generation = self.generation.load(Ordering::SeqCst);
        if let Ok(mut entries) = self.entries.write() {
            entries.retain(|_, entry| entry.generation == generation);
            entries.insert(
                key.to_owned(),
                Entry {
                    generation,
                    written: Instant::now(),
                    value: value.clone(),
                },
            );
        }
    }

    /// Retires every entry: called after any local write to any document a
    /// derived endpoint could read.
    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_entry_survives_reads_and_dies_on_invalidation() {
        let cache = DerivationCache::default();
        assert!(cache.get("releases|*").is_none());
        cache.put("releases|*", &json!(["v1"]));
        assert_eq!(cache.get("releases|*"), Some(json!(["v1"])));
        cache.invalidate();
        assert!(
            cache.get("releases|*").is_none(),
            "a write must retire the derived answers"
        );
        // The stale entry is also physically gone at the next store, so
        // generations do not accumulate.
        cache.put("summary|*", &json!({}));
        assert_eq!(
            cache.entries.read().expect("lock").len(),
            1,
            "entries from dead generations are dropped on store"
        );
    }

    #[test]
    fn keys_are_independent() {
        let cache = DerivationCache::default();
        cache.put("releases|a.json", &json!(["x"]));
        cache.put("releases|a.json,b.json", &json!(["x", "y"]));
        assert_eq!(
            cache
                .get("releases|a.json")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            cache
                .get("releases|a.json,b.json")
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }
}
