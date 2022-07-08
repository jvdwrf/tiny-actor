use crate::*;
use futures::Future;
use std::{sync::Arc, time::Duration};

//------------------------------------------------------------------------------------------------
//  Config
//------------------------------------------------------------------------------------------------

/// The config used for spawning new processes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Config {
    pub abort_timer: Duration,
    pub attached: bool,
    pub capacity: Option<usize>,
}

impl Config {
    /// Create a custom spawn configuration.
    pub fn new(capacity: Option<usize>, attached: bool, abort_timer: Duration) -> Self {
        Self {
            abort_timer,
            attached,
            capacity,
        }
    }

    /// A default bounded inbox:
    /// * abort_timer: 1 sec
    /// * attached: true
    /// * capacity: Some(capacity)
    pub fn bounded(capacity: usize) -> Self {
        Self {
            abort_timer: Duration::from_secs(1),
            attached: true,
            capacity: Some(capacity),
        }
    }

    /// A default unbounded inbox:
    /// * abort_timer: 1 sec
    /// * attached: true
    /// * capacity: None
    pub fn unbounded() -> Self {
        Self {
            abort_timer: Duration::from_secs(1),
            attached: true,
            capacity: None,
        }
    }
}

//------------------------------------------------------------------------------------------------
//  Spawn
//------------------------------------------------------------------------------------------------

pub fn spawn<T, R, Fun, Fut>(config: Config, fun: Fun) -> (Child<R>, Address<T>)
where
    Fun: FnOnce(Inbox<T>) -> Fut + Send + 'static,
    Fut: Future<Output = R> + Send + 'static,
    R: Send + 'static,
    T: Send + 'static,
{
    let (inbox, address, channel) = setup_channel(config.capacity);

    let handle = tokio::task::spawn(async move { fun(inbox).await });

    let child = Child::new(
        channel,
        handle,
        SupervisionState::new(config.attached, config.abort_timer),
    );

    (child, address)
}

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
        let inbox = inbox.clone_inbox();
        let handle = tokio::task::spawn(async move { fun(i, inbox).await });
        handles.push(handle);
    }

    let children = ChildPool::new(
        channel,
        handles,
        SupervisionState::new(config.attached, config.abort_timer),
    );

    (children, address)
}

fn setup_channel<T>(capacity: Option<usize>) -> (Inbox<T>, Address<T>, Arc<Channel<T>>) {
    let channel = Arc::new(Channel::new(1, 1, capacity));
    let inbox = Inbox::from_channel(channel.clone());
    let address = Address::from_channel(channel.clone());
    (inbox, address, channel)
}
