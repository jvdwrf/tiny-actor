use crate::*;
use futures::{Future, FutureExt, Stream, task::Spawn};
use std::{any::Any, fmt::Debug, mem::ManuallyDrop, sync::Arc, task::Poll, time::Duration};
use tokio::task::JoinHandle;

/// A `Child` is a handle to an `Actor` with a single `Process`. A `Child` can be awaited to return
/// the exit-value of the `tokio::task`. A `Child` is non-cloneable, and therefore unique to the
/// `Channel`. When the `Child` is dropped, the `Actor` will be `halt`ed and `abort`ed. This can
/// be prevented by detaching the `Child`. More processes can be spawned later, which transforms
/// the `Child` into a `ChildPool`.
pub struct Child<E: Send + 'static, C: AnyChannel + ?Sized = dyn AnyChannel> {
    handle: Option<JoinHandle<E>>,
    channel: Arc<C>,
    link: Link,
    is_aborted: bool,
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Child<E, C> {
    pub(crate) fn new(channel: Arc<C>, join_handle: JoinHandle<E>, link: Link) -> Self {
        Self {
            handle: Some(join_handle),
            link,
            channel,
            is_aborted: false,
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    fn into_parts(self) -> (Arc<C>, JoinHandle<E>, Link, bool) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let mut handle = std::ptr::read(&no_drop.handle);
            let channel = std::ptr::read(&no_drop.channel);
            let link = std::ptr::read(&no_drop.link);
            let is_aborted = std::ptr::read(&no_drop.is_aborted);
            (channel, handle.take().unwrap(), link, is_aborted)
        }
    }

    /// Get the underlying [JoinHandle].
    ///
    /// This will not run the drop-implementation, and therefore the `Actor` will not
    /// be halted/aborted.
    pub fn into_tokio_joinhandle(self) -> JoinHandle<E> {
        self.into_parts().1
    }

    /// Abort the `Actor`.
    ///
    /// Returns `true` if this is the first abort.
    pub fn abort(&mut self) -> bool {
        let was_aborted = self.is_aborted;
        self.is_aborted = true;
        self.handle.as_ref().unwrap().abort();
        !was_aborted
    }

    /// Close the `Channel`.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt the `Process`.
    pub fn halt(&self) {
        self.channel.halt_n(1)
    }

    /// Get the amount of messages in the [Channel].
    pub fn msg_count(&self) -> usize {
        self.channel.msg_count()
    }

    /// Get the current amount of [Addresses](Address).
    pub fn address_count(&self) -> usize {
        self.channel.address_count()
    }

    /// Get the current amount of [Inboxes](Inbox).
    pub fn inbox_count(&self) -> usize {
        self.channel.inbox_count()
    }

    /// Whether the `Channel` is closed.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Attach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
        self.link.attach(duration)
    }

    /// Detach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn detach(&mut self) -> Option<Duration> {
        self.link.detach()
    }

    /// Get a reference to the current [Link] of the `Actor`.
    pub fn link(&self) -> &Link {
        &self.link
    }

    /// Whether the `tokio::task` has exited.
    pub fn exited(&self) -> bool {
        self.handle.as_ref().unwrap().is_finished()
    }

    /// Whether the [Inbox] has exited.
    pub fn inbox_exited(&self) -> bool {
        self.channel.inboxes_exited()
    }

    /// Get the [Capacity] of the `Channel`.
    pub fn capacity(&self) -> &Capacity {
        self.channel.capacity()
    }

    /// Whether the `Actor` has been aborted.
    pub fn is_aborted(&self) -> bool {
        self.is_aborted
    }
}

impl<E: Send + 'static, M: Send + 'static> Child<E, Channel<M>> {
    /// Attempt to spawn an additional `Process` on to this `Channel`.
    ///
    /// This transforms the [Child] into a [ChildPool].
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the `Channel`.
    /// * The `Channel` has already exited.
    pub fn spawn<Fun, Fut>(
        self,
        fun: Fun,
    ) -> Result<ChildPool<E, Channel<M>>, SpawnError<(Self, Fun)>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        M: Send + 'static,
        E: Send + 'static,
    {
        match Inbox::try_create(self.channel.clone()) {
            Some(inbox) => {
                let new_handle = tokio::task::spawn(async move { fun(inbox).await });

                let (channel, old_handle, link, is_aborted) = self.into_parts();

                Ok(ChildPool {
                    channel: channel,
                    handles: Some(vec![old_handle, new_handle]),
                    link,
                    is_aborted: is_aborted,
                })
            }
            None => Err(SpawnError((self, fun))),
        }
    }

    /// Convert the `Child<T, Channel<M>>` into a `Child<T>`
    pub fn into_dyn(self) -> Child<E> {
        let parts = self.into_parts();
        Child {
            handle: Some(parts.1),
            channel: parts.0,
            link: parts.2,
            is_aborted: parts.3,
        }
    }
}

impl<E: Send + 'static> Child<E> {
    /// Attempt to spawn an additional `Process` on to this `Channel`.
    ///
    /// This transforms the [Child] into a [ChildPool].
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the `Channel`.
    /// * The `Channel` has already exited.
    pub fn try_spawn<R, Fun, Fut>(
        self,
        fun: Fun,
    ) -> Result<ChildPool<E>, TrySpawnError<(Self, Fun)>>
    where
        Fun: FnOnce(Inbox<R>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        R: Send + 'static,
        E: Send + 'static,
    {
        let channel = match self.channel.clone().into_any().downcast() {
            Ok(channel) => channel,
            Err(_) => return Err(TrySpawnError::IncorrectType((self, fun))),
        };

        match Inbox::try_create(channel) {
            Some(inbox) => {
                let new_handle = tokio::task::spawn(async move { fun(inbox).await });

                let (channel, old_handle, link, is_aborted) = self.into_parts();

                Ok(ChildPool {
                    channel: channel,
                    handles: Some(vec![old_handle, new_handle]),
                    link,
                    is_aborted: is_aborted,
                })
            }
            None => Err(TrySpawnError::Exited((self, fun))),
        }
    }

    pub fn downcast<M: Send + 'static>(self) -> Result<Child<E, Channel<M>>, Self> {
        let (channel, handle, link, is_aborted) = self.into_parts();
        match channel.clone().into_any().downcast::<Channel<M>>() {
            Ok(channel) => Ok(Child {
                handle: Some(handle),
                channel,
                link,
                is_aborted,
            }),
            Err(_) => Err(Child {
                handle: Some(handle),
                channel,
                link,
                is_aborted,
            }),
        }
    }
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Drop for Child<E, C> {
    fn drop(&mut self) {
        if let Link::Attached(abort_timer) = self.link {
            if !self.is_aborted && !self.exited() {
                if abort_timer.is_zero() {
                    self.abort();
                } else {
                    self.halt();
                    let handle = self.handle.take().unwrap();
                    tokio::task::spawn(async move {
                        tokio::time::sleep(abort_timer).await;
                        handle.abort();
                    });
                }
            }
        }
    }
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Unpin for Child<E, C> {}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Future for Child<E, C> {
    type Output = Result<E, ExitError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        self.handle
            .as_mut()
            .unwrap()
            .poll_unpin(cx)
            .map_err(|e| e.into())
    }
}

/// A [ChildPool] is similar to a [Child], except that the `Actor` can have more
/// than one `Process`. A [ChildPool] can be streamed to get the exit-values of
/// all spawned `tokio::task`s.
pub struct ChildPool<E: Send + 'static, C: AnyChannel + ?Sized = dyn AnyChannel> {
    channel: Arc<C>,
    handles: Option<Vec<JoinHandle<E>>>,
    link: Link,
    is_aborted: bool,
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> ChildPool<E, C> {
    pub(crate) fn new(channel: Arc<C>, handles: Vec<JoinHandle<E>>, link: Link) -> Self {
        Self {
            channel,
            handles: Some(handles),
            link,
            is_aborted: false,
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    pub(crate) fn into_parts(self) -> (Arc<C>, Vec<JoinHandle<E>>, Link, bool) {
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
    /// which was spawned earlier
    ///
    /// This will not run the drop-implementation, and therefore the `Actor` will not
    /// be halted/aborted.
    pub fn into_tokio_joinhandles(self) -> Vec<JoinHandle<E>> {
        self.into_parts().1
    }

    /// Abort the `Actor`.
    ///
    /// Returns `true` if this is the first abort.
    pub fn abort(&mut self) -> bool {
        let was_aborted = self.is_aborted;
        self.is_aborted = true;
        for handle in self.handles.as_ref().unwrap() {
            handle.abort()
        }
        !was_aborted
    }

    /// Close the `Channel`.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt all `Processes`.
    pub fn halt_all(&self) {
        self.channel.halt_n(u32::MAX)
    }

    /// Halt `n` `Processes`.
    pub fn halt_some(&self, n: u32) {
        self.channel.halt_n(n)
    }

    /// Get the amount of messages in the `Channel`.
    pub fn msg_count(&self) -> usize {
        self.channel.msg_count()
    }

    /// Get the current amount of [Addresses](Address).
    pub fn address_count(&self) -> usize {
        self.channel.address_count()
    }

    /// Get the amount of messages in the `Channel`.
    pub fn inbox_count(&self) -> usize {
        self.channel.inbox_count()
    }

    /// Whether the `Channel` is closed.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Attach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
        self.link.attach(duration)
    }

    /// Detach the `Actor`. Returns the old abort-timeout, if it was attached before this.
    pub fn detach(&mut self) -> Option<Duration> {
        self.link.detach()
    }

    /// Get a reference to the current [Link] of the `Actor`.
    pub fn link(&self) -> &Link {
        &self.link
    }

    /// Whether all `tokio::tasks` have exited.
    pub fn exited(&self) -> bool {
        self.handles
            .as_ref()
            .unwrap()
            .iter()
            .all(|handle| handle.is_finished())
    }

    /// The amount of `tokio::task`s that are still alive
    pub fn task_count(&self) -> usize {
        self.handles
            .as_ref()
            .unwrap()
            .iter()
            .filter(|handle| !handle.is_finished())
            .collect::<Vec<_>>()
            .len()
    }

    /// The amount of `Child`ren in this `ChildPool`, this includes both alive and
    /// dead `Processes`.
    pub fn child_count(&self) -> usize {
        self.handles.as_ref().unwrap().len()
    }

    /// Whether the `Actor` is aborted.
    pub fn is_aborted(&self) -> bool {
        self.is_aborted
    }

    /// Get the [Capacity] of the `Channel`.
    pub fn capacity(&self) -> &Capacity {
        self.channel.capacity()
    }
}

impl<E: Send + 'static, M: Send + 'static> ChildPool<E, Channel<M>> {
    pub fn into_dyn(self) -> ChildPool<E> {
        let parts = self.into_parts();
        ChildPool {
            handles: Some(parts.1),
            channel: parts.0,
            link: parts.2,
            is_aborted: parts.3,
        }
    }

    /// Attempt to spawn an additional `Process` on to this `Channel`.
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the `Channel`.
    /// * The `Channel` has already exited.
    pub fn spawn<Fun, Fut>(&mut self, fun: Fun) -> Result<(), SpawnError<Fun>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        E: Send + 'static,
    {
        match Inbox::try_create(self.channel.clone()) {
            Some(inbox) => {
                let handle = tokio::task::spawn(async move { fun(inbox).await });
                self.handles.as_mut().unwrap().push(handle);
                Ok(())
            }
            None => Err(SpawnError(fun)),
        }
    }
}

impl<E: Send + 'static> ChildPool<E> {
    /// Attempt to spawn an additional `Process` on to this `Channel`.
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the `Channel`.
    /// * The `Channel` has already exited.
    pub fn try_spawn<R, Fun, Fut>(&mut self, fun: Fun) -> Result<(), TrySpawnError<Fun>>
    where
        Fun: FnOnce(Inbox<R>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        R: Send + 'static,
        E: Send + 'static,
    {
        let typed_channel = match Arc::downcast(self.channel.clone().into_any()) {
            Ok(channel) => channel,
            Err(_) => return Err(TrySpawnError::Exited(fun)),
        };

        match Inbox::try_create(typed_channel) {
            Some(inbox) => {
                let handle = tokio::task::spawn(async move { fun(inbox).await });
                self.handles.as_mut().unwrap().push(handle);
                Ok(())
            }
            None => Err(TrySpawnError::Exited(fun)),
        }
    }

    pub fn downcast<M: Send + 'static>(self) -> Result<ChildPool<E, Channel<M>>, Self> {
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
}

impl<E: Send + 'static, C: AnyChannel + ?Sized> Stream for ChildPool<E, C> {
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

impl<E: Send + 'static, C: AnyChannel + ?Sized> Drop for ChildPool<E, C> {
    fn drop(&mut self) {
        if let Link::Attached(abort_timer) = self.link {
            if !self.is_aborted && !self.exited() {
                if abort_timer.is_zero() {
                    self.abort();
                } else {
                    self.halt_all();
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

//------------------------------------------------------------------------------------------------
//  Errors
//------------------------------------------------------------------------------------------------

/// An error returned from an exiting tokio-task.
///
/// Can be either because it has panicked, or because it was aborted.
#[derive(Debug, thiserror::Error)]
pub enum ExitError {
    #[error("Child has panicked")]
    Panic(Box<dyn Any + Send>),
    #[error("Child has been aborted")]
    Abort,
}

impl ExitError {
    /// Whether the error is a panic.
    pub fn is_panic(&self) -> bool {
        match self {
            ExitError::Panic(_) => true,
            ExitError::Abort => false,
        }
    }

    /// Whether the error is an abort.
    pub fn is_abort(&self) -> bool {
        match self {
            ExitError::Panic(_) => false,
            ExitError::Abort => true,
        }
    }
}

impl From<tokio::task::JoinError> for ExitError {
    fn from(e: tokio::task::JoinError) -> Self {
        match e.try_into_panic() {
            Ok(panic) => ExitError::Panic(panic),
            Err(_) => ExitError::Abort,
        }
    }
}

/// An error returned when trying to spawn more processes onto a [Channel].
#[derive(Clone, thiserror::Error)]
pub enum TrySpawnError<T> {
    #[error("Channel has already exited")]
    Exited(T),
    #[error("Inbox type did not match")]
    IncorrectType(T),
}

impl<T> Debug for TrySpawnError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exited(_) => f.debug_tuple("Exited").finish(),
            Self::IncorrectType(_) => f.debug_tuple("IncorrectType").finish(),
        }
    }
}

/// An error returned when trying to spawn more processes onto a [Channel].
///
/// This can only happen if the [Channel] has already exited.
#[derive(Clone, thiserror::Error)]
#[error("Can't spawn a new inbox because the channel has exited")]
pub struct SpawnError<T>(pub T);

impl<T> Debug for SpawnError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SpawnError").finish()
    }
}

//------------------------------------------------------------------------------------------------
//  Test
//------------------------------------------------------------------------------------------------

#[cfg(test)]
mod test {
    use std::time::Duration;

    use crate::{test_helper::*, *};

    #[tokio::test]
    async fn downcast_child() {
        let (child, _addr) = spawn(Config::default(), test_loop!()).await;
        matches!(child.into_dyn().downcast::<()>(), Ok(_));

        let (pool, _addr) = spawn_many(0..5, Config::default(), test_many_loop!()).await;
        matches!(pool.into_dyn().downcast::<()>(), Ok(_));
    }

    #[tokio::test]
    async fn child_try_spawn_ok() {
        let (child, _addr) = spawn(Config::default(), test_loop!()).await;
        tokio::time::sleep(Duration::from_millis(1)).await;
        let pool = child
            .into_dyn()
            .try_spawn(test_helper::test_loop!())
            .unwrap();
        assert_eq!(pool.inbox_count(), 2);
    }

    #[tokio::test]
    async fn child_try_spawn_exited() {
        let (child, mut addr) = spawn(Config::default(), test_loop!()).await;
        addr.halt_all();
        (&mut addr).await;

        let res = child.into_dyn().try_spawn(test_loop!());

        matches!(res, Err(TrySpawnError::Exited(_)));
        assert_eq!(addr.inbox_count(), 0);
    }

    #[tokio::test]
    async fn child_try_spawn_incorrect_type() {
        let (child, addr) = spawn(Config::default(), test_loop!()).await;
        let res = child.into_dyn().try_spawn(test_loop!(u32));

        matches!(res, Err(TrySpawnError::IncorrectType(_)));
        assert_eq!(addr.inbox_count(), 1);
    }

    #[tokio::test]
    async fn child_spawn_ok() {
        let (child, _addr) = spawn(Config::default(), test_loop!()).await;
        let pool = child.into_dyn().try_spawn(test_loop!()).unwrap();
        assert_eq!(pool.inbox_count(), 2);
    }

    #[tokio::test]
    async fn child_spawn_exited() {
        let (child, mut addr) = spawn(Config::default(), test_loop!()).await;
        addr.halt_all();
        (&mut addr).await;

        let res = child.spawn(test_loop!());

        matches!(res, Err(SpawnError(_)));
        assert_eq!(addr.inbox_count(), 0);
    }
}
