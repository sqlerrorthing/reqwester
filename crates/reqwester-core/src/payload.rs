use std::borrow::{Borrow, BorrowMut};

pub type PayloadOutput<T> = <<T as HasPayload>::Payload as Payload>::Output;

/// A trait for types that act as containers for a specific [`Payload`]
pub trait HasPayload {
    type Payload: Payload;

    fn payload(&self) -> &Self::Payload;

    fn payload_mut(&mut self) -> &mut Self::Payload;
}

/// Defines the data structure sent in a request and its corresponding successful output type.
pub trait Payload {
    /// The return type
    /// Note: it should not include `Result` wrappers
    type Output;
}
