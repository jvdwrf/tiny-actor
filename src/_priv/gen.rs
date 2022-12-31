macro_rules! dyn_channel_methods {
    () => {
        /// Close the [Channel].
        pub fn close(&self) -> bool {
            self.channel.close()
        }

        /// Halt the actor.
        ///
        /// This also closes the Inbox.
        pub fn halt(&self) {
            self.channel.halt()
        }

        /// Halt n processes.
        pub fn halt_some(&self, n: u32) {
            self.channel.halt_some(n)
        }

        /// Get the amount of processes.
        pub fn process_count(&self) -> usize {
            self.channel.process_count()
        }

        /// Get the amount of messages in the [Channel].
        pub fn msg_count(&self) -> usize {
            self.channel.msg_count()
        }

        /// Get the amount of [Addresses](Address) of the [Channel].
        pub fn address_count(&self) -> usize {
            self.channel.address_count()
        }

        /// Whether the [Channel] is closed.
        pub fn is_closed(&self) -> bool {
            self.channel.is_closed()
        }

        pub fn is_bounded(&self) -> bool {
            self.channel.is_bounded()
        }

        /// Get the [Capacity] of the [Channel].
        pub fn capacity(&self) -> &Capacity {
            self.channel.capacity()
        }

        /// Whether all processes have exited.
        pub fn has_exited(&self) -> bool {
            self.channel.has_exited()
        }

        /// Get the actor's id.
        pub fn actor_id(&self) -> u64 {
            self.channel.actor_id()
        }
    };
}
pub(crate) use dyn_channel_methods;

macro_rules! send_methods {
    () => {
        /// Attempt to send a message to the actor.
        ///
        /// * In the case of an `unbounded` [Channel], when [BackPressure] returns a timeout this fails.
        /// * In the case of a `bounded` [Channel], when it is full this fails.
        ///
        /// For `bounded` channels, this method is the same as [send_now](Address::send_now).
        pub fn try_send(&self, msg: M) -> Result<(), TrySendError<M>> {
            self.channel.try_send(msg)
        }

        /// Attempt to send a message to the actor.
        ///
        /// * In the case of an `unbounded` [Channel], any [BackPressure] is ignored.
        /// * In the case of a `bounded` [Channel], when it is full this fails.
        ///
        /// For `bounded` channels, this method is the same as [try_send](Address::send_now).
        pub fn send_now(&self, msg: M) -> Result<(), TrySendError<M>> {
            self.channel.send_now(msg)
        }

        /// Attempt to send a message to the actor.
        ///
        /// * In the case of an `unbounded` [Channel], when [BackPressure] returns a timeout this waits
        /// untill the timeout is over.
        /// * In the case of a `bounded` [Channel], when it is full this waits untill space is available.
        pub fn send(&self, msg: M) -> SendFut<'_, M> {
            self.channel.send(msg)
        }

        /// Same as [send](Address::send) but it blocking the OS-thread.
        pub fn send_blocking(&self, msg: M) -> Result<(), SendError<M>> {
            self.channel.send_blocking(msg)
        }
    };
}
pub(crate) use send_methods;

macro_rules! child_methods {
    () => {
        /// Get a new [Address] to the [Channel].
        pub fn get_address(&self) -> Address<C> {
            self.channel.add_address();
            Address::from_channel(self.channel.clone())
        }

        /// Attach the actor.
        ///
        /// Returns the old abort-timeout if it was already attached.
        pub fn attach(&mut self, duration: Duration) -> Option<Duration> {
            self.link.attach(duration)
        }

        /// Detach the actor.
        ///
        /// Returns the old abort-timeout if it was attached before.
        pub fn detach(&mut self) -> Option<Duration> {
            self.link.detach()
        }

        /// Whether the actor is aborted.
        pub fn is_aborted(&self) -> bool {
            self.is_aborted
        }

        /// Whether the actor is attached.
        pub fn is_attached(&self) -> bool {
            self.link.is_attached()
        }

        /// Get a reference to the current [Link] of the actor.
        pub fn link(&self) -> &Link {
            &self.link
        }

        pub fn config(&self) -> Config {
            Config {
                link: self.link().clone(),
                capacity: self.capacity().clone(),
            }
        }
    };
}
pub(crate) use child_methods;
