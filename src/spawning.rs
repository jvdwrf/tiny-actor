use crate::*;
use futures::Future;
use std::{sync::Arc};

/// Spawn a new `Actor` with a single `Process`. This will return a [Child] and
/// and [Address]. The `Process` is spawned with a single [Inbox].
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
pub fn spawn<T, R, Fun, Fut>(config: Config, fun: Fun) -> (Child<R>, Address<T>)
where
    Fun: FnOnce(Inbox<T>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
    R: Send + 'static,
    T: Send + 'static,
{
    let (inbox, address, channel) = setup_channel(config.capacity);

    let handle = tokio::task::spawn(async move { fun(inbox).await });

    let child = Child::new(channel, handle, config.link);

    (child, address)
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
///     spawn_pooled(0..5, Config::default(), |i, mut inbox: Inbox<u32>| async move {
///         loop {
///             let msg = inbox.recv().await;
///             println!("Received message on actor {i}: {msg:?}");
///         }
///     });
///# }
/// ```
pub fn spawn_pooled<T, R, I, Fun, Fut>(
    iter: impl IntoIterator<Item = I>,
    config: Config,
    fun: Fun,
) -> (ChildPool<R>, Address<T>)
where
    Fun: FnOnce(I, Inbox<T>) -> Fut + Send + 'static + Clone,
    Fut: Future<Output = R> + Send + 'static,
    R: Send + 'static,
    T: Send + 'static,
    I: Send + 'static,
{
    let iterator = iter.into_iter();
    let mut handles = Vec::with_capacity(iterator.size_hint().0);

    let (inbox, address, channel) = setup_channel(config.capacity);

    for i in iterator {
        let fun = fun.clone();
        let inbox = inbox._clone();
        let handle = tokio::task::spawn(async move { fun(i, inbox).await });
        handles.push(handle);
    }

    let children = ChildPool::new(channel, handles, config.link);

    (children, address)
}

fn setup_channel<T>(capacity: Capacity) -> (Inbox<T>, Address<T>, Arc<Channel<T>>) {
    let channel = Arc::new(Channel::new(1, 1, capacity));
    let inbox = Inbox::from_channel(channel.clone());
    let address = Address::from_channel(channel.clone());
    (inbox, address, channel)
}
