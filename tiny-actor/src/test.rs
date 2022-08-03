use crate::*;

pub use crate as tiny_actor;

#[derive(Message)]
pub struct TestMsg1;

#[protocol]
pub enum TestProt1 {
    Msg(TestMsg1),
}

#[protocol]
pub enum TestProt2 {
    Msg(TestMsg1),
}

macro_rules! test_loop {
    () => {
        test::test_loop!(test::TestProt1)
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
        test::test_many_loop!(test::TestProt1)
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
