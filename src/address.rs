use crate::*;
use concurrent_queue::PushError;
use event_listener::EventListener;
use futures::{Future, FutureExt};
use std::{
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tokio::time::Sleep;

/// An `Address` is the cloneable sender-part of a `Channel`, and is primarily used to send messages
/// to the `Actor`. When all `Address`es are dropped, the `Channel` is closed automatically. `Address`es
/// can be awaited, which will return when the `Actor` has exited.
pub struct Address<M> {
    channel: Arc<Channel<M>>,
    exit_listener: Option<EventListener>,
}

impl<M> Address<M> {
    /// Create a new address from a Channel.
    ///
    /// This does not increment the address-count.
    pub(crate) fn from_channel(channel: Arc<Channel<M>>) -> Self {
        Self {
            channel,
            exit_listener: None,
        }
    }

    pub(crate) fn channel(&self) -> &Arc<Channel<M>> {
        &self.channel
    }

    /// Attempt to send a message into the `Channel`, this can always fail if the `Channel` is
    /// `closed`.
    ///
    /// * `unbounded`: If the [BackPressure] gives a timeout, this method will fail.
    /// * `bounded`: If the `Channel` is full, this method will fail.
    ///
    /// This method is exactly the same as [Address::send_now] for bounded `Channels`.
    pub fn try_send(&self, msg: M) -> Result<(), TrySendError<M>> {
        try_send(&self.channel, msg)
    }

    /// Attempt to send a message into the `Channel`, this can always fail if the `Channel` is
    /// `closed`.
    ///
    /// * `unbounded`: This method ignores any [BackPressure] timeout.
    /// * `bounded`: If the `Channel` is full, this method will fail.
    ///
    /// This method is exactly the same as [Address::try_send] for bounded `Channels`.
    pub fn send_now(&self, msg: M) -> Result<(), TrySendError<M>> {
        send_now(&self.channel, msg)
    }

    /// Attempt to send a message into the `Channel`, this can always fail if the `Channel` is
    /// `closed`.
    ///
    /// * `unbounded` -> If [BackPressure] gives a timeout, this method will wait for it to be
    /// finished.
    /// * `bounded` -> If the `Channel` is full, this method waits for space to be available.
    pub fn send(&self, msg: M) -> Snd<'_, M> {
        send(&self.channel, msg)
    }

    /// Same as [Address::send] except it blocks the current OS-thread.
    pub fn send_blocking(&self, msg: M) -> Result<(), SendError<M>> {
        send_blocking(&self.channel, msg)
    }

    /// Close the channel.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt all `Processes`.
    pub fn halt_all(&self) {
        self.channel.halt_n(u32::MAX)
    }

    /// Halt n `Processes`.
    pub fn halt_some(&self, n: u32) {
        self.channel.halt_n(n)
    }

    /// Get the current amount of [Inboxes](Inbox).
    pub fn inbox_count(&self) -> usize {
        self.channel.inbox_count()
    }

    /// Get the amount of messages in the `Channel`.
    pub fn msg_count(&self) -> usize {
        self.channel.msg_count()
    }

    /// Get the current amount of [Addresses](Address).
    pub fn address_count(&self) -> usize {
        self.channel.address_count()
    }

    /// Whether the `Channel` is closed.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Whether all [Inboxes](Inbox) have exited.
    pub fn exited(&self) -> bool {
        self.channel.inboxes_exited()
    }

    /// Get the [Capacity] of the [Channel].
    pub fn capacity(&self) -> &Capacity {
        self.channel.capacity()
    }

    /// Get an [Address] to the [Channel].
    pub fn get_address(&self) -> Address<M> {
        self.channel.add_address();
        Address::from_channel(self.channel.clone())
    }
}

impl<M> Unpin for Address<M> {}

impl<M> Future for Address<M> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.channel.inboxes_exited() {
            Poll::Ready(())
        } else {
            if self.exit_listener.is_none() {
                self.exit_listener = Some(self.channel.get_exit_listener())
            }
            match self.exit_listener.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => {
                    assert!(self.exited());
                    self.exit_listener = None;
                    Poll::Ready(())
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

impl<M> Clone for Address<M> {
    fn clone(&self) -> Self {
        self.channel.add_address();
        Self {
            channel: self.channel.clone(),
            exit_listener: None,
        }
    }
}

impl<M> Drop for Address<M> {
    fn drop(&mut self) {
        self.channel.remove_address()
    }
}

//------------------------------------------------------------------------------------------------
//  Sending
//------------------------------------------------------------------------------------------------

/// The send-future, this can be `.await`-ed to send the message.
pub struct Snd<'a, M> {
    channel: &'a Channel<M>,
    msg: Option<M>,
    fut: Option<SndFut>,
}

/// Listener for a bounded channel, sleep for an unbounded channel.
enum SndFut {
    Listener(EventListener),
    Sleep(Pin<Box<Sleep>>), // todo: can this box be removed?
}

impl<'a, M> Snd<'a, M> {
    pub(crate) fn new(channel: &'a Channel<M>, msg: M) -> Self {
        Snd {
            channel,
            msg: Some(msg),
            fut: None,
        }
    }
}

impl<'a, M> Unpin for Snd<'a, M> {}

impl<'a, M> Future for Snd<'a, M> {
    type Output = Result<(), SendError<M>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        fn bounded_send<T>(
            pin: &mut Pin<&mut Snd<'_, T>>,
            cx: &mut Context<'_>,
        ) -> Poll<Result<(), address::SendError<T>>> {
            let mut msg = pin.msg.take().unwrap();
            loop {
                // Try to send a message into the channel, and return if possible
                msg = match pin.channel.push_msg(msg) {
                    Ok(()) => {
                        return Poll::Ready(Ok(()));
                    }
                    Err(PushError::Closed(msg)) => {
                        return Poll::Ready(Err(SendError(msg)));
                    }
                    Err(PushError::Full(msg)) => msg,
                };

                // Otherwise, we create the future if it doesn't exist yet.
                if pin.fut.is_none() {
                    pin.fut = Some(SndFut::Listener(pin.channel.get_send_listener()))
                }

                if let SndFut::Listener(listener) = pin.fut.as_mut().unwrap() {
                    // Poll it once, and return if pending, otherwise we loop again.
                    match listener.poll_unpin(cx) {
                        Poll::Ready(()) => pin.fut = None,
                        Poll::Pending => {
                            pin.msg = Some(msg);
                            return Poll::Pending;
                        }
                    }
                } else {
                    unreachable!("Channel must be bounded")
                }
            }
        }

        fn push_msg_unbounded<T>(
            pin: &mut Pin<&mut Snd<'_, T>>,
        ) -> Poll<Result<(), address::SendError<T>>> {
            let msg = pin.msg.take().unwrap();
            match pin.channel.push_msg(msg) {
                Ok(()) => Poll::Ready(Ok(())),
                Err(PushError::Closed(msg)) => Poll::Ready(Err(SendError(msg))),
                Err(PushError::Full(_msg)) => unreachable!(),
            }
        }

        match self.channel.capacity() {
            Capacity::Bounded(_) => bounded_send(&mut self, cx),
            Capacity::Unbounded(backpressure) => match &mut self.fut {
                Some(SndFut::Sleep(sleep_fut)) => match sleep_fut.poll_unpin(cx) {
                    Poll::Ready(()) => {
                        self.fut = None;
                        push_msg_unbounded(&mut self)
                    }
                    Poll::Pending => Poll::Pending,
                },
                None => match backpressure.get_timeout(self.channel.msg_count()) {
                    Some(timeout) => {
                        let mut sleep_fut = Box::pin(tokio::time::sleep(timeout));
                        match sleep_fut.poll_unpin(cx) {
                            Poll::Ready(()) => push_msg_unbounded(&mut self),
                            Poll::Pending => {
                                self.fut = Some(SndFut::Sleep(sleep_fut));
                                Poll::Pending
                            }
                        }
                    }
                    None => push_msg_unbounded(&mut self),
                },
                Some(SndFut::Listener(_)) => unreachable!("Channel must be unbounded"),
            },
        }
    }
}

/// Construct a new `Snd`-future
///
/// See [Address::send]
pub(crate) fn send<M>(channel: &Channel<M>, msg: M) -> Snd<'_, M> {
    Snd::new(&channel, msg)
}

/// Send a message at this moment. This ignores any [BackPressure].
///
/// See [Address::send_now]
pub(crate) fn send_now<M>(channel: &Channel<M>, msg: M) -> Result<(), TrySendError<M>> {
    Ok(channel.push_msg(msg)?)
}

/// Send a message at this moment. This fails if [BackPressure] gives a timeout.
///
/// See [Address::send_now]
pub(crate) fn try_send<M>(channel: &Channel<M>, msg: M) -> Result<(), TrySendError<M>> {
    match channel.capacity() {
        Capacity::Bounded(_) => Ok(channel.push_msg(msg)?),
        Capacity::Unbounded(backoff) => match backoff.get_timeout(channel.msg_count()) {
            Some(_) => Err(TrySendError::Full(msg)),
            None => Ok(channel.push_msg(msg)?),
        },
    }
}

/// Same as `send` except blocking.
pub(crate) fn send_blocking<M>(channel: &Channel<M>, mut msg: M) -> Result<(), SendError<M>> {
    match channel.capacity() {
        Capacity::Bounded(_) => loop {
            msg = match channel.push_msg(msg) {
                Ok(()) => {
                    return Ok(());
                }
                Err(PushError::Closed(msg)) => {
                    return Err(SendError(msg));
                }
                Err(PushError::Full(msg)) => msg,
            };

            channel.get_send_listener().wait();
        },
        Capacity::Unbounded(backoff) => {
            let timeout = backoff.get_timeout(channel.msg_count());
            if let Some(timeout) = timeout {
                std::thread::sleep(timeout);
            }
            channel.push_msg(msg).map_err(|e| match e {
                PushError::Full(_) => unreachable!("unbounded"),
                PushError::Closed(msg) => SendError(msg),
            })
        }
    }
}

/// An error returned when trying to send a message into a `Channel`, but not waiting for space.
///
/// This can be either because the `Channel` is closed, or because it is full.
#[derive(Debug, Clone)]
pub enum TrySendError<M> {
    Closed(M),
    Full(M),
}

impl<M> From<PushError<M>> for TrySendError<M> {
    fn from(e: PushError<M>) -> Self {
        match e {
            PushError::Full(msg) => Self::Full(msg),
            PushError::Closed(msg) => Self::Closed(msg),
        }
    }
}

/// An error returned when sending a message into a `Channel` because the `Channel` is closed.
#[derive(Debug, Clone)]
pub struct SendError<M>(pub M);
