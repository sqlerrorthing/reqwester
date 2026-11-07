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

    /// Send this request without consuming it.
    fn send_ref(&self) -> Self::SendRef<'_>;
}
