use crate::*;
use futures::Future;
use std::{sync::Arc, task::Poll};

/// Spawn a new `Actor` with a single `Process`. This will return a [Child] and
/// and [Address]. The `Process` is spawned with a single [Inbox].
///
/// This will immeadeately start the spawning. `await`-ing the [Spawn]-future will
/// wait until the [Channel] is fully initialized.
///
/// # Example
/// ```no_run
///# use tiny_actor::*;
///# #[tokio::main]
///# async fn main() {
/// let (child, address) =
///     spawn(Config::default(), |mut inbox: Inbox<u32>| async move {
///         loop {
///             let msg = inbox.recv().await;
///             println!("Received message: {msg:?}");
///         }
///     }).await;
///# }
/// ```
pub fn spawn<M, E, Fun, Fut>(config: Config, fun: Fun) -> Spawn<E, M>
where
    Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    M: Send + 'static,
{
    Spawn::new(config, fun)
}

pub struct Spawn<E: Send + 'static, M: Send + 'static> {
    inner: Option<(Child<E, Channel<M>>, Address<M>)>,
}

impl<E: Send + 'static, M: Send + 'static> Spawn<E, M> {
    pub fn new<Fun, Fut>(config: Config, fun: Fun) -> Self
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
    {
        let channel = Arc::new(Channel::<M>::new(1, 1, config.capacity));
        channel.set_spawn_count(1);
        let address = Address::from_channel(channel.clone());
        let inbox = Inbox::from_channel(channel.clone());

        let handle = tokio::task::spawn(async move {
            inbox.channel().decr_spawn_count();
            fun(inbox).await
        });

        let child = Child::new(channel, handle, config.link);

        Self {
            inner: Some((child, address)),
        }
    }

    fn channel(&self) -> &Channel<M> {
        self.inner.as_ref().unwrap().1.channel()
    }
}

impl<M, E> Future for Spawn<E, M>
where
    M: Send + 'static,
    E: Send + 'static,
{
    type Output = (Child<E, Channel<M>>, Address<M>);

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.channel().spawn_count() == 0 {
            return Poll::Ready(self.as_mut().inner.take().unwrap());
        }

        let mut waker_guard = self.channel().spawn_waker_guard();
        match &mut *waker_guard {
            Some(waker) => {
                if !waker.will_wake(cx.waker()) {
                    *waker_guard = Some(cx.waker().clone());
                }
            }
            None => *waker_guard = Some(cx.waker().clone()),
        }
        drop(waker_guard);

        if self.channel().spawn_count() == 0 {
            Poll::Ready(self.as_mut().inner.take().unwrap())
        } else {
            Poll::Pending
        }
    }
}

impl<M, E> Unpin for Spawn<E, M>
where
    M: Send + 'static,
    E: Send + 'static,
{
}

/// Spawn a new `Actor` with a multiple `Process`es. This will return a [ChildPool] and
/// and [Address]. The `Process`es are spawned with [Inbox]es.
///
/// The amount of `Process`es that are spawned is equal to the length of the iterator.
/// Every process get's access to a single item within the iterator as it's first argument.
///
/// # Example
/// ```no_run
///# use tiny_actor::*;
///# #[tokio::main]
///# async fn main() {
/// let (child, address) =
///     spawn_many(0..5, Config::default(), |i, mut inbox: Inbox<u32>| async move {
///         loop {
///             let msg = inbox.recv().await;
///             println!("Received message on actor {i}: {msg:?}");
///         }
///     }).await;
///# }
/// ```
pub fn spawn_many<M, E, I, Fun, Fut>(
    iter: impl IntoIterator<Item = I>,
    config: Config,
    fun: Fun,
) -> SpawnMany<E, M>
where
    Fun: FnOnce(I, Inbox<M>) -> Fut + Send + 'static + Clone,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    M: Send + 'static,
    I: Send + 'static,
{
    SpawnMany::new(iter, config, fun)
}

pub struct SpawnMany<E: Send + 'static, M: Send + 'static> {
    inner: Option<(ChildPool<E, Channel<M>>, Address<M>)>,
}

impl<E: Send + 'static, M: Send + 'static> SpawnMany<E, M> {
    pub fn new<Fun, Fut, I>(iter: impl IntoIterator<Item = I>, config: Config, fun: Fun) -> Self
    where
        Fun: FnOnce(I, Inbox<M>) -> Fut + Send + 'static + Clone,
        Fut: Future<Output = E> + Send + 'static,
        I: Send + 'static,
    {
        let iterator = iter.into_iter().collect::<Vec<_>>();
        let mut handles = Vec::with_capacity(iterator.len());

        let channel = Arc::new(Channel::<M>::new(1, iterator.len(), config.capacity));
        channel.set_spawn_count(iterator.len());
        let address = Address::from_channel(channel.clone());

        for i in iterator {
            let fun = fun.clone();
            let channel = address.channel().clone();
            let handle = tokio::task::spawn(async move {
                channel.decr_spawn_count();
                let inbox = Inbox::from_channel(channel);
                fun(i, inbox).await
            });
            handles.push(handle);
        }

        let child = ChildPool::new(channel, handles, config.link);

        Self {
            inner: Some((child, address)),
        }
    }

    fn channel(&self) -> &Channel<M> {
        self.inner.as_ref().unwrap().1.channel()
    }
}

impl<M, E> Future for SpawnMany<E, M>
where
    M: Send + 'static,
    E: Send + 'static,
{
    type Output = (ChildPool<E, Channel<M>>, Address<M>);

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        if self.channel().spawn_count() == 0 {
            return Poll::Ready(self.as_mut().inner.take().unwrap());
        }

        let mut waker_guard = self.channel().spawn_waker_guard();
        match &mut *waker_guard {
            Some(waker) => {
                if !waker.will_wake(cx.waker()) {
                    *waker_guard = Some(cx.waker().clone());
                }
            }
            None => *waker_guard = Some(cx.waker().clone()),
        }
        drop(waker_guard);

        if self.channel().spawn_count() == 0 {
            Poll::Ready(self.as_mut().inner.take().unwrap())
        } else {
            Poll::Pending
        }
    }
}

impl<M, E> Unpin for SpawnMany<E, M>
where
    M: Send + 'static,
    E: Send + 'static,
{
}
