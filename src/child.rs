use crate::*;
use futures::{Future, FutureExt, Stream};
use std::{any::Any, fmt::Debug, mem::ManuallyDrop, sync::Arc, task::Poll, time::Duration};
use tokio::task::JoinHandle;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SupervisionStatus {
    Attached,
    Detached,
    Aborted,
}

#[derive(Debug, Clone)]
pub struct AlreadyAborted;

//------------------------------------------------------------------------------------------------
//  Child
//------------------------------------------------------------------------------------------------

pub struct Child<T: Send + 'static> {
    handle: Option<JoinHandle<T>>,
    channel: Arc<dyn DynamicChannel>,
    link: Link,
    is_aborted: bool,
}

impl<T: Send + 'static> Child<T> {
    pub(crate) fn new<R>(channel: Arc<Channel<R>>, join_handle: JoinHandle<T>, link: Link) -> Self
    where
        R: Send + 'static,
    {
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
    fn _into_parts(self) -> (Arc<dyn DynamicChannel>, JoinHandle<T>, Link, bool) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let mut handle = std::ptr::read(&no_drop.handle);
            let channel = std::ptr::read(&no_drop.channel);
            let link = std::ptr::read(&no_drop.link);
            let is_aborted = std::ptr::read(&no_drop.is_aborted);
            (channel, handle.take().unwrap(), link, is_aborted)
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    pub fn into_parts(self) -> (JoinHandle<T>, Link, bool) {
        let (_a, b, c, d) = self._into_parts();
        (b, c, d)
    }

    /// Attempt to spawn an additional process linked to this channel. This will turn the
    /// `Child` into a `ChildGroup`.
    ///
    /// Can fail if:
    /// * The inbox-type does not match that of the channel.
    /// * The channel has already exited.
    pub fn try_spawn<R, Fun, Fut>(
        self,
        fun: Fun,
    ) -> Result<ChildPool<T>, TrySpawnError<(Self, Fun)>>
    where
        Fun: FnOnce(Inbox<R>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        R: Send + 'static,
        T: Send + 'static,
    {
        let typed_channel = match Arc::downcast(self.channel.clone().into_any()) {
            Ok(channel) => channel,
            Err(_) => return Err(TrySpawnError::IncorrectInboxType((self, fun))),
        };

        match Inbox::try_from_channel(typed_channel) {
            Some(inbox) => {
                let new_handle = tokio::task::spawn(async move { fun(inbox).await });

                let (channel, old_handle, link, is_aborted) = self._into_parts();

                Ok(ChildPool {
                    channel: channel,
                    handles: Some(vec![old_handle, new_handle]),
                    link,
                    is_aborted: is_aborted
                })
            }
            None => Err(TrySpawnError::Exited((self, fun))),
        }
    }

    /// Close the channel.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt the inbox.
    pub fn halt(&self) {
        self.channel.halt_n(1)
    }

    /// Get the amount of inboxes linked to the channel.
    pub fn inbox_count(&self) -> usize {
        self.channel.inbox_count()
    }

    /// Get the amount of messages linked to the channel.
    pub fn message_count(&self) -> usize {
        self.channel.message_count()
    }

    /// Get the amount of addresses linked to the channel.
    pub fn address_count(&self) -> usize {
        self.channel.address_count()
    }

    /// Whether the channel has been closed.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Attach the process. Returns true if the process was detached.
    /// before this. Returns an error if the process has already been aborted.
    pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
        self.link.attach(duration)
    }

    /// Detach the process. Returns true if the process was attached.
    /// before this. Returns an error if the process has already been aborted.
    pub fn detach(&mut self) -> Option<Duration> {
        self.link.detach()
    }

    /// Abort all processes linked to the channel.
    ///
    /// Returns true if this is the first time aborting
    pub fn abort(&mut self) -> bool {
        let was_aborted = self.is_aborted;
        self.is_aborted = true;
        self.handle.as_ref().unwrap().abort();
        !was_aborted
    }

    /// Get a reference to the current supervision-state
    pub fn link(&self) -> &Link {
        &self.link
    }

    /// Whether all inboxes linked to this channel have exited.
    pub fn has_exited(&self) -> bool {
        self.inbox_count() == 0
    }

    pub fn is_aborted(&self) -> bool {
        self.is_aborted
    }
}

impl<T: Send + 'static> Drop for Child<T> {
    fn drop(&mut self) {
        if let Link::Attached(abort_timer) = self.link {
            if self.inbox_count() != 0 {
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

impl<T: Send + 'static> Future for Child<T> {
    type Output = Result<T, JoinError>;

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
//  ChildGroup
//------------------------------------------------------------------------------------------------

pub struct ChildPool<T: Send + 'static> {
    channel: Arc<dyn DynamicChannel>,
    handles: Option<Vec<JoinHandle<T>>>,
    link: Link,
    is_aborted: bool,
}

impl<T: Send + 'static> ChildPool<T> {
    pub(crate) fn new<R: 'static + Send>(
        channel: Arc<Channel<R>>,
        handles: Vec<JoinHandle<T>>,
        link: Link,
    ) -> Self {
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
    pub(crate) fn _into_parts(self) -> (Arc<dyn DynamicChannel>, Vec<JoinHandle<T>>, Link, bool) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let mut handle = std::ptr::read(&no_drop.handles);
            let channel = std::ptr::read(&no_drop.channel);
            let link = std::ptr::read(&no_drop.link);
            let is_aborted = std::ptr::read(&no_drop.is_aborted);
            (channel, handle.take().unwrap(), link, is_aborted)
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    pub fn into_parts(self) -> (Vec<JoinHandle<T>>, Link, bool) {
        let (_a, b, c, d) = self._into_parts();
        (b, c, d)
    }

    pub fn try_spawn<R, Fun, Fut>(&mut self, fun: Fun) -> Result<(), TrySpawnError<Fun>>
    where
        Fun: FnOnce(Inbox<R>) -> Fut + Send + 'static,
        Fut: Future<Output = T> + Send + 'static,
        R: Send + 'static,
        T: Send + 'static,
    {
        let typed_channel = match Arc::downcast(self.channel.clone().into_any()) {
            Ok(channel) => channel,
            Err(_) => return Err(TrySpawnError::Exited(fun)),
        };

        match Inbox::try_from_channel(typed_channel) {
            Some(inbox) => {
                let handle = tokio::task::spawn(async move { fun(inbox).await });
                self.handles.as_mut().unwrap().push(handle);
                Ok(())
            }
            None => Err(TrySpawnError::Exited(fun)),
        }
    }

    /// Abort all processes linked to the channel.
    ///
    /// Returns true if this is the first time aborting
    pub fn abort(&mut self) -> bool {
        let was_aborted = self.is_aborted;
        self.is_aborted = true;
        for handle in self.handles.as_ref().unwrap() {
            handle.abort()
        }
        !was_aborted
    }

    /// Close the channel.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt all inboxes linked to the channel.
    pub fn halt_all(&self) {
        self.channel.halt_n(u32::MAX)
    }

    pub fn halt_some(&self, n: u32) {
        self.channel.halt_n(n)
    }

    /// Get the amount of inboxes linked to the channel.
    pub fn inbox_count(&self) -> usize {
        self.channel.inbox_count()
    }

    /// Get the amount of messages linked to the channel.
    pub fn message_count(&self) -> usize {
        self.channel.message_count()
    }

    /// Get the amount of addresses linked to the channel.
    pub fn address_count(&self) -> usize {
        self.channel.address_count()
    }

    /// Whether the channel has been closed.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Attach the process. Returns true if the process was detached.
    /// before this. Returns an error if the process has already been aborted.
    pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
        self.link.attach(duration)
    }

    /// Detach the process. Returns true if the process was attached.
    /// before this. Returns an error if the process has already been aborted.
    pub fn detach(&mut self) -> Option<Duration> {
        self.link.detach()
    }

    /// Get a reference to the current supervision-state
    pub fn link(&self) -> &Link {
        &self.link
    }

    /// Whether all inboxes linked to this channel have exited.
    pub fn has_exited(&self) -> bool {
        self.inbox_count() == 0
    }

    pub fn is_aborted(&self) -> bool {
        self.is_aborted
    }
}

impl<T: Send + 'static> Stream for ChildPool<T> {
    type Item = Result<T, JoinError>;

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

impl<T: Send + 'static> Drop for ChildPool<T> {
    fn drop(&mut self) {
        if let Link::Attached(abort_timer) = self.link {
            if self.inbox_count() != 0 {
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
//  DynamicChannel
//------------------------------------------------------------------------------------------------

pub(crate) trait DynamicChannel: Send + 'static {
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
    fn close(&self) -> bool;
    fn halt_n(&self, n: u32);
    fn inbox_count(&self) -> usize;
    fn message_count(&self) -> usize;
    fn address_count(&self) -> usize;
    fn is_closed(&self) -> bool;
}

impl<T: Send + 'static> DynamicChannel for Channel<T> {
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn close(&self) -> bool {
        self.close()
    }
    fn halt_n(&self, n: u32) {
        self.halt_n(n)
    }
    fn inbox_count(&self) -> usize {
        self.inbox_count()
    }
    fn message_count(&self) -> usize {
        self.msg_count()
    }
    fn address_count(&self) -> usize {
        self.address_count()
    }
    fn is_closed(&self) -> bool {
        self.is_closed()
    }
}

//------------------------------------------------------------------------------------------------
//  Errors
//------------------------------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("Child has panicked")]
    Panic(Box<dyn Any + Send>),
    #[error("Child has been aborted")]
    Abort,
}

impl JoinError {
    pub fn is_panic(&self) -> bool {
        match self {
            JoinError::Panic(_) => true,
            JoinError::Abort => false,
        }
    }

    pub fn is_abort(&self) -> bool {
        match self {
            JoinError::Panic(_) => false,
            JoinError::Abort => true,
        }
    }
}

impl From<tokio::task::JoinError> for JoinError {
    fn from(e: tokio::task::JoinError) -> Self {
        match e.try_into_panic() {
            Ok(panic) => JoinError::Panic(panic),
            Err(_) => JoinError::Abort,
        }
    }
}

#[derive(Clone, thiserror::Error)]
pub enum TrySpawnError<T> {
    #[error("Channel has already exited")]
    Exited(T),
    #[error("Inbox type did not match")]
    IncorrectInboxType(T),
}

impl<T> Debug for TrySpawnError<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("TrySpawnError").finish()
    }
}
