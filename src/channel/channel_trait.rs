use event_listener::EventListener;
use std::{
    any::Any,
    fmt::Debug,
    sync::{atomic::Ordering, Arc},
};

use crate::*;

/// A [Channel]-trait, without information about it's message type. Therefore, it's impossible
/// to send or receive messages through this.
pub trait DynChannel {
    fn close(&self) -> bool;
    fn halt_some(&self, n: u32);
    fn halt(&self);
    fn process_count(&self) -> usize;
    fn msg_count(&self) -> usize;
    fn address_count(&self) -> usize;
    fn is_closed(&self) -> bool;
    fn capacity(&self) -> &Capacity;
    fn has_exited(&self) -> bool;
    fn add_address(&self) -> usize;
    fn remove_address(&self) -> usize;
    fn get_exit_listener(&self) -> EventListener;
    fn actor_id(&self) -> u64;
    fn is_bounded(&self) -> bool;
}

pub trait AnyChannel: DynChannel + Debug + Send + Sync + 'static {
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
}

impl<M: Send + 'static> AnyChannel for Channel<M> {
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
}

impl<M> DynChannel for Channel<M> {
    /// Close the channel. Returns `true` if the channel was not closed before this.
    /// Otherwise, this returns `false`.
    ///
    /// ## Notifies
    /// * if `true` -> all send_listeners & recv_listeners
    fn close(&self) -> bool {
        if self.queue.close() {
            self.recv_event.notify(usize::MAX);
            self.send_event.notify(usize::MAX);
            true
        } else {
            false
        }
    }

    /// Halt n inboxes associated with this channel. If `n >= #inboxes`, all inboxes
    /// will be halted. This might leave `halt-count > inbox-count`, however that's not
    /// a problem. If n > i32::MAX, n = i32::MAX.
    ///
    /// # Notifies
    /// * all recv-listeners
    fn halt_some(&self, n: u32) {
        let n = i32::try_from(n).unwrap_or(i32::MAX);

        self.halt_count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                // If the count < 0, act as if it's 0.
                if count < 0 {
                    Some(n)
                } else {
                    // Otherwise, add both together.
                    Some(count.saturating_add(n))
                }
            })
            .unwrap();

        self.recv_event.notify(usize::MAX);
    }

    /// Returns the amount of inboxes this channel has.
    fn process_count(&self) -> usize {
        self.inbox_count.load(Ordering::Acquire)
    }

    /// Returns the amount of messages currently in the channel.
    fn msg_count(&self) -> usize {
        self.queue.len()
    }

    /// Returns the amount of addresses this channel has.
    fn address_count(&self) -> usize {
        self.address_count.load(Ordering::Acquire)
    }

    /// Whether the queue asscociated to the channel has been closed.
    fn is_closed(&self) -> bool {
        self.queue.is_closed()
    }

    /// Capacity of the inbox.
    fn capacity(&self) -> &Capacity {
        &self.capacity
    }

    /// Whether all inboxes linked to this channel have exited.
    fn has_exited(&self) -> bool {
        self.inbox_count.load(Ordering::Acquire) == 0
    }

    /// Add an Address to the channel, incrementing address-count by 1. Afterwards,
    /// a new Address may be created from this channel.
    ///
    /// Returns the previous inbox-count
    fn add_address(&self) -> usize {
        self.address_count.fetch_add(1, Ordering::AcqRel)
    }

    /// Remove an Address from the channel, decrementing address-count by 1. This should
    /// be called from the destructor of the Address.
    ///
    /// ## Notifies
    /// * `prev-address-count == 1` -> all send_listeners & recv_listeners
    ///
    /// ## Panics
    /// * `prev-address-count == 0`
    fn remove_address(&self) -> usize {
        // Subtract one from the inbox count
        let prev_address_count = self.address_count.fetch_sub(1, Ordering::AcqRel);
        assert!(prev_address_count >= 1);
        prev_address_count
    }

    fn get_exit_listener(&self) -> EventListener {
        self.get_exit_listener()
    }

    /// Get the actor_id.
    fn actor_id(&self) -> u64 {
        self.actor_id
    }

    /// Whether the channel is bounded.
    fn is_bounded(&self) -> bool {
        self.capacity.is_bounded()
    }

    // Closes the channel and halts all actors.
    fn halt(&self) {
        self.close();
        self.halt_some(u32::MAX);
    }
}
