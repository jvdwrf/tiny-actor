#![doc = include_str!("../../README.md")]

mod actor;
mod config;
mod gen;
mod spawning;
mod parts;
mod messaging;
mod channel;

pub use actor::*;
pub use config::*;
pub use spawning::*;
pub use parts::*;
pub use messaging::*;
pub use channel::*;
pub use tiny_actor_codegen::*;

#[cfg(test)]
pub(crate) mod test_helper {
    use crate as tiny_actor;

    #[derive(Message)]
    pub struct TestMsg1;

    #[protocol]
    pub enum TestProt1 {
        Msg(TestMsg1)
    }

    #[protocol]
    pub enum TestProt2 {
        Msg(TestMsg1)
    }

    macro_rules! test_loop {
        () => {
            test_helper::test_loop!(test_helper::TestProt1)
        };
        ($ty:ty) => {
            |mut inbox: Inbox<$ty>| async move {
                loop {
                    match inbox.recv().await {
                        Ok(_) => (),
                        Err(e) => break e,
                    }
                }
            }
        };
    }
    pub(crate) use test_loop;

    macro_rules! test_many_loop {
        () => {
            test_helper::test_many_loop!(test_helper::TestProt1)
        };
        ($ty:ty) => {
            |_, mut inbox: Inbox<$ty>| async move {
                loop {
                    match inbox.recv().await {
                        Ok(_) => (),
                        Err(e) => break e,
                    }
                }
            }
        };
    }
    pub(crate) use test_many_loop;
    use tiny_actor_codegen::{Message, protocol};
}
