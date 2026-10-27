//! Per-client limits for the HTTP API: a request budget per credential (token bucket) and a
//! cap on refused requests per source address. Both live in memory, so a restart resets them;
//! they protect the gateway from a runaway or hostile client, they are not a billing quota.
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Mutex,
    time::{Duration, Instant},
};

/// At most this many clients are tracked at once. When full, idle entries are dropped first;
/// if every entry is busy, new clients are refused until some go idle (fail closed).
const MAX_TRACKED: usize = 100_000;

struct Bucket {
    tokens: f64,
    at: Instant,
}

/// A token bucket per key: `per_second` sustained, bursts up to twice that.
pub struct RateLimiter {
    per_second: f64,
    burst: f64,
    buckets: Mutex<HashMap<[u8; 32], Bucket>>,
}

impl RateLimiter {
    pub fn new(per_second: u32) -> Self {
        let per_second = f64::from(per_second.max(1));
        Self {
            per_second,
            burst: per_second * 2.0,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Takes one request from `key`'s budget, or says how long to wait for the next one.
    pub fn check(&self, key: &[u8], now: Instant) -> Result<(), Duration> {
        let key = *blake3::hash(key).as_bytes();
        let mut buckets = self
            .buckets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !buckets.contains_key(&key) && buckets.len() >= MAX_TRACKED {
            let (per_second, burst) = (self.per_second, self.burst);
            buckets.retain(|_, b| refill(b, per_second, burst, now) < burst);
            if buckets.len() >= MAX_TRACKED {
                return Err(Duration::from_secs(1));
            }
        }
        let bucket = buckets.entry(key).or_insert(Bucket {
            tokens: self.burst,
            at: now,
        });
        let tokens = refill(bucket, self.per_second, self.burst, now);
        if tokens >= 1.0 {
            bucket.tokens = tokens - 1.0;
            Ok(())
        } else {
            bucket.tokens = tokens;
            Err(Duration::from_secs_f64((1.0 - tokens) / self.per_second))
        }
    }
}

/// Brings a bucket up to `now` and returns its tokens.
fn refill(bucket: &mut Bucket, per_second: f64, burst: f64, now: Instant) -> f64 {
    let elapsed = now.saturating_duration_since(bucket.at).as_secs_f64();
    bucket.tokens = (bucket.tokens + elapsed * per_second).min(burst);
    bucket.at = now;
    bucket.tokens
}

/// Counts refused requests (HTTP 401 or 403) per source address in fixed one-minute windows. A source
/// that reaches `per_minute` refusals is blocked until its window ends, which makes guessing
/// tokens slow without affecting other sources.
pub struct FailureLimiter {
    per_minute: u32,
    sources: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

const WINDOW: Duration = Duration::from_secs(60);

impl FailureLimiter {
    pub fn new(per_minute: u32) -> Self {
        Self {
            per_minute: per_minute.max(1),
            sources: Mutex::new(HashMap::new()),
        }
    }

    /// How long `source` is still blocked, if it is.
    pub fn blocked(&self, source: IpAddr, now: Instant) -> Option<Duration> {
        let sources = self
            .sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (start, count) = sources.get(&source)?;
        let end = *start + WINDOW;
        (*count >= self.per_minute && now < end).then(|| end - now)
    }

    pub fn record(&self, source: IpAddr, now: Instant) {
        let mut sources = self
            .sources
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !sources.contains_key(&source) && sources.len() >= MAX_TRACKED {
            sources.retain(|_, (start, _)| now < *start + WINDOW);
        }
        if sources.len() >= MAX_TRACKED && !sources.contains_key(&source) {
            return;
        }
        let entry = sources.entry(source).or_insert((now, 0));
        if now >= entry.0 + WINDOW {
            *entry = (now, 0);
        }
        entry.1 = entry.1.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_credential_gets_its_burst_then_its_rate_and_others_are_unaffected() {
        let limiter = RateLimiter::new(5);
        let start = Instant::now();
        for _ in 0..10 {
            assert!(limiter.check(b"agent-a", start).is_ok());
        }
        let wait = limiter.check(b"agent-a", start).unwrap_err();
        assert!(
            wait > Duration::ZERO && wait <= Duration::from_millis(200),
            "{wait:?}"
        );
        // Another credential has its own budget.
        assert!(limiter.check(b"owner", start).is_ok());
        // One token comes back every 200 ms, and no more than the burst accumulates.
        assert!(
            limiter
                .check(b"agent-a", start + Duration::from_millis(210))
                .is_ok()
        );
        assert!(
            limiter
                .check(b"agent-a", start + Duration::from_millis(220))
                .is_err()
        );
        let later = start + Duration::from_secs(60);
        for _ in 0..10 {
            assert!(limiter.check(b"agent-a", later).is_ok());
        }
        assert!(limiter.check(b"agent-a", later).is_err());
    }

    #[test]
    fn refusals_block_only_their_source_for_the_rest_of_the_minute() {
        let limiter = FailureLimiter::new(3);
        let guesser: IpAddr = "192.0.2.7".parse().unwrap();
        let other: IpAddr = "192.0.2.8".parse().unwrap();
        let start = Instant::now();
        for i in 0..3 {
            assert_eq!(limiter.blocked(guesser, start), None, "blocked after {i}");
            limiter.record(guesser, start);
        }
        let left = limiter
            .blocked(guesser, start + Duration::from_secs(10))
            .unwrap();
        assert_eq!(left, Duration::from_secs(50));
        assert_eq!(limiter.blocked(other, start), None);
        // The next window starts clean, and keeps counting: guessing again is blocked again.
        let next = start + Duration::from_secs(61);
        assert_eq!(limiter.blocked(guesser, next), None);
        limiter.record(guesser, next);
        assert_eq!(limiter.blocked(guesser, next), None);
        limiter.record(guesser, next);
        limiter.record(guesser, next);
        assert_eq!(
            limiter.blocked(guesser, next + Duration::from_secs(1)),
            Some(Duration::from_secs(59))
        );
    }
}
