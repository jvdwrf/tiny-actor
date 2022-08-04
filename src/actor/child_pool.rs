use crate::*;
use futures::{Future, FutureExt, Stream};
use std::{fmt::Debug, mem::ManuallyDrop, pin::Pin, sync::Arc, task::Poll, time::Duration};
use tokio::{task::JoinHandle, time::Sleep};

/// A child-pool is the non clone-able reference to an actor with a multiple processes.
///
/// child-pools can be of two forms:
/// * `ChildPool<E, Channel<M>>`: This is the default form, it can be transformed into a `ChildPool<E>` using
/// [ChildPool::into_dyn]. Additional processes can be spawned using [ChildPool::spawn].
/// * `ChildPool<E>`: This form is a dynamic child-pool, it can be transformed back into a `ChildPool<E, Channel<M>>`
/// using [ChildPool::downcast::<M>]. Additional processes can be spawned using [ChildPool::try_spawn].
///
/// A child-pool can be streamed which returns values of `E` when the processes exit.
#[derive(Debug)]
pub struct ChildPool<E, C = dyn AnyChannel>
where
    E: Send + 'static,
    C: DynChannel + ?Sized,
{
    pub(super) channel: Arc<C>,
    pub(super) handles: Option<Vec<JoinHandle<E>>>,
    pub(super) link: Link,
    pub(super) is_aborted: bool,
}

impl<E, C> ChildPool<E, C>
where
    E: Send + 'static,
    C: DynChannel + ?Sized,
{
    pub(crate) fn new(channel: Arc<C>, handles: Vec<JoinHandle<E>>, link: Link) -> Self {
        Self {
            channel,
            handles: Some(handles),
            link,
            is_aborted: false,
        }
    }

    fn into_parts(self) -> (Arc<C>, Vec<JoinHandle<E>>, Link, bool) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let mut handle = std::ptr::read(&no_drop.handles);
            let channel = std::ptr::read(&no_drop.channel);
            let link = std::ptr::read(&no_drop.link);
            let is_aborted = std::ptr::read(&no_drop.is_aborted);
            (channel, handle.take().unwrap(), link, is_aborted)
        }
    }

    /// Get the underlying [JoinHandles](JoinHandle). The order does not necessarily reflect
    /// the order in which processes were spawned.
    ///
    /// This will not run drop, and therefore the `Actor` will not be halted/aborted.
    pub fn into_tokio_joinhandles(self) -> Vec<JoinHandle<E>> {
        self.into_parts().1
    }

    /// Abort the actor.
    ///
    /// Returns `true` if this is the first abort.
    pub fn abort(&mut self) -> bool {
        self.channel.close();
        let was_aborted = self.is_aborted;
        self.is_aborted = true;
        for handle in self.handles.as_ref().unwrap() {
            handle.abort()
        }
        !was_aborted
    }

    /// Whether all tasks have finished.
    pub fn is_finished(&self) -> bool {
        self.handles
            .as_ref()
            .unwrap()
            .iter()
            .all(|handle| handle.is_finished())
    }

    /// The amount of tasks that are alive.
    ///
    /// This should give the same result as [ChildPool::process_count], as long as
    /// an inbox is only dropped whenever it's task finishes.
    pub fn task_count(&self) -> usize {
        self.handles
            .as_ref()
            .unwrap()
            .iter()
            .filter(|handle| !handle.is_finished())
            .collect::<Vec<_>>()
            .len()
    }

    /// The amount of handles to processes that this pool contains. This can be bigger
    /// than the `process_count` or `task_count` if processes have exited.
    pub fn handle_count(&self) -> usize {
        self.handles.as_ref().unwrap().len()
    }

    /// Attempt to spawn an additional process on the channel.
    ///
    /// This method can fail if
    /// * the message-type does not match that of the channel.
    /// * the channel has already exited.
    pub fn try_spawn<M, Fun, Fut>(&mut self, fun: Fun) -> Result<(), TrySpawnError<Fun>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        M: Send + 'static,
        E: Send + 'static,
        C: AnyChannel,
    {
        let channel = match Arc::downcast::<Channel<M>>(self.channel.clone().into_any()) {
            Ok(channel) => channel,
            Err(_) => return Err(TrySpawnError::Exited(fun)),
        };

        match channel.try_add_inbox() {
            Ok(_) => {
                let inbox = Inbox::from_channel(channel);
                let handle = tokio::task::spawn(async move { fun(inbox).await });
                self.handles.as_mut().unwrap().push(handle);
                Ok(())
            }
            Err(_) => Err(TrySpawnError::Exited(fun)),
        }
    }

    /// Downcast the `ChildPool<E>` to a `ChildPool<E, Channel<M>>`
    pub fn downcast<M>(self) -> Result<ChildPool<E, Channel<M>>, Self>
    where
        M: Send + 'static,
        C: AnyChannel,
    {
        let (channel, handles, link, is_aborted) = self.into_parts();
        match channel.clone().into_any().downcast::<Channel<M>>() {
            Ok(channel) => Ok(ChildPool {
                handles: Some(handles),
                channel,
                link,
                is_aborted,
            }),
            Err(_) => Err(ChildPool {
                handles: Some(handles),
                channel,
                link,
                is_aborted,
            }),
        }
    }

    /// Halts the actor, and then waits for it to exit.
    /// todo
    ///
    /// If the timeout expires before the actor has exited, the actor will be aborted.
    pub fn shutdown(self, timer: Duration) -> ShutdownPoolFut<E, C> {
        ShutdownPoolFut {
            sleep: Some(Box::pin(tokio::time::sleep(timer))),
            child_pool: self,
            exits: Some(Vec::new()),
        }
    }

    gen::dyn_channel_methods!();
    gen::child_methods!();
}

impl<E, M> ChildPool<E, Channel<M>>
where
    E: Send + 'static,
{
    /// Convert the `ChildPool<E, Channel<M>` into a `ChildPool<E>`.
    pub fn into_dyn(self) -> ChildPool<E>
    where
        M: Send + 'static,
    {
        let parts = self.into_parts();
        ChildPool {
            handles: Some(parts.1),
            channel: parts.0,
            link: parts.2,
            is_aborted: parts.3,
        }
    }

    /// Attempt to spawn an additional process on the channel.
    ///
    /// This method fails if the channel has already exited.
    pub fn spawn<Fun, Fut>(&mut self, fun: Fun) -> Result<(), SpawnError<Fun>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        E: Send + 'static,
        M: Send + 'static,
    {
        match self.channel.try_add_inbox() {
            Ok(_) => {
                let inbox = Inbox::from_channel(self.channel.clone());
                let handle = tokio::task::spawn(async move { fun(inbox).await });
                self.handles.as_mut().unwrap().push(handle);
                Ok(())
            }
            Err(_) => Err(SpawnError(fun)),
        }
    }
}

#[cfg(feature = "internals")]
impl<E, C> ChildPool<E, C>
where
    E: Send + 'static,
    C: DynChannel + ?Sized,
{
    pub fn transform_channel<C2: DynChannel + ?Sized>(
        self,
        func: fn(Arc<C>) -> Arc<C2>,
    ) -> ChildPool<E, C2> {
        let (channel, handles, link, is_aborted) = self.into_parts();
        ChildPool {
            channel: func(channel),
            handles: Some(handles),
            link,
            is_aborted,
        }
    }

    pub fn channel_ref(&self) -> &C {
        &self.channel
    }
}

impl<E: Send + 'static, C: DynChannel + ?Sized> Stream for ChildPool<E, C> {
    type Item = Result<E, ExitError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        if self.handles.as_ref().unwrap().len() == 0 {
            return Poll::Ready(None);
        }

        for (i, handle) in self.handles.as_mut().unwrap().iter_mut().enumerate() {
            if let Poll::Ready(res) = handle.poll_unpin(cx) {
                self.handles.as_mut().unwrap().swap_remove(i);
                return Poll::Ready(Some(res.map_err(Into::into)));
            }
        }

        Poll::Pending
    }
}

impl<E: Send + 'static, C: DynChannel + ?Sized> Drop for ChildPool<E, C> {
    fn drop(&mut self) {
        if let Link::Attached(abort_timer) = self.link {
            if !self.is_aborted && !self.is_finished() {
                if abort_timer.is_zero() {
                    self.abort();
                } else {
                    self.halt();
                    let handles = self.handles.take().unwrap();
                    tokio::task::spawn(async move {
                        tokio::time::sleep(abort_timer).await;
                        for handle in handles {
                            handle.abort()
                        }
                    });
                }
            }
        }
    }
}

#[derive(Debug)]
pub struct ShutdownPoolFut<E: Send + 'static, C: DynChannel + ?Sized> {
    sleep: Option<Pin<Box<Sleep>>>,
    child_pool: ChildPool<E, C>,
    exits: Option<Vec<Result<E, ExitError>>>,
}

impl<E: Send + 'static, C: DynChannel + ?Sized> Unpin for ShutdownPoolFut<E, C> {}
impl<E: Send + 'static, C: DynChannel + ?Sized> Future for ShutdownPoolFut<E, C> {
    type Output = Vec<Result<E, ExitError>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut_self = &mut *self;

        let new_exits = mut_self
            .child_pool
            .handles
            .as_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
            .filter_map(|(i, handle)| {
                if let Poll::Ready(exit) = handle.poll_unpin(cx) {
                    mut_self
                        .exits
                        .as_mut()
                        .unwrap()
                        .push(exit.map_err(Into::into));
                    Some(i)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        for i in new_exits.into_iter().rev() {
            mut_self.child_pool.handles.as_mut().unwrap().swap_remove(i);
        }

        if mut_self.child_pool.handle_count() == 0 {
            Poll::Ready(self.exits.take().unwrap())
        } else {
            if let Some(sleep) = &mut self.sleep {
                if let Poll::Ready(()) = sleep.poll_unpin(cx) {
                    self.child_pool.abort();
                }
            };
            Poll::Pending
        }
    }
}
