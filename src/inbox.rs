use crate::*;
use event_listener as el;
use futures::{Future, FutureExt, Stream};
use std::{
    fmt::Debug,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

pub struct Inbox<T> {
    // The underlying channel
    channel: Arc<Channel<T>>,
    // The listener for receiving events
    listener: Option<el::EventListener>,
    // Whether this inbox has signaled halt yet
    signaled_halt: bool,
}

impl<T> Inbox<T> {
    pub(crate) fn from_channel(channel: Arc<Channel<T>>) -> Self {
        Inbox {
            channel,
            listener: None,
            signaled_halt: false,
        }
    }

    /// Create a new inbox from a channel. Returns `None` if the `Channel` has exited.
    pub(crate) fn try_from_channel(channel: Arc<Channel<T>>) -> Option<Self> {
        match channel.try_add_inbox() {
            true => Some(Self {
                channel,
                listener: None,
                signaled_halt: false,
            }),
            false => None,
        }
    }

    /// This will attempt to receive a message from the inbox.
    pub fn try_recv(&mut self) -> Result<Option<T>, RecvError> {
        // If we have not yet signaled for supervision yet, and there is a signal ready.
        if !self.signaled_halt && self.channel.inbox_should_halt() {
            self.signaled_halt = true;
            Err(RecvError::Halted)

        // Otherwise, we can just try to receive a message.
        } else {
            self.channel
                .take_next_msg()
                .map_err(|()| RecvError::ClosedAndEmpty)
        }
    }

    /// This will wait for a message in the inbox.
    pub fn recv(&mut self) -> Rcv<'_, T> {
        Rcv { inbox: self }
    }

    /// Try to send a message into the channel.
    pub fn try_send(&self, msg: T) -> Result<(), TrySendError<T>> {
        Ok(self.channel.push_msg(msg)?)
    }

    /// Send a message into the channel.
    pub fn send(&self, msg: T) -> Snd<'_, T> {
        Snd::new(&self.channel, msg)
    }

    /// Receive a message while blocking the scheduler.
    pub fn recv_blocking(&mut self) -> Result<T, RecvError> {
        loop {
            match self.try_recv() {
                Ok(None) => (),
                Ok(Some(msg)) => {
                    self.listener = None;
                    return Ok(msg);
                }
                Err(signal) => {
                    self.listener = None;
                    match signal {
                        RecvError::Halted => return Err(RecvError::Halted),
                        RecvError::ClosedAndEmpty => return Err(RecvError::ClosedAndEmpty),
                    }
                }
            }

            self.channel.recv_listener().wait();
        }
    }

    /// Close the channel.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt all inboxes linked to the channel.
    pub fn halt(&self) {
        self.channel.halt_n(u32::MAX)
    }

    /// Halt n inboxes linked to the channel.
    pub fn halt_some(&self, n: u32) {
        self.channel.halt_n(n)
    }

    /// Get the amount of inboxes linked to the channel.
    pub fn inbox_count(&self) -> usize {
        self.channel.inbox_count()
    }

    /// Get the amount of messages linked to the channel.
    pub fn message_count(&self) -> usize {
        self.channel.msg_count()
    }

    /// Get the amount of addresses linked to the channel.
    pub fn address_count(&self) -> usize {
        self.channel.address_count()
    }

    /// Whether the channel has been closed.
    pub fn is_closed(&self) -> bool {
        self.channel.is_closed()
    }

    /// Clone, for use within the crate.
    pub(crate) fn clone_inbox(&self) -> Self {
        self.channel.add_inbox();
        Self {
            channel: self.channel.clone(),
            listener: None,
            signaled_halt: self.signaled_halt.clone(),
        }
    }
}

impl<T> Stream for Inbox<T> {
    type Item = Result<T, Halted>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        loop {
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

            if self.listener.is_none() {
                self.listener = Some(self.channel.recv_listener())
            }

            match self.listener.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => {}
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<T> Drop for Inbox<T> {
    fn drop(&mut self) {
        self.channel.remove_inbox()
    }
}

impl<T> Debug for Inbox<T> {
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

pub struct Rcv<'a, T> {
    inbox: &'a mut Inbox<T>,
}

impl<'a, T> Unpin for Rcv<'a, T> {}

impl<'a, T> Future for Rcv<'a, T> {
    type Output = Result<T, RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        loop {
            match self.inbox.try_recv() {
                Ok(None) => (),
                Ok(Some(msg)) => {
                    return Poll::Ready(Ok(msg));
                }
                Err(signal) => match signal {
                    RecvError::Halted => return Poll::Ready(Err(RecvError::Halted)),
                    RecvError::ClosedAndEmpty => {
                        return Poll::Ready(Err(RecvError::ClosedAndEmpty))
                    }
                },
            }

            if self.inbox.listener.is_none() {
                self.inbox.listener = Some(self.inbox.channel.recv_listener())
            }

            match self.inbox.listener.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => self.inbox.listener = None,
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<'a, T> Debug for Rcv<'a, T> {
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
