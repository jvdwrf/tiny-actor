use crate::*;
use futures::{Future, FutureExt, Stream};
use std::{any::Any, fmt::Debug, mem::ManuallyDrop, sync::Arc, task::Poll, time::Duration};
use tokio::task::JoinHandle;

#[derive(Debug)]
pub struct Child<E, C = dyn AnyActor>
where
    E: Send + 'static,
    C: DynActor + ?Sized,
{
    handle: Option<JoinHandle<E>>,
    channel: Arc<C>,
    link: Link,
    is_aborted: bool,
}

impl<E, C> Child<E, C>
where
    E: Send + 'static,
    C: DynActor + ?Sized,
{
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
    /// This will not run the drop-implementation, and therefore the actor will not
    /// be halted/aborted.
    pub fn into_tokio_joinhandle(self) -> JoinHandle<E> {
        self.into_parts().1
    }

    /// Abort the actor.
    ///
    /// Returns `true` if this is the first abort.
    pub fn abort(&mut self) -> bool {
        self.channel.close();
        let was_aborted = self.is_aborted;
        self.is_aborted = true;
        self.handle.as_ref().unwrap().abort();
        !was_aborted
    }

    /// Whether the task has finished.
    pub fn is_finished(&self) -> bool {
        self.handle.as_ref().unwrap().is_finished()
    }

    /// Convert the [Child] into a [ChildPool].
    pub fn into_pool(self) -> ChildPool<E, C> {
        let (channel, handle, link, is_aborted) = self.into_parts();
        ChildPool {
            channel,
            handles: Some(vec![handle]),
            link,
            is_aborted,
        }
    }

    gen::child_methods!();
    gen::any_channel_methods!();
}

impl<E, C> Child<E, C>
where
    E: Send + 'static,
    C: AnyActor + ?Sized,
{
    /// Downcast the `Child<E>` to a `Child<E, Actor<M>>`.
    pub fn downcast<M: Send + 'static>(self) -> Result<Child<E, Actor<M>>, Self> {
        let (channel, handle, link, is_aborted) = self.into_parts();
        match channel.clone().into_any().downcast::<Actor<M>>() {
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

impl<E, M> Child<E, Actor<M>>
where
    E: Send + 'static,
    M: Send + 'static,
{
    /// Convert the `Child<T, Actor<M>>` into a `Child<T>`
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

impl<E: Send + 'static, C: DynActor + ?Sized> Drop for Child<E, C> {
    fn drop(&mut self) {
        if let Link::Attached(abort_timer) = self.link {
            if !self.is_aborted && !self.is_finished() {
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

impl<E: Send + 'static, C: DynActor + ?Sized> Unpin for Child<E, C> {}

impl<E: Send + 'static, C: DynActor + ?Sized> Future for Child<E, C> {
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

//------------------------------------------------------------------------------------------------
//  ChildPool
//------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct ChildPool<E, C = dyn AnyActor>
where
    E: Send + 'static,
    C: DynActor + ?Sized,
{
    channel: Arc<C>,
    handles: Option<Vec<JoinHandle<E>>>,
    link: Link,
    is_aborted: bool,
}

impl<E, C> ChildPool<E, C>
where
    E: Send + 'static,
    C: DynActor + ?Sized,
{
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
    /// This will not run the drop-implementation, and therefore the `Actor` will not
    /// be halted/aborted.
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

    /// Whether all tasks have exited.
    pub fn is_finished(&self) -> bool {
        self.handles
            .as_ref()
            .unwrap()
            .iter()
            .all(|handle| handle.is_finished())
    }

    /// The amount of tasks that are alive
    pub fn task_count(&self) -> usize {
        self.handles
            .as_ref()
            .unwrap()
            .iter()
            .filter(|handle| !handle.is_finished())
            .collect::<Vec<_>>()
            .len()
    }

    /// The amount of `Children` in this `ChildPool`, this includes both alive and
    /// dead processes.
    pub fn child_count(&self) -> usize {
        self.handles.as_ref().unwrap().len()
    }

    /// Convert the [ChildPool] into a [Child]. Succeeds if there is exactly one child in the
    /// pool.
    pub fn try_into_child(self) -> Result<Child<E, C>, Self> {
        if self.handles.as_ref().unwrap().len() == 1 {
            let (channel, mut handles, link, is_aborted) = self.into_parts();
            Ok(Child {
                channel,
                handle: Some(handles.pop().unwrap()),
                link,
                is_aborted,
            })
        } else {
            Err(self)
        }
    }

    gen::any_channel_methods!();
    gen::child_methods!();
}

impl<E, C> ChildPool<E, C>
where
    E: Send + 'static,
    C: AnyActor + ?Sized,
{
    /// Attempt to spawn an additional process onto the [Actor].
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the [Actor].
    /// * The [Actor] has already exited.
    pub fn try_spawn<M, Fun, Fut>(&mut self, fun: Fun) -> Result<(), TrySpawnError<Fun>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        M: Send + 'static,
        E: Send + 'static,
    {
        let channel = match Arc::downcast::<Actor<M>>(self.channel.clone().into_any()) {
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

    /// Downcast the `ChildPool<E>` to a `ChildPool<E, Actor<M>>`
    pub fn downcast<M: Send + 'static>(self) -> Result<ChildPool<E, Actor<M>>, Self> {
        let (channel, handles, link, is_aborted) = self.into_parts();
        match channel.clone().into_any().downcast::<Actor<M>>() {
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

impl<E: Send + 'static, M: Send + 'static> ChildPool<E, Actor<M>> {
    /// Convert the `ChildPool<E, Actor<M>` into a `ChildPool<E>`.
    pub fn into_dyn(self) -> ChildPool<E> {
        let parts = self.into_parts();
        ChildPool {
            handles: Some(parts.1),
            channel: parts.0,
            link: parts.2,
            is_aborted: parts.3,
        }
    }

    /// Attempt to spawn an additional process on to this [Actor].
    ///
    /// This method can fail for 2 reasons:
    /// * The [Inbox]-type does not match that of the [Actor].
    /// * The [Actor] has already exited.
    pub fn spawn<Fun, Fut>(&mut self, fun: Fun) -> Result<(), SpawnError<Fun>>
    where
        Fun: FnOnce(Inbox<M>) -> Fut + Send + 'static,
        Fut: Future<Output = E> + Send + 'static,
        E: Send + 'static,
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

impl<E: Send + 'static, C: DynActor + ?Sized> Stream for ChildPool<E, C> {
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

impl<E: Send + 'static, C: DynActor + ?Sized> Drop for ChildPool<E, C> {
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

//------------------------------------------------------------------------------------------------
//  Errors
//------------------------------------------------------------------------------------------------

/// An error returned from an exiting task.
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

/// An error returned when trying to spawn more processes onto a [Actor].
#[derive(Clone, thiserror::Error)]
pub enum TrySpawnError<T> {
    #[error("Actor has already exited")]
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

/// An error returned when spawning more processes onto a [Actor].
///
/// This can only happen if the [Actor] has already exited.
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
        let (child, _addr) = spawn(Config::default(), test_loop!());
        matches!(child.into_dyn().downcast::<()>(), Ok(_));

        let (pool, _addr) = spawn_many(0..5, Config::default(), test_many_loop!());
        matches!(pool.into_dyn().downcast::<()>(), Ok(_));
    }

    #[tokio::test]
    async fn child_try_spawn_ok() {
        let (child, _addr) = spawn_one(Config::default(), test_loop!());
        tokio::time::sleep(Duration::from_millis(1)).await;
        let mut child = child.into_dyn();
        child.try_spawn(test_helper::test_loop!()).unwrap();
        assert_eq!(child.inbox_count(), 2);
    }

    #[tokio::test]
    async fn child_try_spawn_exited() {
        let (child, mut addr) = spawn_one(Config::default(), test_loop!());
        addr.halt();
        (&mut addr).await;

        let res = child.into_dyn().try_spawn(test_loop!());

        matches!(res, Err(TrySpawnError::Exited(_)));
        assert_eq!(addr.inbox_count(), 0);
    }

    #[tokio::test]
    async fn child_try_spawn_incorrect_type() {
        let (child, addr) = spawn_one(Config::default(), test_loop!());
        let res = child.into_dyn().try_spawn(test_loop!(u32));

        matches!(res, Err(TrySpawnError::IncorrectType(_)));
        assert_eq!(addr.inbox_count(), 1);
    }

    #[tokio::test]
    async fn child_spawn_ok() {
        let (child, _addr) = spawn_one(Config::default(), test_loop!());
        let mut child = child.into_dyn();
        child.try_spawn(test_loop!()).unwrap();
        assert_eq!(child.inbox_count(), 2);
    }

    #[tokio::test]
    async fn child_spawn_exited() {
        let (mut child, mut addr) = spawn_one(Config::default(), test_loop!());
        addr.halt();
        (&mut addr).await;

        let res = child.spawn(test_loop!());

        matches!(res, Err(SpawnError(_)));
        assert_eq!(addr.inbox_count(), 0);
    }
}
