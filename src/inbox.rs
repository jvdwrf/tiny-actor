use crate::*;
use std::{fmt::Debug, sync::Arc};

/// An `Inbox` is a receiver-part of the `Channel`, and is primarily used to take messages out
/// of the `Channel`. `Inbox`es can only be created by spawning new `Process`es and should stay
/// coupled to the `tokio::task` they were spawned with. Therefore, an `Inbox` should only be
/// dropped when the `tokio::task` is exiting.
pub struct Inbox<M> {
    // The underlying channel
    channel: Arc<Channel<M>>,
    // The listener for streaming events
    // listener: Option<EventListener>,
    // Whether this inbox has signaled halt yet
    check_for_halt: bool,
}

impl<M> Inbox<M> {
    /// Create the inbox from a channel.
    ///
    /// This does not increment the inbox_count.
    pub(crate) fn from_channel(channel: Arc<Channel<M>>) -> Self {
        Inbox {
            channel,
            // listener: None,
            check_for_halt: true,
        }
    }

    /// Create a new inbox from a channel. Returns `None` if the `Channel` has exited.
    ///  
    /// This increments the inbox-count automatically
    pub(crate) fn try_create(channel: Arc<Channel<M>>) -> Option<Self> {
        match channel.try_add_receiver() {
            Ok(_) => Some(Self {
                channel,
                // listener: None,
                check_for_halt: true,
            }),
            Err(()) => None,
        }
    }

    /// This will attempt to receive a message from the [Inbox]. If there is no message, this
    /// will return `None`.
    pub fn try_recv(&mut self) -> Result<M, TryRecvError> {
        self.channel.try_recv(&mut self.check_for_halt)
    }

    /// Wait until there is a message in the [Inbox].
    pub fn recv(&mut self) -> Rcv<'_, M>
    where
        M: Send + 'static,
    {
        self.channel.recv(&mut self.check_for_halt)
    }

    /// Same as [Address::try_send]
    pub fn try_send(&self, msg: M) -> Result<(), TrySendError<M>> {
        self.channel.try_send(msg)
    }

    /// Same as [Address::send_now]
    pub fn send_now(&self, msg: M) -> Result<(), TrySendError<M>> {
        self.channel.send_now(msg)
    }

    /// Same as [Address::send]
    pub fn send(&self, msg: M) -> Snd<'_, M>
    where
        M: Send + 'static,
    {
        self.channel.send(msg)
    }

    /// Same as [Address::send_blocking]
    pub fn send_blocking(&self, msg: M) -> Result<(), SendError<M>> {
        self.channel.send_blocking(msg)
    }

    /// Close the channel.
    pub fn close(&self) -> bool {
        self.channel.close()
    }

    /// Halt all `Processes`.
    pub fn halt_all(&self) {
        self.channel.halt(u32::MAX)
    }

    /// Halt n `Processes`.
    pub fn halt_some(&self, n: u32) {
        self.channel.halt(n)
    }

    /// Get the current amount of [Inboxes](Inbox).
    pub fn inbox_count(&self) -> usize {
        self.channel.receiver_count()
    }

    /// Get the amount of messages in the `Channel`.
    pub fn msg_count(&self) -> usize {
        self.channel.msg_count()
    }

    /// Get the current amount of [Addresses](Address).
    pub fn address_count(&self) -> usize {
        self.channel.sender_count()
    }

    /// Whether the channel is `closed`.
    pub fn is_closed(&self) -> bool {
        self.channel.closed()
    }

    /// Get the [Capacity] of the `Channel`.
    pub fn capacity(&self) -> &Capacity {
        self.channel.capacity()
    }

    /// Private clone
    pub(crate) fn _clone(&self) -> Self {
        self.channel.add_receiver();
        Self {
            channel: self.channel.clone(),
            check_for_halt: true,
        }
    }
}

impl<M> Drop for Inbox<M> {
    fn drop(&mut self) {
        self.channel.remove_receiver()
    }
}

impl<M> Debug for Inbox<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inbox")
            .field("signaled_halt", &self.check_for_halt)
            .finish()
    }
}