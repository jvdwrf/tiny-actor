use event_listener::EventListener;
use futures::{stream::FusedStream, FutureExt, Stream};

use crate::*;
use std::{fmt::Debug, sync::Arc};

/// An Inbox is a non clone-able receiver part of a channel.
///
/// An Inbox is mostly used to receive messages, with [Inbox::recv], [Inbox::try_recv] or
/// [futures::Stream].
#[derive(Debug)]
pub struct Inbox<M> {
    // The underlying channel
    channel: Arc<Channel<M>>,
    // Whether the inbox has signaled halt yet
    signaled_halt: bool,
    // The recv_listener for streams and Rcv
    recv_listener: Option<EventListener>,
}

impl<M> Inbox<M> {
    /// This does not increment the inbox_count.
    pub(crate) fn from_channel(channel: Arc<Channel<M>>) -> Self {
        Inbox {
            channel,
            signaled_halt: false,
            recv_listener: None,
        }
    }

    /// Attempt to receive a message from the [Inbox]. If there is no message, this
    /// returns `None`.
    pub fn try_recv(&mut self) -> Result<M, TryRecvError> {
        self.channel.try_recv(&mut self.signaled_halt)
    }

    /// Wait until there is a message in the [Inbox], or until the channel is closed.
    pub fn recv(&mut self) -> RecvFut<'_, M> {
        self.channel
            .recv(&mut self.signaled_halt, &mut self.recv_listener)
    }

    /// Get a new [Address] to the [Channel].
    pub fn get_address(&self) -> Address<Channel<M>> {
        self.channel.add_address();
        Address::from_channel(self.channel.clone())
    }

    gen::send_methods!();
    gen::dyn_channel_methods!();
}

impl<M> Stream for Inbox<M> {
    type Item = Result<M, HaltedError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.recv().poll_unpin(cx).map(|res| match res {
            Ok(msg) => Some(Ok(msg)),
            Err(e) => match e {
                RecvError::Halted => Some(Err(HaltedError)),
                RecvError::ClosedAndEmpty => None,
            },
        })
    }
}

impl<M> FusedStream for Inbox<M> {
    fn is_terminated(&self) -> bool {
        self.channel.is_closed()
    }
}

impl<M> Drop for Inbox<M> {
    fn drop(&mut self) {
        self.channel.remove_inbox();
    }
}

#[cfg(feature = "internals")]
impl<M> Inbox<M> {
    pub fn channel_ref(&self) -> &Channel<M> {
        &self.channel
    }
}
