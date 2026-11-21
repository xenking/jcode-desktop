//! Cache accounting for distinct completed model requests.

use std::collections::HashSet;

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
