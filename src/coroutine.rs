use crate::context::AsyncGameContext;
use anput_jobs::coroutine::meta;
use std::{
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
