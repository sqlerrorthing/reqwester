use futures_core::future::BoxFuture;
use reqwester_core::{HasPayload, Payload, Request};
use reqwester_util::retryable::{Immediate, RetryPolicyExt, RetryableExt};
use reqwester_util::traced::TracedExt;
use std::fmt;
use std::future::IntoFuture;
use std::io;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Ping;
impl Payload for Ping {
    type Output = u32;
}

#[derive(Debug, PartialEq)]
struct Boom;
impl fmt::Display for Boom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("boom")
    }
}

#[derive(Clone)]
struct Failing(Ping);

impl HasPayload for Failing {
    type Payload = Ping;
    fn payload(&self) -> &Ping { &self.0 }
    fn payload_mut(&mut self) -> &mut Ping { &mut self.0 }
}
impl IntoFuture for Failing {
    type Output = Result<u32, Boom>;
    type IntoFuture = BoxFuture<'static, Result<u32, Boom>>;
    fn into_future(self) -> Self::IntoFuture { self.send() }
}
impl Request for Failing {
    type Error = Boom;
    type Send = BoxFuture<'static, Result<u32, Boom>>;
    type SendRef<'a> = BoxFuture<'a, Result<u32, Boom>>;
    fn send(self) -> Self::Send { Box::pin(async { Err(Boom) }) }
    fn send_ref(&self) -> Self::SendRef<'_> { Box::pin(async { Err(Boom) }) }
}

#[derive(Clone)]
struct Buffer(Arc<Mutex<Vec<u8>>>);
impl io::Write for Buffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> { self.0.lock().unwrap().extend_from_slice(buf); Ok(buf.len()) }
    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn capture() -> (Buffer, tracing::subscriber::DefaultGuard) {
    let buffer = Buffer(Default::default());
    let writer = buffer.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();

    (buffer, tracing::subscriber::set_default(subscriber))
}

impl Buffer {
    fn text(&self) -> String { String::from_utf8(self.0.lock().unwrap().clone()).unwrap() }
}

#[tokio::test]
async fn logs_failure_inside_the_span() {
    let (log, _guard) = capture();

    assert_eq!(Failing(Ping).traced().await, Err(Boom));

    let log = log.text();
    assert!(log.contains("request{request=\"Ping\"}"), "{log}");
    assert!(log.contains("WARN"), "{log}");
    assert!(log.contains("request failed") && log.contains("error=boom"), "{log}");
}

#[tokio::test]
async fn attempts_vs_whole_run() {
    let policy = || Immediate.max_attempts(3);

    let (log, guard) = capture();
    assert_eq!(Failing(Ping).traced().retryable(policy()).await, Err(Boom));
    assert_eq!(log.text().matches("request failed").count(), 3);
    drop(guard);

    let (log, _guard) = capture();
    assert_eq!(Failing(Ping).retryable(policy()).traced().await, Err(Boom));
    assert_eq!(log.text().matches("request failed").count(), 1);
}
