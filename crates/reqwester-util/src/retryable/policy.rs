use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::Arc;
use std::time::Duration;

/// State of the run, given to a [`RetryPolicy`] after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attempt {
    /// Failed attempts so far (starts at `1`).
    pub number: u32,
    /// Time since the first attempt.
    pub elapsed: Duration,
}

/// What to do after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Send again after `after`.
    Retry { after: Duration },
    /// Return the last error.
    Stop,
}

impl Decision {
    /// Retry immediately.
    pub const RETRY_NOW: Self = Self::Retry {
        after: Duration::ZERO,
    };

    pub const fn retry_after(after: Duration) -> Self {
        Self::Retry { after }
    }
}

/// Decides whether and when to retry. Stateless, so one policy can be shared.
pub trait RetryPolicy {
    fn decide(&self, attempt: Attempt) -> Decision;
}

impl<P: RetryPolicy + ?Sized> RetryPolicy for &P {
    fn decide(&self, attempt: Attempt) -> Decision {
        (**self).decide(attempt)
    }
}

impl<P: RetryPolicy + ?Sized> RetryPolicy for Box<P> {
    fn decide(&self, attempt: Attempt) -> Decision {
        (**self).decide(attempt)
    }
}

impl<P: RetryPolicy + ?Sized> RetryPolicy for Arc<P> {
    fn decide(&self, attempt: Attempt) -> Decision {
        (**self).decide(attempt)
    }
}

/// Chaining of policies.
pub trait RetryPolicyExt: RetryPolicy + Sized {
    /// Retry if both agree; waits the longer delay.
    fn and<O: RetryPolicy>(self, other: O) -> And<Self, O> {
        And(self, other)
    }

    /// Retry if either agrees; waits the shorter delay.
    fn or<O: RetryPolicy>(self, other: O) -> Or<Self, O> {
        Or(self, other)
    }

    /// At most `attempts` attempts in total.
    fn max_attempts(self, attempts: u32) -> And<Self, MaxAttempts> {
        self.and(MaxAttempts(attempts))
    }

    /// Stop after `elapsed` since the first attempt.
    fn max_elapsed(self, elapsed: Duration) -> And<Self, MaxElapsed> {
        self.and(MaxElapsed(elapsed))
    }

    /// Delay is at most `max`.
    fn cap(self, max: Duration) -> Cap<Self> {
        Cap { inner: self, max }
    }

    /// Randomly shortens the delay by up to `ratio` (`0.0..=1.0`).
    fn jitter(self, ratio: f64) -> Jitter<Self> {
        Jitter {
            inner: self,
            ratio: ratio.clamp(0.0, 1.0),
        }
    }

    /// Erases the policy type.
    fn boxed(self) -> Box<dyn RetryPolicy + Send + Sync>
    where
        Self: Send + Sync + 'static,
    {
        Box::new(self)
    }
}

impl<P: RetryPolicy> RetryPolicyExt for P {}

/// Never retries.
#[derive(Debug, Clone, Copy, Default)]
pub struct Never;

impl RetryPolicy for Never {
    fn decide(&self, _: Attempt) -> Decision {
        Decision::Stop
    }
}

/// Always retries immediately.
#[derive(Debug, Clone, Copy, Default)]
pub struct Immediate;

impl RetryPolicy for Immediate {
    fn decide(&self, _: Attempt) -> Decision {
        Decision::RETRY_NOW
    }
}

/// Always retries after the same delay.
#[derive(Debug, Clone, Copy)]
pub struct Fixed(pub Duration);

impl RetryPolicy for Fixed {
    fn decide(&self, _: Attempt) -> Decision {
        Decision::retry_after(self.0)
    }
}

/// Delay grows by `step` each attempt.
#[derive(Debug, Clone, Copy)]
pub struct Linear {
    pub step: Duration,
}

impl Linear {
    pub const fn new(step: Duration) -> Self {
        Self { step }
    }
}

impl RetryPolicy for Linear {
    fn decide(&self, attempt: Attempt) -> Decision {
        Decision::retry_after(self.step.saturating_mul(attempt.number))
    }
}

/// Delay is multiplied by `factor` each attempt.
#[derive(Debug, Clone, Copy)]
pub struct Exponential {
    pub initial: Duration,
    pub factor: f64,
}

impl Exponential {
    pub const fn new(initial: Duration) -> Self {
        Self {
            initial,
            factor: 2.0,
        }
    }

    pub const fn factor(mut self, factor: f64) -> Self {
        self.factor = factor;
        self
    }
}

impl RetryPolicy for Exponential {
    fn decide(&self, attempt: Attempt) -> Decision {
        let exp = attempt.number.saturating_sub(1).min(i32::MAX as u32) as i32;
        let secs = self.initial.as_secs_f64() * self.factor.powi(exp);

        Decision::retry_after(Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX))
    }
}

/// At most `.0` attempts in total.
#[derive(Debug, Clone, Copy)]
pub struct MaxAttempts(pub u32);

impl RetryPolicy for MaxAttempts {
    fn decide(&self, attempt: Attempt) -> Decision {
        if attempt.number < self.0 {
            Decision::RETRY_NOW
        } else {
            Decision::Stop
        }
    }
}

/// Stops once `.0` has passed since the first attempt.
#[derive(Debug, Clone, Copy)]
pub struct MaxElapsed(pub Duration);

impl RetryPolicy for MaxElapsed {
    fn decide(&self, attempt: Attempt) -> Decision {
        if attempt.elapsed < self.0 {
            Decision::RETRY_NOW
        } else {
            Decision::Stop
        }
    }
}

/// A policy from a function.
#[derive(Clone, Copy)]
pub struct FromFn<F>(pub F);

impl<F: Fn(Attempt) -> Decision> RetryPolicy for FromFn<F> {
    fn decide(&self, attempt: Attempt) -> Decision {
        (self.0)(attempt)
    }
}

/// See [`RetryPolicyExt::and`].
#[derive(Debug, Clone, Copy)]
pub struct And<A, B>(pub A, pub B);

impl<A: RetryPolicy, B: RetryPolicy> RetryPolicy for And<A, B> {
    fn decide(&self, attempt: Attempt) -> Decision {
        match (self.0.decide(attempt), self.1.decide(attempt)) {
            (Decision::Retry { after: a }, Decision::Retry { after: b }) => {
                Decision::retry_after(a.max(b))
            }
            _ => Decision::Stop,
        }
    }
}

/// See [`RetryPolicyExt::or`].
#[derive(Debug, Clone, Copy)]
pub struct Or<A, B>(pub A, pub B);

impl<A: RetryPolicy, B: RetryPolicy> RetryPolicy for Or<A, B> {
    fn decide(&self, attempt: Attempt) -> Decision {
        match (self.0.decide(attempt), self.1.decide(attempt)) {
            (Decision::Retry { after: a }, Decision::Retry { after: b }) => {
                Decision::retry_after(a.min(b))
            }
            (retry @ Decision::Retry { .. }, Decision::Stop)
            | (Decision::Stop, retry @ Decision::Retry { .. }) => retry,
            (Decision::Stop, Decision::Stop) => Decision::Stop,
        }
    }
}

/// See [`RetryPolicyExt::cap`].
#[derive(Debug, Clone, Copy)]
pub struct Cap<P> {
    inner: P,
    max: Duration,
}

impl<P: RetryPolicy> RetryPolicy for Cap<P> {
    fn decide(&self, attempt: Attempt) -> Decision {
        match self.inner.decide(attempt) {
            Decision::Retry { after } => Decision::retry_after(after.min(self.max)),
            Decision::Stop => Decision::Stop,
        }
    }
}

/// See [`RetryPolicyExt::jitter`].
#[derive(Debug, Clone, Copy)]
pub struct Jitter<P> {
    inner: P,
    ratio: f64,
}

impl<P: RetryPolicy> RetryPolicy for Jitter<P> {
    fn decide(&self, attempt: Attempt) -> Decision {
        match self.inner.decide(attempt) {
            Decision::Retry { after } => {
                let random = RandomState::new().build_hasher().finish() as f64 / u64::MAX as f64;

                Decision::retry_after(after.mul_f64(1.0 - self.ratio * random))
            }
            Decision::Stop => Decision::Stop,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn at(number: u32, elapsed_ms: u64) -> Attempt {
        Attempt {
            number,
            elapsed: MS(elapsed_ms),
        }
    }

    #[test]
    fn exponential_grows_and_caps() {
        let p = Exponential::new(MS(100)).cap(MS(350));

        assert_eq!(p.decide(at(1, 0)), Decision::retry_after(MS(100)));
        assert_eq!(p.decide(at(2, 0)), Decision::retry_after(MS(200)));
        assert_eq!(p.decide(at(3, 0)), Decision::retry_after(MS(350)));
        assert_eq!(p.decide(at(u32::MAX, 0)), Decision::retry_after(MS(350)));
    }

    #[test]
    fn and_stops_when_any_stops_and_takes_longest_delay() {
        let p = Fixed(MS(10)).max_attempts(3).max_elapsed(MS(100));

        assert_eq!(p.decide(at(1, 0)), Decision::retry_after(MS(10)));
        assert_eq!(p.decide(at(3, 0)), Decision::Stop);
        assert_eq!(p.decide(at(1, 100)), Decision::Stop);
    }

    #[test]
    fn or_retries_when_any_retries() {
        let p = Never.or(Fixed(MS(5)));
        assert_eq!(p.decide(at(9, 0)), Decision::retry_after(MS(5)));
        assert_eq!(Never.or(Never).decide(at(1, 0)), Decision::Stop);
    }

    #[test]
    fn jitter_only_shortens() {
        let p = Fixed(MS(100)).jitter(0.5);
        for _ in 0..50 {
            let Decision::Retry { after } = p.decide(at(1, 0)) else {
                panic!()
            };
            assert!((MS(50)..=MS(100)).contains(&after));
        }
    }
}
