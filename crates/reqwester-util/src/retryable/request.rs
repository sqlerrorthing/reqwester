use super::{Attempt, Decision, DefaultSleeper, RetryPolicy, Sleeper};
use futures_core::future::BoxFuture;
use reqwester_core::Request;
use reqwester_core::payload::{HasPayload, HasPayloadOutput};
use reqwester_core::request::RequestResult;
use std::future::IntoFuture;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Instant;

/// Re-sends clones of the request while its error is retryable and the policy allows.
///
/// `'r` is the lifetime the request borrows (`'static` for owned).
pub struct RetryableRequest<'r, R, P, S = DefaultSleeper> {
    inner: R,
    policy: Arc<P>,
    sleeper: S,
    marker: PhantomData<&'r ()>,
}

impl<'r, R, P> RetryableRequest<'r, R, P> {
    pub fn new(inner: R, policy: P) -> Self {
        Self::with_sleeper(inner, Arc::new(policy), DefaultSleeper::default())
    }
}

impl<'r, R, P, S> RetryableRequest<'r, R, P, S> {
    /// With a shared policy and a custom [`Sleeper`].
    pub fn with_sleeper(inner: R, policy: Arc<P>, sleeper: S) -> Self {
        Self {
            inner,
            policy,
            sleeper,
            marker: PhantomData,
        }
    }

    pub fn inner(&self) -> &R {
        &self.inner
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

async fn run<R, P, S>(inner: &R, policy: &P, sleeper: &S) -> RequestResult<R>
where
    R: Request + Clone,
    P: RetryPolicy<R::Error> + ?Sized,
    S: Sleeper,
{
    let started = Instant::now();
    let mut failed = 0u32;

    loop {
        let error = match inner.clone().send().await {
            Ok(output) => return Ok(output),
            Err(error) => error,
        };

        failed = failed.saturating_add(1);

        let attempt = Attempt {
            number: failed,
            elapsed: started.elapsed(),
            error: &error,
        };

        match policy.decide(attempt) {
            Decision::Stop => return Err(error),
            Decision::Retry { after } => {
                drop(error);

                if !after.is_zero() {
                    sleeper.sleep(after).await;
                }
            }
        }
    }
}

impl<R: Clone, P, S: Clone> Clone for RetryableRequest<'_, R, P, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            policy: Arc::clone(&self.policy),
            sleeper: self.sleeper.clone(),
            marker: PhantomData,
        }
    }
}

impl<R: HasPayload, P, S> HasPayload for RetryableRequest<'_, R, P, S> {
    type Payload = R::Payload;

    fn payload(&self) -> &Self::Payload {
        self.inner.payload()
    }

    fn payload_mut(&mut self) -> &mut Self::Payload {
        self.inner.payload_mut()
    }
}

impl<'r, R, P, S> IntoFuture for RetryableRequest<'r, R, P, S>
where
    R: Request + Clone + Send + Sync + 'r,
    R::Send: Send,
    R::Error: Send + 'r,
    HasPayloadOutput<R>: Send + 'r,
    P: RetryPolicy<R::Error> + Send + Sync + 'r,
    S: Sleeper,
{
    type Output = RequestResult<Self>;
    type IntoFuture = <Self as Request>::Send;

    fn into_future(self) -> Self::IntoFuture {
        self.send()
    }
}

impl<'r, R, P, S> Request for RetryableRequest<'r, R, P, S>
where
    R: Request + Clone + Send + Sync + 'r,
    R::Send: Send,
    R::Error: Send + 'r,
    HasPayloadOutput<R>: Send + 'r,
    P: RetryPolicy<R::Error> + Send + Sync + 'r,
    S: Sleeper,
{
    type Error = R::Error;
    type Send = BoxFuture<'r, RequestResult<Self>>;
    type SendRef<'a>
        = BoxFuture<'a, RequestResult<Self>>
    where
        Self: 'a;

    fn send(self) -> Self::Send {
        Box::pin(async move { run(&self.inner, &*self.policy, &self.sleeper).await })
    }

    fn send_ref(&self) -> Self::SendRef<'_> {
        Box::pin(run(&self.inner, &*self.policy, &self.sleeper))
    }
}

/// `.retryable(policy)` for any request.
pub trait RetryableExt: Request + Sized {
    fn retryable<'r, P>(self, policy: P) -> RetryableRequest<'r, Self, P> {
        RetryableRequest::new(self, policy)
    }
}

impl<R: Request> RetryableExt for R {}
