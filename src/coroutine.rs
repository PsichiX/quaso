use crate::context::AsyncGameContext;
use anput_jobs::{
    JobHandle, JobLocation, JobPriority,
    coroutine::{meta, spawn_on},
};
use intuicio_data::lifetime::LifetimeWeakState;
use std::{
    future::poll_fn,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll},
};

#[derive(Debug, Default, Clone)]
pub struct AsyncNextFrame {
    origin_frame: usize,
    current_frame: Arc<AtomicUsize>,
}

impl AsyncNextFrame {
    pub fn tick(&mut self) {
        self.origin_frame = self.origin_frame.wrapping_add(1);
        self.current_frame
            .store(self.origin_frame, Ordering::SeqCst);
    }
}

impl Future for AsyncNextFrame {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let current_frame = self.current_frame.load(Ordering::SeqCst);
        if current_frame == self.origin_frame {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            self.origin_frame = current_frame;
            cx.waker().wake_by_ref();
            Poll::Ready(())
        }
    }
}

pub async fn async_lifetime_bound<F: Future>(
    lifetimes: impl IntoIterator<Item = LifetimeWeakState>,
    future: F,
) -> Option<F::Output> {
    let lifetimes = lifetimes.into_iter().collect::<Vec<_>>();
    let mut future = Box::pin(future);
    poll_fn(move |cx| {
        if lifetimes.iter().any(|state| state.upgrade().is_none()) {
            cx.waker().wake_by_ref();
            Poll::Ready(None)
        } else {
            cx.waker().wake_by_ref();
            future
                .as_mut()
                .poll(cx)
                .map(|output: <F as Future>::Output| Some(output))
        }
    })
    .await
}

pub async fn async_game_context<'a>() -> Option<AsyncGameContext<'a>> {
    meta::<AsyncGameContext>("context")
        .await
        .and_then(|context| unsafe { context.as_mut_ptr() })
        .and_then(|context| unsafe { context.as_mut() }.map(AsyncGameContext::fork))
}

pub async fn async_delta_time() -> f32 {
    meta("delta_time")
        .await
        .and_then(|dt| dt.read().map(|dt| *dt))
        .unwrap_or_default()
}

pub async fn async_delay(mut seconds: f32) {
    while seconds > 0.0 {
        let delta = async_delta_time().await;
        seconds -= delta;
        async_next_frame().await;
    }
}

pub async fn async_next_frame() {
    if let Some(context) = async_game_context().await {
        context.async_next_frame.clone().await;
    }
}

pub async fn defer<F>(job: F) -> JobHandle<F::Output>
where
    F: Future + Send + Sync + 'static,
    <F as Future>::Output: Send,
{
    spawn_on(JobLocation::Local, JobPriority::Normal, job).await
}
