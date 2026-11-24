//! Retention and accounting for distinct completed model requests.

use std::collections::HashSet;

pub const SHORT_COUNTDOWN_MAX_SECS: u64 = 20 * 60;

/// A provider's retention window and how long is left of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheFacts {
    pub ttl_secs: u64,
    pub remaining_secs: u64,
    pub written_ago_secs: u64,
    /// The provider documents this as a retention estimate, not an expiry.
    pub is_estimate: bool,
    pub cached_tokens: Option<u64>,
}

impl CacheFacts {
    pub fn new(
        ttl_secs: u64,
        is_estimate: bool,
        touched_ms: i64,
        now_ms: i64,
        cached_tokens: Option<u64>,
    ) -> Self {
        let written_ago_secs = now_ms.saturating_sub(touched_ms).max(0) as u64 / 1000;
        Self {
            ttl_secs,
            remaining_secs: ttl_secs.saturating_sub(written_ago_secs),
            written_ago_secs,
            is_estimate,
            cached_tokens,
        }
    }

    pub fn after(self, elapsed_secs: u64) -> Self {
        Self {
            remaining_secs: self.remaining_secs.saturating_sub(elapsed_secs),
            written_ago_secs: self.written_ago_secs.saturating_add(elapsed_secs),
            ..self
        }
    }

    pub fn is_cold(&self) -> bool {
        self.remaining_secs == 0
    }

    /// `Some(0)` means the window is open but close enough to warn.
    pub fn secs_until_warning(&self) -> Option<u64> {
        if self.is_estimate || self.is_cold() {
            return None;
        }
        let boundary = if self.ttl_secs > SHORT_COUNTDOWN_MAX_SECS {
            SHORT_COUNTDOWN_MAX_SECS - 1
        } else {
            (self.ttl_secs / 10).max(60)
        };
        Some(self.remaining_secs.saturating_sub(boundary))
    }

    pub fn expiring_soon(&self) -> bool {
        self.secs_until_warning() == Some(0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    prompt: Option<u64>,
    pub read: Option<u64>,
    pub written: Option<u64>,
}

impl Tokens {
    /// `prompt` is the provider-normalized prompt size, not raw input usage.
    pub fn new(prompt: Option<u64>, read: Option<u64>, written: Option<u64>) -> Self {
        Self {
            prompt,
            read,
            written,
        }
    }

    pub fn percent(self) -> Option<f32> {
        let prompt = self.prompt?;
        let read = self.read?;
        (prompt > 0 && read <= prompt).then(|| (read as f64 * 100.0 / prompt as f64) as f32)
    }

    pub fn not_read(self) -> Option<u64> {
        self.prompt?.checked_sub(self.read?)
    }

    pub fn uncached(self) -> Option<u64> {
        self.not_read()?.checked_sub(self.written?)
    }

    fn add(self, next: Self) -> Self {
        fn sum(a: Option<u64>, b: Option<u64>) -> Option<u64> {
            a?.checked_add(b?)
        }
        Self {
            prompt: sum(self.prompt, next.prompt),
            read: sum(self.read, next.read),
            written: sum(self.written, next.written),
        }
    }
}

#[derive(Default)]
pub struct Usage {
    requests: HashSet<String>,
    pub session: Option<Tokens>,
    pub last: Option<Tokens>,
}

impl Usage {
    /// Returns false on redelivery, not when two different requests match.
    pub fn record(&mut self, request_id: &str, tokens: Tokens) -> bool {
        if !self.requests.insert(request_id.to_owned()) {
            return false;
        }
        self.session = Some(self.session.map_or(tokens, |sum| sum.add(tokens)));
        self.last = Some(tokens);
        true
    }

    pub fn count(&self) -> usize {
        self.requests.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_retention_warns_only_below_twenty_minutes() {
        let cache = CacheFacts::new(3600, false, 0, 40 * 60_000, None);
        assert_eq!(cache.secs_until_warning(), Some(1));
        assert!(!cache.expiring_soon());
        assert!(cache.after(1).expiring_soon());
        assert!(cache.after(8 * 60).expiring_soon());
        assert!(cache.after(20 * 60).is_cold());
        assert!(!cache.after(20 * 60).expiring_soon());
    }

    #[test]
    fn five_minute_retention_warns_in_its_last_minute() {
        let cache = CacheFacts::new(300, false, 0, 239_000, None);
        assert!(!cache.expiring_soon());
        assert!(cache.after(1).expiring_soon());
        assert!(!cache.after(61).expiring_soon());
    }

    #[test]
    fn estimates_have_no_warning_window() {
        let cache = CacheFacts::new(3600, true, 0, 48 * 60_000, None);
        assert_eq!(cache.remaining_secs, 12 * 60);
        assert_eq!(cache.secs_until_warning(), None);
    }

    #[test]
    fn distinct_calls_are_weighted_by_prompt_size_and_redelivery_is_ignored() {
        let mut usage = Usage::default();
        let warm = Tokens::new(Some(900), Some(810), Some(80));
        let cold = Tokens::new(Some(100), Some(0), Some(0));
        assert_eq!(usage.session, None);
        assert!(usage.record("first", warm));
        assert!(usage.record("second", cold));
        assert!(!usage.record("first", warm));
        assert_eq!(usage.count(), 2);
        assert_eq!(usage.last.unwrap().percent(), Some(0.0));
        assert_eq!(usage.session.unwrap().percent(), Some(81.0));
        assert_eq!(usage.session.unwrap().uncached(), Some(110));
    }

    #[test]
    fn unknown_counters_are_not_zero() {
        let mut usage = Usage::default();
        let no_write = Tokens::new(Some(100), Some(80), None);
        let no_read = Tokens::new(Some(100), None, Some(0));
        assert_eq!(no_write.percent(), Some(80.0));
        assert_eq!(no_write.not_read(), Some(20));
        assert_eq!(no_write.uncached(), None);
        assert!(usage.record("first", no_write));
        assert!(usage.record("second", no_read));
        assert_eq!(usage.session.unwrap().percent(), None);
        assert_eq!(Tokens::new(Some(0), Some(0), Some(0)).percent(), None);
    }
}
