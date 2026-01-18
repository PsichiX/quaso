use crate::{
    context::GameContext,
    game::{CONTEXT_META, DELTA_TIME_META, NEXT_FRAME_QUEUE_META},
    gc::{DynGc, Heartbeat},
};
use keket::database::handle::AssetHandle;
use moirai::{
    coroutine::{meta, move_to, spawn, yield_now},
    job::{JobHandle, JobLocation, JobOptions},
    queue::JobQueue,
};
use std::{borrow::Cow, future::poll_fn, task::Poll};

pub async fn async_heartbeat_bound<F: Future>(
    heartbeats: impl IntoIterator<Item = Heartbeat>,
    future: F,
) -> Option<F::Output> {
    let lifetimes = heartbeats
        .into_iter()
        .map(|heartbeat| heartbeat.0)
        .collect::<Vec<_>>();
    let mut future = Box::pin(future);
    poll_fn(move |cx| {
        if lifetimes.iter().any(|state| state.upgrade().is_none()) {
            cx.waker().wake_by_ref();
            Poll::Ready(None)
        } else {
            cx.waker().wake_by_ref();
            future.as_mut().poll(cx).map(Some)
        }
    })
    .await
}

pub async fn async_cancellable<F: Future>(
    condition: impl Fn() -> bool,
    future: F,
) -> Option<F::Output> {
    let mut future = Box::pin(future);
    poll_fn(move |cx| {
        if condition() {
            cx.waker().wake_by_ref();
            Poll::Ready(None)
        } else {
            cx.waker().wake_by_ref();
            future.as_mut().poll(cx).map(Some)
        }
    })
    .await
}

pub async fn async_game_context<'a>() -> Option<GameContext<'a>> {
    let context = meta::<GameContext>(CONTEXT_META).await?;
    let context = unsafe { context.as_mut_ptr() }?;
    let context = unsafe { context.as_mut() }?;
    Some(GameContext::fork(context))
}

pub async fn async_delta_time() -> f32 {
    meta::<f32>(DELTA_TIME_META)
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
    let Some(queue) = meta::<JobQueue>(NEXT_FRAME_QUEUE_META).await else {
        yield_now().await;
        return;
    };
    let Some(queue) = queue.read().map(|queue| queue.clone()) else {
        yield_now().await;
        return;
    };
    move_to(JobLocation::Queue(queue)).await;
}

pub async fn async_wait_for_asset(handle: AssetHandle) {
    loop {
        let context = async_game_context().await.unwrap();
        if handle.is_ready_to_use(context.assets) {
            break;
        }
        async_next_frame().await;
    }
}

pub async fn async_wait_for_assets(handles: impl IntoIterator<Item = AssetHandle>) {
    for handle in handles {
        async_wait_for_asset(handle).await;
    }
}

pub async fn coroutine<F>(job: F) -> JobHandle<F::Output>
where
    F: Future + Send + Sync + 'static,
    <F as Future>::Output: Send,
{
    spawn(JobLocation::Local, job).await
}

pub async fn coroutine_with_meta<F>(
    meta: impl IntoIterator<Item = (Cow<'static, str>, DynGc)>,
    job: F,
) -> JobHandle<F::Output>
where
    F: Future + Send + Sync + 'static,
    <F as Future>::Output: Send,
{
    spawn(
        JobOptions::default()
            .location(JobLocation::Local)
            .meta_many(meta.into_iter().map(|(id, gc)| (id, gc.0.into()))),
        job,
    )
    .await
}

pub async fn async_remap<T, F: Future>(future: F, f: impl FnOnce(F::Output) -> T) -> T {
    f(future.await)
}
