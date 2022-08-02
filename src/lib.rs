#![doc = include_str!("../README.md")]

mod address;
mod actor;
mod child;
mod config;
mod gen;
mod inbox;
mod spawning;

pub use address::*;
pub use actor::*;
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
