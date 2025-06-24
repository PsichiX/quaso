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
