#![doc = include_str!("../README.md")]

pub(crate) mod address;
pub(crate) mod channel;
pub(crate) mod child;
pub(crate) mod config;
pub(crate) mod inbox;
pub(crate) mod spawning;

pub use address::*;
pub(crate) use channel::*;
pub use child::*;
pub use config::*;
pub use inbox::*;
pub use spawning::*;

#[cfg(test)]
pub(crate) mod test_helper {
    macro_rules! test_loop {
        () => {
            test_helper::test_loop!(())
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
            test_helper::test_many_loop!(())
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
}
