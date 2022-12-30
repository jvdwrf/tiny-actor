use crate::*;
use futures::Future;
use std::sync::Arc;

/// Spawn a new actor with a single process, this returns a [Child] and an [Address].
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
///     });
///# }
/// ```
pub fn spawn_process<M, E, Fun, Fut>(
    config: Config,
    fun: Fun,
) -> (Child<E, Channel<M>>, Address<Channel<M>>)
where
    Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    M: Send + 'static,
{
    let channel = Arc::new(Channel::<M>::new(1, 1, config.capacity));
    let address = Address::from_channel(channel.clone());
    let inbox = Inbox::from_channel(channel.clone());

    let handle = tokio::task::spawn(async move { fun(inbox).await });

    let child = Child::new(channel, handle, config.link);

    (child, address)
}

pub fn spawn_task<E, Fun, Fut>(config: Config, fun: Fun) -> Child<E>
where
    Fun: FnOnce(HaltNotifier) -> Fut + Send + 'static,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
{
    spawn_process(config, |inbox| async move {
        fun(HaltNotifier::new(inbox)).await
    })
    .0
    .into_dyn()
}

/// Spawn a new actor with a single process, this returns a [Child] and an [Address].
///
/// This is the same as [spawn], but returns a [ChildPool] instead of a [Child].
///
/// # Example
/// ```no_run
///# use tiny_actor::*;
///# #[tokio::main]
///# async fn main() {
/// let (child_pool, address) =
///     spawn_one(Config::default(), |mut inbox: Inbox<u32>| async move {
///         loop {
///             let msg = inbox.recv().await;
///             println!("Received message: {msg:?}");
///         }
///     });
///# }
/// ```
pub fn spawn_one_process<M, E, Fun, Fut>(
    config: Config,
    fun: Fun,
) -> (ChildPool<E, Channel<M>>, Address<Channel<M>>)
where
    Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    M: Send + 'static,
{
    let channel = Arc::new(Channel::<M>::new(1, 1, config.capacity));
    let address = Address::from_channel(channel.clone());
    let inbox = Inbox::from_channel(channel.clone());

    let handle = tokio::task::spawn(async move { fun(inbox).await });

    let child = ChildPool::new(channel, vec![handle], config.link);

    (child, address)
}

pub fn spawn_one_task<E, Fun, Fut>(config: Config, fun: Fun) -> ChildPool<E>
where
    Fun: FnOnce(HaltNotifier) -> Fut + Send + 'static,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
{
    spawn_one_process(config, |inbox| async move {
        fun(HaltNotifier::new(inbox)).await
    })
    .0
    .into_dyn()
}

/// Spawn a new actor with a multiple process, this returns a [ChildPool] and an [Address].
///
/// The iterator will be passed along as the first argument to every spawned function.
///
/// # Example
/// ```no_run
///# use tiny_actor::*;
///# #[tokio::main]
///# async fn main() {
/// let (child_pool, address) =
///     spawn_many(0..5, Config::default(), |i, mut inbox: Inbox<u32>| async move {
///         loop {
///             let msg = inbox.recv().await;
///             println!("Received message on actor {i}: {msg:?}");
///         }
///     });
///# }
/// ```
pub fn spawn_many_processes<M, E, I, Fun, Fut>(
    iter: impl IntoIterator<Item = I>,
    config: Config,
    fun: Fun,
) -> (ChildPool<E, Channel<M>>, Address<Channel<M>>)
where
    Fun: FnOnce(I, Inbox<M>) -> Fut + Send + 'static + Clone,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    M: Send + 'static,
    I: Send + 'static,
{
    let iter = iter.into_iter();
    let channel = Arc::new(Channel::<M>::new(1, 1, config.capacity));
    let address = Address::from_channel(channel.clone());

    let handles = iter
        .map(|i| {
            let fun = fun.clone();
            let inbox = Inbox::from_channel(address.channel().clone());
            tokio::task::spawn(async move { fun(i, inbox).await })
        })
        .collect::<Vec<_>>();

    address.channel().set_inbox_count(handles.len());

    let child = ChildPool::new(channel, handles, config.link);

    (child, address)
}

pub fn spawn_many_tasks<E, I, Fun, Fut>(
    iter: impl IntoIterator<Item = I>,
    config: Config,
    fun: Fun,
) -> ChildPool<E>
where
    Fun: FnOnce(I, HaltNotifier) -> Fut + Clone + Send + 'static,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    I: Send + 'static,
{
    spawn_many_processes(iter, config, |i, inbox| async move {
        fun(i, HaltNotifier::new(inbox)).await
    })
    .0
    .into_dyn()
}
