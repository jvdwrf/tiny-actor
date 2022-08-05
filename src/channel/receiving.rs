use std::{
    pin::Pin,
    task::{Context, Poll},
};

use crate::*;
use event_listener::EventListener;
use futures::{pin_mut, Future, FutureExt};
use tokio::task::yield_now;

impl<M> Channel<M> {
    /// This will attempt to receive a message from the [Inbox]. If there is no message, this
    /// will return `None`.
    pub fn try_recv(&self, signaled_halt: &mut bool) -> Result<Option<M>, RecvError> {
        if !(*signaled_halt) && self.inbox_should_halt() {
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
        listener: &'a mut Option<EventListener>,
    ) -> Rcv<'a, M> {
        Rcv {
            channel: self,
            signaled_halt,
            listener,
        }
    }

    fn poll_try_recv(
        &self,
        signaled_halt: &mut bool,
        listener: &mut Option<EventListener>,
    ) -> Option<Result<M, RecvError>> {
        match self.try_recv(signaled_halt) {
            Ok(None) => None,
            Ok(Some(msg)) => {
                *listener = None;
                Some(Ok(msg))
            }
            Err(signal) => {
                *listener = None;
                match signal {
                    RecvError::Halted => Some(Err(RecvError::Halted)),
                    RecvError::ClosedAndEmpty => Some(Err(RecvError::ClosedAndEmpty)),
                }
            }
        }
    }
}

/// A future returned by receiving messages from an [Inbox].
///
/// This can be awaited or streamed to get the messages.
#[derive(Debug)]
pub struct Rcv<'a, M> {
    channel: &'a Channel<M>,
    signaled_halt: &'a mut bool,
    listener: &'a mut Option<EventListener>,
}

impl<'a, M> Unpin for Rcv<'a, M> {}

impl<'a, M> Future for Rcv<'a, M> {
    type Output = Result<M, RecvError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            channel,
            signaled_halt,
            listener,
        } = &mut *self;

        // First try to receive once, and yield if successful
        if let Some(res) = channel.poll_try_recv(signaled_halt, listener) {
            let fut = yield_now();
            pin_mut!(fut);
            let _ = fut.poll(cx);
            return Poll::Ready(res);
        }

        loop {
            // Otherwise, acquire a listener, if we don't have one yet
            if listener.is_none() {
                **listener = Some(channel.get_recv_listener())
            }

            // Attempt to receive a message, and return if ready
            if let Some(res) = channel.poll_try_recv(signaled_halt, listener) {
                return Poll::Ready(res);
            }

            // And poll the listener
            match listener.as_mut().unwrap().poll_unpin(cx) {
                Poll::Ready(()) => {
                    **listener = None;
                    // Attempt to receive a message, and return if ready
                    if let Some(res) = channel.poll_try_recv(signaled_halt, listener) {
                        return Poll::Ready(res);
                    }
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<'a, M> Drop for Rcv<'a, M> {
    fn drop(&mut self) {
        *self.listener = None;
    }
}