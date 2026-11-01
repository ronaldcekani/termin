//! Small in-memory frecency scores used to rank palette destinations.
//!
//! Persisting this data belongs to the persistence track; this module provides
//! the bounded ranking primitive for the palette.

use rustc_hash::FxHashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
struct Visit {
    at: Instant,
    count: u16,
}

/// Bounded recency/frequency store. Oldest keys are evicted at capacity.
#[derive(Debug)]
pub struct Frecency {
    visits: FxHashMap<String, Visit>,
    capacity: usize,
}

impl Default for Frecency {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl Frecency {
    pub fn new(capacity: usize) -> Self {
        Self {
            visits: FxHashMap::default(),
            capacity: capacity.max(1),
        }
    }
    pub fn record(&mut self, key: &str, now: Instant) {
        if let Some(v) = self.visits.get_mut(key) {
            v.at = now;
            v.count = v.count.saturating_add(1);
            return;
        }
        if self.visits.len() >= self.capacity {
            if let Some(oldest) = self
                .visits
                .iter()
                .min_by_key(|(_, v)| v.at)
                .map(|(k, _)| k.clone())
            {
                self.visits.remove(&oldest);
            }
        }
        self.visits
            .insert(key.to_owned(), Visit { at: now, count: 1 });
    }
    /// A bounded boost that decays with a one-day half-life.
    pub fn score(&self, key: &str, now: Instant) -> f32 {
        let Some(v) = self.visits.get(key) else {
            return 0.0;
        };
        let age = now.saturating_duration_since(v.at).as_secs_f32();
        (v.count as f32).ln_1p() * (-age / Duration::from_secs(86_400).as_secs_f32()).exp2()
    }
    pub fn len(&self) -> usize {
        self.visits.len()
    }
    pub fn is_empty(&self) -> bool {
        self.visits.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ranking_decays_and_is_bounded() {
        let t = Instant::now();
        let mut f = Frecency::new(1);
        f.record("a", t);
        f.record("a", t);
        assert!(f.score("a", t) > 0.0);
        assert_eq!(
            f.score("a", t + Duration::from_secs(86_400)),
            f.score("a", t) * 0.5
        );
        f.record("b", t + Duration::from_secs(1));
        assert_eq!(f.len(), 1);
        assert_eq!(f.score("a", t), 0.0);
    }
}
