use pin_project_lite::pin_project;
use std::fmt::Display;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, ready};
use std::time::Instant;
use tracing::Span;

pin_project! {
    /// Polls the future inside the span and logs its outcome.
    pub struct TracedFuture<F> {
        #[pin]
        inner: F,
        span: Span,
        started: Instant,
    }
}

impl<F> TracedFuture<F> {
    pub fn new(inner: F, span: Span) -> Self {
        Self {
            inner,
            span,
            started: Instant::now(),
        }
    }
}

impl<F, T, E> Future for TracedFuture<F>
where
    F: Future<Output = Result<T, E>>,
    E: Display,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let _entered = this.span.enter();

        let output = ready!(this.inner.poll(cx));
        let elapsed = this.started.elapsed();

        match &output {
            Ok(_) => tracing::debug!(?elapsed, "request succeeded"),
            Err(error) => tracing::warn!(?elapsed, %error, "request failed"),
        }

        Poll::Ready(output)
    }
}
