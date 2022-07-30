use crate::*;
use futures::Future;
use std::sync::Arc;

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
///     }).await;
///# }
/// ```
pub async fn spawn<M, E, Fun, Fut>(
    config: Config,
    fun: Fun,
) -> (Child<E, InnerChannel<M>>, Address<M>)
where
    Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    M: Send + 'static,
{
    let handle = actor_channel::spawn((), config, |channel: Receiver<InnerChannel<M>>| async move {
        println!("Receiver added");
        // fun(Inbox::from_channel(channel)).await
        todo!()
    });

    // let address = Address::from_channel(handle.shared().clone());
    // let child = Child::from_handle(handle);

    // (child, address)
    todo!()
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
pub async fn spawn_many<M, E, I, Fun, Fut>(
    iter: impl IntoIterator<Item = I>,
    config: Config,
    fun: Fun,
) -> (ChildPool<E, InnerChannel<M>>, Address<M>)
where
    Fun: FnOnce(I, Inbox<M>) -> Fut + Send + 'static + Clone,
    Fut: Future<Output = E> + Send + 'static,
    E: Send + 'static,
    M: Send + 'static,
    I: Send + 'static,
{
    let handle = actor_channel::spawn_many(
        iter,
        (),
        config,
        |i, channel: Arc<InnerChannel<M>>| async move {
            todo!()
            // fun(i, Inbox::from_channel(channel)).await
        },
    );

    // let address = Address::from_channel(handle.shared().clone());
    // let child = ChildPool::from_handle(handle);

    // (child, address)
    todo!()
}
