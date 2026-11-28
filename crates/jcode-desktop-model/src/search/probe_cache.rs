//! In-memory answers to filesystem questions that render needs.
//!
//! Render only calls [`ProbeCache::get`], which never touches the disk. A miss
//! (or a value older than the TTL) queues the key once. The owner drains the
//! queue with [`ProbeCache::take_queued`], runs the probes on a background
//! executor and hands the answers to [`ProbeCache::fill`]. Stale values keep
//! rendering while their refresh is in flight, so nothing flickers when a TTL
//! lapses.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::time::{Duration, Instant};

pub struct ProbeCache<K, V> {
    entries: HashMap<K, (V, Instant)>,
    /// Keys render asked for that no probe has been started for.
    queued: Vec<K>,
    /// Queued or running. A key is probed again only after it is filled.
    in_flight: HashSet<K>,
    ttl: Duration,
    capacity: usize,
}

impl<K: Hash + Eq + Clone, V: PartialEq> ProbeCache<K, V> {
    /// `capacity` bounds memory: past it, filling drops entries older than `ttl`.
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            queued: Vec::new(),
            in_flight: HashSet::new(),
            ttl,
            capacity,
        }
    }

    /// The last known answer, if any. Never blocks. Queues a probe when the
    /// answer is missing or older than the TTL.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ToOwned<Owned = K> + ?Sized,
    {
        let now = crate::fs_probe::now();
        let fresh = self
            .entries
            .get(key)
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) < self.ttl);
        if !fresh && !self.in_flight.contains(key) {
            let owned = key.to_owned();
            self.in_flight.insert(owned.clone());
            self.queued.push(owned);
        }
        self.entries.get(key).map(|(value, _)| value)
    }

    /// The last known answer without asking for a refresh.
    pub fn peek<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.entries.get(key).map(|(value, _)| value)
    }

    /// Keys to probe now. Each is returned once until it is filled.
    #[allow(clippy::drain_collect, reason = "Keep queue capacity for the next render pass")]
    pub fn take_queued(&mut self) -> Vec<K> {
        self.queued.drain(..).collect()
    }

    /// Record a probe result. Returns whether the visible answer changed, so
    /// the owner redraws only when the frame would differ.
    pub fn fill(&mut self, key: K, value: V) -> bool {
        let now = crate::fs_probe::now();
        self.in_flight.remove(&key);
        if self.entries.len() >= self.capacity && !self.entries.contains_key(&key) {
            let ttl = self.ttl;
            self.entries
                .retain(|_, (_, at)| now.saturating_duration_since(*at) < ttl);
        }
        let changed = self.entries.get(&key).is_none_or(|(old, _)| *old != value);
        self.entries.insert(key, (value, now));
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> ProbeCache<String, bool> {
        ProbeCache::new(Duration::from_secs(5), 2)
    }

    #[test]
    fn a_miss_queues_the_key_once_until_it_is_filled() {
        let mut cache = cache();
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.get("a"), None);
        assert_eq!(cache.take_queued(), vec!["a".to_owned()]);
        assert_eq!(cache.get("a"), None, "still in flight");
        assert!(cache.take_queued().is_empty());
        assert!(cache.fill("a".into(), true));
        assert_eq!(cache.get("a"), Some(&true));
        assert!(cache.take_queued().is_empty());
    }

    #[test]
    fn a_stale_value_keeps_rendering_while_it_refreshes_in_the_background() {
        let mut cache = cache();
        cache.fill("a".into(), true);
        crate::fs_probe::advance_clock(Duration::from_secs(6));
        assert_eq!(cache.get("a"), Some(&true));
        assert_eq!(cache.take_queued(), vec!["a".to_owned()]);
        assert!(!cache.fill("a".into(), true), "same answer is not a change");
        assert_eq!(cache.get("a"), Some(&true));
        assert!(cache.take_queued().is_empty(), "fresh again");
    }

    #[test]
    fn fill_reports_changed_answers_only() {
        let mut cache = cache();
        assert!(cache.fill("a".into(), false));
        assert!(!cache.fill("a".into(), false));
        assert!(cache.fill("a".into(), true));
    }

    #[test]
    fn capacity_drops_only_expired_entries() {
        let mut cache = cache();
        cache.fill("old".into(), true);
        crate::fs_probe::advance_clock(Duration::from_secs(6));
        cache.fill("a".into(), true);
        cache.fill("b".into(), true);
        assert_eq!(cache.get("a"), Some(&true));
        assert_eq!(cache.get("b"), Some(&true));
        assert_eq!(cache.get("old"), None);
    }
}
