use crate::*;
use std::{fmt::Debug, sync::Arc};

/// An inbox, used to receive messages from a channel.
#[derive(Debug)]
pub struct Inbox<M> {
    // The underlying channel
    channel: Arc<Channel<M>>,
    signaled_halt: bool,
}

impl<M> Inbox<M> {
    /// This does not increment the inbox_count.
    pub(crate) fn from_channel(channel: Arc<Channel<M>>) -> Self {
        Inbox {
            channel,
            signaled_halt: false,
        }
    }

    /// Attempt to receive a message from the [Inbox]. If there is no message, this
    /// returns `None`.
    pub fn try_recv(&mut self) -> Result<Option<M>, RecvError> {
        self.channel.try_recv(&mut self.signaled_halt)
    }

    /// Wait until there is a message in the [Inbox], or until the channel is closed.
    pub fn recv(&mut self) -> Rcv<'_, M> {
        self.channel.recv(&mut self.signaled_halt)
    }

    /// Get a new [Address] to the [Channel].
    pub fn get_address(&self) -> Address<Channel<M>> {
        self.channel.add_address();
        Address::from_channel(self.channel.clone())
    }

    gen::send_methods!();
    gen::dyn_channel_methods!();
}

impl<M> Drop for Inbox<M> {
    fn drop(&mut self) {
        self.channel.remove_inbox();
    }
}
