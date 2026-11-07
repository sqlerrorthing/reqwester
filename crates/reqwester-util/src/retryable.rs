//! Retrying of failed requests: [`IsRetryable`] says if an error can be retried,
//! a chainable [`RetryPolicy`] says if and when to retry.
//!
//! ```ignore
//! let policy = Exponential::new(Duration::from_millis(100)).cap(Duration::from_secs(5)).max_attempts(5);
//! let me = api.get_me().retryable(policy).await?;
//! ```

mod policy;
mod request;
mod sleep;

pub use policy::*;
pub use request::*;
pub use sleep::*;

use std::convert::Infallible;
use std::sync::Arc;

/// Whether a request that failed with this error may be sent again.
pub trait IsRetryable {
    fn is_retryable(&self) -> bool;
}

impl IsRetryable for Infallible {
    fn is_retryable(&self) -> bool {
        match *self {}
    }
}

impl<E: IsRetryable + ?Sized> IsRetryable for &E {
    fn is_retryable(&self) -> bool {
        (**self).is_retryable()
    }
}

impl<E: IsRetryable + ?Sized> IsRetryable for Box<E> {
    fn is_retryable(&self) -> bool {
        (**self).is_retryable()
    }
}

impl<E: IsRetryable + ?Sized> IsRetryable for Arc<E> {
    fn is_retryable(&self) -> bool {
        (**self).is_retryable()
    }
}
