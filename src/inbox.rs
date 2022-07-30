use crate::*;
use event_listener as el;
use futures::{Future, FutureExt, Stream};
use std::{
    fmt::Debug,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// An `Inbox` is a receiver-part of the `Channel`, and is primarily used to take messages out
/// of the `Channel`. `Inbox`es can only be created by spawning new `Process`es and should stay
/// coupled to the `tokio::task` they were spawned with. Therefore, an `Inbox` should only be
/// dropped when the `tokio::task` is exiting.
pub struct Inbox<M> {
    // The underlying channel
    channel: Arc<Channel<M>>,
    // The listener for receiving events
    listener: Option<el::EventListener>,
    // Whether this inbox has signaled halt yet
    signaled_halt: bool,
}

impl<M> Inbox<M> {
    /// Create the inbox from a channel.
    ///
    /// This does not increment the inbox_count.
    pub(crate) fn from_channel(channel: Arc<Channel<M>>) -> Self {
        Inbox {
            channel,
            listener: None,
            signaled_halt: false,
        }
    }

    pub(crate) fn channel(&self) -> &Arc<Channel<M>> {
        &self.channel
    }

    /// Create a new inbox from a channel. Returns `None` if the `Channel` has exited.
    ///  
    /// This increments the inbox-count automatically
    pub(crate) fn try_create(channel: Arc<Channel<M>>) -> Option<Self> {
        match channel.try_add_inbox() {
            Ok(_) => Some(Self {
                channel,
                listener: None,
                signaled_halt: false,
            }),
            Err(()) => None,
        }
    }

    /// This will attempt to receive a message from the [Inbox]. If there is no message, this
    /// will return `None`.
    pub fn try_recv(&mut self) -> Result<Option<M>, RecvError> {
        // If we have not yet signaled halt yet, check if we should
        if !self.signaled_halt && self.channel.inbox_should_halt() {
            self.signaled_halt = true;
            Err(RecvError::Halted)

        // Otherwise, just take a msg from the channel
        } else {
            self.channel
                .take_next_msg()
                .map_err(|()| RecvError::ClosedAndEmpty)
        }
    }

    /// Wait until there is a message in the [Inbox].
    pub fn recv(&mut self) -> Rcv<'_, M> {
        Rcv { inbox: self }
    }

    /// Same as [Address::try_send]
    pub fn try_send(&self, msg: M) -> Result<(), TrySendError<M>> {
        try_send(&self.channel, msg)
    }

    /// Same as [Address::send_now]
    pub fn send_now(&self, msg: M) -> Result<(), TrySendError<M>> {
        send_now(&self.channel, msg)
    }

    /// Same as [Address::send]
    pub fn send(&self, msg: M) -> Snd<'_, M> {
        send(&self.channel, msg)
    }

    /// Same as [Address::send_blocking]
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

    /// Whether the channel is `closed`.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Get the [Capacity] of the `Channel`.
    pub fn capacity(&self) -> &Capacity {
        self.channel.capacity()
    }

    /// Private clone
    pub(crate) fn _clone(&self) -> Self {
        self.channel.add_inbox();
        Self {
            channel: self.channel.clone(),
            listener: None,
            signaled_halt: self.signaled_halt.clone(),
        }
    }
}

// It should be fine to share the same event-listener between inbox-stream and 
// rcv-future, as long as both clean up properly after returning Poll::Ready.
// (Always remove the event-listener from the Option)
impl<M> Stream for Inbox<M> {
    type Item = Result<M, Halted>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
            // Attempt to receive a message, and return if necessary
            match self.try_recv() {
                Ok(None) => (),
                Ok(Some(msg)) => {
                    self.listener = None;
                    return Poll::Ready(Some(Ok(msg)));
                }
                Err(signal) => {
                    self.listener = None;
                    match signal {
                        RecvError::Halted => return Poll::Ready(Some(Err(Halted))),
                        RecvError::ClosedAndEmpty => return Poll::Ready(None),
                    }
                }
            }

            // Otherwise, acquire a listener, if we don't have one yet
            if self.listener.is_none() {
                self.listener = Some(self.channel.get_recv_listener())
            }

            // And poll the future
            match self.listener.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => {}
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<M> Drop for Inbox<M> {
    fn drop(&mut self) {
        self.channel.remove_inbox();
    }
}

impl<M> Debug for Inbox<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox")
            .field("listener", &self.listener)
            .field("signaled_halt", &self.signaled_halt)
            .finish()
    }
}

//------------------------------------------------------------------------------------------------
//  Rcv
//------------------------------------------------------------------------------------------------

/// A future returned by receiving messages from an `Inbox`.
///
/// This can be `.await`-ed to get the message from the `Inbox`.
pub struct Rcv<'a, M> {
    inbox: &'a mut Inbox<M>,
}

impl<'a, M> Unpin for Rcv<'a, M> {}

impl<'a, M> Future for Rcv<'a, M> {
    type Output = Result<M, RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            // Attempt to receive a message, and return if necessary
            match self.inbox.try_recv() {
                Ok(None) => (),
                Ok(Some(msg)) => {
                    self.inbox.listener = None;
                    return Poll::Ready(Ok(msg));
                }
                Err(signal) => {
                    self.inbox.listener = None;
                    match signal {
                        RecvError::Halted => return Poll::Ready(Err(RecvError::Halted)),
                        RecvError::ClosedAndEmpty => {
                            return Poll::Ready(Err(RecvError::ClosedAndEmpty))
                        }
                    }
                }
            }

            // Otherwise, acquire a listener, if we don't have one yet
            if self.inbox.listener.is_none() {
                self.inbox.listener = Some(self.inbox.channel.get_recv_listener())
            }

            // And poll the future
            match self.inbox.listener.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => self.inbox.listener = None,
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<'a, M> Debug for Rcv<'a, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rcv").field("inbox", &self.inbox).finish()
    }
}

//------------------------------------------------------------------------------------------------
//  Errors
//------------------------------------------------------------------------------------------------

/// This Inbox has been halted.
#[derive(Debug, thiserror::Error)]
#[error("This inbox has been halted")]
pub struct Halted;

/// Error returned when receiving a message from an inbox.
/// Reasons can be:
/// * `Halted`: This Inbox has been halted and should now exit.
/// * `ClosedAndEmpty`: This Inbox is closed and empty, it can no longer receive new messages.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RecvError {
    /// This inbox has been halted and should now exit.
    Halted,
    /// This inbox has been closed, and contains no more messages. It was closed either because
    /// all addresses have been dropped, or because it was manually closed.
    ClosedAndEmpty,
}
