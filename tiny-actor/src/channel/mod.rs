use crate::*;
use concurrent_queue::{ConcurrentQueue, PopError, PushError};
use event_listener::{Event, EventListener};
use once_cell::sync::Lazy;
use std::{
    fmt::Debug,
    sync::atomic::{AtomicI32, AtomicU64, AtomicUsize, Ordering},
};

mod channel_traits;
mod receiving;
mod sending;
pub use {channel_traits::*, receiving::*, sending::*};

static ACTOR_ID_COUNTER: Lazy<AtomicU64> = Lazy::new(|| AtomicU64::new(0));
fn next_actor_id() -> u64 {
    ACTOR_ID_COUNTER.fetch_add(1, Ordering::AcqRel)
}

/// Contains all data that should be shared between Addresses, Inboxes and the Child.
/// This is wrapped in an Arc to allow sharing between them.
pub struct Channel<P> {
    /// The underlying queue
    queue: ConcurrentQueue<P>,
    /// The capacity of the channel
    capacity: Capacity,
    /// The amount of addresses associated to this channel.
    /// Once this is 0, not more addresses can be created and the Actor is closed.
    address_count: AtomicUsize,
    /// The amount of inboxes associated to this channel.
    /// Once this is 0, it is impossible to spawn new processes, and the Actor.
    /// has exited.
    inbox_count: AtomicUsize,
    /// Subscribe when trying to receive a message from this channel.
    recv_event: Event,
    /// Subscribe when trying to send a message into this channel.
    send_event: Event,
    /// Subscribe when waiting for Actor to exit.
    exit_event: Event,
    /// The amount of processes that should still be halted.
    /// Can be negative bigger than amount of processes in total.
    halt_count: AtomicI32,

    actor_id: u64,
}

impl<M> Channel<M> {
    /// Create a new channel, given an address count, inbox_count and capacity.
    ///
    /// After this, it must be ensured that the correct amount of inboxes and addresses actually exist.
    pub(crate) fn new(address_count: usize, inbox_count: usize, capacity: Capacity) -> Self {
        Self {
            queue: match &capacity {
                Capacity::Bounded(size) => ConcurrentQueue::bounded(size.to_owned()),
                Capacity::Unbounded(_) => ConcurrentQueue::unbounded(),
            },
            capacity,
            address_count: AtomicUsize::new(address_count),
            inbox_count: AtomicUsize::new(inbox_count),
            recv_event: Event::new(),
            send_event: Event::new(),
            exit_event: Event::new(),
            halt_count: AtomicI32::new(0),
            actor_id: next_actor_id(),
        }
    }

    /// Sets the inbox-count
    pub(crate) fn set_inbox_count(&self, count: usize) {
        self.inbox_count.store(count, Ordering::Release)
    }

    /// Try to add an inbox to the channel, incrementing inbox-count by 1. Afterwards,
    /// a new Inbox may be created from this channel.
    ///
    /// Returns the old inbox-count
    ///
    /// Whereas `add_inbox()` also adds an `Inbox` if `inbox-count == 0` this method returns an error instead.
    /// This method is slower than add_inbox, since it uses `fetch-update` on the inbox-count.
    pub(crate) fn try_add_inbox(&self) -> Result<usize, ()> {
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
            Ok(prev) => Ok(prev),
            Err(_) => Err(()),
        }
    }

    /// Remove an Inbox from the channel, decrementing inbox-count by 1. This should be
    /// called during the Inbox's destructor.
    ///
    /// If there are no more Inboxes remaining, this will close the channel and set
    /// `inboxes_dropped` to true. This will also drop any messages still inside the
    /// channel.
    ///
    /// Returns the previous inbox-count.
    ///
    /// ## Notifies
    /// * `prev-inbox-count == 1` -> all exit-listeners
    ///
    /// ## Panics
    /// * `prev-inbox-count == 0`
    pub(crate) fn remove_inbox(&self) -> usize {
        // Subtract one from the inbox count
        let prev_count = self.inbox_count.fetch_sub(1, Ordering::AcqRel);
        assert!(prev_count != 0);

        // If previous count was 1, then all inboxes have been dropped.
        if prev_count == 1 {
            self.close();
            // Also notify the exit-listeners, since the process exited.
            self.exit_event.notify(usize::MAX);
            // drop all messages, since no more inboxes exist.
            while self.take_next_msg().is_ok() {}
        }

        prev_count
    }

    /// Add an Address to the channel, incrementing address-count by 1. Afterwards,
    /// a new Address may be created from this channel.
    ///
    /// Returns the previous inbox-count
    pub(crate) fn add_address(&self) -> usize {
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
    pub(crate) fn remove_address(&self) {
        // Subtract one from the inbox count
        let prev_address_count = self.address_count.fetch_sub(1, Ordering::AcqRel);
        assert!(prev_address_count >= 1);

        // If previous count was 1, then we can close the channel
        if prev_address_count == 1 {
            // This notifies senders and receivers
            self.close();
        }
    }

    /// Takes the next message out of the channel.
    ///
    /// Returns an error if the queue is closed, returns none if there is no message
    /// in the queue.
    ///
    /// ## Notifies
    /// on success -> 1 send_listener & 1 recv_listener
    pub(crate) fn take_next_msg(&self) -> Result<Option<M>, ()> {
        match self.queue.pop() {
            Ok(msg) => {
                self.send_event.notify(1);
                self.recv_event.notify(1);
                Ok(Some(msg))
            }
            Err(PopError::Empty) => Ok(None),
            Err(PopError::Closed) => Err(()),
        }
    }

    /// Push a message into the channel.
    ///
    /// Can fail either because the queue is full, or because it is closed.
    ///
    /// ## Notifies
    /// on success -> 1 recv_listener
    pub(crate) fn push_msg(&self, msg: M) -> Result<(), PushError<M>> {
        match self.queue.push(msg) {
            Ok(()) => {
                self.recv_event.notify(1);
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Close the channel. Returns `true` if the channel was not closed before this.
    /// Otherwise, this returns `false`.
    ///
    /// ## Notifies
    /// * if `true` -> all send_listeners & recv_listeners
    pub(crate) fn close(&self) -> bool {
        if self.queue.close() {
            self.recv_event.notify(usize::MAX);
            self.send_event.notify(usize::MAX);
            true
        } else {
            false
        }
    }

    /// Can be called by an inbox to know whether it should halt.
    ///
    /// This decrements the halt-counter by one when it is called, therefore every
    /// inbox should only receive true from this method once! The inbox keeps it's own
    /// local state about whether it has received true from this method.
    pub(crate) fn inbox_should_halt(&self) -> bool {
        // todo: test this

        // If the count is bigger than 0, we might have to halt.
        if self.halt_count.load(Ordering::Acquire) > 0 {
            // Now subtract 1 from the count
            let prev_count = self.halt_count.fetch_sub(1, Ordering::AcqRel);
            // If the count before updating was bigger than 0, we halt.
            // If this decrements below 0, we treat it as if it's 0.
            if prev_count > 0 {
                return true;
            }
        }

        // Otherwise, just continue
        false
    }

    /// Halt n inboxes associated with this channel. If `n >= #inboxes`, all inboxes
    /// will be halted. This might leave `halt-count > inbox-count`, however that's not
    /// a problem. If n > i32::MAX, n = i32::MAX.
    ///
    /// # Notifies
    /// * `n` recv-listeners
    pub(crate) fn halt_some(&self, n: u32) {
        // todo: test this
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

        self.recv_event.notify(n as usize);
    }

    /// Whether the queue asscociated to the channel has been closed.
    pub(crate) fn is_closed(&self) -> bool {
        self.queue.is_closed()
    }

    /// Returns the amount of messages currently in the channel.
    pub(crate) fn msg_count(&self) -> usize {
        self.queue.len()
    }

    /// Returns the amount of addresses this channel has.
    pub(crate) fn address_count(&self) -> usize {
        self.address_count.load(Ordering::Acquire)
    }

    /// Returns the amount of inboxes this channel has.
    pub(crate) fn inbox_count(&self) -> usize {
        self.inbox_count.load(Ordering::Acquire)
    }

    /// Capacity of the inbox.
    pub(crate) fn capacity(&self) -> &Capacity {
        &self.capacity
    }

    /// Whether all inboxes linked to this channel have exited.
    pub(crate) fn has_exited(&self) -> bool {
        self.inbox_count.load(Ordering::Acquire) == 0
    }

    /// Get a new recv-event listener
    pub(crate) fn get_recv_listener(&self) -> EventListener {
        self.recv_event.listen()
    }

    /// Get a new send-event listener
    pub(crate) fn get_send_listener(&self) -> EventListener {
        self.send_event.listen()
    }

    /// Get a new exit-event listener
    pub(crate) fn get_exit_listener(&self) -> EventListener {
        self.exit_event.listen()
    }

    /// Get the actor_id
    pub(crate) fn actor_id(&self) -> u64 {
        self.actor_id
    }
}

impl<M> Debug for Channel<M> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actor")
            .field("queue", &self.queue)
            .field("capacity", &self.capacity)
            .field("address_count", &self.address_count)
            .field("inbox_count", &self.inbox_count)
            .field("halt_count", &self.halt_count)
            .finish()
    }
}

#[cfg(test)]
mod test {
    use std::sync::Arc;

    use super::{next_actor_id, Channel};
    use crate::*;
    use concurrent_queue::PushError;
    use event_listener::EventListener;
    use futures::FutureExt;

    #[test]
    fn actor_ids() {
        let id1 = next_actor_id();
        let id2 = next_actor_id();
        assert!(id1 < id2);
    }

    #[test]
    fn channel_actor_ids() {
        let id1 = Channel::<()>::new(1, 1, Capacity::Bounded(10)).actor_id();
        let id2 = Channel::<()>::new(1, 1, Capacity::Bounded(10)).actor_id();
        assert!(id1 < id2);
    }

    #[test]
    fn capacity_types() {
        let channel = Channel::<()>::new(1, 1, Capacity::Bounded(10));
        assert!(channel.queue.capacity().is_some());

        let channel = Channel::<()>::new(1, 1, Capacity::Unbounded(BackPressure::default()));
        assert!(channel.queue.capacity().is_none());
    }

    #[test]
    fn closing() {
        let channel = Channel::<()>::new(1, 1, Capacity::default());
        let listeners = Listeners::size_10(&channel);

        channel.close();

        assert!(channel.is_closed());
        assert!(!channel.has_exited());
        assert_eq!(channel.push_msg(()), Err(PushError::Closed(())));
        assert_eq!(channel.take_next_msg(), Err(()));
        listeners.assert_notified(Assert {
            recv: 10,
            exit: 0,
            send: 10,
        });
    }

    #[test]
    fn no_more_senders_remaining() {
        let channel = Channel::<()>::new(1, 1, Capacity::default());
        let listeners = Listeners::size_10(&channel);

        channel.remove_address();

        assert!(channel.is_closed());
        assert!(!channel.has_exited());
        assert_eq!(channel.address_count(), 0);
        assert_eq!(channel.inbox_count(), 1);
        assert_eq!(channel.push_msg(()), Err(PushError::Closed(())));
        assert_eq!(channel.take_next_msg(), Err(()));
        listeners.assert_notified(Assert {
            recv: 10,
            exit: 0,
            send: 10,
        });
    }

    #[test]
    fn exiting() {
        let channel = Channel::<()>::new(1, 1, Capacity::default());
        let listeners = Listeners::size_10(&channel);

        channel.remove_inbox();

        assert!(channel.is_closed());
        assert!(channel.has_exited());
        assert_eq!(channel.inbox_count(), 0);
        assert_eq!(channel.address_count(), 1);
        assert_eq!(channel.push_msg(()), Err(PushError::Closed(())));
        assert_eq!(channel.take_next_msg(), Err(()));
        listeners.assert_notified(Assert {
            recv: 10,
            exit: 10,
            send: 10,
        });
    }

    #[test]
    fn exiting_drops_all_messages() {
        let channel = Channel::<Arc<()>>::new(1, 1, Capacity::default());
        let msg = Arc::new(());
        channel.push_msg(msg.clone()).unwrap();
        assert_eq!(Arc::strong_count(&msg), 2);
        channel.remove_inbox();
        assert_eq!(Arc::strong_count(&msg), 1);
    }

    #[test]
    fn closing_doesnt_drop_messages() {
        let channel = Channel::<Arc<()>>::new(1, 1, Capacity::default());
        let msg = Arc::new(());
        channel.push_msg(msg.clone()).unwrap();
        assert_eq!(Arc::strong_count(&msg), 2);
        channel.close();
        assert_eq!(Arc::strong_count(&msg), 2);
    }

    #[test]
    fn try_add_inbox_with_0_receivers() {
        let channel = Channel::<Arc<()>>::new(1, 1, Capacity::default());
        channel.remove_inbox();
        assert_eq!(channel.try_add_inbox(), Err(()));
        assert_eq!(channel.inbox_count(), 0);
    }

    #[test]
    fn add_inbox_with_0_receivers() {
        let channel = Channel::<Arc<()>>::new(1, 1, Capacity::default());
        channel.remove_inbox();
        matches!(channel.try_add_inbox(), Err(_));
        assert_eq!(channel.inbox_count(), 0);
    }

    #[test]
    fn sending_notifies() {
        let channel = Channel::<()>::new(1, 1, Capacity::default());
        let listeners = Listeners::size_10(&channel);
        channel.push_msg(()).unwrap();

        listeners.assert_notified(Assert {
            recv: 1,
            exit: 0,
            send: 0,
        });
    }

    #[test]
    fn recveiving_notifies() {
        let channel = Channel::<()>::new(1, 1, Capacity::default());
        channel.push_msg(()).unwrap();
        let listeners = Listeners::size_10(&channel);
        channel.take_next_msg().unwrap();

        listeners.assert_notified(Assert {
            recv: 1,
            exit: 0,
            send: 1,
        });
    }

    struct Listeners {
        recv: Vec<EventListener>,
        exit: Vec<EventListener>,
        send: Vec<EventListener>,
    }

    struct Assert {
        recv: usize,
        exit: usize,
        send: usize,
    }

    impl Listeners {
        fn size_10<T>(channel: &Channel<T>) -> Self {
            Self {
                recv: (0..10)
                    .into_iter()
                    .map(|_| channel.get_recv_listener())
                    .collect(),
                exit: (0..10)
                    .into_iter()
                    .map(|_| channel.get_exit_listener())
                    .collect(),
                send: (0..10)
                    .into_iter()
                    .map(|_| channel.get_send_listener())
                    .collect(),
            }
        }

        fn assert_notified(self, assert: Assert) {
            let recv = self
                .recv
                .into_iter()
                .map(|l| l.now_or_never().is_some())
                .filter(|bool| *bool)
                .collect::<Vec<_>>()
                .len();
            let exit = self
                .exit
                .into_iter()
                .map(|l| l.now_or_never().is_some())
                .filter(|bool| *bool)
                .collect::<Vec<_>>()
                .len();
            let send = self
                .send
                .into_iter()
                .map(|l| l.now_or_never().is_some())
                .filter(|bool| *bool)
                .collect::<Vec<_>>()
                .len();

            assert_eq!(assert.recv, recv);
            assert_eq!(assert.exit, exit);
            assert_eq!(assert.send, send);
        }
    }
}

#[cfg(bench)]
mod bench {
    use super::Channel;
    use crate::*;
    use concurrent_queue::ConcurrentQueue;
    use futures::future::{pending, ready};
    use std::{
        ops::Range,
        sync::{
            atomic::{AtomicU64, Ordering},
            Arc,
        },
    };
    use tokio::{sync::mpsc, time::Instant};
    use uuid::Uuid;

    /// 75_000ms
    #[tokio::test]
    async fn bench_mpsc() {
        let time = Instant::now();
        for _ in 0..1000_000 {
            let channel = mpsc::unbounded_channel::<()>();
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 75_000 ms
    #[tokio::test]
    async fn bench_channel() {
        let time = Instant::now();
        for handle in 0..1000_000 {
            let channel = Arc::new(Channel::<()>::new(1, 1, Capacity::default()));
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 90_000 ms
    #[tokio::test]
    async fn bench_channel_and_parts() {
        let handles = used_handles(0..1000_000).await;
        let time = Instant::now();
        for handle in handles {
            let channel = Arc::new(Channel::<()>::new(1, 1, Capacity::default()));
            let address = Address::from_channel(channel.clone());
            let child = Child::new(channel.clone(), handle, Link::default());
            let inbox = Inbox::from_channel(channel);
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 145_000 us
    #[tokio::test]
    async fn bench_tokio_and_channel() {
        let time = Instant::now();
        for _ in 0..1000_000 {
            let handle = tokio::task::spawn(pending::<()>());
            let channel = Channel::<()>::new(1, 1, Capacity::default());
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 300_000 us
    #[tokio::test]
    async fn bench_tokio_and_move_channel() {
        let time = Instant::now();
        for _ in 0..1000_000 {
            let channel = Arc::new(Channel::<()>::new(1, 1, Capacity::default()));
            let address = Address::from_channel(channel.clone());
            let inbox = Inbox::from_channel(channel.clone());
            let handle = tokio::task::spawn(async move {
                pending::<()>().await;
                drop(inbox);
            });
            let child = Child::new(channel, handle, Link::default());
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 1_000_000 us
    #[tokio::test]
    async fn bench_process() {
        let time = Instant::now();
        for _ in 0..1000_000 {
            spawn(Config::default(), |inbox: Inbox<()>| async move {
                pending::<()>().await;
                drop(inbox);
            });
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 1_100_000 us
    #[tokio::test]
    async fn bench_process_with_uuid() {
        let time = Instant::now();
        for _ in 0..1000_000 {
            let uuid = Uuid::new_v4();
            spawn(Config::default(), |inbox: Inbox<()>| async move {
                pending::<()>().await;
                drop(inbox);
            });
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 250_000 us
    #[tokio::test]
    async fn bench_uuid() {
        let time = Instant::now();
        for _ in 0..1000_000 {
            let uuid = Uuid::new_v4();
        }
        println!("{} us", time.elapsed().as_micros());
    }

    /// 4_000 us
    ///
    /// u64::MAX:
    /// 18_446_744_073_709_551_615 / 1_000_000 proc/s / 60s / 60m / 24h / 365d
    /// = 584_942 years of 1_000_000 proc/sec
    #[tokio::test]
    async fn bench_atomic_counter() {
        let counter = AtomicU64::new(0);
        let time = Instant::now();
        for _ in 0..1000_000 {
            let val = counter.fetch_add(1, Ordering::AcqRel);
        }
        println!("{} us", time.elapsed().as_micros());
    }

    #[tokio::test]
    async fn bench_channel_creation3() {
        let time = Instant::now();
        for _ in 0..1000 {
            let handle = tokio::task::spawn(pending::<()>());
            let channel = ConcurrentQueue::<()>::unbounded();
        }
        println!("Task + Queue: {} us", time.elapsed().as_micros());
    }

    async fn used_handles(range: Range<i32>) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = range
            .into_iter()
            .map(|_| tokio::task::spawn(ready(())))
            .collect::<Vec<_>>();

        for handle in &mut handles {
            handle.await.unwrap();
        }

        handles
    }
}
