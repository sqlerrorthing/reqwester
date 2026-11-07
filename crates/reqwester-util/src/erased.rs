//! Type erasure: [`ErasedRequest`] hides the concrete request type behind a `dyn`
//! (only the payload and the error stay in the type), so differently wrapped requests
//! (plain, retrying, traced...) can be stored, returned and passed around uniformly.
//!
//! For whole requesters see the `erased` adapter of `#[requester]`.

mod request;

pub use request::*;
