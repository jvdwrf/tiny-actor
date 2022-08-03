macro_rules! any_channel_methods {
    () => {
        /// Close the [Actor].
        pub fn close(&self) -> bool {
            self.channel.close()
        }

        /// Halt all processes.
        pub fn halt(&self) {
            self.channel.halt_some(u32::MAX)
        }

        /// Halt n processes.
        pub fn halt_some(&self, n: u32) {
            self.channel.halt_some(n)
        }

        /// Get the amount of [Inboxes](Inbox).
        pub fn inbox_count(&self) -> usize {
            self.channel.inbox_count()
        }

        /// Get the amount of messages in the [Actor].
        pub fn msg_count(&self) -> usize {
            self.channel.msg_count()
        }

        /// Get the amount of [Addresses](Address) of the [Actor].
        pub fn address_count(&self) -> usize {
            self.channel.address_count()
        }

        /// Whether the [Actor] is closed.
        pub fn is_closed(&self) -> bool {
            self.channel.is_closed()
        }

        /// Get the [Capacity] of the [Actor]. This cannot be changed.
        pub fn capacity(&self) -> &Capacity {
            self.channel.capacity()
        }

        /// Whether all [Inboxes](Inbox) have been dropped.
        pub fn has_exited(&self) -> bool {
            self.channel.has_exited()
        }

        /// Get the actor_id of the [Actor].
        pub fn actor_id(&self) -> u64 {
            self.channel.actor_id()
        }
    };
}
pub(crate) use any_channel_methods;

macro_rules! send_methods {
    () => {
        /// Attempt to send a message into the [Actor].
        ///
        /// In the case of an `unbounded` [Actor], when [BackPressure] returns a timeout, this will fail.
        /// In the case of a `bounded` [Actor], when it is full, this method will fail.
        ///
        /// For `bounded` channels, this method is the same as [send_now](Address::send_now).
        pub fn try_send<M>(&self, msg: M) -> Result<M::Returns, TrySendError<P>>
        where
            P: Accepts<M>,
            M: Message,
        {
            self.channel.try_send(msg)
        }

        /// Attempt to send a message into the [Actor].
        ///
        /// In the case of an `unbounded` [Actor], any [BackPressure] is ignored.
        /// In the case of a `bounded` [Actor], when it is full, this method will fail.
        ///
        /// For `bounded` channels, this method is the same as [try_send](Address::send_now).
        pub fn send_now<M>(&self, msg: M) -> Result<M::Returns, TrySendError<P>>
        where
            P: Accepts<M>,
            M: Message,
        {
            self.channel.send_now(msg)
        }

        /// Attempt to send a message into the [Actor].
        ///
        /// In the case of an `unbounded` [Actor], when [BackPressure] returns a timeout, this will
        /// wait and then send the message.
        /// In the case of a `bounded` [Actor], when it is full, this will wait untill space is
        /// available.
        pub fn send<M>(&self, msg: M) -> Snd<'_, M, P>
        where
            P: Accepts<M>,
            M: Message,
        {
            self.channel.send(msg)
        }

        /// Same as [send](Address::send) but it blocking the OS-thread.
        pub fn send_blocking<M>(&self, msg: M) -> Result<M::Returns, SendError<P>>
        where
            P: Accepts<M>,
            M: Message,
        {
            self.channel.send_blocking(msg)
        }
    };
}
pub(crate) use send_methods;

macro_rules! child_methods {
    () => {
        /// Attach the actor. Returns the old abort-timeout, if it was attached before this.
        pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
            self.link.attach(duration)
        }

        /// Detach the actor. Returns the old abort-timeout, if it was attached before this.
        pub fn detach(&mut self) -> Option<Duration> {
            self.link.detach()
        }

        /// Whether the actor is aborted.
        pub fn is_aborted(&self) -> bool {
            self.is_aborted
        }

        /// Get a reference to the current [Link] of the actor.
        pub fn link(&self) -> &Link {
            &self.link
        }
    };
}
pub(crate) use child_methods;
