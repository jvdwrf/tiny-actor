use crate::*;
use futures::{Future, FutureExt, Stream};
use std::{
    any::{Any, TypeId},
    fmt::Debug,
    mem::ManuallyDrop,
    sync::Arc,
    task::Poll,
    time::Duration,
};
use tokio::task::JoinHandle;

//------------------------------------------------------------------------------------------------
//  SupervisionState
//------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisionState {
    pub status: SupervisionStatus,
    pub abort_timer: Duration,
}

impl SupervisionState {
    pub(crate) fn attach(&mut self) -> Result<bool, AlreadyAborted> {
        match self.status {
            SupervisionStatus::Attached => Ok(false),
            SupervisionStatus::Detached => {
                self.status = SupervisionStatus::Attached;
                Ok(true)
            }
            SupervisionStatus::Aborted => Err(AlreadyAborted),
        }
    }

    pub(crate) fn detach(&mut self) -> Result<bool, AlreadyAborted> {
        match self.status {
            SupervisionStatus::Detached => Ok(false),
            SupervisionStatus::Attached => {
                self.status = SupervisionStatus::Detached;
                Ok(true)
            }
            SupervisionStatus::Aborted => Err(AlreadyAborted),
        }
    }

    pub(crate) fn set_abort_timer(&mut self, mut timer: Duration) -> Duration {
        std::mem::swap(&mut self.abort_timer, &mut timer);
        timer
    }

    pub(crate) fn new(attached: bool, abort_timer: Duration) -> Self {
        Self {
            status: if attached {
                SupervisionStatus::Attached
            } else {
                SupervisionStatus::Detached
            },
            abort_timer,
        }
    }
}

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
    state: SupervisionState,
}

impl<T: Send + 'static> Child<T> {
    pub(crate) fn new<R>(
        channel: Arc<Channel<R>>,
        join_handle: JoinHandle<T>,
        state: SupervisionState,
    ) -> Self
    where
        R: Send + 'static,
    {
        Self {
            handle: Some(join_handle),
            state,
            channel,
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    fn _into_parts(self) -> (Arc<dyn DynamicChannel>, JoinHandle<T>, SupervisionState) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let mut handle = std::ptr::read(&no_drop.handle);
            let channel = std::ptr::read(&no_drop.channel);
            let state = std::ptr::read(&no_drop.state);
            (channel, handle.take().unwrap(), state)
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    pub fn into_parts(self) -> (JoinHandle<T>, SupervisionState) {
        let (a, b, c) = self._into_parts();
        (b, c)
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

                let (channel, old_handle, state) = self._into_parts();

                Ok(ChildPool {
                    channel: channel,
                    handles: Some(vec![old_handle, new_handle]),
                    state,
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
    pub fn attach(&mut self) -> Result<bool, AlreadyAborted> {
        self.state.attach()
    }

    /// Detach the process. Returns true if the process was attached.
    /// before this. Returns an error if the process has already been aborted.
    pub fn detach(&mut self) -> Result<bool, AlreadyAborted> {
        self.state.detach()
    }

    /// Set the abort-timer of the processes linked to this channel.
    /// Returns the old abort-timer.
    pub fn set_abort_timer(&mut self, timer: Duration) -> Duration {
        self.state.set_abort_timer(timer)
    }

    /// Abort all processes linked to the channel.
    pub fn abort(&mut self) {
        self.state.status = SupervisionStatus::Aborted;
        self.handle.as_ref().unwrap().abort()
    }

    /// Get a reference to the current supervision-state
    pub fn state(&self) -> &SupervisionState {
        &self.state
    }

    /// Whether all inboxes linked to this channel have exited.
    pub fn has_exited(&self) -> bool {
        self.inbox_count() == 0
    }
}

impl<T: Send + 'static> Drop for Child<T> {
    fn drop(&mut self) {
        if self.state.status == SupervisionStatus::Attached && self.inbox_count() != 0 {
            if self.state.abort_timer.is_zero() {
                self.abort();
            } else {
                self.halt();
                let handle = self.handle.take().unwrap();
                let timer = self.state.abort_timer;
                tokio::task::spawn(async move {
                    tokio::time::sleep(timer).await;
                    handle.abort();
                });
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
    state: SupervisionState,
}

impl<T: Send + 'static> ChildPool<T> {
    pub(crate) fn new<R: 'static + Send>(
        channel: Arc<Channel<R>>,
        handles: Vec<JoinHandle<T>>,
        state: SupervisionState,
    ) -> Self {
        Self {
            channel,
            handles: Some(handles),
            state,
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    pub(crate) fn _into_parts(
        self,
    ) -> (
        Arc<dyn DynamicChannel>,
        Vec<JoinHandle<T>>,
        SupervisionState,
    ) {
        let no_drop = ManuallyDrop::new(self);
        unsafe {
            let mut handle = std::ptr::read(&no_drop.handles);
            let channel = std::ptr::read(&no_drop.channel);
            let state = std::ptr::read(&no_drop.state);
            (channel, handle.take().unwrap(), state)
        }
    }

    /// Split the child into it's parts.
    ///
    /// This will not run the destructor, and therefore the child will not be notified.
    pub fn into_parts(self) -> (Vec<JoinHandle<T>>, SupervisionState) {
        let (_a, b, c) = self._into_parts();
        (b, c)
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

    pub fn abort(&mut self) {
        self.state.status = SupervisionStatus::Aborted;
        for handle in self.handles.as_ref().unwrap() {
            handle.abort()
        }
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
    pub fn attach(&mut self) -> Result<bool, AlreadyAborted> {
        self.state.attach()
    }

    /// Detach the process. Returns true if the process was attached.
    /// before this. Returns an error if the process has already been aborted.
    pub fn detach(&mut self) -> Result<bool, AlreadyAborted> {
        self.state.detach()
    }

    /// Set the abort-timer of the processes linked to this channel.
    /// Returns the old abort-timer.
    pub fn set_abort_timer(&mut self, timer: Duration) -> Duration {
        self.state.set_abort_timer(timer)
    }

    /// Get a reference to the current supervision-state
    pub fn state(&self) -> &SupervisionState {
        &self.state
    }

    /// Whether all inboxes linked to this channel have exited.
    pub fn has_exited(&self) -> bool {
        self.inbox_count() == 0
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
        if self.state.status == SupervisionStatus::Attached && self.inbox_count() != 0 {
            if self.state.abort_timer.is_zero() {
                self.abort();
            } else {
                self.halt_all();
                let handles = self.handles.take().unwrap();
                let timer = self.state.abort_timer;
                tokio::task::spawn(async move {
                    tokio::time::sleep(timer).await;
                    for handle in handles {
                        handle.abort()
                    }
                });
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
        self.message_count()
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
