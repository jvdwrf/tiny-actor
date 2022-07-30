use crate::*;
use futures::{Future, Stream, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;

/// A `Child` is a handle to an `Actor` with a single `Process`. A `Child` can be awaited to return
/// the exit-value of the `tokio::task`. A `Child` is non-cloneable, and therefore unique to the
/// `Channel`. When the `Child` is dropped, the `Actor` will be `halt`ed and `abort`ed. This can
/// be prevented by detaching the `Child`. More processes can be spawned later, which transforms
/// the `Child` into a `ChildPool`.
#[must_use = "Dropping this will abort the actor."]
pub struct Child<E: Send + 'static, C: AnyChannel + ?Sized = dyn AnyChannel> {
    handle: Handle<E, C>,
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Child<E, C> {
    pub(crate) fn from_handle(handle: Handle<E, C>) -> Self {
        Self { handle }
    }

    /// Get the underlying [JoinHandle].
    ///
    /// This will not run the drop-implementation, and therefore the `Actor` will not
    /// be halted/aborted.
    pub fn into_joinhandle(self) -> JoinHandle<E> {
        self.handle.into_parts().1.pop().unwrap()
    }

    /// Abort the `Actor`.
    ///
    /// Returns `true` if this is the first abort.
    pub fn abort(&mut self) -> bool {
        self.handle.abort()
    }

    /// Close the `Channel`.
    pub fn close(&self) -> bool {
        self.handle.channel().close()
    }

    /// Halt the `Process`.
    pub fn halt(&self) {
        self.handle.channel().halt(1)
    }

    /// Get the amount of messages in the `Channel`.
    pub fn msg_count(&self) -> usize {
        self.handle.channel().msg_count()
    }

    /// Get the current amount of [Addresses](Address).
    pub fn address_count(&self) -> usize {
        self.handle.channel().sender_count()
    }

    /// Whether the `Channel` is closed.
    pub fn is_closed(&self) -> bool {
        self.handle.channel().closed()
    }

    /// Whether the [Inbox] has exited.
    pub fn inbox_exited(&self) -> bool {
        self.handle.channel().receiver_count() == 0
    }

    /// Get the [Capacity] of the `Channel`.
    pub fn capacity(&self) -> &Capacity {
        self.handle.channel().capacity()
    }

    /// Attach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
        self.handle.attach(duration)
    }

    /// Detach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn detach(&mut self) -> Option<Duration> {
        self.handle.detach()
    }

    /// Get a reference to the current [Link] of the `Actor`.
    pub fn link(&self) -> &Link {
        &self.handle.link()
    }

    /// Whether the `tokio::task` has exited.
    pub fn exited(&self) -> bool {
        self.handle.exited()
    }

    /// Whether the `Actor` has been aborted.
    pub fn is_aborted(&self) -> bool {
        self.handle.is_aborted()
    }
}

impl<E: Send + 'static, M: Send + 'static> Child<E, InnerChannel<M>> {
    /// Attempt to spawn an additional `Process` on to this `Channel`.
    ///
    /// This transforms the [Child] into a [ChildPool].
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the `Channel`.
    /// * The `Channel` has already exited.
    pub fn spawn<Fun, Fut>(
        mut self,
        fun: Fun,
    ) -> Result<ChildPool<E, InnerChannel<M>>, TrySpawnError<Self>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        M: Send + 'static,
        E: Send + 'static,
    {
        match self
            .handle
            .spawn(|receiver| async move { fun(Inbox::new(receiver)).await })
        {
            Ok(_) => Ok(ChildPool {
                handle: self.handle,
            }),
            Err(TrySpawnError(fun)) => Err(TrySpawnError(self)),
        }
    }

    /// Convert the `Child<T, Channel<M>>` into a `Child<T>`
    pub fn into_dyn(self) -> Child<E> {
        let (shared, handle, link, aborted) = self.handle.into_parts();
        Child {
            handle: Handle::from_parts(shared, handle, link, aborted),
        }
    }
}

impl<E: Send + 'static> Child<E> {
    pub fn downcast<M: Send + 'static>(self) -> Child<E, InnerChannel<M>> {
        todo!()
    }
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Unpin for Child<E, C> {}
impl<E: Send + 'static, C: AnyChannel + ?Sized> Future for Child<E, C> {
    type Output = Result<E, ExitError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.handle.poll_next_unpin(cx).map(|ready| match ready {
            Some(exit) => exit,
            None => panic!("Future should not be polled after completion!"),
        })
    }
}

/// A [ChildPool] is similar to a [Child], except that the `Actor` can have more
/// than one `Process`. A [ChildPool] can be streamed to get the exit-values of
/// all spawned `tokio::task`s.
pub struct ChildPool<E: Send + 'static, C: AnyChannel + ?Sized = dyn AnyChannel> {
    handle: Handle<E, C>,
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> ChildPool<E, C> {
    pub(crate) fn from_handle(handle: Handle<E, C>) -> Self {
        Self { handle }
    }

    /// Get the underlying [JoinHandles](JoinHandle). The order does not necessarily reflect
    /// which was spawned earlier
    ///
    /// This will not run the drop-implementation, and therefore the `Actor` will not
    /// be halted/aborted.
    pub fn into_joinhandles(self) -> Vec<JoinHandle<E>> {
        self.handle.into_parts().1
    }

    /// Abort the `Actor`.
    ///
    /// Returns `true` if this is the first abort.
    pub fn abort(&mut self) -> bool {
        self.handle.abort()
    }

    /// Close the `Channel`.
    pub fn close(&self) -> bool {
        self.handle.channel().close()
    }

    /// Halt all `Processes`.
    pub fn halt_all(&self) {
        self.handle.channel().halt(u32::MAX)
    }

    /// Halt `n` `Processes`.
    pub fn halt_some(&self, n: u32) {
        self.handle.channel().halt(n)
    }

    /// Get the amount of messages in the `Channel`.
    pub fn msg_count(&self) -> usize {
        self.handle.channel().msg_count()
    }

    /// Get the current amount of [Addresses](Address).
    pub fn address_count(&self) -> usize {
        self.handle.channel().sender_count()
    }

    /// Get the amount of messages in the `Channel`.
    pub fn inbox_count(&self) -> usize {
        self.handle.channel().receiver_count()
    }

    /// Whether the `Channel` is closed.
    pub fn is_closed(&self) -> bool {
        self.handle.channel().closed()
    }
    /// Get the [Capacity] of the `Channel`.
    pub fn capacity(&self) -> &Capacity {
        self.handle.channel().capacity()
    }

    /// Attach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
        self.handle.attach(duration)
    }

    /// Detach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn detach(&mut self) -> Option<Duration> {
        self.handle.detach()
    }

    /// Get a reference to the current [Link] of the `Actor`.
    pub fn link(&self) -> &Link {
        self.handle.link()
    }

    /// Whether all `tokio::tasks` have exited.
    pub fn exited(&self) -> bool {
        self.handle.exited()
    }

    /// The amount of `tokio::task`s that are still alive
    pub fn task_count(&self) -> usize {
        self.handle.task_count()
    }

    /// The amount of `Child`ren in this `ChildPool`, this includes both alive and
    /// dead `Processes`.
    pub fn child_count(&self) -> usize {
        self.handle.child_count()
    }

    /// Whether the `Actor` is aborted.
    pub fn is_aborted(&self) -> bool {
        self.handle.is_aborted()
    }
}

impl<E: Send + 'static, M: Send + 'static> ChildPool<E, InnerChannel<M>> {
    pub fn into_dyn(self) -> ChildPool<E> {
        let (shared, handle, link, aborted) = self.handle.into_parts();
        ChildPool {
            handle: actor_channel::Handle::from_parts(shared, handle, link, aborted),
        }
    }

    /// Attempt to spawn an additional `Process` on to this `Channel`.
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the `Channel`.
    /// * The `Channel` has already exited.
    pub fn spawn<Fun, Fut>(&mut self, fun: Fun) -> Result<(), TrySpawnError<()>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        E: Send + 'static,
    {
        match self.handle.spawn(|receiver| async move { fun(Inbox::new(receiver)).await })  {
            Ok(_) => Ok(()),
            Err(_) => Err(TrySpawnError(())),
        }
    }
}

impl<E: Send + 'static> ChildPool<E> {
    pub fn downcast<M: Send + 'static>(self) -> Child<E, InnerChannel<M>> {
        todo!()
    }
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Stream for ChildPool<E, C> {
    type Item = Result<E, ExitError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.handle.poll_next_unpin(cx)
    }
}
