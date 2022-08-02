use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::*;
use event_listener::EventListener;
use futures::{Future, FutureExt};

impl<M> Actor<M> {
    /// This will attempt to receive a message from the [Inbox]. If there is no message, this
    /// will return `None`.
    pub fn try_recv(&self, signaled_halt: &mut bool) -> Result<Option<M>, RecvError> {
        if !*signaled_halt && self.inbox_should_halt() {
            *signaled_halt = true;
            Err(RecvError::Halted)
        } else {
            self.take_next_msg().map_err(|()| RecvError::ClosedAndEmpty)
        }
    }

    /// Wait until there is a message in the [Inbox].
    pub fn recv<'a>(
        &'a self,
        signaled_halt: &'a mut bool,
        recv_listener: &'a mut Option<EventListener>,
    ) -> Rcv<'a, M> {
        Rcv {
            channel: self,
            signaled_halt,
            listener: recv_listener,
        }
    }
}

pub(crate) fn poll_recv<M>(
    channel: &Actor<M>,
    signaled_halt: &mut bool,
    listener: &mut Option<EventListener>,
    cx: &mut Context<'_>,
) -> Poll<Result<M, RecvError>> {
    loop {
        // Attempt to receive a message, and return if necessary
        match channel.try_recv(signaled_halt) {
            Ok(None) => (),
            Ok(Some(msg)) => {
                *listener = None;
                return Poll::Ready(Ok(msg));
            }
            Err(signal) => {
                *listener = None;
                match signal {
                    RecvError::Halted => return Poll::Ready(Err(RecvError::Halted)),
                    RecvError::ClosedAndEmpty => {
                        return Poll::Ready(Err(RecvError::ClosedAndEmpty))
                    }
                }
            }
        }

        // Otherwise, acquire a listener, if we don't have one yet
        if listener.is_none() {
            *listener = Some(channel.get_recv_listener())
        }

        // And poll the future
        match listener.as_mut().unwrap().poll_unpin(cx) {
            Poll::Ready(()) => *listener = None,
            Poll::Pending => return Poll::Pending,
        }
    }
}

/// A future returned by receiving messages from an `Inbox`.
///
/// This can be `.await`-ed to get the message from the `Inbox`.
pub struct Rcv<'a, M> {
    channel: &'a Actor<M>,
    signaled_halt: &'a mut bool,
    listener: &'a mut Option<EventListener>,
}

impl<'a, M> Unpin for Rcv<'a, M> {}

impl<'a, M> Future for Rcv<'a, M> {
    type Output = Result<M, RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut_self = &mut *self.as_mut();
        poll_recv(
            mut_self.channel,
            mut_self.signaled_halt,
            mut_self.listener,
            cx,
        )
    }
}

impl<'a, M> std::fmt::Debug for Rcv<'a, M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Rcv")
            // .field("channel", &self.channel)
            .field("signaled_halt", &self.signaled_halt)
            .finish()
    }
}

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
