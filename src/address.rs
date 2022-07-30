use std::sync::Arc;

use crate::*;

/// An `Address` is the cloneable sender-part of a `Channel`, and is primarily used to send messages
/// to the `Actor`. When all `Address`es are dropped, the `Channel` is closed automatically. `Address`es
/// can be awaited, which will return when the `Actor` has exited.
#[must_use = "If all Addresses are dropped, the Channel is closed."]
pub struct Address<M> {
    channel: Arc<Channel<M>>,
}

impl<M> Address<M> {
    /// Create a new address from a Channel.
    ///
    /// This does not increment the address-count.
    pub(crate) fn from_channel(channel: Arc<Channel<M>>) -> Self {
        Self { channel }
    }

    /// Attempt to send a message into the `Channel`, this can always fail if the `Channel` is
    /// `closed`.
    ///
    /// * `unbounded`: If the [BackPressure] gives a timeout, this method will fail.
    /// * `bounded`: If the `Channel` is full, this method will fail.
    ///
    /// This method is exactly the same as [Address::send_now] for bounded `Channels`.
    pub fn try_send(&self, msg: M) -> Result<(), TrySendError<M>> {
        self.channel.try_send(msg)
    }

    /// Attempt to send a message into the `Channel`, this can always fail if the `Channel` is
    /// `closed`.
    ///
    /// * `unbounded`: This method ignores any [BackPressure] timeout.
    /// * `bounded`: If the `Channel` is full, this method will fail.
    ///
    /// This method is exactly the same as [Address::try_send] for bounded `Channels`.
    pub fn send_now(&self, msg: M) -> Result<(), TrySendError<M>> {
        self.channel.send_now(msg)
    }

    /// Attempt to send a message into the `Channel`, this can always fail if the `Channel` is
    /// `closed`.
    ///
    /// * `unbounded` -> If [BackPressure] gives a timeout, this method will wait for it to be
    /// finished.
    /// * `bounded` -> If the `Channel` is full, this method waits for space to be available.
    pub fn send(&self, msg: M) -> Snd<'_, M> {
        self.channel.send(msg)
    }

    /// Same as [Address::send] except it blocks the current OS-thread.
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

    /// Whether the `Channel` is closed.
    pub fn is_closed(&self) -> bool {
        self.channel.closed()
    }

    /// Whether all [Inboxes](Inbox) have exited.
    pub fn exited(&self) -> bool {
        self.inbox_count() == 0
    }

    /// Get the [Capacity] of the `Channel`.
    pub fn capacity(&self) -> &Capacity {
        self.channel.capacity()
    }

    pub fn exit(&self) -> Exit<'_>
    where
        M: Send + 'static,
    {
        self.channel.exit()
    }
}

impl<M> Clone for Address<M> {
    fn clone(&self) -> Self {
        self.channel.add_sender();
        Self {
            channel: self.channel.clone(),
        }
    }
}

impl<M> Drop for Address<M> {
    fn drop(&mut self) {
        if self.channel.remove_sender() == 1 {
            self.close();
        }
    }
}
