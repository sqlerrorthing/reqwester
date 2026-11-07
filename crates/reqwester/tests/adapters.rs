#![feature(impl_trait_in_assoc_type)]

use reqwester::util::retryable::*;
use reqwester::{HasPayload, Payload, Request, requester};
use futures_core::future::BoxFuture;
use std::future::IntoFuture;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

#[derive(Debug, PartialEq)]
pub enum Error {
    Flaky,
    Fatal,
}

impl IsRetryable for Error {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Flaky)
    }
}

#[derive(Debug, Clone, Payload)]
#[payload(returns = String)]
pub struct Greet<'a> {
    pub name: &'a str,
}

#[derive(Debug, Clone, Payload)]
#[payload(returns = u32)]
pub struct Count {
    pub items: Vec<u8>,
}

/// Fails (with `Flaky`) until it was polled `fail_times + 1` times in total.
pub struct Fake<'p, P: Payload> {
    marker: std::marker::PhantomData<&'p ()>,
    payload: P,
    calls: Arc<AtomicU32>,
    fail_times: u32,
    output: fn(&P) -> P::Output,
}

impl<P: Payload + Clone> Clone for Fake<'_, P> {
    fn clone(&self) -> Self {
        Self { marker: Default::default(), payload: self.payload.clone(), calls: self.calls.clone(), fail_times: self.fail_times, output: self.output }
    }
}

impl<P: Payload> Fake<'_, P> {
    fn attempt(&self) -> Result<P::Output, Error> {
        if self.calls.fetch_add(1, Ordering::SeqCst) < self.fail_times {
            Err(Error::Flaky)
        } else {
            Ok((self.output)(&self.payload))
        }
    }
}

impl<P: Payload> HasPayload for Fake<'_, P> {
    type Payload = P;
    fn payload(&self) -> &P { &self.payload }
    fn payload_mut(&mut self) -> &mut P { &mut self.payload }
}

impl<'p, P: Payload + Send + Sync + 'p> IntoFuture for Fake<'p, P> where P::Output: Send + 'p {
    type Output = Result<P::Output, Error>;
    type IntoFuture = BoxFuture<'p, Result<P::Output, Error>>;
    fn into_future(self) -> Self::IntoFuture { self.send() }
}

impl<'p, P: Payload + Send + Sync + 'p> Request for Fake<'p, P> where P::Output: Send + 'p {
    type Error = Error;
    type Send = BoxFuture<'p, Result<P::Output, Error>>;
    type SendRef<'a> = BoxFuture<'a, Result<P::Output, Error>> where Self: 'a;
    fn send(self) -> Self::Send { Box::pin(async move { self.attempt() }) }
    fn send_ref(&self) -> Self::SendRef<'_> { Box::pin(async move { self.attempt() }) }
}

#[requester(Send, factory, retryable)]
pub trait Api {
    type Error: IsRetryable + Send + 'static;

    fn greet<'a>(&self, name: &'a str);
    fn count(&self, items: Vec<u8>);
}

#[derive(Clone)]
pub struct Impl {
    calls: Arc<AtomicU32>,
    fail_times: u32,
}

#[requester(Send, Clone)]
impl Api for Impl {
    type Error = Error;

    fn greet<'a>(&self, name: &'a str) {
        Fake {
            marker: Default::default(),
            payload: Greet::new(name),
            calls: self.calls.clone(),
            fail_times: self.fail_times,
            output: |p| format!("hi {}", p.name),
        }
    }

    fn count(&self, items: Vec<u8>) -> Fake<'static, Count> {
        Fake {
            marker: Default::default(),
            payload: Count::new(items),
            calls: self.calls.clone(),
            fail_times: self.fail_times,
            output: |p| p.items.len() as u32,
        }
    }
}

fn api(fail_times: u32) -> (Impl, Arc<AtomicU32>) {
    let calls = Arc::new(AtomicU32::new(0));
    (Impl { calls: calls.clone(), fail_times }, calls)
}

#[tokio::test]
async fn requester_ext_retries_all_methods() {
    let (api, calls) = api(2);
    let api = api.retryable(Immediate.max_attempts(5));

    let name = String::from("bob");
    assert_eq!(api.greet(name.as_str()).await, Ok("hi bob".to_string()));
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    assert_eq!(api.count(vec![1, 2]).await, Ok(2));
}

#[tokio::test]
async fn gives_up_by_policy() {
    let (api, calls) = api(100);
    let api = api.retryable(Immediate.max_attempts(3));

    assert_eq!(api.count(vec![]).await, Err(Error::Flaky));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn factory_for_closures_arc_and_retryable() {
    fn create_with<F: ApiFactory>(f: &F) -> F::Requester { f.create() }

    let (api, calls) = api(2);

    let closure = { let api = api.clone(); move || api.clone() };
    let plain = create_with(&closure);
    assert_eq!(plain.count(vec![1]).await, Err(Error::Flaky));

    let factory = Arc::new(closure.retryable(Immediate.max_attempts(3)));
    let retrying = create_with(&factory);
    assert_eq!(retrying.count(vec![1, 2, 3]).await, Ok(3));
    // 1 failed by the plain requester, then 1 failed + 1 succeeded by the retrying one
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn retryable_requester_is_a_requester_and_spawnable() {
    fn takes_requester<R: Api>(r: R) -> R { r }

    let (api, _) = api(1);
    let api = takes_requester(api.retryable(Immediate.max_attempts(2)));

    let out = tokio::spawn(async move { api.count(vec![9]).await }).await.unwrap();
    assert_eq!(out, Ok(1));
}
