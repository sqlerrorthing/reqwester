//! Tracing of requests: a span per sent request and an event with its outcome and duration.
//! Payloads are never logged, as they may hold secrets.
//!
//! Order matters when combined with retries: `.traced().retryable(..)` traces every attempt,
//! `.retryable(..).traced()` traces the whole run.

mod future;
mod request;

pub use future::*;
pub use request::*;
