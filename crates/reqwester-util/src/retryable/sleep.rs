use std::future::{Future, Ready, ready};
use std::time::Duration;

/// Waits between attempts.
pub trait Sleeper: Clone + Send + Sync + 'static {
    type Sleep: Future<Output = ()> + Send + 'static;

    fn sleep(&self, duration: Duration) -> Self::Sleep;
}

/// [`tokio::time::sleep`].
#[cfg(feature = "tokio")]
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSleeper;

#[cfg(feature = "tokio")]
impl Sleeper for TokioSleeper {
    type Sleep = tokio::time::Sleep;

    fn sleep(&self, duration: Duration) -> Self::Sleep {
        tokio::time::sleep(duration)
    }
}

/// Ignores delays.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoSleep;

impl Sleeper for NoSleep {
    type Sleep = Ready<()>;

    fn sleep(&self, _: Duration) -> Self::Sleep {
        ready(())
    }
}

/// [`TokioSleeper`] with the `tokio` feature, [`NoSleep`] otherwise.
#[cfg(feature = "tokio")]
pub type DefaultSleeper = TokioSleeper;

/// [`TokioSleeper`] with the `tokio` feature, [`NoSleep`] otherwise.
#[cfg(not(feature = "tokio"))]
pub type DefaultSleeper = NoSleep;
