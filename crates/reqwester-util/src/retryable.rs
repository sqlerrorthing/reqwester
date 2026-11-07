//! Retrying of failed requests: a chainable [`RetryPolicy`], which sees the error,
//! decides if and when to retry. Use `when` to retry only some errors.
//!
//! ```ignore
//! let policy = Exponential::new(Duration::from_millis(100)).max_attempts(5).when(|e: &Error| e.is_transient());
//! let me = api.get_me().retryable(policy).await?;
//! ```

mod policy;
mod request;
mod sleep;

pub use policy::*;
pub use request::*;
pub use sleep::*;
