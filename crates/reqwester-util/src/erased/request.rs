use futures_core::future::BoxFuture;
use reqwester_core::payload::HasPayload;
use reqwester_core::{Payload, Request};
use std::fmt;
use std::future::IntoFuture;

/// Object-safe view of a request.
trait Object<'a, P: Payload, E>: Send + Sync + 'a {
    fn payload(&self) -> &P;

    fn payload_mut(&mut self) -> &mut P;

    fn send(self: Box<Self>) -> BoxFuture<'a, Result<P::Output, E>>;

    fn send_ref(&self) -> BoxFuture<'_, Result<P::Output, E>>;

    fn clone_box(&self) -> Box<dyn Object<'a, P, E> + 'a>;
}

struct Holder<R>(R);

impl<'a, R, P, E> Object<'a, P, E> for Holder<R>
where
    R: Request<Payload = P, Error = E> + Clone + Send + Sync + 'a,
    R::Send: Send,
    P: Payload + 'a,
    E: 'a,
{
    fn payload(&self) -> &P {
        self.0.payload()
    }

    fn payload_mut(&mut self) -> &mut P {
        self.0.payload_mut()
    }

    fn send(self: Box<Self>) -> BoxFuture<'a, Result<P::Output, E>> {
        Box::pin(self.0.send())
    }

    // Sends a clone: the borrowed future of `Request::send_ref` can not be proven `Send`
    // for an arbitrary request.
    fn send_ref(&self) -> BoxFuture<'_, Result<P::Output, E>> {
        Box::pin(self.0.clone().send())
    }

    fn clone_box(&self) -> Box<dyn Object<'a, P, E> + 'a> {
        Box::new(Holder(self.0.clone()))
    }
}

/// A request with its concrete type erased. `'a` is the lifetime it borrows (`'static` if none).
pub struct ErasedRequest<'a, P: Payload + 'a, E: 'a> {
    inner: Box<dyn Object<'a, P, E> + 'a>,
}

impl<'a, P: Payload + 'a, E: 'a> ErasedRequest<'a, P, E> {
    /// Erases `request`. Every send of a request by reference sends a clone of it.
    pub fn new<R>(request: R) -> Self
    where
        R: Request<Payload = P, Error = E> + Clone + Send + Sync + 'a,
        R::Send: Send,
    {
        Self {
            inner: Box::new(Holder(request)),
        }
    }
}

impl<'a, P: Payload + 'a, E: 'a> Clone for ErasedRequest<'a, P, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone_box(),
        }
    }
}

impl<'a, P: Payload + 'a, E: 'a> fmt::Debug for ErasedRequest<'a, P, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ErasedRequest").finish_non_exhaustive()
    }
}

impl<'a, P: Payload + 'a, E: 'a> HasPayload for ErasedRequest<'a, P, E> {
    type Payload = P;

    fn payload(&self) -> &P {
        self.inner.payload()
    }

    fn payload_mut(&mut self) -> &mut P {
        self.inner.payload_mut()
    }
}

impl<'a, P: Payload + 'a, E: 'a> IntoFuture for ErasedRequest<'a, P, E> {
    type Output = Result<P::Output, E>;
    type IntoFuture = BoxFuture<'a, Result<P::Output, E>>;

    fn into_future(self) -> Self::IntoFuture {
        self.send()
    }
}

impl<'a, P: Payload + 'a, E: 'a> Request for ErasedRequest<'a, P, E> {
    type Error = E;
    type Send = BoxFuture<'a, Result<P::Output, E>>;
    type SendRef<'b>
        = BoxFuture<'b, Result<P::Output, E>>
    where
        Self: 'b;

    fn send(self) -> Self::Send {
        self.inner.send()
    }

    fn send_ref(&self) -> Self::SendRef<'_> {
        self.inner.send_ref()
    }
}

/// `.erased()` for any suitable request.
pub trait ErasedExt: Request + Sized {
    fn erased<'a>(self) -> ErasedRequest<'a, Self::Payload, Self::Error>
    where
        Self: Clone + Send + Sync + 'a,
        Self::Send: Send,
        Self::Payload: 'a,
        Self::Error: 'a,
    {
        ErasedRequest::new(self)
    }
}

impl<R: Request> ErasedExt for R {}
