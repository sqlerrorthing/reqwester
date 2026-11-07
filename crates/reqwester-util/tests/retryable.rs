use reqwester_core::{HasPayload, Payload, Request};
use reqwester_util::retryable::*;
use futures_core::future::BoxFuture;
use std::future::IntoFuture;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[derive(Debug, PartialEq)]
enum TestError {
    Flaky,
    Fatal,
}

impl IsRetryable for TestError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::Flaky)
    }
}

#[derive(Clone)]
struct Ping;
impl Payload for Ping {
    type Output = u32;
}

/// Fails `fail_times` times with `error`, then succeeds with the attempt number.
#[derive(Clone)]
struct Flaky {
    payload: Ping,
    calls: Arc<AtomicU32>,
    fail_times: u32,
    error: fn() -> TestError,
}

impl Flaky {
    fn new(fail_times: u32, error: fn() -> TestError) -> (Self, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        (
            Self { payload: Ping, calls: calls.clone(), fail_times, error },
            calls,
        )
    }

    fn attempt(&self) -> Result<u32, TestError> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if n <= self.fail_times { Err((self.error)()) } else { Ok(n) }
    }
}

impl HasPayload for Flaky {
    type Payload = Ping;
    fn payload(&self) -> &Ping { &self.payload }
    fn payload_mut(&mut self) -> &mut Ping { &mut self.payload }
}

impl IntoFuture for Flaky {
    type Output = Result<u32, TestError>;
    type IntoFuture = BoxFuture<'static, Result<u32, TestError>>;
    fn into_future(self) -> Self::IntoFuture { self.send() }
}

impl Request for Flaky {
    type Error = TestError;
    type Send = BoxFuture<'static, Result<u32, TestError>>;
    type SendRef<'a> = BoxFuture<'a, Result<u32, TestError>>;
    fn send(self) -> Self::Send { Box::pin(async move { self.attempt() }) }
    fn send_ref(&self) -> Self::SendRef<'_> { Box::pin(async move { self.attempt() }) }
}

#[tokio::test]
async fn retries_until_success() {
    let (req, calls) = Flaky::new(2, || TestError::Flaky);
    let out = req.retryable(Immediate.max_attempts(5)).await;

    assert_eq!(out, Ok(3));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn stops_on_policy() {
    let (req, calls) = Flaky::new(10, || TestError::Flaky);
    let out = req.retryable(Immediate.max_attempts(3)).await;

    assert_eq!(out, Err(TestError::Flaky));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn does_not_retry_non_retryable_errors() {
    let (req, calls) = Flaky::new(10, || TestError::Fatal);
    let out = req.retryable(Immediate.max_attempts(5)).await;

    assert_eq!(out, Err(TestError::Fatal));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn waits_between_attempts() {
    let (req, _) = Flaky::new(3, || TestError::Flaky);
    let policy = Exponential::new(Duration::from_secs(1)).max_attempts(10);

    let started = tokio::time::Instant::now();
    assert_eq!(req.retryable(policy).await, Ok(4));
    // 1s + 2s + 4s
    assert_eq!(started.elapsed(), Duration::from_secs(7));
}

#[tokio::test]
async fn is_send_and_reusable_by_ref() {
    let (req, calls) = Flaky::new(1, || TestError::Flaky);
    let req = req.retryable(Immediate.max_attempts(3));

    assert_eq!(tokio::spawn(async move {
        let first = req.send_ref().await;
        let second = req.send_ref().await;
        (first, second)
    }).await.unwrap(), (Ok(2), Ok(3)));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
