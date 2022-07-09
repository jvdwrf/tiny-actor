use concurrent_queue::{ConcurrentQueue, PopError, PushError};
use el::{Event, EventListener};
use event_listener as el;
use std::sync::{
    atomic::{AtomicI32, AtomicUsize, Ordering},
    Arc,
};

use crate::*;

/// Contains all data that should be shared between addresses and inboxes.
///
/// This is wrapped in an Arc, to allow sharing.
pub(crate) struct Channel<T> {
    /// The underlying queue
    queue: ConcurrentQueue<T>,

    capacity: Capacity,

    /// The amount of addresses associated to this channel
    address_count: AtomicUsize,
    /// The amount of inboxes associated to this channel
    inbox_count: AtomicUsize,

    /// Notified whenever there might be a new message or signal.
    recv_event: Event,
    /// Notified whenever there is more space to send.
    send_event: Event,
    /// Notified whenever all processes have exited.
    exit_event: Event,

    /// The amount of processes that should still be halted.
    halt_count: AtomicI32,
}

impl<T> Channel<T> {
    /// Create a new channel, given an address count, inbox_count and capacity.
    pub fn new(address_count: usize, inbox_count: usize, capacity: Capacity) -> Self {
        Self {
            queue: match &capacity {
                Capacity::Bounded(size) => ConcurrentQueue::bounded(*size),
                Capacity::Unbounded(_) => ConcurrentQueue::unbounded(),
            },
            capacity,
            address_count: AtomicUsize::new(address_count),
            inbox_count: AtomicUsize::new(inbox_count),
            recv_event: Event::new(),
            send_event: Event::new(),
            exit_event: Event::new(),
            halt_count: AtomicI32::new(0),
        }
    }

    /// Add an extra inbox to the channel, this sets the inbox-count +1.
    ///
    /// ## Panics
    /// Panics if `inbox-count < 1`
    pub fn add_inbox(self: &Arc<Self>) {
        let prev_count = self.inbox_count.fetch_add(1, Ordering::AcqRel);
        assert!(prev_count > 0);
    }

    /// Add an extra inbox to the channel, this sets the inbox-count +1.
    /// False if `inbox-count < 1`, and does not modify the inbox-count.
    ///
    /// This method is slower than add_inbox.
    pub fn try_add_inbox(self: &Arc<Self>) -> bool {
        let result = self
            .inbox_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |val| {
                if val < 1 {
                    None
                } else {
                    Some(val + 1)
                }
            });

        match result {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    /// Remove an inbox from the channel, this sets the inbox-count -1.
    ///
    /// If there are no more inboxes remaining, this will close the channel and set
    /// `inboxes_dropped` to true. This will also drop any messages still inside the
    /// channel.
    ///
    /// ## Notifies
    /// All listeners, if the inbox-count dropped to 0.
    ///
    /// ## Panics
    /// Panics if `inbox-count < 1`
    pub fn remove_inbox(&self) {
        // Subtract one from the inbox count
        let prev_inbox_count = self.inbox_count.fetch_sub(1, Ordering::AcqRel);
        assert!(prev_inbox_count >= 1);

        // If previous count was 1, then all inboxes have been dropped.
        if prev_inbox_count == 1 {
            self.close();
            // Also notify the exit-listeners, since the process exited.
            self.notify_exit_listeners(usize::MAX);
            // drop all messages, since no more inboxes exist.
            while self.take_next_msg().is_ok() {}
        }
    }

    /// Add an extra address to the channel, this sets address-count +1.
    ///
    /// ## Panics
    /// Panics if `address-count < 1`
    pub fn add_address(self: &Arc<Self>) {
        let prev_count = self.address_count.fetch_add(1, Ordering::AcqRel);
        assert!(prev_count > 0);
    }

    /// Remove an address from the channel. this will set address-count -1.
    ///
    /// ## Notifies
    /// Notifies all inboxes if there are no more addresses remaining.
    ///
    /// ## Panics
    /// Panics if `address-count < 1`
    pub fn remove_address(&self) {
        // Subtract one from the inbox count
        let prev_address_count = self.address_count.fetch_sub(1, Ordering::AcqRel);
        assert!(prev_address_count >= 1);

        if prev_address_count == 1 {
            // If previous count was 1, then we can close the channel
            self.close();
        }
    }

    /// Close the channel. Returns `true` if the channel was not closed before this.
    /// Otherwise, this returns `false`.
    ///
    /// ## Notifies
    /// All receivers, if the queue is closed for the first time.
    pub fn close(&self) -> bool {
        if self.queue.close() {
            self.notify_recv_listeners(usize::MAX);
            true
        } else {
            false
        }
    }

    /// Can be called by an inbox to know whether it should halt.
    ///
    /// This decrements the halt-counter by one when it is called, therefore every
    /// inbox should only receive true from this method once!
    pub fn inbox_should_halt(&self) -> bool {
        if self.halt_count.load(Ordering::Acquire) > 0 {
            let prev = self.halt_count.fetch_sub(1, Ordering::AcqRel);
            if prev > 0 {
                return true;
            }
        }
        false
    }

    /// Takes the next message from the channel.
    ///
    /// ## Notifies
    /// If this is successful the next sender and receiver will be notified.
    pub fn take_next_msg(&self) -> Result<Option<T>, ()> {
        match self.queue.pop() {
            Ok(msg) => {
                self.notify_send_listeners(1);
                self.notify_recv_listeners(1);
                Ok(Some(msg))
            }
            Err(PopError::Empty) => Ok(None),
            Err(PopError::Closed) => Err(()),
        }
    }

    /// Push a message to the queue.
    ///
    /// ## Notifies
    /// If this is successful the first receiver will be notified.
    pub fn push_msg(&self, msg: T) -> Result<(), PushError<T>> {
        match self.queue.push(msg) {
            Ok(()) => {
                self.notify_recv_listeners(1);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Halt n inboxes associated with this channel.
    ///
    /// If `n >= #inboxes`, all inboxes will be halted.
    ///
    /// # Notifies
    /// Notifies `n` inboxes.
    pub fn halt_n(&self, n: u32) {
        let n = i32::try_from(n).unwrap_or(i32::MAX);

        self.halt_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                if count < 0 {
                    Some(n)
                } else {
                    Some(count.saturating_add(n))
                }
            })
            .unwrap();

        self.notify_recv_listeners(n as usize);
    }

    /// Whether the channel has been closed.
    ///
    /// A closed channel can no longer
    pub fn is_closed(&self) -> bool {
        self.queue.is_closed()
    }

    /// Returns the amount of messages currently in the channel.
    pub fn msg_count(&self) -> usize {
        self.queue.len()
    }

    /// Returns the amount of addresses this channel has.
    pub fn address_count(&self) -> usize {
        self.address_count.load(Ordering::Acquire)
    }

    /// Returns the amount of inboxes this channel has.
    pub fn inbox_count(&self) -> usize {
        self.inbox_count.load(Ordering::Acquire)
    }

    /// Capacity of the inbox
    pub fn capacity(&self) -> &Capacity {
        &self.capacity
    }
}

/// Listener helper functions.
///
/// These will not be directly part of public api
impl<T> Channel<T> {
    /// Get a new recv-event listener
    ///
    /// This will be notified whenever there are new messages in the inbox,
    /// or when the process could receive a signal.
    pub fn recv_listener(&self) -> EventListener {
        self.recv_event.listen()
    }

    /// Get a new send-event listener
    ///
    /// This will be notifier when there is more inbox space available.
    pub fn send_listener(&self) -> EventListener {
        self.send_event.listen()
    }

    /// Get a new exit-event listener
    ///
    /// This will be notified whenever inbox is exiting
    pub fn exit_listener(&self) -> EventListener {
        self.exit_event.listen()
    }

    /// Notify the recv-listeners
    fn notify_recv_listeners(&self, amount: usize) {
        self.recv_event.notify(amount);
    }

    /// Notify the send-listeners
    fn notify_send_listeners(&self, amount: usize) {
        self.send_event.notify(amount);
    }

    /// Notify the exit-listeners
    fn notify_exit_listeners(&self, amount: usize) {
        self.exit_event.notify(amount);
    }
}
