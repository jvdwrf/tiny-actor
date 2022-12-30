//! Module containing all the different parts of the actor and their spawn functions.
//! The most important of these are: [Child], [ChildPool], [Address] and [Inbox] with
//! [spawn], [spawn_one] and [spawn_many]. Ready documentation on their respective parts
//! for more information.

mod address;
mod child;
mod child_pool;
mod inbox;
mod spawning;
mod shutdown;
mod halt_notifier;

pub use address::*;
pub use child::*;
pub use child_pool::*;
pub use inbox::*;
pub use spawning::*;
pub use shutdown::*;
pub use halt_notifier::*;