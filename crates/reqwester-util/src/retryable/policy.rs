use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::sync::Arc;
use std::time::Duration;

/// State of the run, given to a [`RetryPolicy`] after a failed attempt.
#[derive(Debug)]
pub struct Attempt<'a, E> {
    /// Failed attempts so far (starts at `1`).
    pub number: u32,
    /// Time since the first attempt.
    pub elapsed: Duration,
    /// The error of the failed attempt.
    pub error: &'a E,
}

impl<E> Clone for Attempt<'_, E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E> Copy for Attempt<'_, E> {}

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
pub trait RetryPolicy<E> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision;
}

impl<E, P: RetryPolicy<E> + ?Sized> RetryPolicy<E> for &P {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
        (**self).decide(attempt)
    }
}

impl<E, P: RetryPolicy<E> + ?Sized> RetryPolicy<E> for Box<P> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
        (**self).decide(attempt)
    }
}

impl<E, P: RetryPolicy<E> + ?Sized> RetryPolicy<E> for Arc<P> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
        (**self).decide(attempt)
    }
}

/// Marks policies that can be chained with [`RetryPolicyExt`].
///
/// A separate, error-agnostic trait: chaining must not depend on `E`, or `Fixed(..).and(..)`
/// could not be called before the error type is known. Implement it for custom policies.
pub trait Chainable {}

/// Chaining of policies.
pub trait RetryPolicyExt: Chainable + Sized {
    /// Retry if both agree; waits the longer delay.
    fn and<O>(self, other: O) -> And<Self, O> {
        And(self, other)
    }

    /// Retry if either agrees; waits the shorter delay.
    fn or<O>(self, other: O) -> Or<Self, O> {
        Or(self, other)
    }

    /// At most `attempts` attempts in total.
    fn max_attempts(self, attempts: u32) -> And<Self, MaxAttempts> {
        And(self, MaxAttempts(attempts))
    }

    /// Stop after `elapsed` since the first attempt.
    fn max_elapsed(self, elapsed: Duration) -> And<Self, MaxElapsed> {
        And(self, MaxElapsed(elapsed))
    }

    /// Retry only errors accepted by `predicate`.
    fn when<E, F: Fn(&E) -> bool>(self, predicate: F) -> And<Self, When<F>> {
        And(self, When(predicate))
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
    fn boxed<E>(self) -> Box<dyn RetryPolicy<E> + Send + Sync>
    where
        Self: RetryPolicy<E> + Send + Sync + 'static,
    {
        Box::new(self)
    }
}

impl<T: Chainable> RetryPolicyExt for T {}

impl Chainable for Never {}
impl Chainable for Immediate {}
impl Chainable for Fixed {}
impl Chainable for Linear {}
impl Chainable for Exponential {}
impl Chainable for MaxAttempts {}
impl Chainable for MaxElapsed {}
impl<F> Chainable for FromFn<F> {}
impl<F> Chainable for When<F> {}
impl<A, B> Chainable for And<A, B> {}
impl<A, B> Chainable for Or<A, B> {}
impl<P> Chainable for Cap<P> {}
impl<P> Chainable for Jitter<P> {}
impl<P: ?Sized> Chainable for Box<P> {}
impl<P: ?Sized> Chainable for Arc<P> {}

/// Never retries.
#[derive(Debug, Clone, Copy, Default)]
pub struct Never;

impl<E> RetryPolicy<E> for Never {
    fn decide(&self, _: Attempt<'_, E>) -> Decision {
        Decision::Stop
    }
}

/// Always retries immediately.
#[derive(Debug, Clone, Copy, Default)]
pub struct Immediate;

impl<E> RetryPolicy<E> for Immediate {
    fn decide(&self, _: Attempt<'_, E>) -> Decision {
        Decision::RETRY_NOW
    }
}

/// Always retries after the same delay.
#[derive(Debug, Clone, Copy)]
pub struct Fixed(pub Duration);

impl<E> RetryPolicy<E> for Fixed {
    fn decide(&self, _: Attempt<'_, E>) -> Decision {
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

impl<E> RetryPolicy<E> for Linear {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
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

impl<E> RetryPolicy<E> for Exponential {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
        let exp = attempt.number.saturating_sub(1).min(i32::MAX as u32) as i32;
        let secs = self.initial.as_secs_f64() * self.factor.powi(exp);

        Decision::retry_after(Duration::try_from_secs_f64(secs).unwrap_or(Duration::MAX))
    }
}

/// At most `.0` attempts in total.
#[derive(Debug, Clone, Copy)]
pub struct MaxAttempts(pub u32);

impl<E> RetryPolicy<E> for MaxAttempts {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
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

impl<E> RetryPolicy<E> for MaxElapsed {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
        if attempt.elapsed < self.0 {
            Decision::RETRY_NOW
        } else {
            Decision::Stop
        }
    }
}

/// Retries only errors accepted by the predicate.
#[derive(Clone, Copy)]
pub struct When<F>(pub F);

impl<E, F: Fn(&E) -> bool> RetryPolicy<E> for When<F> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
        if (self.0)(attempt.error) {
            Decision::RETRY_NOW
        } else {
            Decision::Stop
        }
    }
}

/// A policy from a function.
#[derive(Clone, Copy)]
pub struct FromFn<F>(pub F);

impl<E, F: Fn(Attempt<'_, E>) -> Decision> RetryPolicy<E> for FromFn<F> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
        (self.0)(attempt)
    }
}

/// See [`RetryPolicyExt::and`].
#[derive(Debug, Clone, Copy)]
pub struct And<A, B>(pub A, pub B);

impl<E, A: RetryPolicy<E>, B: RetryPolicy<E>> RetryPolicy<E> for And<A, B> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
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

impl<E, A: RetryPolicy<E>, B: RetryPolicy<E>> RetryPolicy<E> for Or<A, B> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
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

impl<E, P: RetryPolicy<E>> RetryPolicy<E> for Cap<P> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
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

impl<E, P: RetryPolicy<E>> RetryPolicy<E> for Jitter<P> {
    fn decide(&self, attempt: Attempt<'_, E>) -> Decision {
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

    fn at(number: u32, elapsed_ms: u64) -> Attempt<'static, ()> {
        Attempt {
            number,
            elapsed: MS(elapsed_ms),
            error: &(),
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
    fn when_looks_at_the_error() {
        let p = Immediate.when(|e: &u32| *e > 1);
        let at = |error| Attempt { number: 1, elapsed: MS(0), error };

        assert_eq!(p.decide(at(&1)), Decision::Stop);
        assert_eq!(p.decide(at(&2)), Decision::RETRY_NOW);
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
