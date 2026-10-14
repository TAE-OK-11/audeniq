//! A job handler that panics must fail that job only. Without this the panic
//! ends its queue loop, the process exits and every other in-flight job
//! (DSP sends included) is cut off mid-run until its lease expires.

use std::{
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    task::{Context, Poll},
};

/// `future`, with a panic while polling it returned as `Err(message)`.
pub struct CatchUnwind<F>(Pin<Box<F>>);

pub fn catch<F: Future>(future: F) -> CatchUnwind<F> {
    CatchUnwind(Box::pin(future))
}

impl<F: Future> Future for CatchUnwind<F> {
    type Output = Result<F::Output, String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let inner = self.0.as_mut();
        match catch_unwind(AssertUnwindSafe(|| inner.poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(value)) => Poll::Ready(Ok(value)),
            Err(payload) => Poll::Ready(Err(payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "non-string panic".into()))),
        }
    }
}

#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn a_panic_becomes_an_error() {
        let out = super::catch(async {
            tokio::task::yield_now().await;
            panic!("boom");
        })
        .await;
        assert_eq!(out, Err::<(), _>("boom".to_string()));
        assert_eq!(super::catch(async { 7 }).await, Ok(7));
    }
}
