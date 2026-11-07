use crate::payload::{HasPayload, PayloadOutput, RequestResult};


/// A ready to send request.
pub trait Request:
    HasPayload 
    + IntoFuture<
        Output = Result<PayloadOutput<Self>, Self::Error>,
        IntoFuture = Self::Send
    >
{
    type Error;

    type Send: Future<Output = Result<RequestResult<Self>, Self::Error>>;

    type SendRef<'a>: Future<Output = Result<RequestResult<Self>, Self::Error>> + 'a
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
