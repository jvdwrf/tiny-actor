// use crate::{AnyChannel, DynChannel};

// #[derive(Debug)]
// pub(crate) struct TaskChannel;

// impl AnyChannel for TaskChannel {
//     fn into_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync> {
//         self
//     }
// }

// impl DynChannel for TaskChannel {
//     fn close(&self) -> bool {
        
//     }

//     fn halt_some(&self, n: u32) {
//         todo!()
//     }

//     fn halt(&self) {
//         todo!()
//     }

//     fn process_count(&self) -> usize {
//         todo!()
//     }

//     fn msg_count(&self) -> usize {
//         todo!()
//     }

//     fn address_count(&self) -> usize {
//         todo!()
//     }

//     fn is_closed(&self) -> bool {
//         todo!()
//     }

//     fn capacity(&self) -> &crate::Capacity {
//         todo!()
//     }

//     fn has_exited(&self) -> bool {
//         todo!()
//     }

//     fn add_address(&self) -> usize {
//         todo!()
//     }

//     fn remove_address(&self) -> usize {
//         todo!()
//     }

//     fn get_exit_listener(&self) -> event_listener::EventListener {
//         todo!()
//     }

//     fn actor_id(&self) -> u64 {
//         todo!()
//     }

//     fn is_bounded(&self) -> bool {
//         todo!()
//     }
// }