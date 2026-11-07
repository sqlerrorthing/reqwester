use crate::payload::{HasPayload, HasPayloadOutput};

pub type RequestResult<T> =
    Result<HasPayloadOutput<T>, <T as Request>::Error>;

/// A ready to send request.
pub trait Request:
    HasPayload 
    + IntoFuture<
        Output = RequestResult<Self>,
        IntoFuture = Self::Send
    >
{
    type Error;

    type Send: Future<Output = RequestResult<Self>>;

    type SendRef<'a>: Future<Output = RequestResult<Self>> + 'a
    where
        Self: 'a;

    /// Send this request.
    fn send(self) -> Self::Send;

    /// Send this request by reference.
    ///
    /// This method is analogous to [`send`], but it doesn’t take the ownership of self.
    /// This allows to send the same (or slightly different) requester over and over.
    fn send_ref(&self) -> Self::SendRef<'_>;
}
