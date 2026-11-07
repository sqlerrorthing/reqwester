pub type HasPayloadOutput<T> = <<T as HasPayload>::Payload as Payload>::Output;

/// A container of a [`Payload`].
pub trait HasPayload {
    type Payload: Payload;

    fn payload(&self) -> &Self::Payload;

    fn payload_mut(&mut self) -> &mut Self::Payload;
}

/// Data sent in a request and its successful output.
pub trait Payload {
    /// The output, without `Result` wrappers.
    type Output;
}
