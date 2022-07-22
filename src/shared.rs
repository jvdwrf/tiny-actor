// use actor_channel::{AnyChannel, Capacity, Exit, Haltable};
// use std::{any::Any, sync::Arc};
// pub(crate) struct Shared<C: ?Sized = dyn AnyChannel> {
//     channel: Arc<C>,
// }

// impl<C: ?Sized> Shared<C> {
//     pub fn new(channel: Arc<C>) -> Self {
//         Self { channel }
//     }
// }

// impl<C: AnyChannel + 'static> Shared<C> {
//     pub fn into_inner(self) -> Arc<C> {
//         self.channel
//     }
// }

// impl<C> Haltable for Shared<C>
// where
//     C: ?Sized + AnyChannel,
// {
//     fn halt(&self, n: u32) {
//         self.channel.halt(n)
//     }
// }

// impl<C> AnyChannel for Shared<C>
// where
//     C: AnyChannel + ?Sized
// {
//     fn close(&self) -> bool {
//         self.channel.close()
//     }
//     fn closed(&self) -> bool {
//         self.channel.closed()
//     }
//     fn capacity(&self) -> &Capacity {
//         self.channel.capacity()
//     }
//     fn should_halt(&self) -> bool {
//         self.channel.should_halt()
//     }
//     fn receiver_count(&self) -> usize {
//         self.channel.receiver_count()
//     }
//     fn sender_count(&self) -> usize {
//         self.channel.sender_count()
//     }
//     fn msg_count(&self) -> usize {
//         self.channel.msg_count()
//     }
//     fn add_receiver(&self) -> usize {
//         self.channel.add_receiver()
//     }
//     fn try_add_receiver(&self) -> Result<usize, ()> {
//         self.channel.try_add_receiver()
//     }
//     fn remove_receiver(&self) {
//         self.channel.remove_receiver()
//     }
//     fn add_sender(&self) -> usize {
//         self.channel.add_sender()
//     }
//     fn remove_sender(&self) -> usize {
//         self.channel.remove_sender()
//     }
//     fn exited(&self) -> bool {
//         self.channel.exited()
//     }
//     fn exit(&self) -> Exit<'_> {
//         self.channel.exit()
//     }
//     fn exit_blocking(&self) {
//         self.channel.exit_blocking()
//     }
//     // fn into_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync> {
//     //     self.channel
//     // }
// }
