use std::{sync::Arc, any::Any};
use event_listener::EventListener;

use crate::*;

/// A [Channel]-trait, without information about it's message type. Therefore, it's impossible
/// to send or receive messages through this.
pub trait AnyChannel {
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
    fn close(&self) -> bool;
    fn halt_some(&self, n: u32);
    fn halt(&self);
    fn inbox_count(&self) -> usize;
    fn msg_count(&self) -> usize;
    fn address_count(&self) -> usize;
    fn is_closed(&self) -> bool;
    fn capacity(&self) -> &Capacity;
    fn has_exited(&self) -> bool;
    fn add_address(&self) -> usize;
    fn remove_address(&self);
    fn get_exit_listener(&self) -> EventListener;
}

impl<M: Send + 'static> AnyChannel for Channel<M> {
    fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
        self
    }
    fn close(&self) -> bool {
        self.close()
    }
    fn halt_some(&self, n: u32) {
        self.halt_some(n)
    }
    fn halt(&self) {
        self.halt_some(u32::MAX)
    }
    fn inbox_count(&self) -> usize {
        self.inbox_count()
    }
    fn msg_count(&self) -> usize {
        self.msg_count()
    }
    fn address_count(&self) -> usize {
        self.address_count()
    }
    fn is_closed(&self) -> bool {
        self.is_closed()
    }
    fn capacity(&self) -> &Capacity {
        self.capacity()
    }
    fn has_exited(&self) -> bool {
        self.has_exited()
    }
    fn add_address(&self) -> usize {
        self.add_address()
    }
    fn remove_address(&self) {
        self.remove_address()
    }
    fn get_exit_listener(&self) -> EventListener {
        self.get_exit_listener()
    }
}