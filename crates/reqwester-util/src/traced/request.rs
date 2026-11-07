use super::TracedFuture;
use reqwester_core::Request;
use reqwester_core::payload::HasPayload;
use reqwester_core::request::RequestResult;
use std::fmt::Display;
use std::future::IntoFuture;

/// Wraps a request, tracing every send. No boxing: it is `Send` whenever the inner request is.
pub struct TracedRequest<R> {
    inner: R,
    name: &'static str,
}

impl<R: HasPayload> TracedRequest<R> {
    /// Named after the payload type.
    pub fn new(inner: R) -> Self {
        Self::named(inner, short_type_name::<R::Payload>())
    }
}

impl<R> TracedRequest<R> {
    pub fn named(inner: R, name: &'static str) -> Self {
        Self { inner, name }
    }

    pub fn inner(&self) -> &R {
        &self.inner
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    fn span(&self) -> tracing::Span {
        tracing::debug_span!("request", request = self.name)
    }
}

fn short_type_name<T>() -> &'static str {
    let name = std::any::type_name::<T>();
    let name = name.split('<').next().unwrap_or(name);

    name.rsplit("::").next().unwrap_or(name)
}

impl<R: Clone> Clone for TracedRequest<R> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            name: self.name,
        }
    }
}

impl<R: HasPayload> HasPayload for TracedRequest<R> {
    type Payload = R::Payload;

    fn payload(&self) -> &Self::Payload {
        self.inner.payload()
    }

    fn payload_mut(&mut self) -> &mut Self::Payload {
        self.inner.payload_mut()
    }
}

impl<R> IntoFuture for TracedRequest<R>
where
    R: Request,
    R::Error: Display,
{
    type Output = RequestResult<Self>;
    type IntoFuture = <Self as Request>::Send;

    fn into_future(self) -> Self::IntoFuture {
        self.send()
    }
}

impl<R> Request for TracedRequest<R>
where
    R: Request,
    R::Error: Display,
{
    type Error = R::Error;
    type Send = TracedFuture<R::Send>;
    type SendRef<'a>
        = TracedFuture<R::SendRef<'a>>
    where
        Self: 'a;

    fn send(self) -> Self::Send {
        let span = self.span();

        TracedFuture::new(self.inner.send(), span)
    }

    fn send_ref(&self) -> Self::SendRef<'_> {
        TracedFuture::new(self.inner.send_ref(), self.span())
    }
}

/// `.traced()` for any request.
pub trait TracedExt: Request + Sized {
    fn traced(self) -> TracedRequest<Self> {
        TracedRequest::new(self)
    }
}

impl<R: Request> TracedExt for R {}
